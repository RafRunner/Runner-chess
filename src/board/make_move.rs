use crate::{
    bitboard::BitBoard,
    board::Board,
    chess_move::{Move, MoveKind},
    piece::{Color, Piece, PieceKind},
    square::{Delta, Square},
};

impl Board {
    pub fn make_move(&self, mv: Move) -> Self {
        let mut after_move = self.clone();
        after_move.en_passant = None;

        let us = self.side_to_move;
        let them = us.oposite();

        let from = mv.from();
        let to = mv.to();
        let piece = self.piece_at(from).expect("No piece in 'from' square");
        let target = self.piece_at(to);

        match mv.kind() {
            MoveKind::Normal => {
                after_move.move_piece(piece, from, to, None);

                if let Some(captured) = target {
                    after_move.pieces[captured] &= !to.bb();
                }
            }

            MoveKind::DoublePush => {
                after_move.move_piece(piece, from, to, None);

                let delta = if us == Color::White {
                    Delta::NORTH
                } else {
                    Delta::SOUTH
                };

                after_move.en_passant = Some(
                    from.offset(delta)
                        .expect("Double Push: unexpected from Square"),
                )
            }
            MoveKind::EnPassant => {
                after_move.move_piece(piece, from, to, None);

                let delta = if us == Color::White {
                    Delta::SOUTH
                } else {
                    Delta::NORTH
                };
                let captured = to.offset(delta).expect("En Passant: unexpected to Square");

                after_move.pieces[Piece::new(them, PieceKind::Pawn)] &= !captured.bb();
                after_move.mailbox[captured] = None;
            }
            MoveKind::Castle => {
                // move the king
                after_move.move_piece(piece, from, to, None);

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

                after_move.move_piece(Piece::new(us, PieceKind::Rook), rook_from, rook_to, None);
            }
            MoveKind::Promotion(piece_kind) => {
                let new_piece = Some(Piece::new(us, piece_kind));

                after_move.move_piece(piece, from, to, new_piece);

                if let Some(captured) = target {
                    after_move.pieces[captured] &= !to.bb();
                }
            }
        }

        after_move.by_color = [BitBoard::EMPTY; 2];

        for piece in Piece::ALL {
            after_move.by_color[piece.color()] |= after_move.pieces[piece];
        }

        after_move.castling = self.castling.update_move(mv);
        after_move.side_to_move = them;

        debug_assert!(after_move.sanity_check().is_ok());

        after_move
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
    use super::*;

    const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    const CASTLE_POS_W: &str = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
    const CASTLE_POS_B: &str = "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1";

    // ---------- helpers ----------

    fn board(fen: &str) -> Board {
        Board::from_fen(fen).unwrap_or_else(|_| panic!("FEN inválido no teste: {fen}"))
    }

    fn mv(from: Square, to: Square, kind: MoveKind) -> Move {
        Move::new(from, to, kind)
    }

    /// Compara tudo que o Board guarda, campo a campo, para a falha dizer o que quebrou.
    fn assert_same_position(actual: &Board, expected: &Board) {
        for sq in BitBoard::FULL {
            assert_eq!(
                actual.mailbox[sq], expected.mailbox[sq],
                "mailbox em {sq:?}"
            );
        }
        for p in Piece::ALL {
            assert_eq!(actual.pieces[p], expected.pieces[p], "bitboard de {p:?}");
        }
        assert_eq!(actual.side_to_move, expected.side_to_move, "side_to_move");
        assert_eq!(actual.castling, expected.castling, "direitos de roque");
        assert_eq!(actual.en_passant, expected.en_passant, "casa de en passant");
        // quando adicionar halfmove/fullmove, compare aqui também
    }

    fn check(fen: &str, m: Move, expected_fen: &str) {
        let after = board(fen).make_move(m);
        assert!(after.sanity_check().is_ok());
        assert_same_position(&after, &board(expected_fen));
    }

    // ---------- lances normais ----------

    #[test]
    fn quiet_knight_move() {
        check(
            STARTPOS,
            mv(Square::G1, Square::F3, MoveKind::Normal),
            "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPPPPP/RNBQKB1R b KQkq - 1 1",
        );
    }

    #[test]
    fn single_pawn_push() {
        check(
            STARTPOS,
            mv(Square::E2, Square::E3, MoveKind::Normal),
            "rnbqkbnr/pppppppp/8/8/8/4P3/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
        );
    }

    #[test]
    fn white_pawn_capture() {
        check(
            "4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1",
            mv(Square::E4, Square::D5, MoveKind::Normal),
            "4k3/8/8/3P4/8/8/8/4K3 b - - 0 1",
        );
    }

    #[test]
    fn black_pawn_capture() {
        check(
            "4k3/8/8/3p4/4P3/8/8/4K3 b - - 0 1",
            mv(Square::D5, Square::E4, MoveKind::Normal),
            "4k3/8/8/8/4p3/8/8/4K3 w - - 0 2",
        );
    }

    #[test]
    fn knight_captures_queen() {
        check(
            "4k3/8/8/3q4/8/4N3/8/4K3 w - - 0 1",
            mv(Square::E3, Square::D5, MoveKind::Normal),
            "4k3/8/8/3N4/8/8/8/4K3 b - - 0 1",
        );
    }

    // ---------- avanço duplo / en passant ----------

    #[test]
    fn white_double_push_sets_ep() {
        check(
            STARTPOS,
            mv(Square::E2, Square::E4, MoveKind::DoublePush),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        );
    }

    #[test]
    fn black_double_push_sets_ep() {
        check(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
            mv(Square::D7, Square::D5, MoveKind::DoublePush),
            "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2",
        );
    }

    #[test]
    fn ep_square_cleared_after_other_move() {
        check(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
            mv(Square::G8, Square::F6, MoveKind::Normal),
            "rnbqkb1r/pppppppp/5n2/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2",
        );
    }

    #[test]
    fn white_en_passant() {
        check(
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            mv(Square::E5, Square::D6, MoveKind::EnPassant),
            "4k3/8/3P4/8/8/8/8/4K3 b - - 0 1",
        );
    }

    #[test]
    fn black_en_passant() {
        check(
            "4k3/8/8/8/3Pp3/8/8/4K3 b - d3 0 1",
            mv(Square::E4, Square::D3, MoveKind::EnPassant),
            "4k3/8/8/8/8/3p4/8/4K3 w - - 0 2",
        );
    }

    // ---------- roque ----------

    #[test]
    fn white_castle_kingside() {
        check(
            CASTLE_POS_W,
            mv(Square::E1, Square::G1, MoveKind::Castle),
            "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1",
        );
    }

    #[test]
    fn white_castle_queenside() {
        check(
            CASTLE_POS_W,
            mv(Square::E1, Square::C1, MoveKind::Castle),
            "r3k2r/8/8/8/8/8/8/2KR3R b kq - 1 1",
        );
    }

    #[test]
    fn black_castle_kingside() {
        check(
            CASTLE_POS_B,
            mv(Square::E8, Square::G8, MoveKind::Castle),
            "r4rk1/8/8/8/8/8/8/R3K2R w KQ - 1 2",
        );
    }

    #[test]
    fn black_castle_queenside() {
        check(
            CASTLE_POS_B,
            mv(Square::E8, Square::C8, MoveKind::Castle),
            "2kr3r/8/8/8/8/8/8/R3K2R w KQ - 1 2",
        );
    }

    // ---------- direitos de roque ----------

    #[test]
    fn king_move_removes_both_rights() {
        check(
            CASTLE_POS_W,
            mv(Square::E1, Square::F1, MoveKind::Normal),
            "r3k2r/8/8/8/8/8/8/R4K1R b kq - 1 1",
        );
    }

    #[test]
    fn rook_move_removes_one_right() {
        check(
            CASTLE_POS_W,
            mv(Square::A1, Square::B1, MoveKind::Normal),
            "r3k2r/8/8/8/8/8/8/1R2K2R b Kkq - 1 1",
        );
    }

    #[test]
    fn capturing_rook_removes_both_sides_rights() {
        // Th1xh8: branco perde K (torre saiu), preto perde k (torre capturada)
        check(
            CASTLE_POS_W,
            mv(Square::H1, Square::H8, MoveKind::Normal),
            "r3k2R/8/8/8/8/8/8/R3K3 b Qq - 0 1",
        );
    }

    #[test]
    fn black_capturing_rook_removes_rights() {
        check(
            CASTLE_POS_B,
            mv(Square::A8, Square::A1, MoveKind::Normal),
            "4k2r/8/8/8/8/8/8/r3K2R w Kk - 0 2",
        );
    }

    // ---------- promoção ----------

    #[test]
    fn white_quiet_promotion_all_kinds() {
        for (kind, ch) in [
            (PieceKind::Queen, 'Q'),
            (PieceKind::Rook, 'R'),
            (PieceKind::Bishop, 'B'),
            (PieceKind::Knight, 'N'),
        ] {
            check(
                "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
                mv(Square::A7, Square::A8, MoveKind::Promotion(kind)),
                &format!("{ch}3k3/8/8/8/8/8/8/4K3 b - - 0 1"),
            );
        }
    }

    #[test]
    fn white_capture_promotion() {
        check(
            "1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            mv(
                Square::A7,
                Square::B8,
                MoveKind::Promotion(PieceKind::Knight),
            ),
            "1N2k3/8/8/8/8/8/8/4K3 b - - 0 1",
        );
    }

    #[test]
    fn black_capture_promotion() {
        check(
            "4k3/8/8/8/8/8/p7/1R2K3 b - - 0 1",
            mv(
                Square::A2,
                Square::B1,
                MoveKind::Promotion(PieceKind::Queen),
            ),
            "4k3/8/8/8/8/8/8/1q2K3 w - - 0 2",
        );
    }

    #[test]
    fn capture_promotion_on_rook_corner_removes_right() {
        check(
            "4k3/8/8/8/8/8/6p1/4K2R b K - 0 1",
            mv(
                Square::G2,
                Square::H1,
                MoveKind::Promotion(PieceKind::Queen),
            ),
            "4k3/8/8/8/8/8/8/4K2q w - - 0 2",
        );
    }

    // ---------- sequência ----------

    #[test]
    fn italian_game_into_castle() {
        // 1.e4 e5 2.Nf3 Nc6 3.Bc4 Nf6 4.O-O
        let moves = [
            mv(Square::E2, Square::E4, MoveKind::DoublePush),
            mv(Square::E7, Square::E5, MoveKind::DoublePush),
            mv(Square::G1, Square::F3, MoveKind::Normal),
            mv(Square::B8, Square::C6, MoveKind::Normal),
            mv(Square::F1, Square::C4, MoveKind::Normal),
            mv(Square::G8, Square::F6, MoveKind::Normal),
            mv(Square::E1, Square::G1, MoveKind::Castle),
        ];

        let mut b = board(STARTPOS);
        for m in moves {
            b = b.make_move(m);
            assert!(b.sanity_check().is_ok());
        }

        assert_same_position(
            &b,
            &board("r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQ1RK1 b kq - 5 4"),
        );
        println!("{b}");
    }
}
