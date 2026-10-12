mod limits;
mod ordering;

pub use limits::Limits;

use std::{
    io::{self, Write},
    time::Instant,
};

use crate::{
    bench,
    board::Board,
    chess_move::Move,
    eval::{eval, INF_SCORE, MATE, MAX_EVAL, MAX_PLY},
};

pub struct Searcher {
    nodes: u64,
    seldepth: u32,
    start: Instant,
    deadline: Option<Instant>, // from wtime/winc or movetime
    stopped: bool,             // later: Arc<AtomicBool> for UCI `stop`
                               // later: killers, history, a handle to the TT
}

const MAX_DEPTH: u32 = 7;

impl Searcher {
    pub fn new() -> Self {
        Self {
            nodes: 0,
            seldepth: 0,
            start: Instant::now(),
            deadline: None,
            stopped: false,
        }
    }

    /// Searches to the depth in `limits` and writes an `info` line about it to `out`.
    pub fn search_root(
        &mut self,
        board: &Board,
        limits: Limits,
        out: &mut impl Write,
    ) -> io::Result<Option<Move>> {
        let depth = limits.depth.unwrap_or(MAX_DEPTH);
        let mut best_score = -INF_SCORE;

        let legal_ordered: Vec<(Move, Board)> = self.legal_ordered(board).collect();

        let mut best_move = legal_ordered.first().map(|(mv, _)| *mv);

        for (mv, next) in legal_ordered {
            let score = -self.negamax(&next, depth - 1, 1, -INF_SCORE, -best_score);

            if score > best_score {
                best_score = score;
                best_move = Some(mv);
            }
        }

        if let Some(mv) = best_move {
            let elapsed = self.start.elapsed();
            writeln!(
                out,
                "info depth {depth} score {} nodes {} seldepth {} nps {} time {} pv {mv}",
                uci_score(best_score),
                self.nodes,
                self.seldepth,
                bench::nps(self.nodes, elapsed),
                elapsed.as_millis(),
            )?;
        }

        Ok(best_move)
    }

    fn negamax(&mut self, board: &Board, depth: u32, ply: u32, mut alpha: i16, beta: i16) -> i16 {
        if depth == 0 {
            return self.quiescence(board, ply, alpha, beta);
        }
        if ply >= MAX_PLY as u32 {
            return eval(board);
        }
        self.nodes += 1;

        let mut best = -INF_SCORE;
        let mut has_legal_move = false;

        for (_, next) in self.legal_ordered(board) {
            has_legal_move = true;
            let score = -self.negamax(&next, depth - 1, ply + 1, -beta, -alpha);

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
                -MATE + ply as i16
            } else {
                0
            };
        }

        best
    }

    fn quiescence(&mut self, board: &Board, ply: u32, mut alpha: i16, beta: i16) -> i16 {
        if ply > self.seldepth {
            self.seldepth = ply;
        }
        if ply >= MAX_PLY as u32 {
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
            let score = -self.quiescence(&next, ply + 1, -beta, -alpha);

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
}

impl Default for Searcher {
    fn default() -> Self {
        Self::new()
    }
}

/// A score as UCI wants it: `cp <centipawns>`, or `mate <moves>`, negative when
/// the side to move is the one getting mated.
fn uci_score(score: i16) -> String {
    if score.abs() <= MAX_EVAL {
        return format!("cp {score}");
    }

    let plies = MATE - score.abs();
    let moves = (plies + 1) / 2;
    format!("mate {}", if score > 0 { moves } else { -moves })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{board, mirror_fen, PERFT_POSITIONS};

    // search

    fn best_move(fen: &str, depth: u32) -> Option<String> {
        let limits = Limits {
            depth: Some(depth),
            ..Limits::default()
        };
        Searcher::new()
            .search_root(&board(fen), limits, &mut io::sink())
            .unwrap()
            .map(Move::to_uci)
    }

    /// What a search of `fen` to `depth` writes.
    fn output(fen: &str, depth: u32) -> String {
        let limits = Limits {
            depth: Some(depth),
            ..Limits::default()
        };
        let mut out = Vec::new();
        Searcher::new()
            .search_root(&board(fen), limits, &mut out)
            .unwrap();
        String::from_utf8(out).unwrap()
    }

    /// The score of `fen` for the side to move, searched `depth` plies deep.
    fn score(fen: &str, depth: u32) -> i16 {
        Searcher::new().negamax(&board(fen), depth, 0, -INF_SCORE, INF_SCORE)
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
            assert_eq!(score(&fen, 1), -MATE, "{fen}");
        }
        for fen in [stalemated.to_string(), mirror_fen(stalemated)] {
            assert_eq!(score(&fen, 1), 0, "{fen}");
        }
        // one ply until the mate
        for fen in [mate_in_one.to_string(), mirror_fen(mate_in_one)] {
            assert_eq!(score(&fen, 2), MATE - 1, "{fen}");
        }
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
            assert!(eval(&board(&fen)) < -500, "{fen}");
            assert!(score(&fen, 0) > 0, "{fen}");
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
        // Nxa8 follows, which standing pat on the static eval would miss
        for fen in both_colors("r3k3/2N5/8/8/8/8/8/7K b - - 0 1") {
            assert!(eval(&board(&fen)) > 100, "{fen}");
            assert!(score(&fen, 0) < -100, "{fen}");
        }
    }

    #[test]
    fn quiescence_sees_mate() {
        // checkmated right at the horizon
        for fen in both_colors("5Q1k/8/6K1/8/8/8/8/8 b - - 0 1") {
            assert_eq!(score(&fen, 0), -MATE, "{fen}");
        }
    }

    // info

    #[test]
    fn writes_one_info_line() {
        let out = output(Board::STARTPOS, 2);
        let best = best_move(Board::STARTPOS, 2).unwrap();

        assert_eq!(out.lines().count(), 1, "{out}");
        assert!(out.starts_with("info depth 2 score cp "), "{out}");
        for field in [" nodes ", " seldepth ", " nps ", " time "] {
            assert!(out.contains(field), "{out}");
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

    #[test]
    fn uci_score_format() {
        assert_eq!(uci_score(35), "cp 35");
        assert_eq!(uci_score(-120), "cp -120");
        assert_eq!(uci_score(MATE - 1), "mate 1");
        assert_eq!(uci_score(MATE - 3), "mate 2");
        assert_eq!(uci_score(-MATE + 2), "mate -1");
        assert_eq!(uci_score(-MATE + 4), "mate -2");
    }
}
