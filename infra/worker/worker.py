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
  reclaimed one resumes it.

The engines are built for the CPU of the worker, from the sources the job names. A job
that is stopped or past its deadline empties the group, and so does any failure, since
the worker that would replace this one would fail the same way.

The value in @ comes from lib/runner-chess-stack.ts.
"""
import datetime
import hashlib
import json
import os
import re
import subprocess
import tarfile
import traceback
import urllib.request
from pathlib import Path

BUCKET = "@BUCKET@"
WORK = Path("/opt/worker")
LOG = WORK / "worker.log"
# the commit of the fastchess the SPRTs of the repository played with, 1.8.2
FASTCHESS = "f618e34"
CARGO = "/root/.cargo/bin/cargo"


class Stopped(Exception):
    """The job was stopped, is past its deadline, or isn't the one this worker plays"""


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    os.chdir(WORK)
    instance = region = group = None
    try:
        instance = imds("instance-id")
        region = imds("placement/region")
        run("dnf", "install", "-y", "gcc", "gcc-c++", "make", "git", "zstd", "python3-boto3")
        import boto3

        s3 = boto3.client("s3", region_name=region)
        autoscaling = boto3.client("autoscaling", region_name=region)
        found = autoscaling.describe_auto_scaling_instances(InstanceIds=[instance])
        group = found["AutoScalingInstances"][0]["AutoScalingGroupName"]

        run("sh", "-c", "curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs"
            " | sh -s -- -y --profile minimal", env=dict(os.environ, HOME="/root"))
        job = read_job(s3)
        if job["kind"] == "datagen":
            datagen(s3, job, instance)
        elif job["kind"] == "sprt":
            sprt(s3, job)
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
    until = datetime.datetime.fromisoformat(job["until"].replace("Z", "+00:00"))
    if job.get("stop") or datetime.datetime.now(datetime.timezone.utc) >= until:
        raise Stopped(f"the job {job['name']} is stopped or past its deadline")
    if playing and (job["kind"], job["name"]) != (playing["kind"], playing["name"]):
        raise Stopped(f"the job is now {job['name']}, not {playing['name']}")
    return job


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


def fetch_book(s3, name):
    book = WORK / "books" / name
    if not book.exists():
        book.parent.mkdir(exist_ok=True)
        s3.download_file(BUCKET, f"books/{name}", str(book))
    return book


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

        options = ["--book", fetch_book(s3, part["book"])] if "book" in part else []
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
    total = 0
    for page in s3.get_paginator("list_objects_v2").paginate(Bucket=BUCKET, Prefix=prefix):
        for chunk in page.get("Contents", []):
            found = re.search(r"-(\d+)\.txt\.zst$", chunk["Key"])
            total += int(found.group(1)) if found else 0
    return total


# sprt


def sprt(s3, job):
    """The match of the job, from where the state in the bucket left it, if anywhere"""
    prefix = f"sprt/{job['name']}"
    engine = build(s3, job["engine"]["sourceKey"], "engine")
    baseline = build(s3, job["baseline"]["sourceKey"], "baseline")
    fastchess = build_fastchess()
    book = fetch_book(s3, job["book"])

    match = WORK / "sprt"
    match.mkdir(exist_ok=True)
    state, games, out = match / "state.json", match / "games.pgn", match / "fastchess.log"
    # the paths of a resumed match are those it started with, the same on every worker
    if download_if_there(s3, f"{prefix}/state.json", state):
        download_if_there(s3, f"{prefix}/games.pgn", games)
        download_if_there(s3, f"{prefix}/fastchess.log", out)
        log(f"resuming {prefix} from its state")
        command = [fastchess, "-config", f"file={state}", f"outname={state}"]
    else:
        tc = scaled_tc(job["tc"], job["referenceNps"], bench(baseline, job["npsDepth"])[1])
        log(f"{job['tc']} at the speed of the reference is {tc} here")
        low, high = job["bounds"]
        command = [
            fastchess,
            "-engine", f"cmd={engine}", f"name={job['engine']['name']}",
            "-engine", f"cmd={baseline}", f"name={job['baseline']['name']}",
            "-each", f"tc={tc}", "option.OwnBook=false",
            "-openings", f"file={book}", "format=pgn", "order=random",
            "-rounds", "5000", "-games", "2", "-repeat",
            # a core for fastchess and the system
            "-concurrency", str(os.cpu_count() - 1),
            "-sprt", f"elo0={low}", f"elo1={high}", "alpha=0.05", "beta=0.05",
            "-pgnout", f"file={games}",
            "-config", f"outname={state}",
            "-autosaveinterval", "10",
        ]

    def upload():
        for path in (state, games, out):
            if path.exists():
                s3.upload_file(str(path), BUCKET, f"{prefix}/{path.name}")

    with out.open("a") as output:
        log(" ".join(map(str, command)))
        process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT)
        while True:
            try:
                process.wait(timeout=60)
                break
            except subprocess.TimeoutExpired:
                upload()
                try:
                    read_job(s3, playing=job)
                except Stopped:
                    process.terminate()
                    process.wait()
                    upload()
                    raise
    upload()
    if process.returncode != 0:
        raise RuntimeError(f"fastchess ended with {process.returncode}")

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
