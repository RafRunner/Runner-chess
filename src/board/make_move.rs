use crate::{
    bitboard::BitBoard,
    board::Board,
    chess_move::{Move, MoveKind},
    piece::{Color, Piece, PieceKind},
    square::{Delta, Square},
};

impl Board {
    pub fn make_move(&self, mv: Move) -> Self {
        let mut next = self.clone();
        next.en_passant = None;

        let us = self.side_to_move;
        let them = us.oposite();
        let white_to_move = us == Color::White;

        let from = mv.from();
        let to = mv.to();
        let piece = self.piece_at(from).expect("No piece in 'from' square");
        let target = self.piece_at(to);

        let delta = if white_to_move {
            Delta::SOUTH
        } else {
            Delta::NORTH
        };

        match mv.kind() {
            MoveKind::Normal | MoveKind::Capture => {
                next.move_piece(piece, from, to, None);

                debug_assert_eq!(
                    target.is_some(),
                    mv.is_capture(),
                    "{mv}: capture flag doesn't match the board"
                );
                if let Some(captured) = target {
                    next.pieces[captured] &= !to.bb();
                }
            }
            MoveKind::DoublePush => {
                next.move_piece(piece, from, to, None);

                next.en_passant = Some(
                    to.offset(delta)
                        .expect("Double Push: unexpected 'to' Square"),
                )
            }
            MoveKind::EnPassant => {
                next.move_piece(piece, from, to, None);

                let captured = to
                    .offset(delta)
                    .expect("En Passant: unexpected 'to' Square");

                next.pieces[Piece::new(them, PieceKind::Pawn)] &= !captured.bb();
                next.mailbox[captured] = None;
            }
            MoveKind::Castle => {
                // move the king
                next.move_piece(piece, from, to, None);

                // and the rook
                let (rook_from, rook_to) = if to == Square::G1 {
                    (Square::H1, Square::F1)
                } else if to == Square::C1 {
                    (Square::A1, Square::D1)
                } else if to == Square::G8 {
                    (Square::H8, Square::F8)
                } else if to == Square::C8 {
                    (Square::A8, Square::D8)
                } else {
                    panic!("Castling: unexpected to square")
                };

                next.move_piece(Piece::new(us, PieceKind::Rook), rook_from, rook_to, None);
            }
            MoveKind::Promotion(kind) | MoveKind::PromotionCapture(kind) => {
                let new_piece = Some(Piece::new(us, kind.to_piece_kind()));

                next.move_piece(piece, from, to, new_piece);

                debug_assert_eq!(
                    target.is_some(),
                    mv.is_capture(),
                    "{mv}: capture flag doesn't match the board"
                );
                if let Some(captured) = target {
                    next.pieces[captured] &= !to.bb();
                }
            }
        }

        // Housekeeping
        next.by_color = [BitBoard::EMPTY; 2];

        for piece in Piece::ALL {
            next.by_color[piece.color()] |= next.pieces[piece];
        }

        next.castling = self.castling.update_move(mv);
        next.side_to_move = them;

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

        self.mailbox[from] = None;
        self.mailbox[to] = Some(new_piece);
    }
}

#[cfg(test)]
mod tests {
    use crate::board::{
        test_utils::{board, find_move},
        Board,
    };

    const CASTLING_WHITE: &str = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
    const CASTLING_BLACK: &str = "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1";

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
    fn double_push_sets_en_passant() {
        check(
            Board::STARTPOS,
            "e2e4",
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        );
        check(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
            "d7d5",
            "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2",
        );
    }

    #[test]
    fn any_other_move_clears_en_passant() {
        check(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
            "g8f6",
            "rnbqkb1r/pppppppp/5n2/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2",
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
