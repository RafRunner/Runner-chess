use std::{
    cmp::Reverse,
    io::{self, Write},
    time::{Duration, Instant},
};

use crate::{
    bench,
    board::Board,
    chess_move::{Move, MoveKind},
    eval::{eval, INF_SCORE, MATE, MAX_EVAL},
};

/// What a `go` command allows the search to spend.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Limits {
    pub depth: Option<u32>,
    pub movetime: Option<Duration>,
    /// wtime and btime, indexed by color
    pub time: [Option<Duration>; 2],
    /// winc and binc, indexed by color
    pub inc: [Duration; 2],
    pub movestogo: Option<u32>,
}

pub struct Searcher {
    nodes: u64,
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
                "info depth {depth} score {} nodes {} nps {} time {} pv {mv}",
                uci_score(best_score),
                self.nodes,
                bench::nps(self.nodes, elapsed),
                elapsed.as_millis(),
            )?;
        }

        Ok(best_move)
    }

    fn negamax(&mut self, board: &Board, depth: u32, ply: u32, mut alpha: i16, beta: i16) -> i16 {
        self.nodes += 1;
        if depth == 0 {
            return self.quiescence(board, ply, alpha, beta);
        }

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

    fn quiescence(&mut self, board: &Board, ply: u32, mut _alpha: i16, _beta: i16) -> i16 {
        let static_eval = eval(board);

        if board.legal_successors().next().is_none() {
            if board.is_in_check(board.side_to_move()) {
                -MATE + ply as i16
            } else {
                0
            }
        } else {
            static_eval
        }
    }

    fn move_sort_key(&self, board: &Board, mv: Move) -> i32 {
        let mut key: i32 = 0;

        if mv.is_capture() {
            let attacker = board
                .piece_at(mv.from())
                .map_or(0, |p| p.kind().index() as i32);
            let victim = board
                .piece_at(mv.to())
                .map_or(0, |p| p.kind().index() as i32);

            // gets bigger if the victim is more valuable than the attacker
            key += 1 + 6 * victim + (5 - attacker);
        }
        // Promotion creates a piece of such value, so we add it to the key
        if let MoveKind::Promotion(piece) | MoveKind::PromotionCapture(piece) = mv.kind() {
            key += piece.to_piece_kind().index() as i32 * 6;
        }
        // TODO maybe check for checks, but currently the check is expensive

        key
    }

    // Possibly strong moves come first
    fn legal_ordered<'a>(&self, board: &'a Board) -> impl Iterator<Item = (Move, Board)> + 'a {
        let mut pseudo_legal = Vec::new();
        board.generate_moves(&mut pseudo_legal);
        pseudo_legal.sort_by_key(|&mv| Reverse(self.move_sort_key(board, mv)));

        pseudo_legal
            .into_iter()
            .filter_map(move |mv| board.make_legal_move(mv).map(|next| (mv, next)))
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

    // move ordering

    /// The legal moves of `fen` in the order the search tries them.
    fn ordered(fen: &str) -> Vec<Move> {
        Searcher::new()
            .legal_ordered(&board(fen))
            .into_iter()
            .map(|(mv, _)| mv)
            .collect()
    }

    fn ordered_uci(fen: &str) -> Vec<String> {
        ordered(fen).into_iter().map(Move::to_uci).collect()
    }

    fn index_of(moves: &[String], uci: &str) -> usize {
        moves
            .iter()
            .position(|mv| mv == uci)
            .unwrap_or_else(|| panic!("{uci} not in {moves:?}"))
    }

    #[test]
    fn ordering_keeps_every_legal_move() {
        for fen in PERFT_POSITIONS {
            let mut ordered = ordered_uci(fen);
            let mut legal: Vec<String> = board(fen)
                .legal_successors()
                .map(|(mv, _)| mv.to_uci())
                .collect();

            ordered.sort();
            legal.sort();
            assert_eq!(ordered, legal, "{fen}");
        }
    }

    #[test]
    fn captures_come_before_quiet_moves() {
        for fen in PERFT_POSITIONS {
            let moves = ordered(fen);
            let Some(first_quiet) = moves
                .iter()
                .position(|mv| !mv.is_capture() && !mv.is_promotion())
            else {
                continue;
            };

            assert!(
                moves[first_quiet..].iter().all(|mv| !mv.is_capture()),
                "{fen}: {moves:?}"
            );
        }
    }

    #[test]
    fn most_valuable_victim_first() {
        // Qxd7 takes a rook, exf5 only a knight, even though the pawn is the cheaper attacker
        let moves = ordered_uci("6k1/3r4/8/5n2/4P3/8/8/3Q2K1 w - - 0 1");
        assert_eq!(moves[0], "d1d7", "{moves:?}");
        assert!(
            index_of(&moves, "d1d7") < index_of(&moves, "e4f5"),
            "{moves:?}"
        );
    }

    #[test]
    fn least_valuable_attacker_first() {
        // the same knight, taken by a pawn or by the queen
        let moves = ordered_uci("6k1/8/8/5n2/4P3/7Q/8/6K1 w - - 0 1");
        assert!(
            index_of(&moves, "e4f5") < index_of(&moves, "h3f5"),
            "{moves:?}"
        );
    }

    #[test]
    fn en_passant_is_ordered_as_a_capture() {
        // the target square is empty, but the victim is a pawn
        let moves = ordered_uci("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
        assert_eq!(moves[0], "e5d6", "{moves:?}");
    }

    #[test]
    fn promotions_come_by_the_value_of_the_new_piece() {
        let moves = ordered_uci("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1");
        assert_eq!(
            moves[..4],
            ["b7b8q", "b7b8r", "b7b8b", "b7b8n"],
            "{moves:?}"
        );
    }

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
        // Qxd5 wins a pawn and loses the queen to exd5
        assert_avoids("4k3/8/4p3/3p4/8/8/8/3QK3 w - - 0 1", 2, "d1d5");
    }

    #[test]
    fn finds_mate_in_one() {
        for depth in 2..=3 {
            assert_best("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", depth, "a1a8");
        }
    }

    #[test]
    fn avoids_walking_into_mate() {
        // Rxa6 wins a knight, and then Re1 is mate
        assert_avoids("4r1k1/5ppp/n7/8/8/8/5PPP/R5K1 w - - 0 1", 3, "a1a6");
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
            for depth in 1..=3 {
                assert_eq!(
                    score(fen, depth),
                    score(&mirrored, depth),
                    "{fen} vs {mirrored} at depth {depth}"
                );
            }
        }
    }

    // info

    #[test]
    fn writes_one_info_line() {
        let out = output(Board::STARTPOS, 2);
        let best = best_move(Board::STARTPOS, 2).unwrap();

        assert_eq!(out.lines().count(), 1, "{out}");
        assert!(out.starts_with("info depth 2 score cp "), "{out}");
        for field in [" nodes ", " nps ", " time "] {
            assert!(out.contains(field), "{out}");
        }
        assert!(out.ends_with(&format!(" pv {best}\n")), "{out}");
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
