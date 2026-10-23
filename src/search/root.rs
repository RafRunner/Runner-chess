use std::{
    io::{self, Write},
    time::Instant,
};

use crate::{
    bench,
    board::Board,
    chess_move::Move,
    eval::Accumulators,
    score::Score,
    search::{NodeInfo, Outcome, Searcher},
    zobrist::ZobristHash,
};

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
        self.hashes.extend_from_slice(history);
        self.root_index = history.len();
        self.push_hash(board.hash());
        self.accumulators.push(Accumulators::new(board));

        let mut best_move = self
            .ordering
            .legal_ordered(board, 0, None)
            .next()
            .map(|(mv, _)| mv);

        for depth in 1..=self.budget.depth {
            let mut best_score = -Score::INF;
            let mut root = NodeInfo::full_window(board, depth, 0);

            for (mv, next) in self.ordering.legal_ordered(board, 0, best_move) {
                let score = self.search_child(root.board, root.child(&next), Some(mv));

                if self.stopped {
                    break;
                }

                if root.update(&mut best_score, score) != Outcome::Worse {
                    best_move = Some(mv);
                }
            }

            self.print_info_line(out, depth, best_move, best_score)?;

            if self.stopped {
                break;
            }
            self.completed_depth = depth;
            self.completed_score = Some(best_score);
            if self.budget.soft.is_some_and(|soft| Instant::now() >= soft)
                || self
                    .budget
                    .soft_nodes
                    .is_some_and(|nodes| self.nodes >= nodes)
            {
                break;
            }
            best_score = -Score::INF;
        }

        Ok(best_move)
    }

    fn print_info_line(
        &self,
        out: &mut impl Write,
        depth: u32,
        best_move: Option<Move>,
        best_score: Score,
    ) -> io::Result<()> {
        if let Some(mv) = best_move
            && best_score > -Score::INF
        {
            let elapsed = self.budget.start.elapsed();
            writeln!(
                out,
                "info depth {depth} score {}{} nodes {} seldepth {} nps {} time {} pv {mv}",
                best_score.to_uci(),
                if self.stopped { " lowerbound" } else { "" },
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
        search::{Budget, MoveOrdering, TranspositionTable, tests::best_move},
        test_utils::{KIWIPETE, board},
    };

    /// What a search of `fen` to `depth` writes.
    fn output(fen: &str, depth: u32) -> String {
        let mut out = Vec::new();
        Searcher::new(
            Budget::depth(Some(depth)),
            &mut TranspositionTable::new(1),
            &mut MoveOrdering::new(),
        )
        .search_root(&board(fen), &[], &mut out)
        .unwrap();
        String::from_utf8(out).unwrap()
    }

    // budget

    /// The best move and the output of a search of `fen` stopped after `nodes` nodes.
    fn search_with_node_limit(fen: &str, nodes: u64) -> (Option<String>, String) {
        let budget = Budget::depth(Some(10)).with_nodes(nodes);
        let mut out = Vec::new();
        let mut tt = TranspositionTable::new(1);
        let best = Searcher::new(budget, &mut tt, &mut MoveOrdering::new())
            .search_root(&board(fen), &[], &mut out)
            .unwrap();
        (best.map(Move::to_uci), String::from_utf8(out).unwrap())
    }

    /// The first move of the `pv` in the last line of `out`.
    fn last_pv(out: &str) -> Option<&str> {
        let pv = out.lines().last()?.split(" pv ").nth(1)?;
        pv.split_whitespace().next()
    }

    #[test]
    fn an_interrupted_iteration_is_reported_as_a_lower_bound() {
        // from the start, depth 1 takes 20 nodes and depth 2 ends at 88: 50 stops in the
        // middle of it, so its line can only say that the score is at least that much
        let (_, out) = search_with_node_limit(Board::STARTPOS, 50);
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
        // 100 nodes stop it: GUIs show the last line as the engine's choice
        let fen = "4r1k1/5ppp/n7/8/8/8/5PPP/R5K1 w - - 0 1";
        let (best, out) = search_with_node_limit(fen, 100);
        assert_ne!(best.as_deref(), Some("a1a6"), "{out}");
        assert_eq!(last_pv(&out), best.as_deref(), "{out}");

        // and wherever else the limit falls
        for fen in [Board::STARTPOS, KIWIPETE, fen] {
            for nodes in [30, 100, 300, 1_000, 3_000, 10_000] {
                let (best, out) = search_with_node_limit(fen, nodes);
                assert_eq!(last_pv(&out), best.as_deref(), "{fen} {nodes}\n{out}");
            }
        }
    }

    #[test]
    fn the_first_iteration_always_finishes() {
        // a limit below what depth 1 needs still gets a searched move
        let (best, out) = search_with_node_limit(Board::STARTPOS, 1);
        assert!(out.starts_with("info depth 1 "), "{out}");
        assert_eq!(best, best_move(Board::STARTPOS, 1));
    }

    #[test]
    fn a_soft_node_limit_lets_the_iteration_finish() {
        let budget = Budget::depth(Some(Score::MAX_PLY)).with_soft_nodes(2_000);
        let mut tt = TranspositionTable::new(1);
        let mut ordering = MoveOrdering::new();
        let mut searcher = Searcher::new(budget, &mut tt, &mut ordering);
        let mut out = Vec::new();
        searcher
            .search_root(&board(KIWIPETE), &[], &mut out)
            .unwrap();
        let out = String::from_utf8(out).unwrap();

        // the iteration that passed the limit still finished, and was the last
        assert!(searcher.nodes() >= 2_000);
        assert!(!out.contains("lowerbound"), "{out}");
        let last = out.lines().last().unwrap();
        let score = searcher.score().unwrap().to_uci();
        assert!(last.contains(&format!("score {score} ")), "{score} {last}");
    }

    #[test]
    fn stops_at_the_node_limit() {
        let budget = Budget::depth(Some(7)).with_nodes(1000);
        let mut tt = TranspositionTable::new(1);
        let mut ordering = MoveOrdering::new();
        let mut searcher = Searcher::new(budget, &mut tt, &mut ordering);
        let best = searcher
            .search_root(&board(Board::STARTPOS), &[], &mut io::sink())
            .unwrap();

        // the limit is checked at every node, so the search stops right at it
        assert!(
            (1000..1010).contains(&searcher.nodes()),
            "{}",
            searcher.nodes()
        );
        assert!(best.is_some());
    }

    #[test]
    fn no_legal_moves_means_no_best_move() {
        assert_eq!(best_move("5Q1k/8/6K1/8/8/8/8/8 b - - 0 1", 3), None);
    }

    // info

    #[test]
    fn writes_an_info_line_per_depth() {
        let out = output(Board::STARTPOS, 3);
        let best = best_move(Board::STARTPOS, 3).unwrap();
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

        // Nxd5 exd5 Rxd5 Nxd5: an exchange with no checks, so only the captures of
        // quiescence can take the search past the depth
        let out = output("7k/7p/4pn2/3p4/8/2N5/8/K2R4 w - - 0 1", 1);
        assert!(info_field(&out, "seldepth") >= 3, "{out}");
    }

    #[test]
    fn info_reports_mates_in_moves() {
        // giving mate in 1, and getting mated in 1 (Ka7 Ra1#)
        let out = output("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", 2);
        assert!(out.contains(" score mate 1 "), "{out}");

        let out = output("k7/2K5/8/8/8/8/8/7R b - - 0 1", 3);
        assert!(out.contains(" score mate -1 "), "{out}");
    }

    #[test]
    fn no_info_without_legal_moves() {
        assert_eq!(output("5Q1k/8/6K1/8/8/8/8/8 b - - 0 1", 3), "");
    }
}
