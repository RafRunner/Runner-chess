# Changelog

Every version is a git tag, built into `engines/` by `scripts/build-engine.sh`.

- **Bench** is the node count of `runner-chess bench search` (depth 6), measured on
  each tag's own binary. It only changes when the search does, so a refactor keeps
  it and any other change shows up in it.
- **Strength** is what was measured with `scripts/match.sh` against the previous
  version, unless stated otherwise. Where nothing is listed, no match was recorded.

## 1.3.0 - 2026-10-30

### Changed
- A network twice as wide, `nets/net-007.bin`: 256 neurons on the data of net-006,
  trained with `wdl = 0.5` instead of 0.75, so that half of its target is the result of
  the game and half the score of the search. It gives 1.25 times the scores of its
  data, which the engine divides its output by.

### Measured
- Bench: 31518 (from 26531).
- Speed: 4.52 million nodes a second on `bench search 17` on an M4 Pro, from 5.14 with
  net-006.
- Strength: +69.3 ± 22.2 Elo against 1.2.1 at 10+0.1, an SPRT [0, 10] accepted after
  396 games (135 wins, 57 losses, 204 draws).
- The same 256 neurons trained with `wdl = 0.75` lost to net-006: by 66.4 ± 26.9 Elo in
  302 games when the output layer summed in 64 bits and searched 36% slower, and by
  about 60 at 8% slower. Their losses on a million positions of 1.2.0 that none of the
  networks trained on are in `docs/nnue.md`.

## 1.2.1 - 2026-10-30

### Changed
- Faster on the same tree:
  - a child's accumulators are written from its parent's in one pass, instead of
    copied and then changed a piece at a time;
  - the lists of moves of each ply are kept from one node to the next, instead of
    allocated for every node;
  - the legality and the checks of the moves come from what each node works out once:
    the pinned pieces, those that would uncover a check, and the squares from where
    each kind of piece checks;
  - the table entry of a child is asked from memory as the move is made.
- Each half of the output layer sums in 32 bits and both together in 64, so that a
  wider network can't overflow and still vectorises.
- Internal: the tests score positions with a search from depth 1, as the engine runs
  it, and only what a single node does by `negamax` alone.

### Added
- The `d` command also writes the hash of the position, and labels its FEN.

### Measured
- Bench: 26531 (unchanged).
- Speed: 5.14 million nodes a second on `bench search 17` on an M4 Pro, from 3.97 for
  1.2.0: 30% faster.
- Strength: +61.8 ± 21.3 Elo against 1.2.0 at 10+0.1, an SPRT [0, 10] accepted after
  398 games (116 wins, 46 losses, 236 draws), all of it from the speed.

## 1.2.0 - 2026-10-29

### Changed
- A new network of the same shape, `nets/net-006.bin`: trained on 90 million positions,
  30 million each of the games of 1.1.1, 0.10.0 and 0.9.2 against themselves, half of
  each starting from `popularpos_lichess_v3.epd`. It gives 1.57 times the scores of
  its data, and its output is still divided by 1.5.

### Measured
- Bench: 26531 (from 32169).
- Strength: +37.2 ± 17.7 Elo against 1.1.1 at 10+0.1, SPRT [0, 10] passed in 778
  games (261 wins, 178 losses, 339 draws).
- Quantisation: rounding the weights moves net-006's output by up to 2.7 centipawns on
  the reference positions, against 25 for net-005.

## 1.1.1 - 2026-10-28

### Fixed
- A search on the clock of a position without legal moves, mated or stalemated,
  panicked from depth 8 on, looking for the share of the nodes of a best move it
  doesn't have, instead of answering `bestmove 0000`.

### Measured
- Bench: 32169 (unchanged).

## 1.1.0 - 2026-10-28

### Added
- The time of a move depends on how sure the search is of it: from depth 8 on, the
  soft limit is multiplied by `(1.5 - share) * 1.35`, where share is the part of the
  nodes of the search that went to the best move. A best move that took most of them
  stops the search earlier, down to 0.675 of the limit, and one that took few lets it
  go on, up to 2.025.

### Fixed
- A draw found by the search, by repetition or by the fifty-move rule, counts as a
  node. When every move of the root was one, the search counted no node at all, which
  the time of a move divided by.

### Measured
- Bench: 32169. The search is the same as in 1.0.1: the one node more is the draws
  now counted.
- Strength: +51.6 ± 18.3 Elo against 1.0.1 at 10+0.1, an SPRT [0, 10] accepted after
  468 games (128 wins, 59 losses, 281 draws).
- Ordering the moves of the root by the nodes each took, tried before this, was
  neutral against 1.0.1 at 10+0.1: +4.4 ± 14.3 Elo in 1022 games counting only the
  last search of each move, and -4.7 ± 12.7 in 1340 adding them up. Neither was kept.

## 1.0.1 - 2026-10-28

### Added
- The `uci` command writes the name of the network before `uciok`, as an
  `info string` that GUIs show.

### Changed
- The root and quiescence get their moves from the same move picker as the other
  nodes, the only way left to order them. Quiescence takes from it only the captures
  that don't lose material in the exchange, as before.
- The history of the move ordering lives on the heap, so handing the ordering to the
  search thread copies a pointer instead of its 32 KB.

### Measured
- Bench: 32168 (unchanged).

## 1.0.0 - 2026-10-27

### Added
- The mate with bishop and knight against a lone king, which the network alone often
  missed: the side with them gets a bonus of 30 centipawns for each square the lone
  king is closer to a corner of the bishop's color, counting files plus ranks, and 15
  for each king move less between the two kings.

### Fixed
- A FEN without the halfmove clock and the move number, as En Croissant sends the
  positions of its books, starts them at 0 and 1. It used to be rejected, which left
  the engine on the position before, where it could answer with a move illegal in the
  GUI's.
- Promotions in uppercase, like `a7a8Q`, are read too.
- A command the engine ignores, as for an invalid FEN or an illegal move, gets an
  `info string` telling why, which the GUI keeps in its log. It used to go only to
  stderr.

### Measured
- Bench: 32168 (unchanged). No position of the bench has bishop and knight against a
  lone king, so no match was played.
- The mate: from 50 random positions of king, bishop and knight against king, the
  engine at 0.5 s a move mated Stockfish, defending at 0.1 s, in 49, in 29 moves on
  average and 43 at most. The other, `8/3K4/8/8/B7/8/N7/7k w`, ran into the 50-move
  rule, counted from the start. Without the bonus, 0.10.1 mated in 6 of the first 10.

## 0.10.1 - 2026-10-27

### Changed
- A new network of the same shape, `nets/net-005.bin`: trained on 90 million positions,
  30 million each of the games of 0.10.0, 0.9.2 and 0.9.1 against themselves, half of
  those of 0.10.0 and 0.9.2 starting from `popularpos_lichess_v3.epd`. Its output is
  still divided by 1.5.
- The attacks of bishops and rooks come from magic bitboards, one flat table for each,
  instead of ray by ray. The rays only build and check the tables.
- Internal: the tests share one search harness.

### Added
- The trainer's `validate` also tells how many times bigger a network's scores are
  than those of the data it gets.

### Measured
- Bench: 32168 (from 33554).
- Strength: +333 ± 59 Elo against 0.10.0 at 10+0.1, SPRT [0, 10] passed in 172 games
  (136 wins, 8 losses, 28 draws).
- The data, each network against the one before at 10+0.1, with the search of 0.10.0:
  30 million positions of 0.10.0 against net-003, +181 ± 43 in 226 games; 60 million,
  with those of 0.9.2, against 30, +83 ± 28 in 380; 90 million, with those of 0.9.1,
  against 60, +38 ± 18 in 756. Half the games from the book had gained +28.5 ± 16.6
  over random openings alone, while relabelling the endgames with the Syzygy tables
  lost 10.3 ± 12.8.
- Speed: the magic bitboards are about as fast as the rays on an M4: perft 1% slower,
  the search 2% faster, within the noise.
- Quantisation: rounding the weights moves net-005's output by up to 25 centipawns on
  the reference positions, against 7 for net-003: the steps of 1/64 of the output layer
  nearly all fall the same way for this network.

## 0.10.0 - 2026-10-25

### Added
- Principal variation search: the first move of a node gets the whole window, the
  others a null window, searched again only when they beat alpha.
- Aspiration windows: from depth 4, each iteration starts 25 centipawns around the
  score of the last one, and the side it falls out of opens wider each time.
- Reverse futility pruning: up to depth 7 off the PV, a static eval 80 centipawns per
  ply above beta fails high without searching.
- Futility pruning and late move pruning of the quiet moves that don't give check,
  near the horizon.
- Internal iterative reduction: from depth 4, a node without a move from the table
  gets a ply less.
- Static exchange evaluation: the quiescence skips the captures that lose material,
  the main search tries them after the quiet moves, and near the horizon prunes the
  moves that lose too much of it.

### Changed
- The static eval is computed once per node, and refined by the score of the table
  when its bound says it's closer to the real value.
- The table keeps two entries per slot, the deepest of the current search and the
  latest of the rest. Entries of earlier searches give way to the new ones, and a
  position stored again keeps its move when the new search has none. `Option<Move>`
  takes 2 bytes, so an entry is still 16.
- Null move pruning reduces `3 + depth / 3`, plus a ply for every 200 centipawns the
  eval is above beta, up to 3, and no longer runs on the PV.
- Late move reductions reduce a ply more off the PV.
- Moves come a stage at a time: the move of the table, the captures and promotions that
  don't lose material, the quiet moves, then the captures that do. The exchange of a
  capture only gets evaluated when its turn comes, and each stage only gets sorted
  when it starts. The quiet moves still get their keys as the node starts: with the
  history that the moves before them leave, the tree grew a third.
- The pruning decides before the move gets made: whether it gives check and whether
  it's legal come from the board as it is, so the moves that get pruned, two thirds of
  them, are never made. Each move's check is found once, and passed to its node.

### Measured
- Bench: 33554 (from 469750). At depth 10 the search visits 287457 nodes, against
  10410916 for 0.9.2.
- Speed: the stages, the pruning before making the moves and the check found once
  take 9% more nodes per second on the search bench at depth 16 (3.84 million against
  3.51), with the same tree.
- Strength: +470 ± 94 Elo against 0.9.2 at 10+0.1, SPRT [0, 10] passed in 160 games
  (141 wins, 1 loss, 18 draws).
- Before that, with a match runner of the session instead of `scripts/match.sh`, from
  balanced 8-ply random openings, each played with both colors:
  - +357 ± 49 Elo against 0.9.2 at 2+0.02, in 400 games (336 wins, 27 losses,
    37 draws)
  - before net-003, and while the search still had the check extension: +338 ± 45
    Elo against 0.9.0 at 2+0.02, in 400 games, and +390 ± 111 at 10+0.1, in 120
- Each change against the one before, with net-002, at 2+0.02, up to 600 games:
  PVS +54 ± 72 (only 52 games), RFP +70 ± 30 (SPRT passed), aspiration windows +20 ± 23, a check
  extension +16 ± 24 (taken out afterwards: it never measured better than neutral),
  futility and late move pruning +40 ± 23, the refined static
  eval +24 ± 24, SEE in the quiescence +84 ± 33 (SPRT passed), the table +111 ± 40
  (SPRT passed), the order of losing captures +4 ± 24, SEE pruning +24 ± 23, null
  move pruning -3 ± 24. Internal iterative reduction lost 97 ± 39 with the old table,
  which lost the move of every node that failed low, and taking it out of the final
  search loses 37 ± 24. An `improving` flag (the static eval against two plies
  before) for RFP, LMP and LMR lost 58 ± 29, and was reverted. The extra reduction
  off the PV was only measured together with it.

## 0.9.2 - 2026-10-24

### Changed
- A new network of the same shape, `nets/net-003.bin`: trained on 30 million positions
  of 0.9.1's games against itself, 30 passes. Its output is divided by 1.5, since it
  gives about 1.5 times the scores it learned from.

### Added
- `datagen --book <epd>`: every game starts from a random position of the book and 1
  or 2 random moves, instead of 8 or 9 random moves from the starting position.

### Measured
- Bench: 469750 (from 486750).
- Validation: a loss of 0.0549 on 1 million positions of 0.9.1's games it didn't
  train on, against 0.0617 for net-002.
- Strength: +177 ± 45 Elo against 0.9.1 at 10+0.1, SPRT [0, 10] passed in 238 games
  (155 wins, 43 losses, 40 draws).
- Against Stockfish 19 with `UCI_Elo` 2600, at 60+0.6 on `8moves_v3.pgn`: +76 ± 46
  Elo in 200 games (111 wins, 68 losses, 21 draws), about 2675 on Stockfish's scale,
  which was calibrated at 120+1 on the same book.
- Bishop and knight against a lone king evaluate at about +0.8, since 0.9.1 almost
  never won them: 0.4% of the 75 thousand such positions in its games.

## 0.9.1 - 2026-10-23

### Changed
- Move ordering sorts plain integers: every move packed with its key and its index into
  a `u64`, in a buffer the ordering keeps between nodes. `sort_by_cached_key` allocated
  pairs of key and index at every node, then moved the moves around to match them. Ties
  still keep the order of generation, so the search is the same.

### Measured
- Bench: 486750 (unchanged).
- Speed: 27% more nodes per second, and 21% less time to depth 11 on the search bench
  (2856 ms against 3628 ms). No match was played, since the search is the same.

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
