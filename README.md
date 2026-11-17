# runner-chess

A UCI chess engine written in Rust, with no dependencies. It evaluates positions with
an NNUE network trained on its own games, and the network is embedded in the binary.

## Building

Needs Rust 1.88 or newer (edition 2024).

```
cargo build --release
```

The binary is `target/release/runner-chess`. To build it for the CPU it runs on:

```
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

A binary built this way can crash on other CPUs.

## Usage

The engine speaks [UCI](https://www.wbec-ridderkerk.nl/html/UCIProtocol.html), so it
works with any UCI GUI (Cute Chess, Arena, Banksia) or match runner (fastchess,
cutechess-cli). It is single-threaded.

| Option     | Default   | What it does |
|------------|-----------|--------------|
| `Hash`     | 64        | Size of the transposition table, in MB (1 to 65536) |
| `Ponder`   | false     | Lets the GUI send `go ponder` to think on the opponent's time |
| `OwnBook`  | false     | Plays moves from the engine's opening book while it has them |
| `BookFile` | `<empty>` | A Polyglot book to use instead of the built-in one |
| `Variety`  | 0         | In centipawns: picks at random among the moves this close to the best one. 0 always plays the best |
| `EvalNoise`| 0         | In centipawns (0 to 50): random noise added to the evaluation, so that games don't repeat |

Rating lists and engine matches should leave `OwnBook`, `Variety` and `EvalNoise` at
their defaults.

Besides the UCI loop, the binary has a few commands of its own:

```
runner-chess bench [search [depth]]   # a benchmark; the search one prints a node count
runner-chess datagen <dir> [options]  # self-play games as training data
runner-chess book <lines> <out.bin>   # writes a Polyglot book from lines of moves
```

From UCI, `go perft <depth>` prints a perft divide.

## How it works

**Board:** bitboards, with legality checked per move against pins and checks worked out
once per node.

**Search:**
- iterative deepening with aspiration windows, and principal variation search
- a transposition table of two-entry buckets, which ages out older searches
- quiescence search on the captures that don't lose material (SEE)
- null move pruning, reverse futility pruning, futility and late move pruning, and
  SEE pruning
- late move reductions adjusted by history, and internal iterative reduction
- staged move ordering: the table move, good captures by MVV-LVA, killers, history,
  then the losing captures
- time management that stops earlier the larger the share of nodes the best move takes
- pondering

**Evaluation:** an NNUE network, `(768x4hm -> 256)x2 -> 1x8`, with four king buckets
mirrored to files a to d, and 8 output buckets. Training uses
[bullet](https://github.com/jw1912/bullet), on positions from the engine's own games
(`runner-chess datagen`). See [`docs/nnue.md`](docs/nnue.md).

## Development

- `cargo test --release` runs the tests.
- `scripts/build-engine.sh <tag>` builds a tagged version into `engines/`, and
  `scripts/match.sh` plays two versions against each other with fastchess, with SPRT
  support.
- Every change to the search has to keep the node count of `runner-chess bench search`,
  unless the change is meant to alter the search, in which case it needs a match.
- `trainer/` trains the networks with bullet, and `infra/` runs data generation,
  training data preparation and SPRT matches on AWS (see
  [`infra/README.md`](infra/README.md)).
- [`CHANGELOG.md`](CHANGELOG.md) records every version with its bench, the strength
  measured, and the ideas that were tried and dropped.

## License

runner-chess is free software, under the [GNU General Public License v3.0](LICENSE)
or any later version.
