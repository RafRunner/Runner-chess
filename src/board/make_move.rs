use crate::{
    board::Board,
    chess_move::{Move, MoveKind},
    piece::{Color, Piece, PieceKind},
    square::Square,
};

impl Board {
    pub fn make_move(&self, mv: Move) -> Self {
        let mut next = self.clone();
        next.en_passant = None;
        if let Some(ep) = self.en_passant {
            next.hash = next.hash.toggle_en_passant_key(ep);
        }

        let us = self.side_to_move;
        let them = us.oposite();
        let white_to_move = us == Color::White;

        let from = mv.from();
        let to = mv.to();
        let piece = self.piece_at(from).expect("No piece in 'from' square");
        let target = self.piece_at(to);

        match mv.kind() {
            MoveKind::Normal | MoveKind::Capture => {
                if let Some(captured) = target {
                    next.remove_piece(captured, to);
                }
                next.move_piece(piece, from, to, None);

                debug_assert_eq!(
                    target.is_some(),
                    mv.is_capture(),
                    "{mv}: capture flag doesn't match the board"
                );
            }
            MoveKind::DoublePush => {
                next.move_piece(piece, from, to, None);

                let ep_square = mv.get_ep_square(us);

                if self.can_take_ep(ep_square, them) {
                    next.en_passant = Some(ep_square);
                    next.hash = next.hash.toggle_en_passant_key(ep_square);
                }
            }
            MoveKind::EnPassant => {
                next.move_piece(piece, from, to, None);

                let captured = mv.get_ep_square(us);

                next.remove_piece(Piece::new(them, PieceKind::Pawn), captured);
            }
            MoveKind::Castle => {
                // move the king
                next.move_piece(piece, from, to, None);

                // and the rook
                let (rook_from, rook_to) = mv.get_rook_squares();

                next.move_piece(Piece::new(us, PieceKind::Rook), rook_from, rook_to, None);
            }
            MoveKind::Promotion(kind) | MoveKind::PromotionCapture(kind) => {
                if let Some(captured) = target {
                    next.remove_piece(captured, to);
                }
                let new_piece = Some(Piece::new(us, kind.to_piece_kind()));
                next.move_piece(piece, from, to, new_piece);

                debug_assert_eq!(
                    target.is_some(),
                    mv.is_capture(),
                    "{mv}: capture flag doesn't match the board"
                );
            }
        }

        // Housekeeping
        next.castling = self.castling.update_move(mv);
        next.side_to_move = them;
        next.hash = next
            .hash
            .toggle_castling_key(self.castling())
            .toggle_castling_key(next.castling())
            .toggle_turn_key();

        // Clocks
        if piece.kind() == PieceKind::Pawn || mv.is_capture() {
            next.halfmove_clock = 0;
        } else {
            next.halfmove_clock += 1;
        }
        if !white_to_move {
            next.fullmove_counter += 1;
        }

        debug_assert_eq!(next.check_invariants(), Ok(()));

        next
    }

    fn move_piece(&mut self, piece: Piece, from: Square, to: Square, new_piece: Option<Piece>) {
        let new_piece = new_piece.unwrap_or(piece);

        self.pieces[piece] &= !from.bb();
        self.pieces[new_piece] |= to.bb();
        self.by_color[piece.color()] ^= from.bb() | to.bb();

        self.mailbox[from] = None;
        self.mailbox[to] = Some(new_piece);

        self.hash = self
            .hash
            .toggle_piece_key(piece, from)
            .toggle_piece_key(new_piece, to);
    }

    fn remove_piece(&mut self, piece: Piece, sq: Square) {
        self.pieces[piece] &= !sq.bb();
        self.by_color[piece.color()] &= !sq.bb();

        self.mailbox[sq] = None;

        self.hash = self.hash.toggle_piece_key(piece, sq);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        board::Board,
        test_utils::{CASTLING_BLACK, CASTLING_WHITE, board, find_move},
    };

    /// plays `uci` on `fen` and compares the whole resulting FEN, clocks included
    fn check(fen: &str, uci: &str, expected: &str) {
        let before = board(fen);
        let after = before.make_move(find_move(&before, uci));
        // make_move's debug_assert does this too, but not in release builds
        assert_eq!(after.check_invariants(), Ok(()), "{fen} {uci}");
        assert_eq!(after.to_fen(), expected, "{fen} {uci}");
    }

    #[test]
    fn quiet_moves() {
        check(
            Board::STARTPOS,
            "g1f3",
            "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPPPPP/RNBQKB1R b KQkq - 1 1",
        );
        check(
            Board::STARTPOS,
            "e2e3",
            "rnbqkbnr/pppppppp/8/8/8/4P3/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
        );
    }

    #[test]
    fn captures() {
        check(
            "4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1",
            "e4d5",
            "4k3/8/8/3P4/8/8/8/4K3 b - - 0 1",
        );
        check(
            "4k3/8/8/3p4/4P3/8/8/4K3 b - - 0 1",
            "d5e4",
            "4k3/8/8/8/4p3/8/8/4K3 w - - 0 2",
        );
        check(
            "4k3/8/8/3q4/8/4N3/8/4K3 w - - 0 1",
            "e3d5",
            "4k3/8/8/3N4/8/8/8/4K3 b - - 0 1",
        );
    }

    #[test]
    fn double_push_sets_en_passant_when_a_pawn_can_take() {
        #[rustfmt::skip]
        let cases = [
            // a black pawn on either side of the pushed one
            ("4k3/8/8/8/3p4/8/4P3/4K3 w - - 0 1", "e2e4", "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1"),
            ("4k3/8/8/8/5p2/8/4P3/4K3 w - - 0 1", "e2e4", "4k3/8/8/8/4Pp2/8/8/4K3 b - e3 0 1"),
            ("4k3/3p4/8/4P3/8/8/8/4K3 b - - 0 1", "d7d5", "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2"),
            // on the edges of the board
            ("4k3/8/8/8/1p6/8/P7/4K3 w - - 0 1",  "a2a4", "4k3/8/8/8/Pp6/8/8/4K3 b - a3 0 1"),
            ("4k3/7p/8/6P1/8/8/8/4K3 b - - 0 1",  "h7h5", "4k3/8/8/6Pp/8/8/8/4K3 w - h6 0 2"),
            // legality isn't checked, like in Polyglot: taking would leave the black
            // king in check from the rook
            ("8/8/8/8/R3p2k/8/3P4/4K3 w - - 0 1", "d2d4", "8/8/8/8/R2Pp2k/8/8/4K3 b - d3 0 1"),
        ];
        for (fen, uci, expected) in cases {
            check(fen, uci, expected);
        }
    }

    #[test]
    fn double_push_without_a_pawn_beside_leaves_no_en_passant() {
        #[rustfmt::skip]
        let cases = [
            (Board::STARTPOS, "e2e4", "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"),
            // the pawn on e4 is diagonal to d5, it can only take it normally
            (
                "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
                "d7d5",
                "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
            ),
            // the pawn beside it has the same color
            ("4k3/8/8/8/3P4/8/4P3/4K3 w - - 0 1", "e2e4", "4k3/8/8/8/3PP3/8/8/4K3 b - - 0 1"),
            // a black pawn diagonal to e3, but behind it instead of beside e4
            ("4k3/8/8/8/8/8/3pP3/K7 w - - 0 1",   "e2e4", "4k3/8/8/8/4P3/8/3p4/K7 b - - 0 1"),
            // a pawn on the other edge of the board
            ("4k3/8/8/8/8/7p/P7/4K3 w - - 0 1",   "a2a4", "4k3/8/8/8/P7/7p/8/4K3 b - - 0 1"),
        ];
        for (fen, uci, expected) in cases {
            check(fen, uci, expected);
        }
    }

    #[test]
    fn any_other_move_clears_en_passant() {
        check(
            "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1",
            "e8d8",
            "3k4/8/8/8/3pP3/8/8/4K3 w - - 1 2",
        );
    }

    /// the captured pawn is beside the capturing one, not on the target square
    #[test]
    fn en_passant_removes_the_pawn_behind() {
        check(
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "e5d6",
            "4k3/8/3P4/8/8/8/8/4K3 b - - 0 1",
        );
        check(
            "4k3/8/8/8/3Pp3/8/8/4K3 b - d3 0 1",
            "e4d3",
            "4k3/8/8/8/8/3p4/8/4K3 w - - 0 2",
        );
    }

    #[test]
    fn castling_moves_the_rook_too() {
        #[rustfmt::skip]
        let cases = [
            (CASTLING_WHITE, "e1g1", "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1"),
            (CASTLING_WHITE, "e1c1", "r3k2r/8/8/8/8/8/8/2KR3R b kq - 1 1"),
            (CASTLING_BLACK, "e8g8", "r4rk1/8/8/8/8/8/8/R3K2R w KQ - 1 2"),
            (CASTLING_BLACK, "e8c8", "2kr3r/8/8/8/8/8/8/R3K2R w KQ - 1 2"),
        ];
        for (fen, uci, expected) in cases {
            check(fen, uci, expected);
        }
    }

    #[test]
    fn castling_rights_follow_king_and_rooks() {
        #[rustfmt::skip]
        let cases = [
            (CASTLING_WHITE, "e1f1", "r3k2r/8/8/8/8/8/8/R4K1R b kq - 1 1"), // king moves
            (CASTLING_WHITE, "a1b1", "r3k2r/8/8/8/8/8/8/1R2K2R b Kkq - 1 1"), // one rook moves
            // a rook capturing a rook: one side loses it by moving, the other by capture
            (CASTLING_WHITE, "h1h8", "r3k2R/8/8/8/8/8/8/R3K3 b Qq - 0 1"),
            (CASTLING_BLACK, "a8a1", "4k2r/8/8/8/8/8/8/r3K2R w Kk - 0 2"),
        ];
        for (fen, uci, expected) in cases {
            check(fen, uci, expected);
        }
    }

    #[test]
    fn promotions() {
        for letter in ['q', 'r', 'b', 'n'] {
            let piece = letter.to_ascii_uppercase();
            check(
                "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
                &format!("a7a8{letter}"),
                &format!("{piece}3k3/8/8/8/8/8/8/4K3 b - - 0 1"),
            );
        }
        // capturing
        check(
            "1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            "a7b8n",
            "1N2k3/8/8/8/8/8/8/4K3 b - - 0 1",
        );
        check(
            "4k3/8/8/8/8/8/p7/1R2K3 b - - 0 1",
            "a2b1q",
            "4k3/8/8/8/8/8/8/1q2K3 w - - 0 2",
        );
        // capturing the h1 rook also takes away its castling right
        check(
            "4k3/8/8/8/8/8/6p1/4K2R b K - 0 1",
            "g2h1q",
            "4k3/8/8/8/8/8/8/4K2q w - - 0 2",
        );
    }

    #[test]
    fn clocks() {
        #[rustfmt::skip]
        let cases = [
            // pawn moves and captures reset the halfmove clock, anything else counts up
            ("4k3/8/8/8/8/8/4P3/4K3 w - - 7 30",        "e2e3", "4k3/8/8/8/8/4P3/8/4K3 b - - 0 30"),
            ("4k3/8/8/3q4/8/4N3/8/4K3 w - - 7 30",      "e3d5", "4k3/8/8/3N4/8/8/8/4K3 b - - 0 30"),
            ("4k3/8/8/8/8/8/8/4K2R w K - 7 30",         "h1h2", "4k3/8/8/8/8/8/7R/4K3 b - - 8 30"),
            // the fullmove number goes up after black moves
            ("4k3/8/8/8/8/8/8/4K3 b - - 7 30",          "e8d8", "3k4/8/8/8/8/8/8/4K3 w - - 8 31"),
        ];
        for (fen, uci, expected) in cases {
            check(fen, uci, expected);
        }
    }

    #[test]
    fn italian_game_into_castling() {
        let mut b = board(Board::STARTPOS);
        for uci in ["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6", "e1g1"] {
            b = b.make_move(find_move(&b, uci));
        }
        assert_eq!(
            b.to_fen(),
            "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQ1RK1 b kq - 5 4"
        );
    }
}
