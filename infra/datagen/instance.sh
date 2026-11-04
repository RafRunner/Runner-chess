#!/bin/bash
# The user data of every datagen worker: builds the engine for its own CPU, checks that it
# searches the tree of the ref, then plays chunks of the job in s3://@BUCKET@/job.json
# until every part has its positions, and leaves the group. Each chunk goes to
# s3://@BUCKET@/<job>/<part>/<seed>-<positions>.txt.zst once played: a worker reclaimed in
# the middle loses only the one it was playing, and the group replaces it.
set -euo pipefail

imds() {
    local token
    token=$(curl -sX PUT http://169.254.169.254/latest/api/token \
        -H 'X-aws-ec2-metadata-token-ttl-seconds: 60')
    curl -s -H "X-aws-ec2-metadata-token: $token" "http://169.254.169.254/latest/meta-data/$1"
}
id=$(imds instance-id)
bucket=@BUCKET@
work=/opt/datagen
mkdir -p "$work/books"
cd "$work"
exec > >(tee "$work/instance.log") 2>&1

group=$(aws autoscaling describe-auto-scaling-instances --instance-ids "$id" \
    --query 'AutoScalingInstances[0].AutoScalingGroupName' --output text)
stop_the_group() {
    aws autoscaling set-desired-capacity --auto-scaling-group-name "$group" --desired-capacity 0
}
on_exit() {
    local status=$?
    aws s3 cp "$work/instance.log" "s3://$bucket/logs/$id.log" || true
    # a broken worker would be replaced by another as broken, again and again
    if [[ $status != 0 ]]; then
        stop_the_group || true
    fi
    shutdown -h now
}
trap on_exit EXIT
set -x

dnf install -y gcc jq zstd
export HOME=/root
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"

aws s3 cp "@SOURCE@" source.tar.gz
mkdir src
tar -xzf source.tar.gz -C src
(cd src && RUSTFLAGS='-C target-cpu=native' cargo build --release)
engine=$work/src/target/release/runner-chess
nodes=$("$engine" bench search | awk '/Nodes searched/ { print $4 }')
if [[ $nodes != @BENCH@ ]]; then
    echo "bench search counted $nodes nodes, not @BENCH@"
    exit 1
fi

# the positions of a part so far, from the names of its chunks
played() {
    aws s3api list-objects-v2 --bucket "$bucket" --prefix "$1/" --query 'Contents[].Key' \
        --output text | tr '\t' '\n' | sed -n 's/.*-\([0-9]*\)\.txt\.zst$/\1/p' |
        awk '{ sum += $1 } END { print sum + 0 }'
}

chunk=0
while true; do
    # read again before every chunk, so that a job can change or stop while it runs
    aws s3 cp "s3://$bucket/job.json" job.json
    job=$(jq -r .name job.json)
    if [[ $(jq -r '.stop // false' job.json) == true ]] ||
        (($(date +%s) >= $(date -d "$(jq -r .until job.json)" +%s))); then
        echo "the job is stopped or past its deadline"
        stop_the_group
        exit 0
    fi

    part=
    for i in $(seq 0 $(($(jq '.parts | length' job.json) - 1))); do
        name=$(jq -r ".parts[$i].name" job.json)
        if (($(played "$job/$name") < $(jq ".parts[$i].positions" job.json))); then
            part=$i
            break
        fi
    done
    if [[ -z $part ]]; then
        break
    fi

    name=$(jq -r ".parts[$part].name" job.json)
    book=$(jq -r ".parts[$part].book // empty" job.json)
    options=()
    if [[ -n $book ]]; then
        [[ -f books/$book ]] || aws s3 cp "s3://$bucket/books/$book" "books/$book"
        options=(--book "books/$book")
    fi
    chunk=$((chunk + 1))
    # 60 bits of the worker and the chunk: no two chunks play the same games
    seed=$((0x$(printf '%s-%s' "$id" "$chunk" | sha256sum | cut -c1-15)))

    rm -rf games
    "$engine" datagen games --threads "$(nproc)" --positions "$(jq .chunk job.json)" \
        --nodes "$(jq ".parts[$part].nodes" job.json)" --seed "$seed" "${options[@]}" \
        >datagen.log
    cat games/*.txt >chunk.txt
    positions=$(wc -l <chunk.txt)
    zstd -q -3 -f chunk.txt -o chunk.txt.zst
    aws s3 cp chunk.txt.zst "s3://$bucket/$job/$name/$seed-$positions.txt.zst"
    aws s3 cp "$work/instance.log" "s3://$bucket/logs/$id.log"
done

echo "every part has its positions"
aws autoscaling terminate-instance-in-auto-scaling-group --instance-id "$id" \
    --should-decrement-desired-capacity
