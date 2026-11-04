#!/usr/bin/python3
"""The user data of every datagen worker, on Amazon Linux 2023 and its Python 3.9.

Builds the engine for its own CPU, checks that it searches the tree of the ref, then
plays chunks of the job in s3://BUCKET/job.json until every part has its positions,
and leaves the group. Each chunk goes to s3://BUCKET/<job>/<part>/<seed>-<positions>.txt.zst
once played: a worker reclaimed in the middle loses only the one it was playing, and
the group replaces it. Any failure empties the group, since the worker that would
replace this one would fail the same way.

The values in @ come from lib/datagen-stack.ts.
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
SOURCE = "@SOURCE@"
BENCH = int("@BENCH@")

WORK = Path("/opt/datagen")
LOG = WORK / "worker.log"


class Stopped(Exception):
    """The job was stopped, or is past its deadline"""


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    os.chdir(WORK)
    instance = region = group = None
    try:
        instance = imds("instance-id")
        region = imds("placement/region")
        run("dnf", "install", "-y", "gcc", "zstd", "python3-boto3")
        import boto3

        s3 = boto3.client("s3", region_name=region)
        autoscaling = boto3.client("autoscaling", region_name=region)
        found = autoscaling.describe_auto_scaling_instances(InstanceIds=[instance])
        group = found["AutoScalingInstances"][0]["AutoScalingGroupName"]

        engine = build(s3)
        play(engine, s3, instance)
        log("every part has its positions: one worker fewer")
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


def build(s3):
    """The engine of the ref, built with `-C target-cpu=native`"""
    env = dict(os.environ, HOME="/root")
    run("sh", "-c", "curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs"
        " | sh -s -- -y --profile minimal", env=env)

    bucket, key = SOURCE.removeprefix("s3://").split("/", 1)
    s3.download_file(bucket, key, "source.tar.gz")
    with tarfile.open("source.tar.gz") as source:
        source.extractall("src")
    run("/root/.cargo/bin/cargo", "build", "--release", cwd="src",
        env=dict(env, RUSTFLAGS="-C target-cpu=native"))

    engine = WORK / "src/target/release/runner-chess"
    bench = subprocess.run([engine, "bench", "search"], capture_output=True, text=True, check=True)
    nodes = int(re.search(r"Nodes searched\s*:\s*(\d+)", bench.stdout).group(1))
    if nodes != BENCH:
        raise RuntimeError(f"bench search counted {nodes} nodes, not {BENCH}")
    log(f"bench search: {nodes} nodes, as expected")
    return engine


def play(engine, s3, instance):
    """Chunks until every part of the job has its positions"""
    chunk = 0
    while True:
        # read again before every chunk, so that a job can change or stop while it runs
        job = json.loads(s3.get_object(Bucket=BUCKET, Key="job.json")["Body"].read())
        until = datetime.datetime.fromisoformat(job["until"].replace("Z", "+00:00"))
        if job.get("stop") or datetime.datetime.now(datetime.timezone.utc) >= until:
            raise Stopped("the job is stopped or past its deadline")

        part = next(
            (p for p in job["parts"] if played(s3, f"{job['name']}/{p['name']}/") < p["positions"]),
            None,
        )
        if part is None:
            return

        options = []
        if "book" in part:
            book = WORK / "books" / part["book"]
            if not book.exists():
                book.parent.mkdir(exist_ok=True)
                s3.download_file(BUCKET, f"books/{part['book']}", str(book))
            options = ["--book", book]

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
        key = f"{job['name']}/{part['name']}/{seed}-{positions}.txt.zst"
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


# The way out uses the CLI of the image, which works even when boto3 didn't install


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
