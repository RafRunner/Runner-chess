#!/usr/bin/python3
"""The user data of every worker, on Amazon Linux 2023 and its Python 3.9.

Plays the job of s3://BUCKET/job.json, then leaves the group, one worker fewer. A job
is one of:

- datagen: chunks of positions, each uploaded as
  datagen/<job>/<part>/<seed>-<positions>.txt.zst once played, until every part has
  its positions. A worker reclaimed in the middle loses only the chunk it was playing,
  and the group replaces it;
- sprt: a match of fastchess between two engines, until the SPRT decides, with its
  state uploaded to sprt/<job>/ every minute, from where the worker that replaces a
  reclaimed one resumes it;
- prepare: the training data of the trainer, the chunks of some datagen jobs converted
  to bullet's binary format and shuffled, uploaded as prepared/<job>.data. A worker
  reclaimed in the middle loses it all, and the one that replaces it starts over.

The engines are built for the CPU of the worker, from the sources the job names. A job
that is stopped or past its deadline empties the group, and so does any failure, since
the worker that would replace this one would fail the same way.

The bucket is the worker's Bucket tag, from lib/runner-chess-stack.ts, which gives this
script gzipped: user data has 16 KB at most.
"""
import datetime
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import tarfile
import time
import traceback
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

BUCKET = None
WORK = Path("/opt/worker")
LOG = WORK / "worker.log"
# the commit of the fastchess the SPRTs of the repository played with, 1.8.2
FASTCHESS = "f618e34"
CARGO = "/root/.cargo/bin/cargo"


class Stopped(Exception):
    """The job was stopped, is past its deadline, or isn't the one this worker plays"""


def main():
    global BUCKET
    WORK.mkdir(parents=True, exist_ok=True)
    os.chdir(WORK)
    instance = region = group = None
    try:
        instance = imds("instance-id")
        region = imds("placement/region")
        BUCKET = tag("Bucket")
        run("dnf", "install", "-y", "gcc", "gcc-c++", "make", "git", "zstd", "python3-boto3")
        import boto3
        from botocore.config import Config

        # a connection for each of the chunks a prepare job downloads at once
        s3 = boto3.client("s3", region_name=region,
                          config=Config(max_pool_connections=os.cpu_count() + 4))
        autoscaling = boto3.client("autoscaling", region_name=region)
        found = autoscaling.describe_auto_scaling_instances(InstanceIds=[instance])
        group = found["AutoScalingInstances"][0]["AutoScalingGroupName"]

        run("sh", "-c", "curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs"
            " | sh -s -- -y --profile minimal", env=dict(os.environ, HOME="/root"))
        job = read_job(s3)
        shut_down_after(job["until"])
        if job["kind"] == "datagen":
            datagen(s3, job, instance)
        elif job["kind"] == "sprt":
            sprt(s3, job, instance)
        elif job["kind"] == "prepare":
            prepare(s3, job, instance)
        else:
            raise RuntimeError(f"a job of an unknown kind, {job['kind']}")

        log("the job is done: one worker fewer")
        autoscaling.terminate_instance_in_auto_scaling_group(
            InstanceId=instance, ShouldDecrementDesiredCapacity=True
        )
    except Stopped as reason:
        log(str(reason))
        stop_the_group(group, region)
    except Exception:
        log(traceback.format_exc())
        stop_the_group(group, region)
    finally:
        if instance:
            upload_log(instance)
        subprocess.run(["shutdown", "-h", "now"])


def read_job(s3, playing=None):
    """The job, unless it is stopped, past its deadline, or another than `playing`"""
    job = json.loads(s3.get_object(Bucket=BUCKET, Key="job.json")["Body"].read())
    if job.get("stop") or datetime.datetime.now(datetime.timezone.utc) >= deadline(job["until"]):
        raise Stopped(f"the job {job['name']} is stopped or past its deadline")
    if playing and (job["kind"], job["name"]) != (playing["kind"], playing["name"]):
        raise Stopped(f"the job is now {job['name']}, not {playing['name']}")
    return job


def deadline(until):
    return datetime.datetime.fromisoformat(until.replace("Z", "+00:00"))


def shut_down_after(until):
    """Shuts the worker down a quarter of an hour past the deadline, even if a step of
    the job hangs: the worker that replaces it finds the job past its deadline, and
    empties the group"""
    left = deadline(until) - datetime.datetime.now(datetime.timezone.utc)
    minutes = max(0, int(left.total_seconds() // 60)) + 15
    subprocess.run(["shutdown", "-h", f"+{minutes}"], check=True)


def build(s3, key, name):
    """The engine of the sources at `key`, built with `-C target-cpu=native`"""
    s3.download_file(BUCKET, key, f"{name}.tar.gz")
    with tarfile.open(f"{name}.tar.gz") as source:
        source.extractall(name)
    run(CARGO, "build", "--release", cwd=name,
        env=dict(os.environ, HOME="/root", RUSTFLAGS="-C target-cpu=native"))
    return WORK / name / "target/release/runner-chess"


def bench(engine, depth=None):
    """The nodes and the nodes a second of `bench search`"""
    command = [engine, "bench", "search"] + ([str(depth)] if depth else [])
    out = subprocess.run(command, capture_output=True, text=True, check=True).stdout
    nodes = int(re.search(r"Nodes searched\s*:\s*(\d+)", out).group(1))
    nps = int(re.search(r"Nodes/second\s*:\s*(\d+)", out).group(1))
    return nodes, nps


def fetch_openings(s3, name):
    openings = WORK / "openings" / name
    if not openings.exists():
        openings.parent.mkdir(exist_ok=True)
        s3.download_file(BUCKET, f"openings/{name}", str(openings))
    return openings


# datagen


def datagen(s3, job, instance):
    """Chunks until every part of the job has its positions"""
    engine = build(s3, job["sourceKey"], "engine")
    nodes, _ = bench(engine)
    if nodes != job["bench"]:
        raise RuntimeError(f"bench search counted {nodes} nodes, not {job['bench']}")
    log(f"bench search: {nodes} nodes, as expected")

    chunk = 0
    while True:
        # read again before every chunk, so that a job can change or stop while it runs
        job = read_job(s3, playing=job)
        part = next(
            (p for p in job["parts"]
             if played(s3, f"datagen/{job['name']}/{p['name']}/") < p["positions"]),
            None,
        )
        if part is None:
            return

        options = ["--openings", fetch_openings(s3, part["openings"])] if "openings" in part else []
        if "variety" in part:
            options += ["--variety", str(part["variety"]),
                        "--variety-plies", str(part.get("varietyPlies", 20))]
        chunk += 1
        # 60 bits of the worker and the chunk: no two chunks play the same games
        seed = int(hashlib.sha256(f"{instance}-{chunk}".encode()).hexdigest()[:15], 16)
        subprocess.run(["rm", "-rf", "games"], check=True)
        with open("datagen.log", "w") as out:
            subprocess.run(
                [engine, "datagen", "games", "--threads", str(os.cpu_count()),
                 "--positions", str(job["chunk"]), "--nodes", str(part["nodes"]),
                 "--seed", str(seed), *options],
                stdout=out, check=True,
            )

        text = "".join(f.read_text() for f in sorted(Path("games").glob("*.txt")))
        positions = text.count("\n")
        Path("chunk.txt").write_text(text)
        run("zstd", "-q", "-3", "-f", "chunk.txt", "-o", "chunk.txt.zst")
        key = f"datagen/{job['name']}/{part['name']}/{seed}-{positions}.txt.zst"
        s3.upload_file("chunk.txt.zst", BUCKET, key)
        log(f"chunk {chunk}: {positions} positions of {part['name']}, as {key}")
        upload_log(instance)


def played(s3, prefix):
    """The positions of a part so far, from the names of its chunks"""
    return sum(positions for _, positions in chunks(s3, prefix))


def chunks(s3, prefix):
    """The keys of the chunks under `prefix`, with their positions, from their names"""
    found = []
    for page in s3.get_paginator("list_objects_v2").paginate(Bucket=BUCKET, Prefix=prefix):
        for chunk in page.get("Contents", []):
            positions = re.search(r"-(\d+)\.txt\.zst$", chunk["Key"])
            if positions:
                found.append((chunk["Key"], int(positions.group(1))))
    return found


# sprt


def sprt(s3, job, instance):
    """The match of the job, from where the state in the bucket left it, if anywhere,
    played by the scripts/match.sh the job came with, as the matches of the repository"""
    prefix = f"sprt/{job['name']}"
    match = WORK / "sprt"
    match.mkdir(exist_ok=True)
    script = match / "match.sh"
    s3.download_file(BUCKET, f"{prefix}/match.sh", str(script))
    # the script names each engine after its file
    engine = match / job["engine"]["name"]
    baseline = match / job["baseline"]["name"]
    shutil.copy(build(s3, job["engine"]["sourceKey"], "engine"), engine)
    shutil.copy(build(s3, job["baseline"]["sourceKey"], "baseline"), baseline)
    fastchess = build_fastchess()
    openings = fetch_openings(s3, job["openings"])

    # the paths of a resumed match are those it started with, the same on every worker
    state, games, out = match / "match.json", match / "match.pgn", match / "match.log"
    names = {state: "state.json", games: "games.pgn", out: "fastchess.log"}
    if download_if_there(s3, f"{prefix}/state.json", state):
        download_if_there(s3, f"{prefix}/games.pgn", games)
        download_if_there(s3, f"{prefix}/fastchess.log", out)
        log(f"resuming {prefix} from its state")
        command = ["bash", script, "--resume", match / "match"]
    else:
        tc = scaled_tc(job["tc"], job["referenceNps"], bench(baseline, job["npsDepth"])[1])
        log(f"{job['tc']} at the speed of the reference is {tc} here")
        low, high = job["bounds"]
        command = [
            "bash", script, "--bounds", f"{low},{high}", "--tc", tc,
            # a core for fastchess and the system
            "--concurrency", str(os.cpu_count() - 1),
            "--book", openings, "--out", match / "match",
            engine, baseline,
            "--", "-autosaveinterval", "10",
        ]

    def upload():
        for path, name in names.items():
            if path.exists():
                s3.upload_file(str(path), BUCKET, f"{prefix}/{name}")
        upload_log(instance)

    log(" ".join(map(str, command)))
    # its own session: stopping has to reach fastchess behind the script
    process = subprocess.Popen(
        command, stdout=subprocess.DEVNULL, stderr=LOG.open("a"), start_new_session=True,
        env=dict(os.environ, PATH=f"{fastchess.parent}:{os.environ['PATH']}"),
    )
    while True:
        try:
            process.wait(timeout=60)
            break
        except subprocess.TimeoutExpired:
            upload()
            try:
                read_job(s3, playing=job)
            except Stopped:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait()
                upload()
                raise
    upload()
    if process.returncode != 0:
        raise RuntimeError(f"the match ended with {process.returncode}")

    results = [
        line for line in out.read_text().splitlines()
        if line.startswith(("Elo:", "LLR:", "Games:", "SPRT"))
    ]
    s3.put_object(Bucket=BUCKET, Key=f"{prefix}/result.txt", Body="\n".join(results[-4:]) + "\n")
    log("\n".join(results[-4:]))


def build_fastchess():
    run("git", "clone", "--filter=blob:none", "https://github.com/Disservin/fastchess.git")
    run("git", "checkout", FASTCHESS, cwd="fastchess")
    run("make", "-j", str(os.cpu_count()), cwd="fastchess")
    return WORK / "fastchess/fastchess"


def scaled_tc(tc, reference_nps, nps):
    """`tc`, as `<seconds>+<increment>`, slower or faster by how much faster or slower
    this CPU searches than the reference, so that the engines search as many nodes"""
    base, increment = (float(x) for x in tc.split("+"))
    scale = reference_nps / nps
    return f"{base * scale:.1f}+{increment * scale:.2f}"


# prepare

# a position in bullet's binary format
POSITION_BYTES = 32


def prepare(s3, job, instance):
    """The chunks of the sources of the job, every one converted on its own, put
    together, shuffled in memory and uploaded"""
    key = f"prepared/{job['name']}.data"
    if exists(s3, key):
        log(f"{key} is there already")
        return
    utils = build_bullet_utils(job["bullet"])
    found = [c for source in job["sources"] for c in chunks(s3, f"datagen/{source}/")]
    expected = sum(positions for _, positions in found)
    log(f"{len(found)} chunks, {expected} positions")
    upload_log(instance)

    data = expected * POSITION_BYTES
    # the chunks put together, and the same shuffled
    if shutil.disk_usage(WORK).free < 2 * data + 2**30:
        raise RuntimeError(f"no room for twice {data} bytes")
    memory = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
    if memory * 3 // 4 < data:
        raise RuntimeError(f"no memory to shuffle {data} bytes")

    parts = WORK / "parts"
    parts.mkdir(exist_ok=True)

    def convert(numbered):
        number, (chunk, _) = numbered
        text, converted = parts / f"{number}.txt", parts / f"{number}.data"
        s3.download_file(BUCKET, chunk, f"{text}.zst")
        subprocess.run(["zstd", "-q", "-d", "--rm", "-f", f"{text}.zst"], check=True)
        out = subprocess.run(
            [utils, "convert", "--from", "text", "--input", text, "--output", converted],
            capture_output=True, text=True, check=True,
        ).stdout
        text.unlink()
        return converted, out.count("error parsing")

    with ThreadPoolExecutor(os.cpu_count()) as pool:
        converted = list(pool.map(convert, enumerate(found)))
    malformed = sum(errors for _, errors in converted)
    log(f"converted, {malformed} lines malformed")
    upload_log(instance)
    read_job(s3, playing=job)

    together, shuffled = WORK / "together.data", WORK / "shuffled.data"
    with together.open("wb") as out:
        for part, _ in converted:
            with part.open("rb") as chunk:
                shutil.copyfileobj(chunk, out)
            part.unlink()
    positions = together.stat().st_size // POSITION_BYTES
    run(utils, "shuffle", "--input", together, "--output", shuffled,
        "--mem-used-mb", str(memory * 3 // 4 // 2**20))
    together.unlink()
    log(f"shuffled {positions} positions")
    upload_log(instance)
    read_job(s3, playing=job)

    from boto3.s3.transfer import TransferConfig

    # parts of 64 MB, as a file has 10,000 of them at most
    s3.upload_file(str(shuffled), BUCKET, key,
                   Config=TransferConfig(multipart_chunksize=64 * 2**20))
    summary = {
        "sources": job["sources"],
        "chunks": len(found),
        "expected": expected,
        "positions": positions,
        "malformed": malformed,
    }
    s3.put_object(Bucket=BUCKET, Key=f"prepared/{job['name']}.json",
                  Body=json.dumps(summary, indent=2) + "\n")
    log(f"{positions} positions in {key}")


def build_bullet_utils(commit):
    run("git", "clone", "--filter=blob:none", "https://github.com/jw1912/bullet.git")
    run("git", "checkout", commit, cwd="bullet")
    run(CARGO, "build", "--release", "--package", "bullet-utils", cwd="bullet",
        env=dict(os.environ, HOME="/root"))
    return WORK / "bullet/target/release/bullet-utils"


def exists(s3, key):
    from botocore.exceptions import ClientError

    try:
        s3.head_object(Bucket=BUCKET, Key=key)
        return True
    except ClientError as error:
        if error.response["Error"]["Code"] in ("404", "NoSuchKey"):
            return False
        raise


def download_if_there(s3, key, path):
    from botocore.exceptions import ClientError

    try:
        s3.download_file(BUCKET, key, str(path))
        return True
    except ClientError as error:
        if error.response["Error"]["Code"] in ("404", "NoSuchKey"):
            return False
        raise


# the way out uses the CLI of the image, which works even when boto3 didn't install


def stop_the_group(group, region):
    if group:
        subprocess.run(["aws", "autoscaling", "set-desired-capacity", "--region", region,
                        "--auto-scaling-group-name", group, "--desired-capacity", "0"])


def upload_log(instance):
    subprocess.run(["aws", "s3", "cp", str(LOG), f"s3://{BUCKET}/logs/{instance}.log"],
                   stdout=subprocess.DEVNULL)


def imds(path):
    """A value of the instance metadata, with a token of IMDSv2"""
    url = "http://169.254.169.254/latest"
    token_request = urllib.request.Request(
        f"{url}/api/token", method="PUT",
        headers={"X-aws-ec2-metadata-token-ttl-seconds": "60"},
    )
    token = urllib.request.urlopen(token_request, timeout=5).read().decode()
    request = urllib.request.Request(
        f"{url}/meta-data/{path}", headers={"X-aws-ec2-metadata-token": token}
    )
    return urllib.request.urlopen(request, timeout=5).read().decode()


def tag(key):
    """A tag of the worker, which the group may give it a little after the start"""
    for _ in range(12):
        try:
            return imds(f"tags/instance/{key}")
        except urllib.error.HTTPError as error:
            if error.code != 404:
                raise
            time.sleep(10)
    raise RuntimeError(f"no {key} tag")


def run(*command, **options):
    """A command, its output to the log, failing if it fails"""
    log(" ".join(map(str, command)))
    with LOG.open("a") as out:
        subprocess.run(command, stdout=out, stderr=subprocess.STDOUT, check=True, **options)


def log(message):
    line = f"{datetime.datetime.now():%Y-%m-%d %H:%M:%S} {message}"
    print(line, flush=True)
    with LOG.open("a") as out:
        out.write(line + "\n")


if __name__ == "__main__":
    main()
