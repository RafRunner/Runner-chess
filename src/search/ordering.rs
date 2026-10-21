use std::cmp::Reverse;

use crate::{
    board::Board,
    chess_move::{Move, MoveKind},
    piece::Color,
    score::Score,
    search::NodeInfo,
};

pub(super) struct MoveOrdering {
    killers: [[Option<Move>; 2]; Score::MAX_PLY as usize],
    // History is 34 KB. When moving this struct to UCI, wrap inside a box to avoid copying
    history: [[[i32; 64]; 64]; 2],
}

impl MoveOrdering {
    const NOISY_BASE: i32 = 2_000_000;
    const KILLER_BASE: i32 = 1_000_000;
    pub(super) const MAX_BONUS: i32 = 16_384;

    pub(super) fn new() -> Self {
        Self {
            killers: [[None; 2]; Score::MAX_PLY as usize],
            history: [[[0; 64]; 64]; 2],
        }
    }

    pub(super) fn on_beta_cutoff(&mut self, node: NodeInfo, mv: Move, quiets_tried: &[Move]) {
        if !mv.is_quiet() {
            return;
        }
        let old = self.killers[node.ply as usize][0].replace(mv);
        if old != Some(mv) {
            self.killers[node.ply as usize][1] = old;
        }

        let bonus = (300 * node.depth as i32 - 250).min(Self::MAX_BONUS);

        self.update_history(node.board.side_to_move(), mv, bonus);

        for &mv in quiets_tried {
            self.update_history(node.board.side_to_move(), mv, -bonus);
        }
    }

    pub(super) fn is_killer(&self, mv: Move, ply: u32) -> bool {
        self.killers[ply as usize][0] == Some(mv) || self.killers[ply as usize][1] == Some(mv)
    }

    pub(super) fn history_bonus(&self, mv: Move, color: Color) -> i32 {
        self.history[color][mv.from()][mv.to()]
    }

    fn update_history(&mut self, color: Color, mv: Move, bonus: i32) {
        let history = &mut self.history[color];
        let h = history[mv.from()][mv.to()];
        history[mv.from()][mv.to()] += bonus - h * bonus.abs() / Self::MAX_BONUS;
    }

    fn move_sort_key(&self, board: &Board, ply: u32, mv: Move, first: Option<Move>) -> i32 {
        if first == Some(mv) {
            return i32::MAX;
        }
        if !mv.is_quiet() {
            let mut key = Self::NOISY_BASE;
            if mv.is_capture() {
                let attacker = board
                    .piece_at(mv.from())
                    .map_or(0, |p| p.kind().index() as i32);
                let victim = board
                    .piece_at(mv.to())
                    .map_or(0, |p| p.kind().index() as i32);

                key += 1 + 6 * victim + (5 - attacker);
            }

            if let MoveKind::Promotion(piece) | MoveKind::PromotionCapture(piece) = mv.kind() {
                key += piece.to_piece_kind().index() as i32 * 6;
            }

            return key;
        }

        match self.killers[ply as usize]
            .iter()
            .position(|&killer| killer == Some(mv))
        {
            Some(idx) => Self::KILLER_BASE + (1 - idx as i32),
            None => self.history_bonus(mv, board.side_to_move()),
        }
    }

    pub(super) fn legal_ordered<'board>(
        &self,
        board: &'board Board,
        ply: u32,
        first: Option<Move>,
    ) -> impl Iterator<Item = (Move, Board)> + 'board {
        self.ordered_moves(board, ply, |_| true, first)
    }

    pub(super) fn captures_ordered<'board>(
        &self,
        board: &'board Board,
    ) -> impl Iterator<Item = (Move, Board)> + 'board {
        self.ordered_moves(board, 0, |mv| mv.is_capture(), None)
    }

    /// Possibly strong moves come first
    fn ordered_moves<'board>(
        &self,
        board: &'board Board,
        ply: u32,
        filter: impl Fn(&Move) -> bool,
        first: Option<Move>,
    ) -> impl Iterator<Item = (Move, Board)> + 'board {
        let mut moves = board.generate_moves();
        moves.retain(|mv| filter(mv));
        moves.sort_by_cached_key(|&mv| Reverse(self.move_sort_key(board, ply, mv, first)));

        moves
            .into_iter()
            .filter_map(move |mv| board.make_legal_move(mv).map(|next| (mv, next)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{KIWIPETE, PERFT_POSITIONS, board, find_move};

    /// The legal moves of `fen` in the order the search tries them at `ply`, with `first`
    /// (the move from the TT) put before the others.
    fn ordered_with(
        ordering: &MoveOrdering,
        fen: &str,
        ply: u32,
        first: Option<Move>,
    ) -> Vec<Move> {
        ordering
            .legal_ordered(&board(fen), ply, first)
            .map(|(mv, _)| mv)
            .collect()
    }

    /// The order before the search taught anything to the ordering.
    fn ordered(fen: &str) -> Vec<Move> {
        ordered_after(fen, None)
    }

    /// The same, with `first` put before the others.
    fn ordered_after(fen: &str, first: Option<Move>) -> Vec<Move> {
        ordered_with(&MoveOrdering::new(), fen, 0, first)
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

    /// The quiet moves of `moves`, in the order they come.
    fn quiets(moves: &[Move]) -> Vec<String> {
        moves
            .iter()
            .filter(|mv| mv.is_quiet())
            .map(|mv| mv.to_uci())
            .collect()
    }

    /// The quiet moves of `fen` before the search taught anything to the ordering.
    fn fresh_quiets(fen: &str) -> Vec<String> {
        quiets(&ordered(fen))
    }

    /// `uci` caused a beta cutoff in `fen`, after the quiet moves `tried` failed to.
    fn cut(
        ordering: &mut MoveOrdering,
        fen: &str,
        uci: &str,
        ply: u32,
        depth: u32,
        tried: &[&str],
    ) {
        let position = board(fen);
        let tried: Vec<Move> = tried.iter().map(|t| find_move(&position, t)).collect();
        ordering.on_beta_cutoff(
            NodeInfo::full_window(&position, depth, ply),
            find_move(&position, uci),
            &tried,
        );
    }

    /// No capture or promotion comes after a quiet move.
    fn assert_noisy_first(moves: &[Move]) {
        let first_quiet = moves
            .iter()
            .position(|mv| mv.is_quiet())
            .unwrap_or(moves.len());
        assert!(
            moves[first_quiet..].iter().all(|mv| mv.is_quiet()),
            "{moves:?}"
        );
    }

    // the order of a fresh ordering

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
    fn the_tt_move_comes_first_and_only_once() {
        // the last move gets promoted, and the others keep their order
        for fen in PERFT_POSITIONS {
            let moves = ordered(fen);
            let last = *moves.last().unwrap();
            let after = ordered_after(fen, Some(last));

            assert_eq!(after[0], last, "{fen}");
            assert_eq!(after[1..], moves[..moves.len() - 1], "{fen}");
        }
    }

    #[test]
    fn captures_come_before_quiet_moves() {
        for fen in PERFT_POSITIONS {
            assert_noisy_first(&ordered(fen));
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

    #[test]
    fn quiescence_gets_only_the_captures_in_the_same_order() {
        for fen in PERFT_POSITIONS {
            let captures: Vec<Move> = MoveOrdering::new()
                .captures_ordered(&board(fen))
                .map(|(mv, _)| mv)
                .collect();
            let expected: Vec<Move> = ordered(fen)
                .into_iter()
                .filter(|mv| mv.is_capture())
                .collect();

            assert_eq!(captures, expected, "{fen}");
        }
    }

    // killers

    #[test]
    fn a_killer_is_the_first_quiet_move_of_its_ply() {
        // the last one, so it can only get to the front by being a killer
        let killer = fresh_quiets(KIWIPETE).pop().unwrap();
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);

        let moves = ordered_with(&ordering, KIWIPETE, 3, None);
        assert_noisy_first(&moves);
        assert_eq!(quiets(&moves)[0], killer, "{moves:?}");
    }

    #[test]
    fn the_tt_move_comes_before_the_killers() {
        let mut fresh = fresh_quiets(KIWIPETE);
        let (killer, tt) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let tt_move = find_move(&board(KIWIPETE), &tt);
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);

        let moves = ordered_with(&ordering, KIWIPETE, 3, Some(tt_move));
        assert_eq!(moves[0], tt_move, "{moves:?}");
        assert_noisy_first(&moves[1..]);
        assert_eq!(quiets(&moves[1..])[0], killer, "{moves:?}");
    }

    #[test]
    fn killers_belong_to_their_ply() {
        // `strong` cut deeper, so only being a killer puts `killer` before it
        let mut fresh = fresh_quiets(KIWIPETE);
        let (killer, strong) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);
        cut(&mut ordering, KIWIPETE, &strong, 9, 10, &[]);

        let at_3 = quiets(&ordered_with(&ordering, KIWIPETE, 3, None));
        assert_eq!(at_3[..2], [killer.as_str(), strong.as_str()], "{at_3:?}");

        let at_4 = quiets(&ordered_with(&ordering, KIWIPETE, 4, None));
        assert_eq!(at_4[..2], [strong.as_str(), killer.as_str()], "{at_4:?}");
    }

    #[test]
    fn each_ply_keeps_the_two_newest_killers() {
        // `strong` cut deeper than the others, so it beats any of them that isn't a killer
        let mut fresh = fresh_quiets(KIWIPETE);
        let strong = fresh.pop().unwrap();
        let (a, b, c) = (
            fresh.pop().unwrap(),
            fresh.pop().unwrap(),
            fresh.pop().unwrap(),
        );
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &strong, 9, 10, &[]);
        for mv in [&a, &b, &c] {
            cut(&mut ordering, KIWIPETE, mv, 2, 1, &[]);
        }

        let moves = quiets(&ordered_with(&ordering, KIWIPETE, 2, None));
        assert_eq!(
            moves[..4],
            [c.as_str(), b.as_str(), strong.as_str(), a.as_str()],
            "{moves:?}"
        );
    }

    #[test]
    fn a_repeated_killer_does_not_push_out_the_other() {
        let mut fresh = fresh_quiets(KIWIPETE);
        let strong = fresh.pop().unwrap();
        let (a, b) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &strong, 9, 10, &[]);
        for mv in [&a, &b, &b] {
            cut(&mut ordering, KIWIPETE, mv, 2, 1, &[]);
        }

        let moves = quiets(&ordered_with(&ordering, KIWIPETE, 2, None));
        assert_eq!(
            moves[..3],
            [b.as_str(), a.as_str(), strong.as_str()],
            "{moves:?}"
        );
    }

    #[test]
    fn killers_reach_the_deepest_ply() {
        // the search orders moves up to MAX_PLY - 1, and evaluates beyond it
        let ply = Score::MAX_PLY - 1;
        let killer = fresh_quiets(KIWIPETE).pop().unwrap();
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, ply, 1, &[]);

        let moves = quiets(&ordered_with(&ordering, KIWIPETE, ply, None));
        assert_eq!(moves[0], killer, "{moves:?}");
    }

    #[test]
    fn captures_and_promotions_teach_nothing() {
        // they get ordered by what they take and what they become
        let promotion = "4k3/1P6/8/8/8/8/8/4K3 w - - 0 1";
        for (fen, noisy) in [(KIWIPETE, "e5f7"), (promotion, "b7b8n")] {
            let mut ordering = MoveOrdering::new();
            let before = ordered_with(&ordering, fen, 3, None);
            cut(&mut ordering, fen, noisy, 3, 10, &[]);

            for ply in [0, 3] {
                assert_eq!(
                    ordered_with(&ordering, fen, ply, None),
                    before,
                    "{fen} {noisy}"
                );
            }
        }
    }

    #[test]
    fn a_noisy_cutoff_keeps_the_killers() {
        // `strong` cut deeper, so `killer` only stays ahead of it while it is a killer
        let mut fresh = fresh_quiets(KIWIPETE);
        let (killer, strong) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &strong, 9, 10, &[]);
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);
        for noisy in ["e5f7", "e5d7"] {
            cut(&mut ordering, KIWIPETE, noisy, 3, 1, &[]);
        }

        let moves = quiets(&ordered_with(&ordering, KIWIPETE, 3, None));
        assert_eq!(moves[..2], [killer.as_str(), strong.as_str()], "{moves:?}");
    }

    // history

    #[test]
    fn history_brings_a_cutting_quiet_move_forward_at_every_ply() {
        let late = fresh_quiets(KIWIPETE).pop().unwrap();
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &late, 9, 4, &[]);

        // plies where it isn't a killer
        for ply in [0, 1, 8, 10] {
            let moves = ordered_with(&ordering, KIWIPETE, ply, None);
            assert_noisy_first(&moves);
            assert_eq!(quiets(&moves)[0], late, "ply {ply}: {moves:?}");
        }
    }

    #[test]
    fn deeper_cutoffs_weigh_more() {
        // `shallow` comes first in a fresh order, so equal bonuses would keep it ahead
        let fresh = fresh_quiets(KIWIPETE);
        let (shallow, deep) = (&fresh[0], &fresh[1]);
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, shallow, 9, 2, &[]);
        cut(&mut ordering, KIWIPETE, deep, 9, 8, &[]);

        let moves = quiets(&ordered_with(&ordering, KIWIPETE, 0, None));
        assert!(
            index_of(&moves, deep) < index_of(&moves, shallow),
            "{moves:?}"
        );
    }

    #[test]
    fn quiets_tried_before_a_cutoff_go_last() {
        let fresh = fresh_quiets(KIWIPETE);
        let (tried, cutter) = (&fresh[0], fresh.last().unwrap());
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, cutter, 9, 4, &[tried.as_str()]);

        let moves = quiets(&ordered_with(&ordering, KIWIPETE, 0, None));
        assert_eq!(moves.first(), Some(cutter), "{moves:?}");
        assert_eq!(moves.last(), Some(tried), "{moves:?}");
    }

    #[test]
    fn history_stays_below_captures_and_killers() {
        // no amount of cutoffs takes a move out of the quiet band, either way
        let mut fresh = fresh_quiets(KIWIPETE);
        let (killer, pushed) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let pulled = fresh.remove(0);
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);

        let kiwipete = board(KIWIPETE);
        let (pushed_move, pulled_move) =
            (find_move(&kiwipete, &pushed), find_move(&kiwipete, &pulled));
        for _ in 0..100_000 {
            ordering.on_beta_cutoff(
                NodeInfo::full_window(&kiwipete, 9, 20),
                pushed_move,
                &[pulled_move],
            );
        }

        let at_0 = ordered_with(&ordering, KIWIPETE, 0, None);
        assert_noisy_first(&at_0);
        let quiets_at_0 = quiets(&at_0);
        assert_eq!(quiets_at_0.first(), Some(&pushed), "{quiets_at_0:?}");
        assert_eq!(quiets_at_0.last(), Some(&pulled), "{quiets_at_0:?}");

        let at_3 = ordered_with(&ordering, KIWIPETE, 3, None);
        assert_noisy_first(&at_3);
        assert_eq!(
            quiets(&at_3)[..2],
            [killer.as_str(), pushed.as_str()],
            "{at_3:?}"
        );
    }

    #[test]
    fn history_is_kept_apart_for_each_color() {
        // the same pieces on the same squares, so every move exists for both colors
        let white = "4k3/8/8/8/8/8/8/R3K3 w - - 0 1";
        let black = "4K3/8/8/8/8/8/8/r3k3 b - - 0 1";
        let before = ordered(black);
        let late = quiets(&before).pop().unwrap();
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, white, &late, 9, 10, &[]);

        assert_eq!(quiets(&ordered_with(&ordering, white, 0, None))[0], late);
        assert_eq!(ordered_with(&ordering, black, 0, None), before);
    }
}
