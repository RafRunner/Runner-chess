#!/usr/bin/env bash
# Turns what `runner-chess datagen` writes into the training data of `trainer/`,
# with bullet-utils:
#
#   scripts/prepare-data.sh <name> <text files or directories>...
#
# writes data/<name>.data, shuffled, in bullet's binary format (32 bytes a
# position), from the .txt files given and those in the directories given.
#
# bullet-utils comes from ../bullet (cargo b -r --package bullet-utils), or from
# $BULLET_UTILS.
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
utils=${BULLET_UTILS:-"$repo/../bullet/target/release/bullet-utils"}
if [[ ! -x $utils ]]; then
    echo "no bullet-utils at $utils, build it with cargo b -r --package bullet-utils" >&2
    exit 1
fi

name=$1
shift
out="$repo/data"
mkdir -p "$out"
work=$(mktemp -d "$out/.prepare-XXXXXX")
trap 'rm -rf "$work"' EXIT

# every .txt of the directories given, and the files given
files=()
for path in "$@"; do
    if [[ -d $path ]]; then
        while IFS= read -r file; do
            files+=("$file")
        done < <(find "$path" -maxdepth 1 -name '*.txt' | sort)
    else
        files+=("$path")
    fi
done
echo "${#files[@]} files"

cat "${files[@]}" >"$work/all.txt"
"$utils" convert --from text --input "$work/all.txt" --output "$work/all.data"
rm "$work/all.txt"
"$utils" shuffle --input "$work/all.data" --output "$work/shuffled.data" --mem-used-mb 4096
rm "$work/all.data"

mv "$work/shuffled.data" "$out/$name.data"
echo "$(($(wc -c <"$out/$name.data") / 32)) positions in $out/$name.data"
