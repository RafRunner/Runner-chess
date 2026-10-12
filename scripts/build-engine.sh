#!/usr/bin/env bash
# Builds a version of the engine into engines/, to be compared with scripts/match.sh.
#
#   scripts/build-engine.sh <tag> [<commit>]
#       Builds the version tagged <tag> as engines/runner-chess-<tag>. If the tag
#       doesn't exist yet, it is created first, on <commit> (default: HEAD).
#       Only committed code is built, whatever the working tree has.
#
#   scripts/build-engine.sh --dev [<name>]
#       Builds the working tree, uncommitted changes included, as
#       engines/runner-chess-<name> (default: dev), without tagging anything.
set -euo pipefail

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
    exit "${1:-0}"
}

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
engines="$repo/engines"
mkdir -p "$engines"

case "${1:-}" in
"" | -h | --help)
    usage
    ;;
--dev)
    name=${2:-dev}
    cargo build --release --manifest-path "$repo/Cargo.toml"
    cp "$repo/target/release/runner-chess" "$engines/runner-chess-$name"
    ;;
-*)
    echo "unknown option $1" >&2
    usage 1
    ;;
*)
    name=$1
    commit=${2:-HEAD}

    if git -C "$repo" rev-parse -q --verify "refs/tags/$name" >/dev/null; then
        tagged=$(git -C "$repo" rev-parse "$name^{commit}")
        wanted=$(git -C "$repo" rev-parse "$commit^{commit}")
        if [[ $# -ge 2 && $tagged != "$wanted" ]]; then
            echo "tag $name already exists, on $(git -C "$repo" rev-parse --short "$tagged")" >&2
            exit 1
        fi
    else
        git -C "$repo" tag "$name" "$commit"
        echo "created tag $name on $(git -C "$repo" rev-parse --short "$name")"

        if [[ $commit == HEAD && -n $(git -C "$repo" status --porcelain --untracked-files=no) ]]; then
            echo "note: uncommitted changes are not part of $name" >&2
        fi
    fi

    # a clean copy of the tagged code, so the working tree doesn't leak into the build
    build_dir=$(mktemp -d)
    trap 'rm -rf "$build_dir"' EXIT
    git -C "$repo" archive "$name" | tar -x -C "$build_dir"
    cargo build --release --manifest-path "$build_dir/Cargo.toml"
    cp "$build_dir/target/release/runner-chess" "$engines/runner-chess-$name"
    ;;
esac

echo "built engines/runner-chess-$name"
