#!/usr/bin/python3
"""The user data of every instance of the benchmark, on Amazon Linux 2023 and its
Python 3.9.

Builds the engine of its Source tag, a key in the bucket, for its own CPU, runs the
search bench, plays datagen on every vCPU for SECONDS seconds, leaves the results in
s3://BUCKET/bench/<Run tag>/<instance type>/ and terminates the instance, after 45
minutes at most whatever happens.

The value in @ comes from lib/runner-chess-stack.ts.
"""
import datetime
import os
import subprocess
import tarfile
import traceback
import urllib.request
from pathlib import Path

BUCKET = "@BUCKET@"
SECONDS = 300

WORK = Path("/opt/bench")
LOG = WORK / "instance.log"


def main():
    subprocess.run(["shutdown", "-h", "+45"])
    WORK.mkdir(parents=True, exist_ok=True)
    os.chdir(WORK)
    out = None
    try:
        # the tag that bench/run.ts gives every instance it launches together
        out = f"bench/{imds('tags/instance/Run')}/{imds('instance-type')}"
        run("dnf", "install", "-y", "gcc", "python3-boto3")
        import boto3

        s3 = boto3.client("s3", region_name=imds("placement/region"))
        engine = build(s3, imds("tags/instance/Source"))

        run("lscpu", output="lscpu.txt")
        run(engine, "bench", "search", output="bench.txt")
        try:
            run(engine, "datagen", "games", "--threads", str(os.cpu_count()),
                "--positions", "1000000000", "--seed", "1",
                output="datagen.txt", timeout=SECONDS)
        except subprocess.TimeoutExpired:
            log(f"datagen stopped after {SECONDS} s")  # every game it finished is kept

        for name in ["lscpu.txt", "bench.txt", "datagen.txt"]:
            s3.upload_file(name, BUCKET, f"{out}/{name}")
        s3.put_object(Bucket=BUCKET, Key=f"{out}/done", Body=b"done\n")
    except Exception:
        log(traceback.format_exc())
    finally:
        # with the CLI of the image, which works even when boto3 didn't install
        if out:
            subprocess.run(["aws", "s3", "cp", str(LOG), f"s3://{BUCKET}/{out}/instance.log"])
        subprocess.run(["shutdown", "-h", "now"])


def build(s3, key):
    """The engine of the sources at `key`, built with `-C target-cpu=native`"""
    env = dict(os.environ, HOME="/root")
    run("sh", "-c", "curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs"
        " | sh -s -- -y --profile minimal", env=env)

    s3.download_file(BUCKET, key, "source.tar.gz")
    with tarfile.open("source.tar.gz") as source:
        source.extractall("src")
    run("/root/.cargo/bin/cargo", "build", "--release", cwd="src",
        env=dict(env, RUSTFLAGS="-C target-cpu=native"))
    return WORK / "src/target/release/runner-chess"


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


def run(*command, output=None, **options):
    """A command, its output to `output` or else to the log, failing if it fails"""
    log(" ".join(map(str, command)))
    with open(output or LOG, "w" if output else "a") as out:
        subprocess.run(command, stdout=out, stderr=subprocess.STDOUT, check=True, **options)


def log(message):
    line = f"{datetime.datetime.now():%Y-%m-%d %H:%M:%S} {message}"
    print(line, flush=True)
    with LOG.open("a") as out:
        out.write(line + "\n")


if __name__ == "__main__":
    main()
