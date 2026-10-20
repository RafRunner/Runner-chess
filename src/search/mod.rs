mod draw;
mod limits;
mod node;
mod ordering;
mod tt;

pub use limits::Budget;
pub use limits::Limits;
pub use tt::Bound;
pub use tt::Entry;
pub use tt::TranspositionTable;

use node::{NodeInfo, Outcome};

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::{
    bench, board::Board, chess_move::Move, eval::eval, score::Score, zobrist::ZobristHash,
};

/// A search of one position, from `go` to `bestmove`. `'search` is how long it can use
/// what it borrows from the engine: the table and the `stop` flag.
pub struct Searcher<'search> {
    budget: Budget,
    nodes: u64,
    seldepth: u32,
    completed_depth: u32,
    /// The positions of the game before the root, then the line being searched
    hashes: Vec<ZobristHash>,
    tt: &'search mut TranspositionTable,
    /// The index of the hash of the root position on the hashes vec
    root_index: usize,
    null_ply: Option<u32>, // the ply that passed, in the line being searched
    /// The search ran out of budget or got stopped, and is on its way back to the root
    stopped: bool,
    /// Set from another thread when the GUI sends `stop`
    stop: Option<&'search AtomicBool>,
    // later: killers, history
}

impl<'search> Searcher<'search> {
    pub fn new(budget: Budget, tt: &'search mut TranspositionTable) -> Self {
        Self {
            budget,
            nodes: 0,
            seldepth: 0,
            completed_depth: 0,
            hashes: Vec::with_capacity(100 + Score::MAX_PLY as usize),
            tt,
            root_index: 0,
            null_ply: None,
            stopped: false,
            stop: None,
        }
    }

    /// The search also ends, as soon as it can, once `stop` gets set
    pub fn stopped_by(self, stop: &'search AtomicBool) -> Self {
        Self {
            stop: Some(stop),
            ..self
        }
    }

    /// How many positions the searches so far have visited.
    pub fn nodes(&self) -> u64 {
        self.nodes
    }

    fn push_hash(&mut self, hash: ZobristHash) {
        self.hashes.push(hash);
    }

    fn pop_hash(&mut self) {
        self.hashes.pop();
    }

    /// Searches within the budget and writes an `info` line about it to `out`.
    /// `history` has the positions of the game before `board`, oldest first, so the
    /// search can see repetitions.
    pub fn search_root(
        &mut self,
        board: &Board,
        history: &[ZobristHash],
        out: &mut impl Write,
    ) -> io::Result<Option<Move>> {
        let mut best_score = -Score::INF;

        self.hashes.extend_from_slice(history);
        self.root_index = history.len();
        self.push_hash(board.hash());

        let mut legal_ordered: Vec<(Move, Board)> = self.legal_ordered(board, None).collect();

        let mut best_move = legal_ordered.first().map(|(mv, _)| *mv);

        for depth in 1..=self.budget.depth {
            if let Some(idx) =
                best_move.and_then(|best| legal_ordered.iter().position(|(mv, _)| mv == &best))
            {
                legal_ordered[..=idx].rotate_right(1);
            }

            let mut root = NodeInfo::new(board, depth, 0, best_score, Score::INF);
            for (mv, next) in legal_ordered.iter() {
                let score = self.search_child(root.child(next));

                if self.stopped {
                    break;
                }

                if matches!(
                    root.update(&mut best_score, score),
                    Outcome::Raised | Outcome::Cut
                ) {
                    best_move = Some(*mv);
                }
            }

            self.print_info_line(out, depth, best_move, best_score)?;

            if self.stopped {
                break;
            }
            self.completed_depth = depth;
            if self.budget.soft.is_some_and(|soft| Instant::now() >= soft) {
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

    /// The score of `board` for the side to move, searched `depth` plies deep, only as
    /// precisely as the window (alpha, beta) needs:
    /// - alpha is what the side to move already has for sure elsewhere on the way from the
    ///   root, so a score at or below it won't change its choice
    /// - beta is what the opponent already has for sure elsewhere, so it won't let the game
    ///   come here if this position is worth that much: the search stops as soon as a move
    ///   reaches it
    ///
    /// Fail soft: a score outside the window is a bound (see [`Bound`]), at most that when
    /// <= alpha, at least that when >= beta. The children get (-beta, -alpha), since what
    /// one side has for sure is the limit of the other.
    fn negamax(&mut self, mut node: NodeInfo<'_>) -> Score {
        if self.check_hard_budget() {
            return Score::DRAW;
        }

        if let Some(score) = self.check_draw(node.board, node.ply) {
            return score;
        }
        if node.depth == 0 {
            return self.quiescence(node);
        }
        self.nodes += 1;
        if node.ply >= Score::MAX_PLY {
            return eval(node.board);
        }

        let entry = self.tt.probe(node.board.hash());
        if let Some(score) = self.tt_cut(entry, node) {
            return score;
        }
        let tt_move = entry.and_then(|e| e.best_move);

        // the table says the value is at most something below beta: passing won't get there
        let below_beta =
            entry.is_some_and(|e| e.bound == Bound::Upper && e.score.from_tt(node.ply) < node.beta);
        if !below_beta && let Some(null_cut) = self.null_move_pruning(node) {
            return null_cut;
        }

        let mut best = -Score::INF;
        let mut has_legal_move = false;
        let original_alpha = node.alpha;
        let mut best_move = None;

        for (mv, next) in self.legal_ordered(node.board, tt_move) {
            has_legal_move = true;

            let score = self.search_child(node.child(&next));

            if self.stopped {
                break;
            }

            match node.update(&mut best, score) {
                Outcome::Worse => (),
                Outcome::Raised => {
                    best_move = Some(mv);
                }
                Outcome::Cut => {
                    best_move = Some(mv);
                    break;
                }
            }
        }

        if !has_legal_move {
            best = if node.board.is_in_check(node.board.side_to_move()) {
                Score::mated_in(node.ply)
            } else {
                Score::DRAW
            };
        }

        if !self.stopped {
            let bound = Bound::new(best, original_alpha, node.beta);
            self.tt.store(
                node.board.hash(),
                node.depth,
                best.to_tt(node.ply),
                bound,
                best_move,
            );
        }

        best
    }

    /// Searches only captures, and every evasion when in check, until
    /// the position is quiet: the static eval is only reliable where nothing hangs
    fn quiescence(&mut self, mut node: NodeInfo<'_>) -> Score {
        if self.check_hard_budget() {
            return Score::DRAW;
        }

        if node.ply > self.seldepth {
            self.seldepth = node.ply;
        }
        if node.ply >= Score::MAX_PLY {
            self.nodes += 1;
            return eval(node.board);
        }
        if node.board.is_in_check(node.board.side_to_move()) {
            // If we're in check, we need to check all possible responses, not just captures
            return self.negamax(NodeInfo { depth: 1, ..node });
        }
        self.nodes += 1;

        let static_eval = eval(node.board);
        if static_eval >= node.beta {
            // Not capturing is already too good for the opponent. No point in continuing
            return static_eval;
        }
        if static_eval > node.alpha {
            // Found something better
            node.alpha = static_eval;
        }

        let mut best = static_eval;

        for (_, next) in self.captures_ordered(node.board) {
            // We need to push and pop to the history because of check extensions
            self.push_hash(next.hash());
            let score = -self.quiescence(node.child(&next));
            self.pop_hash();

            if self.stopped {
                break;
            }

            if node.update(&mut best, score) == Outcome::Cut {
                break;
            }
        }

        best
    }

    /// The score of `child` for the side to move here, with its hash on the stack meanwhile
    fn search_child(&mut self, child: NodeInfo<'_>) -> Score {
        self.push_hash(child.board.hash());
        let score = -self.negamax(child);
        self.pop_hash();
        score
    }

    fn check_hard_budget(&mut self) -> bool {
        if self.stopped {
            return true;
        }
        if self.completed_depth == 0 {
            return false;
        }
        if self.budget.nodes.is_some_and(|nodes| self.nodes() >= nodes) {
            self.stopped = true
        }
        if self.nodes.is_multiple_of(2048)
            && (self.budget.hard.is_some_and(|hard| Instant::now() >= hard)
                || self.stop.is_some_and(|stop| stop.load(Ordering::Relaxed)))
        {
            self.stopped = true;
        }

        self.stopped
    }

    /// The score of the table entry for `board` when it can stand for this search: deep
    /// enough, with a bound the window can use (see [`Bound`]), not at the root, which has
    /// to return a move, and not for a position that already appeared, whose score may
    /// depend on the way to it
    fn tt_cut(&self, entry: Option<Entry>, node: NodeInfo<'_>) -> Option<Score> {
        let entry = entry?;
        if node.ply == 0 || u32::from(entry.depth) < node.depth || self.has_appeared(node.board) {
            return None;
        }

        let score = entry.score.from_tt(node.ply);
        let usable = match entry.bound {
            Bound::Exact => true,
            Bound::Lower => score >= node.beta,
            Bound::Upper => score <= node.alpha,
        };
        usable.then_some(score)
    }

    /// The side to move is so far ahead that even passing the turn
    /// keeps it above beta: a shallower search confirms it
    fn null_move_pruning(&mut self, node: NodeInfo<'_>) -> Option<Score> {
        let board = node.board;

        if node.ply > 0
            && node.depth >= 3
            && self.null_ply != Some(node.ply - 1)
            && !board.is_in_check(board.side_to_move())
            && board.has_non_pawn_material(board.side_to_move()) // zugzwang protection
            && eval(board) >= node.beta
        {
            let reduce = 2 + node.depth / 4;
            let null = board.make_null_move();

            let previous = self.null_ply.replace(node.ply);
            let score = self.search_child(node.scout_child(&null, reduce, -node.beta));
            self.null_ply = previous;

            if score >= node.beta {
                return Some(if score.is_mate() { node.beta } else { score });
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{KIWIPETE, PERFT_POSITIONS, board, mirror_fen};

    // search

    fn best_move(fen: &str, depth: u32) -> Option<String> {
        Searcher::new(Budget::depth(Some(depth)), &mut TranspositionTable::new(1))
            .search_root(&board(fen), &[], &mut io::sink())
            .unwrap()
            .map(Move::to_uci)
    }

    /// What a search of `fen` to `depth` writes.
    fn output(fen: &str, depth: u32) -> String {
        let mut out = Vec::new();
        Searcher::new(Budget::depth(Some(depth)), &mut TranspositionTable::new(1))
            .search_root(&board(fen), &[], &mut out)
            .unwrap();
        String::from_utf8(out).unwrap()
    }

    /// The score of `fen` for the side to move, searched `depth` plies deep.
    pub(super) fn score(fen: &str, depth: u32) -> Score {
        score_with(&mut TranspositionTable::new(1), fen, depth)
    }

    /// The same, with a table that may remember earlier searches.
    fn score_with(tt: &mut TranspositionTable, fen: &str, depth: u32) -> Score {
        let board = board(fen);
        let mut searcher = Searcher::new(Budget::depth(Some(depth)), tt);
        searcher.push_hash(board.hash());
        let node = NodeInfo::new(&board, depth, 0, -Score::INF, Score::INF);
        searcher.negamax(node)
    }

    /// The same move seen from the other side of the board: e2e4 becomes e7e5.
    fn mirror_uci(mv: &str) -> String {
        mv.chars()
            .map(|c| match c {
                '1'..='8' => (b'1' + b'8' - c as u8) as char,
                _ => c,
            })
            .collect()
    }

    /// `expected` must be the best move of `fen`, and its mirror the best move of
    /// the mirrored position, so every rule is checked for both colors.
    pub(super) fn assert_best(fen: &str, depth: u32, expected: &str) {
        assert_eq!(best_move(fen, depth).as_deref(), Some(expected), "{fen}");

        let mirrored = mirror_fen(fen);
        assert_eq!(
            best_move(&mirrored, depth),
            Some(mirror_uci(expected)),
            "{mirrored}"
        );
    }

    /// Like `assert_best`, for a move that must not be played.
    fn assert_avoids(fen: &str, depth: u32, avoided: &str) {
        assert_ne!(best_move(fen, depth).as_deref(), Some(avoided), "{fen}");

        let mirrored = mirror_fen(fen);
        assert_ne!(
            best_move(&mirrored, depth),
            Some(mirror_uci(avoided)),
            "{mirrored}"
        );
    }

    #[test]
    fn takes_a_hanging_queen() {
        for depth in 1..=3 {
            assert_best("4k3/8/8/3q4/8/8/3R4/4K3 w - - 0 1", depth, "d2d5");
        }
    }

    #[test]
    fn avoids_a_poisoned_capture() {
        // Qxd5 wins a pawn and loses the queen to exd5, which only quiescence sees at depth 1
        for depth in 1..=3 {
            assert_avoids("4k3/8/4p3/3p4/8/8/8/3QK3 w - - 0 1", depth, "d1d5");
        }
    }

    #[test]
    fn finds_mate_in_one() {
        // at depth 1 the mated side is already in quiescence, in check without evasions
        for depth in 1..=3 {
            assert_best("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", depth, "a1a8");
        }
    }

    #[test]
    fn avoids_walking_into_mate() {
        // Rxa6 wins a knight, and then Re1 is mate, at depth 2 found by quiescence
        for depth in 2..=3 {
            assert_avoids("4r1k1/5ppp/n7/8/8/8/5PPP/R5K1 w - - 0 1", depth, "a1a6");
        }
    }

    #[test]
    fn prefers_the_fastest_mate() {
        // Qf8 mates now, other moves still mate in 2
        assert_best("7k/8/6K1/8/8/8/8/5Q2 w - - 0 1", 4, "f1f8");
    }

    #[test]
    fn does_not_stalemate_when_winning() {
        // Qc7 leaves the king on a8 without moves
        for depth in 1..=3 {
            assert_avoids("k7/8/1Q6/8/8/8/8/7K w - - 0 1", depth, "b6c7");
        }
    }

    #[test]
    fn mate_and_stalemate_scores() {
        let mated = "5Q1k/8/6K1/8/8/8/8/8 b - - 0 1";
        let stalemated = "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1";
        let mate_in_one = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";

        for fen in [mated.to_string(), mirror_fen(mated)] {
            assert_eq!(score(&fen, 1), Score::mated_in(0), "{fen}");
        }
        for fen in [stalemated.to_string(), mirror_fen(stalemated)] {
            assert_eq!(score(&fen, 1), Score::DRAW, "{fen}");
        }
        // one ply until the mate
        for fen in [mate_in_one.to_string(), mirror_fen(mate_in_one)] {
            assert_eq!(score(&fen, 2), Score::mate_in(1), "{fen}");
        }
    }

    // transposition table

    #[test]
    fn mates_from_the_tt_keep_their_distance() {
        // the second search finds the mates of the first one in the table, stored
        // plies away from the root
        for (fen, depth, mate) in [
            ("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", 2, Score::mate_in(1)),
            ("7k/8/5K2/8/8/8/8/6R1 w - - 0 1", 4, Score::mate_in(3)), // Kf7 Kh7 Rh1#
        ] {
            for fen in both_colors(fen) {
                let mut tt = TranspositionTable::new(1);
                assert_eq!(score_with(&mut tt, &fen, depth), mate, "{fen}");
                assert_eq!(score_with(&mut tt, &fen, depth), mate, "{fen}");
            }
        }
    }

    #[test]
    fn a_slower_mate_than_alpha_is_an_upper_bound() {
        // at ply 3 the mate in 1 is 4 plies from the root, short of an alpha of mate in 3
        // plies: a fail low, even though the stored score, counted from the node, is a
        // mate in 1 ply
        let board = board("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
        let mut tt = TranspositionTable::new(1);
        let mut searcher = Searcher::new(Budget::depth(None), &mut tt);
        searcher.push_hash(board.hash());

        let node = NodeInfo::new(&board, 2, 3, Score::mate_in(3), Score::INF);
        let score = searcher.negamax(node);
        assert_eq!(score, Score::mate_in(4));
        assert_eq!(tt.probe(board.hash()).unwrap().bound, Bound::Upper);
    }

    /// What `tt_cut` makes of an entry with `bound` at depth 5, for a node at ply 1
    /// and depth 5 inside the window (alpha, beta).
    fn tt_cutoff(bound: Bound, score: i32, alpha: i32, beta: i32) -> Option<Score> {
        let board = board(Board::STARTPOS);
        let mut tt = TranspositionTable::new(1);
        tt.store(board.hash(), 5, Score::new(score), bound, None);
        let mut searcher = Searcher::new(Budget::depth(None), &mut tt);
        // as in the search, the node is on the stack of hashes when it's probed
        searcher.push_hash(board.hash());
        let entry = searcher.tt.probe(board.hash());
        let node = NodeInfo::new(&board, 5, 1, Score::new(alpha), Score::new(beta));
        searcher.tt_cut(entry, node)
    }

    /// The score `tt_cut` cuts with for `board` at `depth` and `ply`, with the whole
    /// window open.
    fn tt_cut_at(searcher: &Searcher, board: &Board, depth: u32, ply: u32) -> Option<Score> {
        let entry = searcher.tt.probe(board.hash());
        let node = NodeInfo::new(board, depth, ply, -Score::INF, Score::INF);
        searcher.tt_cut(entry, node)
    }

    #[test]
    fn the_bound_decides_the_cutoff() {
        // the value is exactly 100
        assert_eq!(
            tt_cutoff(Bound::Exact, 100, 150, 200),
            Some(Score::new(100))
        );
        assert_eq!(tt_cutoff(Bound::Exact, 100, 0, 50), Some(Score::new(100)));
        // at least 100: it's enough when beta is below
        assert_eq!(tt_cutoff(Bound::Lower, 100, 0, 50), Some(Score::new(100)));
        assert_eq!(tt_cutoff(Bound::Lower, 100, 0, 200), None);
        // at most 100: it's enough when alpha is above
        assert_eq!(
            tt_cutoff(Bound::Upper, 100, 150, 200),
            Some(Score::new(100))
        );
        assert_eq!(tt_cutoff(Bound::Upper, 100, 0, 200), None);
    }

    #[test]
    fn only_a_deep_enough_entry_cuts() {
        let board = board(Board::STARTPOS);
        let mut tt = TranspositionTable::new(1);
        tt.store(board.hash(), 2, Score::new(30), Bound::Exact, None);
        let mut searcher = Searcher::new(Budget::depth(None), &mut tt);
        searcher.push_hash(board.hash());

        assert_eq!(tt_cut_at(&searcher, &board, 2, 1), Some(Score::new(30)));
        // too shallow to stand for a search of depth 3, though its move still goes first
        assert_eq!(tt_cut_at(&searcher, &board, 3, 1), None);
        // and the root never cuts, it has to return a move
        assert_eq!(tt_cut_at(&searcher, &board, 2, 0), None);
    }

    #[test]
    fn a_position_seen_before_does_not_trust_the_table() {
        // the entry may come from a time when a repetition through it wasn't possible
        let board = board("4k3/8/8/8/8/8/8/R3K3 w - - 10 30");
        let mut tt = TranspositionTable::new(1);
        tt.store(board.hash(), 5, Score::new(500), Bound::Exact, None);
        let mut searcher = Searcher::new(Budget::depth(None), &mut tt);

        searcher.push_hash(board.hash());
        assert_eq!(tt_cut_at(&searcher, &board, 5, 1), Some(Score::new(500)));

        // two moves of each side later, the same position
        for _ in 0..3 {
            searcher.push_hash(ZobristHash::empty());
        }
        searcher.push_hash(board.hash());
        assert_eq!(tt_cut_at(&searcher, &board, 5, 1), None);
    }

    #[test]
    fn an_interrupted_node_is_not_stored() {
        // the node limit stops the search in the middle of the moves of the root
        let board = board(KIWIPETE);
        let budget = Budget::depth(Some(4)).with_nodes(1000);
        let mut tt = TranspositionTable::new(1);
        let mut searcher = Searcher::new(budget, &mut tt);
        searcher.completed_depth = 1; // the first iteration is never stopped
        searcher.push_hash(board.hash());
        let node = NodeInfo::new(&board, 4, 0, -Score::INF, Score::INF);
        searcher.negamax(node);

        assert!(searcher.stopped);
        assert!(tt.probe(board.hash()).is_none());
    }

    // budget

    /// The best move and the output of a search of `fen` stopped after `nodes` nodes.
    fn search_with_node_limit(fen: &str, nodes: u64) -> (Option<String>, String) {
        let budget = Budget::depth(Some(10)).with_nodes(nodes);
        let mut out = Vec::new();
        let mut tt = TranspositionTable::new(1);
        let best = Searcher::new(budget, &mut tt)
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
    fn stops_at_the_node_limit() {
        let budget = Budget::depth(Some(7)).with_nodes(1000);
        let mut tt = TranspositionTable::new(1);
        let mut searcher = Searcher::new(budget, &mut tt);
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

    #[test]
    fn mirrored_positions_have_the_same_score() {
        for fen in PERFT_POSITIONS {
            let mirrored = mirror_fen(fen);
            // deeper is slow in debug builds: quiescence follows every capture in these
            for depth in 0..=2 {
                assert_eq!(
                    score(fen, depth),
                    score(&mirrored, depth),
                    "{fen} vs {mirrored} at depth {depth}"
                );
            }
        }
    }

    // quiescence, which is all a depth 0 search does

    /// `fen` and its mirror, so every rule is checked for both colors.
    pub(super) fn both_colors(fen: &str) -> [String; 2] {
        [fen.to_string(), mirror_fen(fen)]
    }

    #[test]
    fn quiescence_sees_the_recapture() {
        // Black is a queen down, but exd5 takes it back
        for fen in both_colors("4k3/8/4p3/3Q4/8/8/8/4K3 b - - 0 1") {
            assert!(eval(&board(&fen)) < Score::new(-500), "{fen}");
            assert!(score(&fen, 0) > Score::DRAW, "{fen}");
        }
    }

    #[test]
    fn quiescence_stands_pat_when_captures_lose() {
        // the only capture, Qxd5, loses the queen, so not capturing keeps the static eval
        for fen in both_colors("4k3/8/4p3/3p4/8/8/8/3QK3 w - - 0 1") {
            assert_eq!(score(&fen, 0), eval(&board(&fen)), "{fen}");
        }
    }

    #[test]
    fn no_stand_pat_in_check() {
        // Black is up the exchange, but Nc7+ forks king and rook: the king must move and
        // Nxa8 follows, which standing pat on the static eval would miss. The pawns keep
        // the knight from being a dead draw afterwards
        for fen in both_colors("r3k3/2N4p/8/8/8/8/7P/7K b - - 0 1") {
            assert!(eval(&board(&fen)) > Score::new(100), "{fen}");
            assert!(score(&fen, 0) < Score::new(-100), "{fen}");
        }
    }

    #[test]
    fn quiescence_sees_mate() {
        // checkmated right at the horizon
        for fen in both_colors("5Q1k/8/6K1/8/8/8/8/8 b - - 0 1") {
            assert_eq!(score(&fen, 0), Score::mated_in(0), "{fen}");
        }
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
