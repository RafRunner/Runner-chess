use std::cmp::Reverse;

use super::Searcher;
use crate::{
    board::Board,
    chess_move::{Move, MoveKind},
};

impl Searcher<'_> {
    fn move_sort_key(&self, board: &Board, mv: Move, first: Option<Move>) -> i32 {
        if first == Some(mv) {
            return i32::MAX;
        }
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

    /// Possibly strong moves come first
    fn ordered_moves<'a>(
        &self,
        board: &'a Board,
        filter: impl Fn(&Move) -> bool,
        first: Option<Move>,
    ) -> impl Iterator<Item = (Move, Board)> + 'a {
        let mut moves = board.generate_moves();
        moves.retain(|mv| filter(mv));
        moves.sort_by_cached_key(|&mv| Reverse(self.move_sort_key(board, mv, first)));

        moves
            .into_iter()
            .filter_map(move |mv| board.make_legal_move(mv).map(|next| (mv, next)))
    }

    pub(super) fn legal_ordered<'a>(
        &self,
        board: &'a Board,
        first: Option<Move>,
    ) -> impl Iterator<Item = (Move, Board)> + 'a {
        self.ordered_moves(board, |_| true, first)
    }

    pub(super) fn captures_ordered<'a>(
        &self,
        board: &'a Board,
    ) -> impl Iterator<Item = (Move, Board)> + 'a {
        self.ordered_moves(board, |mv| mv.is_capture(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        search::{Budget, TranspositionTable},
        test_utils::{PERFT_POSITIONS, board},
    };

    /// The legal moves of `fen` in the order the search tries them.
    fn ordered(fen: &str) -> Vec<Move> {
        ordered_after(fen, None)
    }

    /// The same, with `first` (the move from the TT) put before the others.
    fn ordered_after(fen: &str, first: Option<Move>) -> Vec<Move> {
        Searcher::new(Budget::depth(None), &mut TranspositionTable::new(1))
            .legal_ordered(&board(fen), first)
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
}
