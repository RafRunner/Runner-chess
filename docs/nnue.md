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
                      [--openings <epd>] [--variety <cp>] [--variety-plies <n>]
```

| Option | Default |
|---|---|
| `--threads` | every CPU |
| `--positions` | 1,000,000, between all threads; each one stops starting games once they get there |
| `--nodes` | 5,000, the soft limit of every search, with a hard one 10 times higher |
| `--seed` | from the clock, printed at the start |
| `--openings` | none: the games start from random moves; an EPD file of positions to start from, as fastchess's `-openings format=epd` |
| `--variety` | none: in the first `--variety-plies` plies after the opening, one of the moves within this many centipawns of the best, at random |
| `--variety-plies` | 20 |

The games are played inside the process, without UCI, and written to
`<dir>/<version>-<seed>-<thread>.txt`. The files of another run never get written over:
a seed already used fails at the start. Searches are limited by nodes only, so the same
seed and number of threads play the same games again.

- **Threads**: each with its own table (16 MB) and ordering, both cleared before every
  game, and its own random numbers (splitmix64), seeded from `--seed`.
- **Openings**: 8 or 9 random legal moves from the starting position, so that either
  side can be the first to move after them. With `--openings`, 1 or 2 random moves from a
  random position of the file instead, since a game from a position the file also has
  a few moves later would play the same moves. A game whose first search
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
and 6.5 GB. With 1.5.0 on 4 threads: 1,350 positions a second and 99 kept per game, or
630 and 78 from the starts of attacks at 10,000 nodes.

## 3. The bullet format

Checked in the source of `bulletformat` and `bullet-utils`.

### Text

One position per line:

```
<FEN>|<score>|<result>
```

- The line gets split on `|`; the score and the result are trimmed, and the FEN gets
  split on whitespace. So spaces around `|` are optional: `datagen` writes none, 4 bytes
  less a line, but the generations before 1.8 have them, as `<FEN> | 35 | 1.0`, which
  reads the same. Anything after a third `|` is ignored.
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
r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3|35|1.0
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
scripts/train.sh data/<name>.data <net id> [superbatches] [wdl] [--king-buckets] [--wing-outputs]
```

`scripts/train.sh` runs `cargo r -r -- ../data/<name>.data <net id> [superbatches] [wdl]
[flags]` in `trainer/`, and keeps its output in `trainer/checkpoints/<net id>.log`.
net-012 came from `scripts/train.sh data/net-011.data net-012 30 0.3 --king-buckets`.

- **Network**: `(768 -> 256)x2 -> 1x8` with SCReLU, as bullet's `examples/simple`
  with the output buckets of its `progression/2_output_buckets`: 8 output layers, and
  each position uses the one of its bucket, `(pieces - 2) / 4`, from the kings alone to
  the full board. The weights of the hidden layer are quantised by `QA = 255`, those of
  the output by `QB = 64`, and the eval scale is 400. The shape is in
  `trainer/src/lib.rs`, which `validate` shares.
- **`--king-buckets`**: `(768x4hm -> 256)x2`, bullet's `ChessBucketsMirrored` as in its
  `progression/3_input_buckets`: each view mirrors the board to keep its own king on
  files a to d, and has a first layer for each of 4 regions of it, `KING_BUCKETS`:
  castled in the corner (a and b), at home in the middle (c and d), on the second rank,
  further up. A factoriser, weights shared by the 4 and added to each when saving, lets
  a bucket learn only how it differs; with it, a bucket's weights and the factoriser's
  get clipped to ±0.99, so that their sum stays in the usual ±1.98.
- **`--wing-outputs`**: 16 output buckets, the 8 of material for the kings on the same
  wing, a to d or e to h, and 8 more for kings on opposite wings. Measured neutral.
- **Schedule**: every superbatch is one pass over the data, 30 by default, with the
  learning rate decaying as a cosine from 0.001 to 0.001 x 0.3^5. The target is
  `wdl x result + (1 - wdl) x sigmoid(score / 400)`, with `wdl = 0.5` by default.
- **Output**: `trainer/checkpoints/<net id>-<superbatch>/quantised.bin`, the network the
  engine embeds, 401,984 bytes for 256 neurons and 8 buckets: bullet pads it to a
  multiple of 64 bytes, the size of the network's struct with its `align(64)` columns.
  The one the engine uses gets copied to `nets/<net id>.bin` and committed, since
  `trainer/checkpoints` is ignored. At the end, `<net id>-evals.txt` has the evaluation
  of a few positions, in `f32`, for the engine's inference to be compared with.
- **Backend**: Metal by default, `--no-default-features --features cuda` on NVIDIA.
- bullet doesn't implement a validation set yet: it prints a warning when given one.
  `cargo r -r --bin validate -- checkpoints/<net id>-30 ../data/<name>.data [flags]`
  gives the loss of a network on any data, against the result and against the score
  apart, and how many times bigger its scores are than the data's, from which the
  engine's `NETWORK_INFLATION` comes. It reads `quantised.bin`, the network the engine
  plays, since `raw.bin` lacks the factoriser; with `--fen <fen>` instead of the data,
  it gives one position's score, to check the engine's inference against.
- **Validation data**: `data/validation-1.8.0.data`, 4 million positions of 1.8.0 that
  no network trained on, played on AWS as the job `val-1.8.0` and prepared there (25%
  of attacks, against 13% in a generation: the parts were smaller than 4 workers'
  chunks). Next to the loss on 2 million positions of the network's own training data,
  it says how much a network learned by heart. `data/validation-1.2.0.data`, a million
  positions of 1.2.0, gives the scale of a network against fixed scores.

On an M4 Pro, training goes at about 4.6 million positions a second: 30 passes over 10
million positions take 70 seconds, and over the 90 million of net-009, 10 minutes.

### Which data

Every network trains on the games of the current version and of the two before it,
each generation half from random openings and half from `popularpos_lichess_v3.epd`,
in `data/<version>-random/` and `data/<version>-book/`. From 1.5.0 on, a generation
also has positions of attacks on the king, in `data/<version>-attack/`. From 1.7.0 on,
generations are played on Spot instances of AWS, with `infra/` (see its README), 90
million positions for about 2.80 dollars and 6 hours: 39 million of random openings,
39 of the book and 12 of attacks, in `infra/worker/jobs/1.7.0.json`. For net-010:

```
scripts/prepare-data.sh net-010 data/1.7.0-random data/1.7.0-book data/1.7.0-attack \
    data/1.5.0-random data/1.5.0-book data/1.5.0-attack data/1.1.1-random data/1.1.1-book
```

The text of the generations stays in the bucket of `infra/`, and a worker can prepare
the data there too, so that only the shuffled data the trainer reads comes here, 32
bytes a position, against some 3 times as much on disk to prepare it here:

```
cd infra
npm run job -- prepare net-011 1.8.0 1.7.0 1.5.0
npm run job -- fetch net-011    # data/net-011.data
```

The labels of the older generations come from shallower searches of weaker versions,
but more positions still helped: 60 million positions of 0.10.0 and 0.9.2 beat 30
million of 0.10.0 alone by 83 Elo, and adding the 30 million of 0.9.1 was better still.
Older games, like those of 0.8.2 with its hand-written evaluation, stay out. Neither
the book nor the Syzygy tables go further than that: half the games from the book
gained 28 Elo over random openings only, while endgames relabelled with the tables
lost 10, the network learning wins of perfect play that the engine can't convert.
With 90 million positions of the new generation, the older ones still help: see
net-010.

The games of attacks start with the kings castled on opposite wings, as in 1.8% of the
positions of the other games, and where the network erred the most against Stockfish.
`scripts/attack-starts.py` takes such positions from the data of the two generations
before, with both queens on, by move 25 and with `|score| <= 150`, about one a game,
and adds those of `popularpos` and the ends of the lines of `books/aggressive.txt`:
40,882 for 1.5.0, given to `datagen --openings`. Their games search 10,000 nodes a move
instead of 5,000: there, the result of the game is what corrects the network, and only
if the attacks get played well. A quarter of their positions keep the kings on
opposite wings.

### `net-001`

10 million positions of 0.8.2, 30 passes. On 100 positions it didn't train on, against
Stockfish searching to depth 12: a correlation of 0.89, with the sign right in 92% of
the positions, as a static evaluation, against 0.90 and 96% for a search of 0.8.2 of
5000 nodes. With `wdl = 0.75` its output is about twice the scores it learned from,
since it mostly predicts the result of the game: the engine can divide it back by
using a scale of 200 instead of 400.

### `net-007`

The data of net-006 on 256 neurons instead of 128. With `wdl = 0.75`, as every network
before, it lost about 60 Elo to net-006, though its loss was lower even on positions
it never saw: it predicted the results of the games better, and the scores of the
searches worse. With `wdl = 0.5` it predicts the results about as well and the scores
far better, and won by 69. On a million positions of 1.2.0 that none of them trained on:

| network | wdl | against the result | against the score |
|---|---|---|---|
| net-006, 128 | 0.75 | 0.0783 | 0.0062 |
| 256 | 0.75 | 0.0773 | 0.0074 |
| net-007, 256 | 0.5 | 0.0775 | 0.0046 |
| 256 | 0.25 | 0.0793 | 0.0037 |

### `net-008`

net-007 with 8 output buckets, on the same data: the hidden layer stays the same for
every position, and only how its neurons add up depends on the number of pieces, as a
king on e4 is weak in the middlegame and strong in an endgame. It has 2% more weights,
and costs nothing in speed. On the million positions of 1.2.0, 0.0771 against the
result and 0.0046 against the score, a little better than net-007 in both, and it won
by 24 Elo.

### `net-009`

net-008's network on the data of 0.10.0, 1.1.1 and 1.5.0 with the attacks of 1.5.0, 95
million positions: it gives 1.19 times the scores of its data, and won by 31 Elo. On
the million positions of 1.2.0, 0.0761 against the result and 0.0052 against the
score. On 2934 positions from the middle of our games, against Stockfish 19 at depth
18 and in its centipawns:

| | net-008 | net-009 |
|---|---|---|
| static evaluation | 112 | 107 |
| 5000 nodes | 94 | 91 |
| 5000 nodes, kings on opposite wings | 112 | 94 |

The same training without the attacks measured the same in all of them, and no match
was played between the two: the gain of the new generation and that of the attacks
can't be told apart. net-009's scores spread about 15% wider against Stockfish's than
net-008's, so the margins of the search, in centipawns, prune a little more.

### `net-010`

The first generation played on AWS, 90 million positions of 1.7.0, against the window
of before, three networks of net-009's kind on:

- net-010-a: the 90 million of 1.7.0 alone;
- net-010-b: 30 million of them, with 1.5.0 and 1.1.1, the window as it was, 95 million;
- net-010-c: all 90 million, with 1.5.0 and 1.1.1, 156 million.

Against Stockfish 19 on the 2934 positions, in its centipawns, with the correlation,
and on the million positions of 1.2.0 against the result:

| | static evaluation | kings on opposite wings | 5000 nodes | opposite wings | 1.2.0 |
|---|---|---|---|---|---|
| net-009 | 107, 0.632 | 129 | 91, 0.667 | 94 | 0.0761 |
| net-010-a | 99, 0.660 | 113 | 88, 0.683 | 100 | 0.0758 |
| net-010-b | 103, 0.645 | 122 | 89, 0.680 | 93 | 0.0756 |
| net-010-c | 99, 0.661 | 111 | 85, 0.690 | 91 | 0.0751 |

The new generation alone makes the better static evaluation, and the old positions on
top of it the better one with a search: they still help. Against the scores of 1.2.0,
all three are further than net-009 (0.0071, 0.0061 and 0.0062, against 0.0052), as they
learned from the searches of a much newer engine. net-010-c became net-010: it gives
1.13 times the scores of its data, and won by 64 Elo. Its scores spread about 30% wider
against Stockfish's than net-009's.

### `net-011`

The first window of 200 million positions of one generation: the 203 million of 1.8.0,
played on AWS in 6 hours 22 minutes, with the 91 million of 1.7.0 and the 35 million of
1.5.0, 329 million, against the 203 million of 1.8.0 alone:

| | static evaluation | kings on opposite wings | 5000 nodes | opposite wings | 1.2.0 |
|---|---|---|---|---|---|
| net-010 | 99, 0.661 | 111 | 85, 0.690 | 91 | 0.0751 |
| net-011, 329 million | 92, 0.685 | 102 | 82, 0.710 | 84 | 0.0735 |
| 1.8.0 alone, 203 million | 95, 0.676 | 109 | 82, 0.705 | 85 | 0.0732 |

The older generations still helped: against 1.8.0, net-011 won by 35 Elo (+35.2 ± 15.3,
654 games), the network of 1.8.0 alone by 14 (+13.8 ± 16.7 in 580 games, stopped). Yet
twice the data of net-010 barely moved the loss of training, 0.02057 against 0.02013:
256 neurons on one set of inputs is about as much as these networks learn from it.

On its own data, net-011 gives 0.90 times the scores, the first network under 1: the
searches of 1.8.0, with net-010's wider scale, label wider than the results agree with.
On the fixed million positions of 1.2.0 it gives 1.12 times, as net-010, so the engine
keeps dividing by 1.13.

### What next: a bigger network

From the cheapest to the dearest, what gives the network more to learn with:

1. **Horizontal mirroring**: every view seen with its own king on files a to d, the
   board mirrored otherwise. No new weights: what the network learns of one wing serves
   the other. The accumulator of a view gets refreshed when its king crosses the middle,
   seldom. The ground for king buckets.
2. **Output buckets by where the kings are**: the 8 buckets of material twice, kings on
   the same wing or on opposite ones, 16 output layers. Only the output grows, so the
   engine stays as fast and no accumulator changes; aimed at the opposite wings, where
   the error against Stockfish is still the largest. Little new capacity.
3. **Input buckets by the square of the king**: a first layer for each region of the own
   king, mirrored, 4 of them for a start. Where engines gain the most once data
   abounds, at the cost of a refresh of the view whose king changes region. Trained with
   a factoriser, weights shared by every bucket and added to each when saving, so that a
   bucket only learns how it differs and 329 million positions are enough; it costs
   nothing at inference.
4. **A wider hidden layer**, 384 or 512 neurons: the simplest change to the trainer, and
   nodes a second lost about in proportion, some 30 to 40% at 512. At 512, the sum of the
   output layer no longer fits an `i32` with output weights up to 127 (512 x 255² x 127
   > 2³¹): they would have to stay under 63, or be summed otherwise.
5. **More layers**, as `-> 16 -> 32 -> 1`: what the strongest engines play, and the most
   work, of quantisation and inference, wanting more data again.

2 and 3 were tried first, each against net-011 on its 329 million positions: see
net-012.

### `net-012`

net-011's 329 million positions, against net-011, with the losses of `validate` on
validation-1.8.0 (total, against the result, against the score), the gap between it
and 2 million positions of the training data, and the error against Stockfish at 5000
nodes:

| | validation-1.8.0 | gap | 5000 nodes | against net-011 |
|---|---|---|---|---|
| net-011 | 0.02103, 0.06877, 0.00648 | 0.00047 | 82, 0.710 | |
| wing outputs | 0.02106, 0.06875, 0.00655 | 0.00048 | 80, 0.715 | +4.6 ± 20.8, 300 games, stopped |
| 4 king buckets | 0.02087, 0.06818, 0.00675 | 0.00100 | 75, 0.754 | 0.0 ± 22.6, 342 games, stopped |
| net-011, wdl 0.3 | 0.02131, 0.07047, 0.00533 | 0.00048 | | |
| 4 king buckets, wdl 0.3 | 0.02088, 0.06974, 0.00521 | 0.00078 | 77, 0.740 | +27.4 ± 13.7, H1 in 888 games |

The output buckets by wing changed nothing. The king buckets fitted the data far
better, and doubled the gap: with half the target the result of the game, which ~89
positions of a game share, their capacity went to learning the results of these games,
and they fitted the score of the search, what the search plays on, worse than net-011.
With `wdl = 0.3`, the score weighs more, and the error against it fell by a fifth.
Lowering `wdl` alone does most of that; the buckets on top improve both parts, and won
by 31.9 ± 14.2 against net-011 with `wdl = 0.3` (H1 in 700 games). Against 1.8.0,
net-012 won by 86 Elo (+86.4 ± 24.6, 320 games on AWS), at some 5 to 7% fewer nodes a
second.

Other engines that document it found 4 mirrored buckets neutral at 150 million
positions and worth it from about 300 million, and use 1 to 3 billion for 8 or 10:
the gap of net-012 says more data would help it too.

## 5. In the engine

- **Inputs**: for each side's view, `(own piece ? 0 : 384) + 64 x kind + square`, with
  the kinds in the order P N B R Q K and the square from a1 = 0, flipped (`^ 56`) for
  Black's view. With king buckets, `768 x bucket + (input ^ 7)` when the view's king
  is on files e to h, `^ 7` mirroring the file, and the bucket that of its king's
  square seen from its side.
- **Accumulator**: for each view, the bias plus the column of every input on the board.
  A move that takes a view's king into another bucket, or across the middle, refreshes
  that view from the board, rarely enough not to need a cache.
- **Output**, for the side to move:
  `(sum(screlu(us) x w[b][..256]) + sum(screlu(them) x w[b][256..])) / QA + bias[b]`,
  times the scale, divided by `QA x QB`, where `screlu(x) = clamp(x, 0, QA)^2` and `b`
  is the bucket. bullet saves the output weights a bucket after the other once told to
  `transpose` them.
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
