#!/usr/bin/env bash
# Plays two versions of the engine against each other with fastchess and reports
# the Elo of <engine> relative to <baseline>.
#
#   scripts/match.sh [options] <engine> <baseline> [-- <fastchess args>...]
#
# <engine> and <baseline> are versions built by scripts/build-engine.sh (v0.1.0
# means engines/runner-chess-v0.1.0) or paths to any UCI engine. Every opening
# from the book is played twice, once with each color. The engines' own books are
# turned off, so that only these openings get played. A game ends as a win once both
# engines have seen it past 600 centipawns for 3 moves; draws are always played out.
#
# Options:
#   --tc <tc>           time control, in seconds plus increment (default: 10+0.1)
#   --depth <n>         a fixed depth instead of a clock
#   --rounds <n>        pairs of games to play (default: 200, or 5000 with an SPRT)
#   --sprt              stop as soon as <engine> is shown to be 10 Elo better than
#                       <baseline>, or not better at all
#   --non-regression    for changes not meant to gain, like simplifications: stop
#                       as soon as <engine> is shown to be no worse than <baseline>,
#                       or 10 Elo worse. Refactors that keep the bench node count
#                       don't need a match at all
#                       (both measure fastchess's normalized Elo)
#   --concurrency <n>   games played at the same time (default: half the CPUs)
#   --book <book>       a .pgn or .epd of openings, by path or by name in
#                       engines/books (default: 8moves_v3.pgn). A shorter one, like
#                       2moves_v2.pgn, leaves the opening to the engines' clocks
#   --no-adjudication   play every game to its end, for changes to how games get won
set -euo pipefail

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
    exit "${1:-0}"
}

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
engines="$repo/engines"
default_book="$engines/books/8moves_v3.pgn"
book=$default_book
book_url="https://github.com/official-stockfish/books/raw/master/8moves_v3.pgn.zip"

limit="tc=10+0.1"
rounds=""
sprt=""
resign="movecount=3 score=600 twosided=true"
concurrency=$(($(getconf _NPROCESSORS_ONLN) / 2))
if ((concurrency < 1)); then
    concurrency=1
fi

versions=()
while [[ $# -gt 0 ]]; do
    case $1 in
    -h | --help) usage ;;
    --tc) limit="tc=${2:?--tc needs a time control}" && shift 2 ;;
    --depth) limit="depth=${2:?--depth needs a depth}" && shift 2 ;;
    --rounds) rounds=${2:?--rounds needs a number} && shift 2 ;;
    --sprt) sprt="elo0=0 elo1=10 alpha=0.05 beta=0.05" && shift ;;
    --non-regression) sprt="elo0=-10 elo1=0 alpha=0.05 beta=0.05" && shift ;;
    --concurrency) concurrency=${2:?--concurrency needs a number} && shift 2 ;;
    --book) book=${2:?--book needs a book} && shift 2 ;;
    --no-adjudication) resign="" && shift ;;
    --) shift && break ;;
    -*) echo "unknown option $1" >&2 && usage 1 ;;
    *) versions+=("$1") && shift ;;
    esac
done
if [[ ${#versions[@]} -ne 2 ]]; then
    usage 1
fi
if [[ -z $rounds ]]; then
    rounds=$([[ -n $sprt ]] && echo 5000 || echo 200)
fi

if ! command -v fastchess >/dev/null; then
    echo "fastchess not found, see https://github.com/Disservin/fastchess" >&2
    exit 1
fi

# a version name or a path, as the command fastchess runs
engine_cmd() {
    local cmd=$1
    if [[ $cmd != */* ]]; then
        cmd="$engines/runner-chess-$1"
    fi
    if [[ ! -x $cmd ]]; then
        echo "no engine at $cmd, build it with scripts/build-engine.sh $1" >&2
        exit 1
    fi
    echo "$cmd"
}
engine=$(engine_cmd "${versions[0]}")
baseline=$(engine_cmd "${versions[1]}")
engine_name=$(basename "${versions[0]}")
baseline_name=$(basename "${versions[1]}")

if [[ $book != */* ]]; then
    book="$engines/books/$book"
fi
if [[ ! -f $book && $book != "$default_book" ]]; then
    echo "no book at $book" >&2
    exit 1
fi
if [[ ! -f $book ]]; then
    echo "downloading the opening book to $book"
    download_dir=$(mktemp -d)
    trap 'rm -rf "$download_dir"' EXIT
    curl -sSfL -o "$download_dir/book.zip" "$book_url"
    unzip -q "$download_dir/book.zip" -d "$download_dir"
    mkdir -p "$(dirname "$book")"
    mv "$download_dir/8moves_v3.pgn" "$book"
fi

mkdir -p "$engines/games"
pgn="$engines/games/$engine_name-vs-$baseline_name-$(date +%Y%m%d-%H%M%S).pgn"
# fastchess autosaves the match there every 20 games, instead of config.json in
# the current directory
state="${pgn%.pgn}.json"
log="${pgn%.pgn}.log"
echo "games saved to $pgn, and this output to $log"
echo "if interrupted, resume with: fastchess -config file=$state"

# word splitting of $sprt and $resign is on purpose: they hold several fastchess arguments
# shellcheck disable=SC2086
fastchess \
    -engine cmd="$engine" name="$engine_name" \
    -engine cmd="$baseline" name="$baseline_name" \
    -each "$limit" option.OwnBook=false \
    -openings file="$book" format="${book##*.}" order=random \
    -rounds "$rounds" -games 2 -repeat -concurrency "$concurrency" \
    ${sprt:+-sprt $sprt} \
    ${resign:+-resign $resign} \
    -pgnout file="$pgn" \
    -config outname="$state" \
    "$@" | tee "$log"
