# Road to NNUE

What is left before generating training data, how the data gets generated, and the
format bullet expects.

## 1. Before generating data

| Step | Why | Measured |
|---|---|---|
| History with aging, kept in the `Uci` | Each search starts from what the previous ones learned | +18 ± 12 Elo against 0.8.0 at 10+0.1 over 1860 games (the SPRT stopped at LLR 2.73 of 2.94) |
| `generate_captures` for the quiescence | The quiescence stops generating quiet moves and castling just to throw them away | 8% less time to the same depth, with the same nodes |
| A soft node limit in `Budget` | Data generation searches a fixed number of nodes per move | - |
| `go infinite` | Completes the UCI side | - |

### History with aging

- `MoveOrdering` lives in the `Uci`, next to the table, and goes into the search thread
  and back with it.
- On every `go`: halve the history and clear the killers. The killers are indexed by ply
  from the root, which moves two plies between searches.
- On `ucinewgame`: a new `MoveOrdering`.

### `generate_captures`

The captures of `generate_moves`, in the same order, so the search stays the same: the
targets of pieces masked with the opponent's pieces, pawn captures with their
promotions, and en passant. Promotions that don't capture stay out, as the quiescence
skips them today. Checking that the bench node count doesn't change is the test.

### Soft node limit

- **Soft**: no new iteration once it is reached, so the score always comes from a
  complete one.
- **Hard**: a stop in the middle of an iteration, well above the soft limit (10 times,
  for example), only as a safety net.

### `go infinite`

Search up to `MAX_PLY` with no deadline. `bestmove` only comes after `stop`, even when the
search ends earlier, as it can with a forced mate.

## 2. Data generation

A `runner-chess datagen` subcommand that plays games inside the process, without UCI.

- **Threads**: one game per thread (`std::thread::scope`), each with its own small table
  (16 MB), cleared between games, and its own seed.
- **Random numbers**: a small generator such as splitmix64 or xorshift64*, so no
  dependency is needed.
- **Openings**: 8 or 9 random legal plies from the starting position, so both sides get
  to move first. Then a short search (around 10,000 nodes) and discard the opening when
  `|score| > 400`.
- **Moves**: a search with a soft limit of about 5,000 nodes. Keep the move and the score
  of the last complete iteration.
- **Positions kept**, after the random plies:
  - not when the side to move is in check;
  - not when the best move is a capture or a promotion, since the network should learn
    quiet positions;
  - not with mate scores, which would distort the training.
- **End of a game**:
  - by the rules: mate, stalemate, threefold repetition (with the hashes of the game),
    the fifty-move rule, insufficient material;
  - adjudicated, to save time: a win once `|score| >= 2000` for several plies in a row,
    a draw after move 40 once `|score| <= 10` for 8 plies in a row. These numbers are a
    starting point.
- **Output**: once a game ends, its positions get written with its result, one file per
  thread, flushed often so a stopped run keeps what it had.
- **Volume**: 10 to 100 million positions for a first, small network. Text takes about
  70 to 90 bytes per position, so 100 million is around 8 GB.

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

### First network

The bullet docs recommend starting from `examples/simple`, its architecture and training
schedule.

## 4. In the engine, later

- The weights embedded with `include_bytes!` in a `static`, read-only and shared by
  every thread.
- A stack of accumulators, one per ply, in the search state, never in `Board`, which gets
  copied on every move.
- `make_move` telling what changed (the piece that left a square, the one that arrived,
  the one captured) for the accumulator to update from.
- Before trusting it: the engine's evaluation of a few positions has to match what
  bullet reports for the same network.

## Sources

- bullet: https://github.com/jw1912/bullet, its docs on
  [training data](https://github.com/jw1912/bullet/blob/main/docs/3-data.md) and
  [getting started](https://github.com/jw1912/bullet/blob/main/docs/2-getting-started.md)
- The text format and `ChessBoard`: https://github.com/jw1912/bulletformat
  (`src/chess.rs`)
- `bullet-utils`: `crates/utils/src/{convert,shuffle,interleave}.rs` in the bullet
  repository
