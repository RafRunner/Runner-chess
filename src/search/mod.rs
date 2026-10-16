mod limits;
mod ordering;

pub use limits::Budget;
pub use limits::Limits;

use std::io::{self, Write};
use std::time::Instant;

use crate::{
    bench, board::Board, chess_move::Move, eval::eval, score::Score, zobrist::ZobristHash,
};

pub struct Searcher {
    budget: Budget,
    nodes: u64,
    seldepth: u32,
    completed_depth: u32,
    /// The positions of the game before the root, then the line being searched
    hashes: Vec<ZobristHash>,
    /// The index of the hash of the root position on the hashes vec
    root_index: usize,
    stopped: bool, // later: Arc<AtomicBool> for UCI `stop`
                   // later: killers, history, a handle to the TT
}

impl Searcher {
    pub fn new(budget: Budget) -> Self {
        Self {
            budget,
            nodes: 0,
            seldepth: 0,
            completed_depth: 0,
            hashes: Vec::with_capacity(100 + Score::MAX_PLY as usize),
            root_index: 0,
            stopped: false,
        }
    }

    /// How many positions the searches so far have visited.
    pub fn nodes(&self) -> u64 {
        self.nodes
    }

    fn push_hash(&mut self, hash: ZobristHash) {
        self.hashes.push(hash);
    }

    fn pop_hash(&mut self) -> Option<ZobristHash> {
        self.hashes.pop()
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

        let mut legal_ordered: Vec<(Move, Board)> = self.legal_ordered(board).collect();

        let mut best_move = legal_ordered.first().map(|(mv, _)| *mv);

        for depth in 1..=self.budget.depth {
            if let Some(idx) =
                best_move.and_then(|best| legal_ordered.iter().position(|(mv, _)| mv == &best))
            {
                legal_ordered[..=idx].rotate_right(1);
            }
            for (mv, next) in legal_ordered.iter() {
                self.push_hash(next.hash());
                let score = -self.negamax(next, depth - 1, 1, -Score::INF, -best_score);
                self.pop_hash();

                if self.stopped {
                    break;
                }

                if score > best_score {
                    best_score = score;
                    best_move = Some(*mv);
                }
            }

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

    fn negamax(
        &mut self,
        board: &Board,
        depth: u32,
        ply: u32,
        mut alpha: Score,
        beta: Score,
    ) -> Score {
        if self.check_hard_budget() {
            return Score::DRAW;
        }
        if let Some(score) = self.check_draw(ply, board) {
            return score;
        }
        if depth == 0 {
            return self.quiescence(board, ply, alpha, beta);
        }
        self.nodes += 1;
        if ply >= Score::MAX_PLY {
            return eval(board);
        }

        let mut best = -Score::INF;
        let mut has_legal_move = false;

        for (_, next) in self.legal_ordered(board) {
            has_legal_move = true;

            self.push_hash(next.hash());
            let score = -self.negamax(&next, depth - 1, ply + 1, -beta, -alpha);
            self.pop_hash();

            if self.stopped {
                break;
            }

            if score > best {
                best = score;

                if best > alpha {
                    // We found something better, so we raise the floor. The window closes and we cut more
                    alpha = best;

                    if alpha >= beta {
                        // Opponent has something better guaranteed, therefore we can cut the analysis here
                        break;
                    }
                }
            }
        }

        if !has_legal_move {
            return if board.is_in_check(board.side_to_move()) {
                Score::mated_in(ply)
            } else {
                Score::DRAW
            };
        }

        best
    }

    fn quiescence(&mut self, board: &Board, ply: u32, mut alpha: Score, beta: Score) -> Score {
        if self.check_hard_budget() {
            return Score::DRAW;
        }
        if ply > self.seldepth {
            self.seldepth = ply;
        }
        if ply >= Score::MAX_PLY {
            self.nodes += 1;
            return eval(board);
        }
        if board.is_in_check(board.side_to_move()) {
            // If we're in check, we need to check all possible responses, not just captures
            return self.negamax(board, 1, ply, alpha, beta);
        }
        self.nodes += 1;

        let static_eval = eval(board);
        if static_eval >= beta {
            // Not capturing is already too good for the opponent. No point in continuing
            return static_eval;
        }
        if static_eval > alpha {
            // Found something better
            alpha = static_eval;
        }

        let mut best = static_eval;

        for (_, next) in self.captures_ordered(board) {
            // We need to push and pop to the history because of check extensions
            self.push_hash(next.hash());
            let score = -self.quiescence(&next, ply + 1, -beta, -alpha);
            self.pop_hash();

            if self.stopped {
                break;
            }

            if score > best {
                best = score;

                if best > alpha {
                    // We found something better, so we raise the floor. The window closes and we cut more
                    alpha = best;

                    if alpha >= beta {
                        // Opponent has something better guaranteed, therefore we can cut the analysis here
                        break;
                    }
                }
            }
        }

        best
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
            && self.budget.hard.is_some_and(|hard| Instant::now() >= hard)
        {
            self.stopped = true;
        }

        self.stopped
    }

    /// Draws by the rules, checked before quiescence so that a repetition right at
    /// the horizon still counts. The root has to return a move anyway.
    fn check_draw(&self, ply: u32, board: &Board) -> Option<Score> {
        if ply == 0 {
            return None;
        }

        if self.is_repetition(board) {
            return Some(Score::DRAW);
        }
        if board.halfmove_clock() >= 100 {
            // unless the move that completed the fifty moves gave mate
            let mated = board.is_in_check(board.side_to_move())
                && board.legal_successors().next().is_none();

            return Some(if mated {
                Score::mated_in(ply)
            } else {
                Score::DRAW
            });
        }

        None
    }

    /// a position repeats once earlier but strictly after the root, or repeats twice before or at the root
    fn is_repetition(&self, board: &Board) -> bool {
        debug_assert_eq!(self.hashes.last(), Some(&board.hash()));

        let current = self.hashes.len() - 1;
        // nothing from before the last capture or pawn move can come back
        let reach = (board.halfmove_clock() as usize).min(current);

        let mut repetition_counter = 0;

        for back in (4..=reach).step_by(2) {
            if self.hashes[current - back] == self.hashes[current] {
                if current - back > self.root_index {
                    return true;
                }
                repetition_counter += 1;
                if repetition_counter > 1 {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{KIWIPETE, PERFT_POSITIONS, board, find_move, mirror_fen};

    // search

    fn best_move(fen: &str, depth: u32) -> Option<String> {
        Searcher::new(Budget::depth(Some(depth)))
            .search_root(&board(fen), &[], &mut io::sink())
            .unwrap()
            .map(Move::to_uci)
    }

    /// What a search of `fen` to `depth` writes.
    fn output(fen: &str, depth: u32) -> String {
        let mut out = Vec::new();
        Searcher::new(Budget::depth(Some(depth)))
            .search_root(&board(fen), &[], &mut out)
            .unwrap();
        String::from_utf8(out).unwrap()
    }

    /// The score of `fen` for the side to move, searched `depth` plies deep.
    fn score(fen: &str, depth: u32) -> Score {
        let board = board(fen);
        let mut searcher = Searcher::new(Budget::depth(Some(depth)));
        searcher.push_hash(board.hash());
        searcher.negamax(&board, depth, 0, -Score::INF, Score::INF)
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
    fn assert_best(fen: &str, depth: u32, expected: &str) {
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

    // draws by the rules

    #[test]
    fn perpetual_check_saves_the_lost_side() {
        // White is lost, but Qe8+ Kh7 Qh5+ Kg8 Qe8+ repeats, which depth 5 reaches
        for fen in both_colors("6k1/rr4p1/8/8/8/8/ppp1Q1PP/7K w - - 0 1") {
            assert!(eval(&board(&fen)) < Score::new(-100), "{fen}");
            assert_eq!(score(&fen, 5), Score::DRAW, "{fen}");
        }
    }

    #[test]
    fn fifty_move_rule() {
        // 99 plies without a capture or pawn move: only the pawn move doesn't draw
        for depth in 1..=3 {
            assert_best("6k1/8/8/8/8/7P/8/R5K1 w - - 99 80", depth, "h3h4");
        }
    }

    #[test]
    fn mate_beats_the_fifty_move_rule() {
        // Ra8# is the hundredth ply, and still mate
        for fen in both_colors("6k1/5ppp/8/8/8/8/8/R5K1 w - - 99 80") {
            assert_eq!(score(&fen, 2), Score::mate_in(1), "{fen}");
        }
    }

    #[test]
    fn trades_into_a_dead_draw_when_lost() {
        // a knight down, Black trades rooks with check: K+N vs K can't be won
        let fen = "3r3k/8/8/8/8/4N3/8/3R3K b - - 0 1";
        for depth in 2..=3 {
            assert_best(fen, depth, "d8d1");
        }
        for fen in both_colors(fen) {
            assert_eq!(score(&fen, 2), Score::DRAW, "{fen}");
        }
    }

    /// Plays `moves` from `fen` as the game so far, then searches the position they
    /// reach: the best move and what the search wrote.
    fn search_after(fen: &str, moves: &[&str], depth: u32) -> (Option<String>, String) {
        let mut b = board(fen);
        let mut history = Vec::new();
        for &uci in moves {
            history.push(b.hash());
            b = b.make_move(find_move(&b, uci));
        }

        let mut out = Vec::new();
        let best = Searcher::new(Budget::depth(Some(depth)))
            .search_root(&b, &history, &mut out)
            .unwrap();
        (best.map(Move::to_uci), String::from_utf8(out).unwrap())
    }

    /// Black is lost, with the king shuffling around h8 against a queen.
    const LOST_KING: &str = "6k1/8/8/8/8/8/8/KQ6 b - - 0 1";

    #[test]
    fn repetitions_come_from_the_game_too() {
        // Kh8 brings back a position the game already had twice
        let moves = [
            "g8h8", "b1c1", "h8g7", "c1b1", "g7h8", "b1c1", "h8g8", "c1b1",
        ];

        // at depth 1 the repetition is right at the horizon, so it has to be checked
        // before quiescence
        for depth in 1..=3 {
            let (best, out) = search_after(LOST_KING, &moves, depth);
            assert_eq!(best.as_deref(), Some("g8h8"), "{out}");
            assert!(out.contains(" score cp 0 "), "{out}");

            // the same position without the game behind it is just lost
            let (_, out) = search_after(LOST_KING, &[], depth);
            assert!(!out.contains(" score cp 0 "), "{out}");
        }
    }

    #[test]
    fn a_single_repetition_of_the_game_is_not_a_draw_yet() {
        // the game had the position after Kh8 only once: playing it again makes it the
        // second time, and White doesn't have to repeat it a third
        let moves = ["g8h8", "b1c1", "h8g8", "c1b1"];
        for depth in 1..=3 {
            let (_, out) = search_after(LOST_KING, &moves, depth);
            assert!(!out.contains(" score cp 0 "), "{out}");
        }
    }

    #[test]
    fn repetitions_inside_the_search_count_once_after_a_long_game() {
        // the perpetual of perpetual_check_saves_the_lost_side, after two quiet moves:
        // the halfmove clock reaches back into the game, but the repetition itself is
        // all inside the search, where once is enough
        let fen = "6k1/r5p1/1r6/8/8/8/ppp1Q1PP/6K1 w - - 0 1";
        let (best, out) = search_after(fen, &["g1h1", "b6b7"], 5);
        assert_eq!(best.as_deref(), Some("e2e8"), "{out}");
        assert!(out.contains(" score cp 0 "), "{out}");
    }

    // budget

    /// The best move and the output of a search of `fen` stopped after `nodes` nodes.
    fn search_with_node_limit(fen: &str, nodes: u64) -> (Option<String>, String) {
        let mut budget = Budget::depth(Some(10));
        budget.nodes = Some(nodes);
        let mut out = Vec::new();
        let best = Searcher::new(budget)
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
        let mut budget = Budget::depth(Some(7));
        budget.nodes = Some(1000);
        let mut searcher = Searcher::new(budget);
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
    fn both_colors(fen: &str) -> [String; 2] {
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
