# Changelog

Every version is a git tag, built into `engines/` by `scripts/build-engine.sh`.

- **Bench** is the node count of `runner-chess bench search` (depth 6), measured on
  each tag's own binary. It only changes when the search does, so a refactor keeps
  it and any other change shows up in it.
- **Strength** is what was measured with `scripts/match.sh` against the previous
  version, unless stated otherwise. Where nothing is listed, no match was recorded.

## 0.9.0 - 2026-10-23

### Changed
- The evaluation is a neural network, `(768 -> 128)x2 -> 1` with SCReLU, instead of
  the piece-square tables: `nets/net-002.bin`, embedded in the binary. Trained with
  bullet on 25 million positions of 0.8.2's games against itself, 30 passes. Its
  output is halved, since it gives about twice the scores it learned from.
- The hidden layer of both sides' views gets updated move by move from the pieces
  that moved, on a stack next to the hashes of the line being searched.
- Dead positions are still draws, before the network.
- Internal: the root and null move pruning in their own files.

### Added
- `trainer/`, which trains the networks with bullet, and its `validate`, the loss of
  a network on positions it didn't train on. `scripts/prepare-data.sh` turns the
  output of `datagen` into bullet's shuffled binary format.

### Removed
- The piece-square tables.

### Measured
- Bench: 486750 (from 431809). At depth 10 the search visits 66% more nodes than
  0.8.2's, at the same speed.
- Validation: a loss of 0.0575 on 4.9 million positions from games it didn't train
  on, against 0.0606 for a network trained on 10 million.
- Strength: +235 ± 56 Elo against 0.8.2 at 10+0.1, SPRT [0, 10] passed in 202 games
  (154 wins, 35 losses, 13 draws).
- Mates against a lone king defended by Stockfish, within the fifty moves, from 10
  random positions each: all 10 with a rook, 4 with two bishops, and 1 with bishop
  and knight, as 0.8.2.

## 0.8.2 - 2026-10-22

### Added
- `runner-chess datagen <dir>`: games of the engine against itself, written as
  training data in bullet's text format, one file per thread (see `docs/nnue.md`).
  Every game opens with 8 or 9 random moves, every move is a search with a soft
  limit of 5000 nodes, and only quiet positions get written: not in check, with a
  quiet best move and no mate in sight. Games end by the rules or by adjudication.
  The same seed and number of threads play the same games again.
- A soft node limit for the search: no new iteration after that many nodes, so the
  score always comes from one that finished.
- `go infinite`: no deadline, and `bestmove` only after `stop`.
- The `Hash` option, from 1 to 65536 MB.
- `id name` has the version.

### Changed
- The table takes any size: the index scales the hash to the number of entries
  instead of taking its low bits, so `Hash 100` uses 100 MB instead of 64.

### Measured
- Bench: 431809 (from 429937): the same search, with other positions sharing a
  slot of the table. No match was played for that.
- Data generation: about 2600 positions per second on 10 threads, 65 bytes each,
  with 38% White wins, 21% draws and 41% Black wins.

## 0.8.1 - 2026-10-22

### Changed
- The history and the killers carry over from one search to the next, kept in the
  `Uci` next to the table: every `go` halves the history and clears the killers, and
  `ucinewgame` clears both.
- The quiescence generates only the captures, instead of every move to throw the
  quiet ones away.

### Fixed
- Captures and promotions are ordered by the material they win, then by the cheaper
  piece moving, and a promotion wins the new piece less the pawn. Promotions to a
  rook or a bishop, which a queen does better, come after every quiet move. Before,
  promotions tied with pawn captures by accident, so the order of the move generator
  decided between them, and underpromotions came before the killers.

### Measured
- Bench: 429937 (from 432470), from the order of the promotions. The history kept
  between searches doesn't show in it, since every position of the bench starts
  with a new one. With only the captures, the quiescence takes about 8% less time to
  the same depth.
- Strength: +25 ± 15 Elo against 0.8.0 at 10+0.1, SPRT [0, 10] passed in 1368 games
  (498 wins, 400 losses, 470 draws).

## 0.8.0 - 2026-10-21

### Added
- Late move reductions: quiet moves that come late in the order get searched a few
  plies shallower, with a null window just above alpha, and again at full depth and
  with the full window only when that beats alpha. The reduction is
  `0.99 + ln(depth) × ln(move index) / 3.14` plies, rounded, up to two plies less
  with a good history and up to two more with a bad one, and the child always keeps
  at least a ply. Never at the root, below depth 3, for the first 5 moves of a node,
  for captures, promotions and killers, or when in check or giving check.

### Measured
- Bench: 432470 (from 954219): 55% fewer nodes at depth 6, and 86% fewer at depth 10,
  where the search bench went from 8.0 s to 1.2 s, at about 10% fewer nodes per
  second.
- Strength: +100 ± 31 Elo against 0.7.0 at 10+0.1, SPRT [0, 10] passed in 338 games
  (167 wins, 72 losses, 99 draws).

## 0.7.0 - 2026-10-20

### Added
- Killer moves: the last two quiet moves that caused a beta cutoff at each ply come
  right after the captures.
- History heuristic for the other quiet moves: a bonus for the one that caused a
  cutoff, a penalty for those tried before it, kept bounded by a gravity formula.
- The search runs in its own thread, so `stop` ends it, `isready` gets answered
  during it, and `quit` or the end of the input stop it before exiting.

### Changed
- Score margins saturate at infinity instead of overflowing.
- The non-regression SPRT of `scripts/match.sh` tests [-10, 0] instead of [-5, 0].
- Internal: the window, depth and ply of a node travel together (`NodeInfo`), and
  draw detection and move ordering live in their own files.

### Measured
- Bench: 954219 (from 1204565): 21% fewer nodes to the same depth.
- Strength: +128 ± 33 Elo against 0.6.0 at 10+0.1, SPRT [0, 10] passed in 260 games
  (143 wins, 51 losses, 66 draws).
- Absolute strength, against Stockfish with `UCI_Elo` 2400 at 10+0.1: 38 wins, 56
  losses and 6 draws in 100 games (-63 ± 68), about 2340 on that scale.

## 0.6.0 - 2026-10-18

### Added
- Null move pruning: when the static eval is already above beta, a search after
  passing the turn, reduced by `2 + depth / 4` plies, can cut the node. Not in check,
  not twice in a row, not without pieces besides pawns, and not when the table says
  the node fails low. Mates found that way aren't trusted.

### Changed
- The probe of the transposition table and its cutoff became separate steps, so the
  null move can use the entry too.

### Measured
- Bench: 1204565 (from 2495104): 52% fewer nodes to the same depth.
- Strength: +84 ± 30 Elo against 0.5.1 at 10+0.1, SPRT [0, 10] passed in 408 games
  (201 wins, 104 losses, 103 draws).

## 0.5.1 - 2026-10-18

### Fixed
- The table no longer cuts at positions that already appeared in the game or the
  line being searched. Its entries could come from before a repetition through them
  was possible, which made the engine walk into threefold repetitions it thought it
  was winning: in 82 repetition draws of 0.5.0 against a development version, the
  side allowing the repetition thought it was better in 43.

### Added
- `scripts/match.sh --non-regression`, an SPRT for changes not meant to gain.

### Measured
- Bench: 2495104 (unchanged: no bench position has a game behind it).
- Strength: about +33 ± 17 Elo against 0.5.0 at 5+0.05, after about 600 games of a
  non-regression SPRT, stopped before it finished.

## 0.5.0 - 2026-10-16

### Added
- A transposition table of 64 MB, always replacing: exact scores and bounds cut
  the search where the stored depth is enough, and the stored move is tried first
  everywhere. Mate scores get stored relative to the node and restored relative to
  the root. Interrupted searches store nothing.

### Measured
- Bench: 2495104 (from 9352583): 73% fewer nodes to the same depth. At depth 8 the
  search bench went from 66.9 s to 10.6 s.
- Strength: +234 ± 45 Elo against 0.4.1 at 10+0.1, SPRT [0, 10] passed in 194 games
  (130 wins, 16 losses, 48 draws).

## 0.4.1 - 2026-10-16

### Changed
- The move lists start with room for 64 moves, which avoids most reallocations.
- Rust edition 2024.

### Fixed
- The `info` line of an interrupted iteration says `lowerbound`, since its score
  only counts the moves searched before the stop.

### Measured
- Bench: 9352583 (unchanged), with 19% more nodes per second.
- Strength: -11 ± 22 Elo against 0.4.0 at 5+0.05 over 760 games, so no measurable
  gain from the speed: the average depth reached only went from 6.31 to 6.34.
- Absolute strength, measured around this version against Stockfish limited with
  `UCI_Elo`: 91 wins and 9 losses against 1500 (+402 ± 128), and 61 wins, 35 losses
  and 4 draws against 1900 (+92 ± 67), about 1990 on that scale.

## 0.4.0 - 2026-10-15

### Added
- Iterative deepening, with the best move of each iteration searched first in the
  next, and an `info` line per depth.
- Time control for `wtime`, `btime`, `winc`, `binc`, `movestogo`, `movetime` and
  `nodes`: no new iteration after a soft limit, and a stop in the middle of one at a
  hard limit, short of the clock. The first iteration always finishes, and an
  interrupted one still gives its best move when it found one.

### Measured
- Bench: 9352583 (from 10995543).
- Strength: +137 ± 81 Elo against 0.3.1 at 120+1, about +85 without the forfeited
  games.

## 0.3.1 - 2026-10-14

### Added
- Draws by insufficient material: positions where no sequence of moves can end in
  mate are scored as draws.

### Measured
- Bench: 10995543 (unchanged). No measurable change in strength against 0.3.0.

## 0.3.0 - 2026-10-14

### Added
- Zobrist hashing with the Polyglot keys, updated incrementally on every move. The
  en passant square is only recorded when a pawn can take.
- Draws by threefold repetition and by the fifty-move rule inside the search. A
  repetition inside the search counts at once, one with the game only on the third
  time; a mate on the hundredth ply still counts as mate.
- `bench search`, the search benchmark.

### Changed
- A `Score` type for centipawns and mate distances, and the search split into its
  own module.

### Measured
- Bench: 10995543.

## 0.2.0 - 2026-10-12

### Added
- Quiescence search: captures until the position is quiet, and every evasion when in
  check.

## 0.1.0 - 2026-10-12

### Added
- Board with bitboards and a mailbox, FEN parsing and writing with sanity checks.
- Legal move generation, verified against the reference perft results, and
  `bench`, the perft benchmark.
- Evaluation by material and piece-square tables, blended between the middlegame
  and the endgame by the material left.
- Fixed-depth alpha-beta negamax.
- The UCI protocol: `uci`, `isready`, `ucinewgame`, `position`, `go`, `go perft`,
  `debug`, `d` and `quit`.
- `scripts/build-engine.sh` and `scripts/match.sh` to build tagged versions and play
  them against each other with fastchess.

## Tried and left out

- **Evaluation terms** (bishop pair, doubled, isolated and passed pawns, pawn shield
  and open files next to the king): +75 ± 28 Elo against 0.5.0 at 10+0.1, SPRT [0, 10]
  passed in 456 games. Kept out on purpose: the plan is to see how far search goes
  with a minimal evaluation, then move to NNUE. Saved in a git stash.
- **Root moves ordered by the score of the previous iteration**, instead of only
  bringing the best one to the front: neutral, 0 ± 25 Elo after more than 1000 games.
  It only matters when an iteration gets interrupted or the best move changes in the
  middle of one. Worth trying again with PVS, LMR and a time control based on the
  stability of the best move.
- **Check status stored in the board**, recalculated at the end of every move: the
  same nodes, but 9% slower perft and 2% slower search, since every board pays for
  it, illegal ones and nodes the table cuts included. Worth it once checks get
  computed incrementally (the moved piece and the lines it opens) and the move
  generator uses them, for evasions and pins.
- **Check extension**, searching a ply deeper after a move that gives check: -10 ±
  15 Elo against 0.8.0 after about 1350 games, so neutral at best. LMR already
  leaves those moves unreduced.
- **Legality from pins**: the pinned pieces computed once per node, so only king
  moves, en passant, moves out of check and pinned pieces need the full check. The
  same nodes, with 24% faster perft but 2 to 4.5% slower search, in four variants:
  most nodes cut after one to three moves, so the setup of each node costs more than
  the checks it saves. Worth it with a fully legal move generator.
