#!/bin/bash
# The user data of every instance of the benchmark: builds the engine for its own CPU,
# runs the search bench, plays datagen on every vCPU for @SECONDS@ seconds, leaves the
# results in @RESULTS@/<run>/<instance type>/ and terminates the instance.
set -euo pipefail

# whatever goes wrong, the instance terminates
shutdown -h +45

token=$(curl -sX PUT http://169.254.169.254/latest/api/token \
    -H 'X-aws-ec2-metadata-token-ttl-seconds: 300')
type=$(curl -s -H "X-aws-ec2-metadata-token: $token" \
    http://169.254.169.254/latest/meta-data/instance-type)
# the tag that bench/run.ts gives every instance it launches together
run=$(curl -s -H "X-aws-ec2-metadata-token: $token" \
    http://169.254.169.254/latest/meta-data/tags/instance/Run)
out="@RESULTS@/$run/$type"

work=/opt/bench
mkdir -p "$work"
cd "$work"
exec > >(tee "$work/instance.log") 2>&1
# the log goes up even when something failed, and the instance goes away
trap 'aws s3 cp "$work/instance.log" "$out/instance.log"; shutdown -h now' EXIT
set -x

dnf install -y gcc
export HOME=/root
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"

aws s3 cp "@SOURCE@" source.tar.gz
mkdir src
tar -xzf source.tar.gz -C src
(cd src && RUSTFLAGS='-C target-cpu=native' cargo build --release)

lscpu >lscpu.txt
src/target/release/runner-chess bench search >bench.txt
# timeout ends it with 124: a stopped run keeps every game it finished
timeout @SECONDS@ src/target/release/runner-chess datagen games \
    --threads "$(nproc)" --positions 1000000000 --seed 1 >datagen.txt || [[ $? == 124 ]]

for file in lscpu.txt bench.txt datagen.txt; do
    aws s3 cp "$file" "$out/$file"
done
echo done | aws s3 cp - "$out/done"
