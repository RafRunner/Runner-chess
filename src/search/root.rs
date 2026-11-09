use std::{
    io::{self, Write},
    mem,
};

use crate::{
    bench,
    board::Board,
    chess_move::Move,
    score::Score,
    search::{MovePicker, NodeInfo, Outcome, Searcher, tt::Bound},
    zobrist::ZobristHash,
};

/// A move of the root, with the nodes its searches took in every iteration so far
#[derive(Debug, Clone)]
struct RootMove {
    mv: Move,
    score: Score,
    bound: Bound,
    nodes: u64,
}

/// Iterations from this depth on start from a window this wide on each side of the
/// last score
const ASPIRATION_DEPTH: u32 = 4;
const ASPIRATION_DELTA: i16 = 25;

impl Searcher<'_> {
    /// Searches within the budget and writes an `info` line about it to `out`.
    /// `history` has the positions of the game before `board`, oldest first, so the
    /// search can see repetitions.
    pub fn search_root(
        &mut self,
        board: &Board,
        history: &[ZobristHash],
        out: &mut impl Write,
    ) -> io::Result<Option<Move>> {
        self.ordering.new_search();
        self.tt.new_search();
        self.hashes.extend_from_slice(history);
        self.root_index = history.len();
        self.push_hash(board.hash());
        self.accumulators.reset(board);

        // Depth 1 should always finish (unless no legal moves)
        let mut best_move = None;
        let mut root_moves: Vec<RootMove> = Vec::with_capacity(64);

        for depth in 1..=self.budget.depth {
            let best_score =
                self.aspiration_search(board, depth, &mut best_move, &mut root_moves, out)?;

            if self.stopped {
                break;
            }
            self.completed_depth = depth;
            self.completed_score = Some(best_score);
            self.print_info_line(out, depth, best_move, best_score, "")?;

            // the share of the nodes that the best move took tells how sure the search is
            // of it. Without legal moves there's none, and nothing to be sure of
            let best_nodes = root_moves
                .iter()
                .find(|r| Some(r.mv) == best_move)
                .map_or(0, |r| r.nodes);
            let best_share = best_nodes as f64 / self.nodes.max(1) as f64;
            // pondering, it goes on until `ponderhit` or `stop`
            if self
                .clock()
                .is_some_and(|now| self.budget.soft_reached(depth, self.nodes, best_share, now))
            {
                break;
            }
        }

        // an iteration cut short has scores of two depths
        if let (Some(best), Some(margin), false) = (
            best_move,
            self.variety.as_ref().map(|v| v.margin).filter(|&m| m > 0),
            self.stopped,
        ) {
            return Ok(Some(self.close_move(board, best, margin, &root_moves)));
        }

        Ok(best_move)
    }

    /// One of the moves within `margin` of the best of the last iteration, at random. The
    /// ones with an exact score weigh by how close they are; those that only have an
    /// upper bound, which may be far above their score, by half the margin once a null
    /// window at the floor shows they pass it. Mates leave the best move as it is
    fn close_move(
        &mut self,
        board: &Board,
        best_move: Move,
        margin: i16,
        root_moves: &[RootMove],
    ) -> Move {
        let Some(best) = self.completed_score.and_then(Score::centipawns) else {
            return best_move;
        };
        let (best, margin) = (i32::from(best), i32::from(margin));
        let floor = Score::new(best - margin);

        let mut candidates = Vec::new();
        let mut total: i32 = 0;
        for root in root_moves {
            let Some(distance) = root.score.centipawns().map(|s| best - i32::from(s)) else {
                continue;
            };
            if distance >= margin {
                continue;
            }
            match root.bound {
                Bound::Exact => {
                    let weight = margin - distance;
                    total += weight;
                    candidates.push((root.mv, weight));
                }
                Bound::Upper if self.passes(board, root.mv, floor) => {
                    let weight = margin / 2;
                    total += weight;
                    candidates.push((root.mv, weight));
                }
                _ => (),
            }
        }

        let mut pick = self
            .variety
            .as_mut()
            .expect("close move without variety")
            .rng
            .below(total as usize) as i32;
        candidates
            .into_iter()
            .find(|&(_, weight)| {
                pick -= weight;
                pick < 0
            })
            .map_or(best_move, |(mv, _)| mv)
    }

    /// Whether `mv` scores above `floor`, searched as deep as in the last iteration on a
    /// null window there. Not if the search had to stop
    fn passes(&mut self, board: &Board, mv: Move, floor: Score) -> bool {
        if self.stopped {
            return false;
        }

        let next = board.make_move(mv);
        let gives_check = next.is_in_check(next.side_to_move());
        let at_floor = NodeInfo::new(board, self.completed_depth, 0, floor, floor + 1);
        let score = self.search_child(
            board,
            at_floor.scout_child(&next, 0, -floor - 1, gives_check),
            Some(mv),
        );
        !self.stopped && score > floor
    }

    /// The score of the root at `depth`, on a narrow window around the score of the last
    /// iteration, which it rarely moves far from: much faster than the whole window when
    /// it falls inside. When it doesn't, that side of the window opens wider and wider
    /// until it does. A fail high already has a better move, `best_move` gets it
    fn aspiration_search(
        &mut self,
        board: &Board,
        depth: u32,
        best_move: &mut Option<Move>,
        moves: &mut Vec<RootMove>,
        out: &mut impl Write,
    ) -> io::Result<Score> {
        let mut delta = ASPIRATION_DELTA;
        let (mut alpha, mut beta) = match self.completed_score {
            Some(score) if depth >= ASPIRATION_DEPTH && !score.is_mate() => {
                (score - delta, score + delta)
            }
            _ => (-Score::INF, Score::INF),
        };

        loop {
            let score = self.search_window(board, depth, alpha, beta, best_move, moves);

            if self.stopped {
                // only a move that beat alpha has a score to report, and it's at least that
                if score > alpha {
                    self.print_info_line(out, depth, *best_move, score, " lowerbound")?;
                }
                return Ok(score);
            }
            if score <= alpha && alpha > -Score::INF {
                alpha = score - delta;
            } else if score >= beta && beta < Score::INF {
                beta = score + delta;
                self.print_info_line(out, depth, *best_move, score, " lowerbound")?;
            } else {
                return Ok(score);
            }
            delta = delta.saturating_mul(2);
        }
    }

    /// The score of the root at `depth` inside the window (alpha, beta), fail soft
    fn search_window(
        &mut self,
        board: &Board,
        depth: u32,
        alpha: Score,
        beta: Score,
        best_move: &mut Option<Move>,
        root_nodes: &mut Vec<RootMove>,
    ) -> Score {
        let mut best_score = -Score::INF;
        let mut root = NodeInfo::new(board, depth, 0, alpha, beta);
        let buffers = mem::take(&mut self.move_buffers[0]);
        let mut moves =
            MovePicker::new(board, *best_move, self.ordering, 0, buffers).into_iterator(board);

        for (idx, (mv, next)) in moves
            .by_ref()
            .filter_map(|mv| {
                let next = board.make_move(mv);
                (!next.is_in_check(board.side_to_move())).then_some((mv, next))
            })
            .enumerate()
        {
            let gives_check = next.is_in_check(next.side_to_move());
            let before = self.nodes;
            let searched_against = root.alpha;
            let score = self.search_move(root, mv, idx, &next, gives_check);

            let spent = self.nodes - before;
            match root_nodes.iter_mut().find(|r| r.mv == mv) {
                Some(r) => {
                    r.nodes += spent;
                    r.score = score;
                    r.bound = Bound::new(score, searched_against, root.beta);
                }
                None => root_nodes.push(RootMove {
                    mv,
                    nodes: spent,
                    score,
                    bound: Bound::new(score, searched_against, root.beta),
                }),
            }

            if self.stopped {
                break;
            }

            match root.update(&mut best_score, score) {
                Outcome::Worse => (),
                Outcome::Raised => *best_move = Some(mv),
                Outcome::Cut => {
                    *best_move = Some(mv);
                    break;
                }
            }
        }

        self.move_buffers[0] = moves.into_buffers();
        best_score
    }

    fn print_info_line(
        &self,
        out: &mut impl Write,
        depth: u32,
        best_move: Option<Move>,
        best_score: Score,
        bound: &str,
    ) -> io::Result<()> {
        if let Some(mv) = best_move
            && best_score > -Score::INF
        {
            let elapsed = self.budget.start.elapsed();
            writeln!(
                out,
                "info depth {depth} score {}{bound} nodes {} seldepth {} nps {} time {} pv {mv}",
                best_score.to_uci(),
                self.nodes,
                self.seldepth,
                bench::nps(self.nodes, elapsed),
                elapsed.as_millis(),
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rng::Rng,
        search::{
            Budget, MoveOrdering, TranspositionTable, Variety,
            tests::{Searched, best_move, search},
        },
        test_utils::{KIWIPETE, MATE_IN_ONE, MATED, board},
    };

    /// What a search of `fen` to `depth` writes.
    fn output(fen: &str, depth: u32) -> String {
        search(&board(fen), &[], Budget::depth(Some(depth))).out
    }

    // budget

    /// A search of `fen` stopped after `nodes` nodes.
    fn search_with_node_limit(fen: &str, nodes: u64) -> Searched {
        search(&board(fen), &[], Budget::depth(Some(10)).with_nodes(nodes))
    }

    /// The first move of the `pv` in the last line of `out`.
    fn last_pv(out: &str) -> Option<&str> {
        let pv = out.lines().last()?.split(" pv ").nth(1)?;
        pv.split_whitespace().next()
    }

    #[test]
    fn an_interrupted_iteration_is_reported_as_a_lower_bound() {
        // from the start, depth 1 takes 23 nodes and depth 2 ends at 80: 50 stops in the
        // middle of it, so its line can only say that the score is at least that much
        let out = search_with_node_limit(Board::STARTPOS, 50).out;
        let lines: Vec<&str> = out.lines().collect();

        assert!(lines[0].starts_with("info depth 1 "), "{out}");
        assert!(!lines[0].contains(" lowerbound "), "{out}");
        for line in &lines[1..] {
            assert!(line.starts_with("info depth 2 "), "{out}");
            assert!(line.contains(" lowerbound "), "{out}");
        }
    }

    #[test]
    fn the_last_info_line_is_the_best_move() {
        // depth 1 likes Rxa6, but depth 2 sees Re1# and switches to another move before
        // 150 nodes stop it: GUIs show the last line as the engine's choice
        let fen = "4r1k1/5ppp/n7/8/8/8/5PPP/R5K1 w - - 0 1";
        let Searched { best, out, .. } = search_with_node_limit(fen, 150);
        assert_ne!(best.as_deref(), Some("a1a6"), "{out}");
        assert_eq!(last_pv(&out), best.as_deref(), "{out}");

        // and wherever else the limit falls
        for fen in [Board::STARTPOS, KIWIPETE, fen] {
            for nodes in [30, 100, 300, 1_000, 3_000, 10_000] {
                let Searched { best, out, .. } = search_with_node_limit(fen, nodes);
                assert_eq!(last_pv(&out), best.as_deref(), "{fen} {nodes}\n{out}");
            }
        }
    }

    #[test]
    fn the_first_iteration_always_finishes() {
        // a limit below what depth 1 needs still gets a searched move
        let Searched { best, out, .. } = search_with_node_limit(Board::STARTPOS, 1);
        assert!(out.starts_with("info depth 1 "), "{out}");
        assert_eq!(best, best_move(Board::STARTPOS, 1));
    }

    #[test]
    fn a_soft_node_limit_lets_the_iteration_finish() {
        let budget = Budget::depth(Some(Score::MAX_PLY)).with_soft_nodes(2_000);
        let searched = search(&board(KIWIPETE), &[], budget);
        let out = &searched.out;

        // the iteration that passed the limit still finished, and was the last
        assert!(searched.nodes >= 2_000);
        assert!(!out.contains("lowerbound"), "{out}");
        let last = out.lines().last().unwrap();
        let score = searched.score.unwrap().to_uci();
        assert!(last.contains(&format!("score {score} ")), "{score} {last}");
    }

    #[test]
    fn stops_at_the_node_limit() {
        let budget = Budget::depth(Some(7)).with_nodes(1000);
        let Searched { best, nodes, .. } = search(&board(Board::STARTPOS), &[], budget);

        // the limit is checked at every node, so the search stops right at it
        assert!((1000..1010).contains(&nodes), "{nodes}");
        assert!(best.is_some());
    }

    #[test]
    fn no_legal_moves_means_no_best_move_and_no_info() {
        let Searched { best, out, .. } = search(&board(MATED), &[], Budget::depth(Some(3)));
        assert_eq!(best, None);
        assert_eq!(out, "");
    }

    // info

    #[test]
    fn writes_an_info_line_per_depth() {
        let Searched { best, out, .. } =
            search(&board(Board::STARTPOS), &[], Budget::depth(Some(3)));
        let best = best.unwrap();
        let lines: Vec<&str> = out.lines().collect();

        assert_eq!(lines.len(), 3, "{out}");
        for (depth, line) in (1..).zip(&lines) {
            assert!(
                line.starts_with(&format!("info depth {depth} score ")),
                "{out}"
            );
            for field in [" nodes ", " seldepth ", " nps ", " time ", " pv "] {
                assert!(line.contains(field), "{out}");
            }
        }
        assert!(out.ends_with(&format!(" pv {best}\n")), "{out}");
    }

    /// The number after `name` in an `info` line.
    fn info_field(out: &str, name: &str) -> u32 {
        let fields: Vec<&str> = out.split_whitespace().collect();
        let i = fields
            .iter()
            .position(|&field| field == name)
            .unwrap_or_else(|| panic!("no {name} in {out}"));
        fields[i + 1].parse().unwrap()
    }

    #[test]
    fn seldepth_counts_quiescence() {
        // nothing can be captured after a single move from the start, so quiescence
        // stops right at the depth
        let out = output(Board::STARTPOS, 1);
        assert_eq!(info_field(&out, "seldepth"), 1, "{out}");

        // Nxd5 Nxd5 Rxd5: an exchange with no checks, so only the captures of
        // quiescence can take the search past the depth
        let out = output("7k/7p/5n2/3p4/8/2N5/8/K2R4 w - - 0 1", 1);
        assert!(info_field(&out, "seldepth") >= 3, "{out}");
    }

    #[test]
    fn info_reports_mates_in_moves() {
        // giving mate in 1, and getting mated in 1 (Ka7 Ra1#)
        let out = output(MATE_IN_ONE, 2);
        assert!(out.contains(" score mate 1 "), "{out}");

        let out = output("k7/2K5/8/8/8/8/8/7R b - - 0 1", 3);
        assert!(out.contains(" score mate -1 "), "{out}");
    }

    // variety

    /// The move a search of `fen` to `depth` plays with a `Variety` of `margin`, drawn
    /// from `seed`
    fn varied(fen: &str, depth: u32, margin: i16, seed: u64) -> String {
        let (mut tt, mut ordering) = (TranspositionTable::new(1), MoveOrdering::new());
        let variety = Variety {
            margin,
            rng: Rng(seed),
        };
        Searcher::new(Budget::depth(Some(depth)), &mut tt, &mut ordering)
            .with_variety(variety)
            .search_root(&board(fen), &[], &mut io::sink())
            .unwrap()
            .expect("a move")
            .to_uci()
    }

    #[test]
    fn a_clearly_best_move_is_always_drawn() {
        // Qxd5 takes a queen for nothing: every other move is far worse
        let fen = "4k3/8/8/3q4/8/8/8/3QK3 w - - 0 1";
        for seed in 0..20 {
            assert_eq!(varied(fen, 5, 50, seed), "d1d5", "seed {seed}");
        }
    }

    #[test]
    fn a_mate_is_played_whatever_the_margin() {
        for seed in 0..10 {
            assert_eq!(varied(MATE_IN_ONE, 3, 1200, seed), "a1a8", "seed {seed}");
        }
    }

    #[test]
    fn the_same_seed_draws_the_same_move() {
        assert_eq!(
            varied(Board::STARTPOS, 6, 40, 7),
            varied(Board::STARTPOS, 6, 40, 7)
        );
    }

    #[test]
    fn close_moves_share_the_draw_and_stay_close() {
        let (depth, margin) = (6, 40);
        let root = board(Board::STARTPOS);
        let best = search(&root, &[], Budget::depth(Some(depth)))
            .score
            .and_then(Score::centipawns)
            .unwrap();
        // each move on its own, as deep, and the gaps of separate searches allowed for
        let score_of = |uci: &str| {
            let (_, next) = root
                .resolve_move(crate::chess_move::AbstractMove::from_uci(uci).unwrap())
                .unwrap();
            let child = search(&next, &[root.hash()], Budget::depth(Some(depth - 1)));
            -child.score.and_then(Score::centipawns).unwrap()
        };

        let drawn: std::collections::HashSet<String> = (0..30)
            .map(|seed| varied(Board::STARTPOS, depth, margin, seed))
            .collect();
        assert!(drawn.len() > 1, "always {drawn:?}");
        for mv in &drawn {
            let score = score_of(mv);
            assert!(score > best - margin - 30, "{mv}: {score} against {best}");
        }
    }
}
