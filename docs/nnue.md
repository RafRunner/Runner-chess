# Road to NNUE

How the training data gets generated, and the format bullet expects.

## 1. Before generating data (done)

| Step | Version | Measured |
|---|---|---|
| History and killers kept between searches, with aging | 0.8.1 | +25 ± 15 Elo for 0.8.1 against 0.8.0, together with the next two |
| `generate_captures` for the quiescence | 0.8.1 | 8% less time to the same depth |
| Promotions ordered by the material they win | 0.8.1 | 0.6% fewer nodes in the bench |
| A soft node limit in `Budget` | after 0.8.1 | - |
| `go infinite` | after 0.8.1 | - |

- **Soft node limit** (`Budget::with_soft_nodes`): no new iteration once it is reached,
  so the score always comes from a complete one (`Searcher::score`). The hard limit
  (`Budget::with_nodes`, and `go nodes`) stops in the middle of one.
- **`go infinite`**: searches up to `MAX_PLY` with no deadline, and writes `bestmove`
  only after `stop`, even when the search ends earlier. A `go`, `ucinewgame` or `bench`
  that comes without the `stop` ends it, since waiting would hang.

## 2. Data generation

```
runner-chess datagen <dir> [--threads <n>] [--positions <n>] [--nodes <n>] [--seed <n>]
                      [--book <epd>]
```

| Option | Default |
|---|---|
| `--threads` | every CPU |
| `--positions` | 1,000,000, between all threads; each one stops starting games once they get there |
| `--nodes` | 5,000, the soft limit of every search, with a hard one 10 times higher |
| `--seed` | from the clock, printed at the start |
| `--book` | none: the games start from random openings |

The games are played inside the process, without UCI, and written to
`<dir>/<version>-<seed>-<thread>.txt`. The files of another run never get written over:
a seed already used fails at the start. Searches are limited by nodes only, so the same
seed and number of threads play the same games again.

- **Threads**: each with its own table (16 MB) and ordering, both cleared before every
  game, and its own random numbers (splitmix64), seeded from `--seed`.
- **Openings**: 8 or 9 random legal moves from the starting position, so that either
  side can be the first to move after them. With `--book`, 1 or 2 random moves from a
  random position of the book instead, since a game from a book position the book
  also has a few moves later would play the same moves. A game whose first search
  finds `|score| > 400` gets thrown away.
- **Moves**: the best move of the search; the score of the last iteration that
  finished labels the position.
- **Positions kept**, after the random moves:
  - not when the side to move is in check;
  - not when the best move is a capture or a promotion, since the network should learn
    quiet positions;
  - not with mate scores.
- **End of a game**:
  - by the rules: mate, stalemate, the third repetition, the fifty-move rule, and
    positions where no sequence of moves can end in mate;
  - adjudicated: a win once one side is ahead by 2000 or more for 4 plies in a row, a
    draw after move 40 once `|score| <= 10` for 8 plies in a row, and a draw at 1000
    plies, only so that no game goes on forever.
- **Output**: a game gets written once it ends, and its file flushed, so a run stopped
  in the middle keeps every game it finished. At most the last line can be cut, and
  bullet skips lines that don't parse.

Measured with 0.8.1 on 10 threads, 5,000 nodes: about 2,600 positions per second (9
million an hour), 89 kept per game, 65 bytes each. Results: 38% White wins, 21% draws,
41% Black wins. 100 million positions, enough for a first network, take about 11 hours
and 6.5 GB.

## 3. The bullet format

Checked in the source of `bulletformat` and `bullet-utils`.

### Text

One position per line:

```
<FEN> | <score> | <result>
```

- The line gets split on `|`; the score and the result are trimmed, and the FEN gets
  split on whitespace. So the spaces around `|` are optional: `<FEN>|35|1.0` reads the
  same. Anything after a third `|` is ignored.
- From the FEN only the pieces and the side to move are read. Castling, en passant and
  the clocks are ignored, so a full FEN does no harm. Any side other than `b` counts as
  White.
- `score`: centipawns, **from White's point of view**, an `i16`: an integer, so `35.0`
  fails. It isn't clamped, which is why mate scores have to be filtered out.
- `result`: **from White's point of view**: `1.0` for a White win, `0.5` for a draw, `0.0`
  for a Black win. `1`, `1/2`, `0`, `[1.0]`, `[0.5]` and `[0.0]` are accepted too, and
  nothing else: `1-0` or `1.00` fail.
- Lines that don't parse get printed and skipped.

Example, for a position with White to move, 35 centipawns better for White, in a game
White went on to win:

```
r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3 | 35 | 1.0
```

### Binary (`ChessBoard`, "bulletformat")

32 bytes per position: occupancy (`u64`), packed pieces (16 bytes), score (`i16`), result
(`u8`), both king squares and padding. Positions get stored from the side to move's
point of view: with Black to move, the board is flipped, the score negated and the
result inverted. What was the side to move gets lost, which is fine for small networks.
100 million positions take 3.2 GB.

### `bullet-utils`

Built in the bullet repository with `cargo b -r --package bullet-utils`.

```
bullet-utils convert --from text --input data.txt --output data.bin
bullet-utils shuffle --input data.bin --output shuffled.bin --mem-used-mb 8192
bullet-utils interleave a.bin b.bin --output all.bin
```

- `convert`: `-f/--from` (`text`, `marlinformat`, `ataxx`), `-i/--input`, `-o/--output`,
  `-t/--threads` (only for the binary formats).
- `shuffle`: `-i`, `-o` and the memory to use. A larger file gets shuffled in chunks.
- `interleave`: two or more inputs and `-o`.
- `validate` checks a file. The flag names come from the argument parser, so
  `bullet-utils help` has the final word.

The bullet docs suggest, in order: generate, convert, shuffle each file, then
interleave them. They also suggest storing data as viriformat binpacks, which keep
whole games, and converting to bulletformat only when loading speed matters. Text is
the simplest way to start.

## 4. Training

bullet lives next to this repository, in `../bullet`, where `bullet-utils` gets built
with `cargo b -r --package bullet-utils`. The trainer, in `trainer/`, is a project of
its own, so the engine stays without dependencies, and pins bullet to a commit.

```
scripts/prepare-data.sh <name> <.txt files or directories>...   # data/<name>.data
cd trainer && cargo r -r -- ../data/<name>.data <net id> [superbatches] [wdl]
```

- **Network**: `(768 -> 128)x2 -> 1` with SCReLU, as bullet's `examples/simple`: the
  weights of the hidden layer quantised by `QA = 255`, those of the output by
  `QB = 64`, and an eval scale of 400.
- **Schedule**: every superbatch is one pass over the data, 30 by default, with the
  learning rate decaying as a cosine from 0.001 to 0.001 x 0.3^5. The target is
  `wdl x result + (1 - wdl) x sigmoid(score / 400)`, with `wdl = 0.75` by default.
- **Output**: `trainer/checkpoints/<net id>-<superbatch>/quantised.bin`, the network the
  engine embeds, 197,440 bytes for 128 neurons: bullet pads it to a multiple of 64
  bytes, the size of the network's struct with its `align(64)` columns. The one the
  engine uses gets copied to `nets/<net id>.bin` and committed, since
  `trainer/checkpoints` is ignored. At the end,
  `<net id>-evals.txt` has the evaluation of a few positions, in `f32`, for the
  engine's inference to be compared with.
- **Backend**: Metal by default, `--no-default-features --features cuda` on NVIDIA.
- bullet doesn't implement a validation set yet: it prints a warning when given one.
  `cargo r -r --bin validate -- checkpoints/<net id>-30 ../data/<name>.data` gives the
  loss of a network on any data, and how many times bigger its scores are than the
  data's. On the network's own training data, that's the inflation the engine divides
  its output by (`NETWORK_INFLATION`).

On an M4 Pro, training goes at about 4.6 million positions a second: 30 passes over 10
million positions take 70 seconds.

### Which data

Every network trains on the games of the current version and of the two before it,
each generation half from random openings and half from `popularpos_lichess_v3.epd`,
in `data/<version>-random/` and `data/<version>-book/`. For net-005:

```
scripts/prepare-data.sh all-90m data/0.10.0-random data/0.10.0-book \
    data/0.9.2-random data/0.9.2-book data/0.9.1
```

The labels of the older generations come from shallower searches of weaker versions,
but more positions still helped: 60 million positions of 0.10.0 and 0.9.2 beat 30
million of 0.10.0 alone by 83 Elo, and adding the 30 million of 0.9.1 was better still.
Older games, like those of 0.8.2 with its hand-written evaluation, stay out. Neither
the book nor the Syzygy tables go further than that: half the games from the book
gained 28 Elo over random openings only, while endgames relabelled with the tables
lost 10, the network learning wins of perfect play that the engine can't convert.

### `net-001`

10 million positions of 0.8.2, 30 passes. On 100 positions it didn't train on, against
Stockfish searching to depth 12: a correlation of 0.89, with the sign right in 92% of
the positions, as a static evaluation, against 0.90 and 96% for a search of 0.8.2 of
5000 nodes. With `wdl = 0.75` its output is about twice the scores it learned from,
since it mostly predicts the result of the game: the engine can divide it back by
using a scale of 200 instead of 400.

## 5. In the engine

- **Inputs**: for each side's view, `(own piece ? 0 : 384) + 64 x kind + square`, with
  the kinds in the order P N B R Q K and the square from a1 = 0, flipped (`^ 56`) for
  Black's view.
- **Accumulator**: for each view, the bias plus the column of every input on the board.
- **Output**, for the side to move:
  `(sum(screlu(us) x w[..128]) + sum(screlu(them) x w[128..])) / QA + bias`, times the
  scale, divided by `QA x QB`, where `screlu(x) = clamp(x, 0, QA)^2`.
- The weights embedded with `include_bytes!` in a `static`, read-only and shared by
  every thread.
- A stack of accumulators, one pair per ply, in the `Searcher`, never in `Board`, which
  gets copied on every move, and on the heap, so that moving the `Searcher` stays cheap.
- What a move changes comes from the parent and the move, next to `push_hash`, so only
  the moves searched pay for it. Comparing it with the difference of the piece
  bitboards in a `debug_assert` catches mistakes in castling, en passant and
  promotions.
- Before trusting it: incremental updates equal to a refresh along perft trees, the
  same evaluation for a position and its colors swapped, and the evaluations of
  `<net id>-evals.txt` within the quantisation error.

## Sources

- bullet: https://github.com/jw1912/bullet, its docs on
  [training data](https://github.com/jw1912/bullet/blob/main/docs/3-data.md) and
  [getting started](https://github.com/jw1912/bullet/blob/main/docs/2-getting-started.md)
- The text format and `ChessBoard`: https://github.com/jw1912/bulletformat
  (`src/chess.rs`)
- `bullet-utils`: `crates/utils/src/{convert,shuffle,interleave}.rs` in the bullet
  repository
