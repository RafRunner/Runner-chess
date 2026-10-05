use std::iter;

use crate::{
    attacks::attacks,
    board::Board,
    castling::CastlingRights,
    chess_move::{Move, MoveKind},
    piece::{Color, Piece, PieceKind},
    square::Square,
};

impl Board {
    pub fn generate_moves(&self, moves: &mut Vec<Move>) {
        let us = self.side_to_move();
        let them = us.oposite();
        // let white_to_move = us == Color::White;

        let our_pieces = self.by_color(us);
        let their_pieces = self.by_color(them);
        let occupied = our_pieces | their_pieces;

        let king_sq = self.pieces(Piece::new(us, PieceKind::King)).next().unwrap();

        let kinds = [
            PieceKind::Knight,
            PieceKind::Bishop,
            PieceKind::Rook,
            PieceKind::Queen,
            PieceKind::King,
        ];
        for kind in kinds {
            let piece = Piece::new(us, kind);
            for from in self.pieces(piece) {
                let targets = attacks(piece, from, occupied) & !our_pieces;
                for to in targets {
                    moves.push(Move::new(from, to, MoveKind::Normal));
                }
            }
        }

        // Castling
        let castle_checks = [
            (
                Color::White,
                CastlingRights::WHITE_SHORT,
                Square::G1,
                Square::F1.bb() | Square::G1.bb(),
                [Square::F1, Square::G1],
            ),
            (
                Color::White,
                CastlingRights::WHITE_LONG,
                Square::C1,
                Square::D1.bb() | Square::C1.bb() | Square::B1.bb(),
                [Square::D1, Square::C1],
            ),
            (
                Color::Black,
                CastlingRights::BLACK_SHORT,
                Square::G8,
                Square::F8.bb() | Square::G8.bb(),
                [Square::F8, Square::G8],
            ),
            (
                Color::Black,
                CastlingRights::BLACK_LONG,
                Square::C8,
                Square::D8.bb() | Square::C8.bb() | Square::B8.bb(),
                [Square::D8, Square::C8],
            ),
        ];

        for (color, right, king_to, must_be_empty, must_not_be_attacked) in castle_checks {
            if us == color
                && self.castling().has(right)
                && (must_be_empty & occupied).is_empty()
                && must_not_be_attacked
                    .into_iter()
                    .chain(iter::once(king_sq))
                    .all(|sq| !self.is_square_attacked(sq, them))
            {
                moves.push(Move::new(king_sq, king_to, MoveKind::Castle));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use Square as S;

    // the four positions also have pawns, so these only become perft
    // references once pawn moves are generated
    const POSITIONS: [&str; 5] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    ];

    const BISHOP_FROM_D4: [&str; 13] = [
        "a1", "b2", "c3", "e5", "f6", "g7", "h8", "a7", "b6", "c5", "e3", "f2", "g1",
    ];
    const ROOK_FROM_D4: [&str; 14] = [
        "d1", "d2", "d3", "d5", "d6", "d7", "d8", "a4", "b4", "c4", "e4", "f4", "g4", "h4",
    ];

    fn generate(fen: &str) -> (Board, Vec<Move>) {
        let board = Board::from_fen(fen).unwrap_or_else(|err| panic!("{fen}: {err:?}"));
        let mut moves = Vec::new();
        board.generate_moves(&mut moves);
        (board, moves)
    }

    /// sorted, so failures print in a predictable order
    fn sorted(mut names: Vec<String>) -> Vec<String> {
        names.sort();
        names
    }

    /// destination of every move starting on `from`
    fn targets_from(fen: &str, from: Square) -> Vec<String> {
        let (_, moves) = generate(fen);
        sorted(
            moves
                .iter()
                .filter(|mv| mv.from() == from)
                .map(|mv| mv.to().to_algebraic())
                .collect(),
        )
    }

    fn assert_targets(cases: &[(&str, Square, &[&str])]) {
        for &(fen, from, expected) in cases {
            let expected = sorted(expected.iter().map(|s| s.to_string()).collect());
            assert_eq!(targets_from(fen, from), expected, "{fen} from {from}");
        }
    }

    #[test]
    fn knight_moves() {
        #[rustfmt::skip]
        assert_targets(&[
            ("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1", S::D4, &["b3", "b5", "c2", "c6", "e2", "e6", "f3", "f5"]),
            ("4k3/8/8/8/8/8/8/N3K3 w - - 0 1", S::A1, &["b3", "c2"]),
            // own bishop on b3 is skipped, enemy rook on f3 is captured
            ("4k3/8/8/8/3N4/1B3r2/8/4K3 w - - 0 1", S::D4, &["b5", "c2", "c6", "e2", "e6", "f3", "f5"]),
            // only the side to move generates
            ("4k3/8/8/8/3n4/8/8/4K3 b - - 0 1", S::D4, &["b3", "b5", "c2", "c6", "e2", "e6", "f3", "f5"]),
            ("4k3/8/8/8/3N4/8/8/4K3 b - - 0 1", S::D4, &[]),
        ]);
    }

    #[test]
    fn bishop_moves() {
        #[rustfmt::skip]
        assert_targets(&[
            ("4k3/8/8/8/3B4/8/8/4K3 w - - 0 1", S::D4, &BISHOP_FROM_D4),
            // stops before its own knight on c3, captures the enemy knight on f6
            ("4k3/8/5n2/8/3B4/2N5/8/4K3 w - - 0 1", S::D4, &["a7", "b6", "c5", "e3", "e5", "f2", "f6", "g1"]),
        ]);
    }

    #[test]
    fn rook_moves() {
        #[rustfmt::skip]
        assert_targets(&[
            ("4k3/8/8/8/3R4/8/8/4K3 w - - 0 1", S::D4, &ROOK_FROM_D4),
            // stops before its own knight on b4, captures the enemy knight on d7
            ("4k3/3n4/8/8/1N1R4/8/8/4K3 w - - 0 1", S::D4, &["c4", "d1", "d2", "d3", "d5", "d6", "d7", "e4", "f4", "g4", "h4"]),
        ]);
    }

    #[test]
    fn queen_moves_like_bishop_and_rook() {
        let both: Vec<&str> = BISHOP_FROM_D4.into_iter().chain(ROOK_FROM_D4).collect();
        assert_targets(&[("4k3/8/8/8/3Q4/8/8/4K3 w - - 0 1", S::D4, &both)]);
    }

    #[test]
    fn king_moves() {
        #[rustfmt::skip]
        assert_targets(&[
            ("4k3/8/8/8/8/8/8/4K3 w - - 0 1", S::E1, &["d1", "d2", "e2", "f1", "f2"]),
            ("4k3/8/8/8/8/8/8/4K3 b - - 0 1", S::E8, &["d7", "d8", "e7", "f7", "f8"]),
            // own knight on d2 is skipped, the rook on e2 is captured, and f2 is
            // generated even though the rook attacks it: that's the legality filter's job
            ("4k3/8/8/8/8/8/3Nr3/4K3 w - - 0 1", S::E1, &["d1", "e2", "f1", "f2"]),
        ]);
    }

    #[test]
    fn castling() {
        #[rustfmt::skip]
        let cases: [(&str, &[&str]); 17] = [
            ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", &["e1c1", "e1g1"]),
            ("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1", &["e8c8", "e8g8"]),
            // only the rights still held
            ("r3k2r/8/8/8/8/8/8/R3K2R w Kq - 0 1", &["e1g1"]),
            ("r3k2r/8/8/8/8/8/8/R3K2R b Kq - 0 1", &["e8c8"]),
            ("r3k2r/8/8/8/8/8/8/R3K2R w kq - 0 1", &[]),
            // every square between king and rook must be empty, whatever the color
            ("r3k2r/8/8/8/8/8/8/RN2K2R w KQkq - 0 1", &["e1g1"]),
            ("r3k2r/8/8/8/8/8/8/R3KB1R w KQkq - 0 1", &["e1c1"]),
            ("r3k2r/8/8/8/8/8/8/R3K1nR w KQkq - 0 1", &["e1c1"]),
            ("rn2k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1", &["e8g8"]),
            // not out of check (bishop on b4)
            ("r3k2r/8/8/8/1b6/8/8/R3K2R w KQkq - 0 1", &[]),
            // not through an attacked square
            ("r3k2r/8/8/8/2b5/8/8/R3K2R w KQkq - 0 1", &["e1c1"]), // f1
            ("r3k2r/8/8/8/8/8/1n6/R3K2R w KQkq - 0 1", &["e1g1"]), // d1
            ("r3k2r/8/8/2B5/8/8/8/R3K2R b KQkq - 0 1", &["e8c8"]), // f8
            // not into an attacked square
            ("r3k2r/8/8/8/3b4/8/8/R3K2R w KQkq - 0 1", &["e1c1"]), // g1
            ("r3k2r/8/8/8/8/1n6/8/R3K2R w KQkq - 0 1", &["e1g1"]), // c1
            // b1 and b8 may be attacked: only the rook crosses them
            ("r3k2r/8/8/8/8/8/b7/R3K2R w KQkq - 0 1", &["e1c1", "e1g1"]),
            ("r3k2r/B7/8/8/8/8/8/R3K2R b KQkq - 0 1", &["e8c8", "e8g8"]),
        ];
        for (fen, expected) in cases {
            let (_, moves) = generate(fen);
            let castles = moves
                .iter()
                .filter(|mv| mv.kind() == MoveKind::Castle)
                .map(|mv| mv.to_uci())
                .collect();
            let expected = sorted(expected.iter().map(|s| s.to_string()).collect());
            assert_eq!(sorted(castles), expected, "{fen}");
        }
    }

    /// pawnless and with nothing to filter, so it's also a known perft(1)
    #[test]
    fn rooks_and_kings_move_count() {
        for fen in [
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1",
        ] {
            assert_eq!(generate(fen).1.len(), 26, "{fen}");
        }
    }

    #[test]
    fn moves_go_from_own_pieces_to_other_squares() {
        for fen in POSITIONS {
            let (board, moves) = generate(fen);
            let us = board.side_to_move();
            for mv in &moves {
                let moved = board.piece_at(mv.from());
                let captured = board.piece_at(mv.to());
                assert_eq!(moved.map(Piece::color), Some(us), "{fen} {mv}");
                assert_ne!(captured.map(Piece::color), Some(us), "{fen} {mv}");
                assert_ne!(
                    captured.map(Piece::kind),
                    Some(PieceKind::King),
                    "{fen} {mv}"
                );
            }
            let distinct: HashSet<Move> = moves.iter().copied().collect();
            assert_eq!(distinct.len(), moves.len(), "{fen}: duplicated moves");
        }
    }
}
