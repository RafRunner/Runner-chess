#!/usr/bin/env bash
# Trains a network with trainer/, and keeps what it prints next to its checkpoints:
#
#   scripts/train.sh <data> <net id> [superbatches] [wdl]
#
# runs `cargo r -r -- <data> <net id> [superbatches] [wdl]` in trainer/, writing its
# output to trainer/checkpoints/<net id>.log as well.
set -euo pipefail

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
    exit "${1:-0}"
}

if [[ ${1:-} == -h || ${1:-} == --help ]]; then
    usage
fi
if [[ $# -lt 2 ]]; then
    usage 1
fi

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
# the trainer runs from its own directory
data=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
net_id=$2
shift 2

mkdir -p "$repo/trainer/checkpoints"
log="$repo/trainer/checkpoints/$net_id.log"
echo "output also in $log"
cd "$repo/trainer"
cargo r -r --quiet -- "$data" "$net_id" "$@" | tee "$log"
