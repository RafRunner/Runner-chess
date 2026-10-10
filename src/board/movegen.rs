use std::iter;

use crate::{
    attacks::{attacks, PAWN_ATTACKS},
    bitboard::BitBoard,
    board::Board,
    castling::CastlingRights,
    chess_move::{AbstractMove, Move, MoveKind, PromotionPiece},
    piece::{Color, Piece, PieceKind},
    square::{Delta, Square},
};

#[derive(Debug)]
pub struct IllegalMoveError;

impl Board {
    pub fn generate_moves(&self, moves: &mut Vec<Move>) {
        let us = self.side_to_move();
        let them = us.oposite();
        let white_to_move = us == Color::White;

        let our_pieces = self.by_color(us);
        let their_pieces = self.by_color(them);
        let occupied = our_pieces | their_pieces;

        // Normal Pieces
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
                    moves.push(Move::new(
                        from,
                        to,
                        if self.piece_at(to).is_some() {
                            MoveKind::Capture
                        } else {
                            MoveKind::Normal
                        },
                    ));
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
            let king_sq = self.pieces(Piece::new(us, PieceKind::King)).next().unwrap();
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

        // Pawns
        let (starting_rank, delta, needs_empty) = if white_to_move {
            (
                BitBoard::RANK_2,
                Delta::NORTH,
                BitBoard::RANK_3 | BitBoard::RANK_4,
            )
        } else {
            (
                BitBoard::RANK_7,
                Delta::SOUTH,
                BitBoard::RANK_6 | BitBoard::RANK_5,
            )
        };
        let pawns = self.pieces(Piece::new(us, PieceKind::Pawn));
        let pawns_on_home = starting_rank & pawns;
        for from in pawns_on_home {
            if BitBoard::FILES[from.file() as usize] & needs_empty & occupied == BitBoard::EMPTY {
                moves.push(Move::new(
                    from,
                    from.offset(delta * 2).unwrap(),
                    MoveKind::DoublePush,
                ));
            }
        }

        let single_move = if white_to_move {
            pawns << 8
        } else {
            pawns >> 8
        } & !occupied;
        for to in single_move {
            let from = to.offset(-delta).unwrap();
            self.add_pawn_moves(from, to, moves);
        }

        for from in pawns {
            for to in PAWN_ATTACKS[us][from] & their_pieces {
                self.add_pawn_moves(from, to, moves);
            }
        }

        if let Some(to) = self.en_passant() {
            for from in PAWN_ATTACKS[them][to] & pawns {
                moves.push(Move::new(from, to, MoveKind::EnPassant))
            }
        }
    }

    pub fn legal_successors(&self) -> impl Iterator<Item = (Move, Board)> + '_ {
        let mut pseudo_legal = Vec::new();
        self.generate_moves(&mut pseudo_legal);

        pseudo_legal.into_iter().filter_map(|mv| {
            let next = self.make_move(mv);
            (!next.is_in_check(self.side_to_move)).then_some((mv, next))
        })
    }

    pub fn resolve_move(&self, mv: AbstractMove) -> Result<(Move, Board), IllegalMoveError> {
        for (legal, next) in self.legal_successors() {
            if mv.is_move(legal) {
                return Ok((legal, next));
            }
        }

        Err(IllegalMoveError)
    }

    fn add_pawn_moves(&self, from: Square, to: Square, moves: &mut Vec<Move>) {
        let is_capture = self.piece_at(to).is_some();
        if to.rank() == 0 || to.rank() == 7 {
            for kind in PromotionPiece::ALL {
                moves.push(Move::new(
                    from,
                    to,
                    if is_capture {
                        MoveKind::PromotionCapture(kind)
                    } else {
                        MoveKind::Promotion(kind)
                    },
                ));
            }
        } else {
            moves.push(Move::new(
                from,
                to,
                if is_capture {
                    MoveKind::Capture
                } else {
                    MoveKind::Normal
                },
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::test_utils::{board, find_move, moves_of, KIWIPETE, PERFT_POSITIONS, POSITION_3};
    use Square as S;

    const KIWIPETE_BLACK: &str =
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 1";

    const BISHOP_FROM_D4: [&str; 13] = [
        "a1", "b2", "c3", "e5", "f6", "g7", "h8", "a7", "b6", "c5", "e3", "f2", "g1",
    ];
    const ROOK_FROM_D4: [&str; 14] = [
        "d1", "d2", "d3", "d5", "d6", "d7", "d8", "a4", "b4", "c4", "e4", "f4", "g4", "h4",
    ];

    fn generate(fen: &str) -> (Board, Vec<Move>) {
        let b = board(fen);
        let moves = moves_of(&b);
        (b, moves)
    }

    /// sorted, so failures print in a predictable order
    fn sorted(mut names: Vec<String>) -> Vec<String> {
        names.sort();
        names
    }

    fn expected(names: &[&str]) -> Vec<String> {
        sorted(names.iter().map(|s| s.to_string()).collect())
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
        for &(fen, from, names) in cases {
            assert_eq!(
                targets_from(fen, from),
                expected(names),
                "{fen} from {from}"
            );
        }
    }

    /// every move starting on `from`, in UCI, so promotions show the piece
    fn uci_from(fen: &str, from: Square) -> Vec<String> {
        let (_, moves) = generate(fen);
        sorted(
            moves
                .iter()
                .filter(|mv| mv.from() == from)
                .map(|mv| mv.to_uci())
                .collect(),
        )
    }

    fn assert_uci_from(cases: &[(&str, Square, &[&str])]) {
        for &(fen, from, names) in cases {
            assert_eq!(uci_from(fen, from), expected(names), "{fen} from {from}");
        }
    }

    /// kind of the generated move written as `uci`
    fn kind_of(fen: &str, uci: &str) -> MoveKind {
        find_move(&board(fen), uci).kind()
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
        for (fen, names) in cases {
            let (_, moves) = generate(fen);
            let castles = moves
                .iter()
                .filter(|mv| mv.kind() == MoveKind::Castle)
                .map(|mv| mv.to_uci())
                .collect();
            assert_eq!(sorted(castles), expected(names), "{fen}");
        }
    }

    #[test]
    fn pawn_pushes() {
        #[rustfmt::skip]
        assert_uci_from(&[
            ("7k/8/8/8/8/8/4P3/K7 w - - 0 1", S::E2, &["e2e3", "e2e4"]),
            ("7k/8/8/8/8/4P3/8/K7 w - - 0 1", S::E3, &["e3e4"]), // double push only from the start
            ("7k/4p3/8/8/8/8/8/K7 b - - 0 1", S::E7, &["e7e6", "e7e5"]),
            ("7k/8/4p3/8/8/8/8/K7 b - - 0 1", S::E6, &["e6e5"]),
            ("7k/8/8/8/8/8/P7/4K3 w - - 0 1", S::A2, &["a2a3", "a2a4"]),
            ("7k/8/8/8/8/8/7P/K7 w - - 0 1",  S::H2, &["h2h3", "h2h4"]),
            // a piece of either color right in front blocks both pushes
            ("7k/8/8/8/8/4n3/4P3/K7 w - - 0 1", S::E2, &[]),
            ("7k/8/8/8/8/4N3/4P3/K7 w - - 0 1", S::E2, &[]),
            ("7k/4p3/4N3/8/8/8/8/K7 b - - 0 1", S::E7, &[]),
            // two squares ahead it only blocks the double push
            ("7k/8/8/8/4n3/8/4P3/K7 w - - 0 1", S::E2, &["e2e3"]),
            ("7k/4p3/8/4N3/8/8/8/K7 b - - 0 1", S::E7, &["e7e6"]),
        ]);
        let white = "7k/8/8/8/8/8/4P3/K7 w - - 0 1";
        assert_eq!(kind_of(white, "e2e3"), MoveKind::Normal);
        assert_eq!(kind_of(white, "e2e4"), MoveKind::DoublePush);
        assert_eq!(
            kind_of("7k/4p3/8/8/8/8/8/K7 b - - 0 1", "e7e5"),
            MoveKind::DoublePush
        );
    }

    #[test]
    fn pawn_captures() {
        #[rustfmt::skip]
        assert_uci_from(&[
            ("7k/8/8/8/8/3n1n2/4P3/K7 w - - 0 1", S::E2, &["e2d3", "e2e3", "e2e4", "e2f3"]),
            ("7k/4p3/3N1N2/8/8/8/8/K7 b - - 0 1", S::E7, &["e7d6", "e7e5", "e7e6", "e7f6"]),
            // never its own pieces
            ("7k/8/8/8/8/3N1N2/4P3/K7 w - - 0 1", S::E2, &["e2e3", "e2e4"]),
            // only forward
            ("7k/8/8/8/8/8/4P3/K2n1n2 w - - 0 1", S::E2, &["e2e3", "e2e4"]),
            // no wrapping around the edge of the board
            ("7k/8/8/8/8/1n5n/P7/4K3 w - - 0 1", S::A2, &["a2a3", "a2a4", "a2b3"]),
            ("7k/8/8/8/8/n5n1/7P/4K3 w - - 0 1", S::H2, &["h2g3", "h2h3", "h2h4"]),
        ]);
        assert_eq!(
            kind_of("7k/8/8/8/8/3n1n2/4P3/K7 w - - 0 1", "e2d3"),
            MoveKind::Capture
        );
    }

    #[test]
    fn pawn_promotions() {
        #[rustfmt::skip]
        assert_uci_from(&[
            ("7k/4P3/8/8/8/8/8/K7 w - - 0 1", S::E7, &["e7e8q", "e7e8r", "e7e8b", "e7e8n"]),
            // capturing onto the last rank promotes too
            ("3r3k/4P3/8/8/8/8/8/K7 w - - 0 1", S::E7, &[
                "e7e8q", "e7e8r", "e7e8b", "e7e8n",
                "e7d8q", "e7d8r", "e7d8b", "e7d8n",
            ]),
            ("7k/8/8/8/8/8/3p4/K3R3 b - - 0 1", S::D2, &[
                "d2d1q", "d2d1r", "d2d1b", "d2d1n",
                "d2e1q", "d2e1r", "d2e1b", "d2e1n",
            ]),
            // blocked, and nothing to capture
            ("4r2k/4P3/8/8/8/8/8/K7 w - - 0 1", S::E7, &[]),
        ]);
        assert_eq!(
            kind_of("7k/4P3/8/8/8/8/8/K7 w - - 0 1", "e7e8n"),
            MoveKind::Promotion(PromotionPiece::Knight)
        );
    }

    #[test]
    fn en_passant() {
        #[rustfmt::skip]
        let cases: [(&str, &[&str]); 5] = [
            ("7k/8/8/3pP3/8/8/8/K7 w - d6 0 1",  &["e5d6"]),
            ("7k/8/8/2PpP3/8/8/8/K7 w - d6 0 1", &["c5d6", "e5d6"]), // from both sides
            ("7k/8/8/8/3pP3/8/8/K7 b - e3 0 1",  &["d4e3"]),
            ("7k/8/8/3pP3/8/8/8/K7 w - - 0 1",   &[]), // the last move wasn't d7-d5
            ("7k/8/8/3p2P1/8/8/8/K7 w - d6 0 1", &[]), // no pawn beside d5
        ];
        for (fen, names) in cases {
            let (_, moves) = generate(fen);
            let captures = moves
                .iter()
                .filter(|mv| mv.kind() == MoveKind::EnPassant)
                .map(|mv| mv.to_uci())
                .collect();
            assert_eq!(sorted(captures), expected(names), "{fen}");
        }

        // the capturing pawn can still push, and d6 isn't also generated as a normal move
        assert_uci_from(&[("7k/8/8/3pP3/8/8/8/K7 w - d6 0 1", S::E5, &["e5d6", "e5e6"])]);
    }

    /// pseudo-legal: in position 3 b5b6 is pinned and Kb6 is attacked by
    /// c7, so after the legality filter these become perft(1) = 20, 48 and 14
    #[test]
    fn pseudo_legal_move_counts() {
        let cases = [(Board::STARTPOS, 20), (KIWIPETE, 48), (POSITION_3, 16)];
        for (fen, count) in cases {
            assert_eq!(generate(fen).1.len(), count, "{fen}");
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
        for fen in PERFT_POSITIONS.into_iter().chain([KIWIPETE_BLACK]) {
            let (b, moves) = generate(fen);
            let us = b.side_to_move();
            for mv in &moves {
                let moved = b.piece_at(mv.from());
                let captured = b.piece_at(mv.to());
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

    /// legal moves in UCI, sorted
    fn legal_uci(fen: &str) -> Vec<String> {
        sorted(
            board(fen)
                .legal_successors()
                .map(|(mv, _)| mv.to_uci())
                .collect(),
        )
    }

    #[test]
    fn legal_successors_drop_moves_that_leave_the_king_attacked() {
        #[rustfmt::skip]
        let cases: [(&str, &[&str]); 4] = [
            // the knight on e2 is pinned by the rook on e7
            ("4k3/4r3/8/8/8/8/4N3/4K3 w - - 0 1", &["e1d1", "e1d2", "e1f1", "e1f2"]),
            // the king can't step onto the rook's rank
            ("4k3/8/8/8/8/8/7r/4K3 w - - 0 1", &["e1d1", "e1f1"]),
            // in check from a1: f1 is attacked through the square the king leaves,
            // and no rook move blocks or captures
            ("4k3/8/8/8/8/8/8/r3K2R w - - 0 1", &["e1d2", "e1e2", "e1f2"]),
            // bxc6 en passant would take both pawns off the 5th rank and expose
            // the king to the rook; b4 is attacked by the c5 pawn
            ("8/8/8/KPp4r/8/8/8/4k3 w - c6 0 1", &["a5a4", "a5a6", "a5b6", "b5b6"]),
        ];
        for (fen, names) in cases {
            assert_eq!(legal_uci(fen), expected(names), "{fen}");
        }
    }

    /// the filter must be exact: it keeps every safe move and nothing else
    #[test]
    fn legal_successors_split_pseudo_legal_moves_exactly() {
        for fen in PERFT_POSITIONS.into_iter().chain([KIWIPETE_BLACK]) {
            let (b, pseudo_legal) = generate(fen);
            let us = b.side_to_move();
            let legal: HashSet<Move> = b.legal_successors().map(|(mv, _)| mv).collect();
            for mv in pseudo_legal {
                let safe = !b.make_move(mv).is_in_check(us);
                assert_eq!(legal.contains(&mv), safe, "{fen} {mv}");
            }
            for (mv, next) in b.legal_successors() {
                assert_eq!(next.side_to_move(), us.oposite(), "{fen} {mv}");
            }
        }
    }

    /// the flags of every generated move agree with the board it was generated on
    #[test]
    fn move_kinds_match_the_board() {
        // the reference positions and every position one move later, which adds
        // en passant and more promotions to the mix
        let positions: Vec<Board> = PERFT_POSITIONS
            .into_iter()
            .chain([KIWIPETE_BLACK])
            .map(board)
            .flat_map(|b| {
                let next: Vec<Board> = b.legal_successors().map(|(_, next)| next).collect();
                std::iter::once(b).chain(next)
            })
            .collect();

        for b in &positions {
            for mv in moves_of(b) {
                let (from, to, kind) = (mv.from(), mv.to(), mv.kind());
                let piece = b.piece_at(from).unwrap().kind();
                let is_pawn = piece == PieceKind::Pawn;
                let context = format!("{} {mv} {kind:?}", b.to_fen());

                assert_eq!(
                    mv.is_capture(),
                    b.piece_at(to).is_some() || kind == MoveKind::EnPassant,
                    "{context}"
                );
                assert_eq!(
                    matches!(kind, MoveKind::Promotion(_) | MoveKind::PromotionCapture(_)),
                    is_pawn && (to.rank() == 0 || to.rank() == 7),
                    "{context}"
                );
                assert_eq!(
                    kind == MoveKind::DoublePush,
                    is_pawn && from.rank().abs_diff(to.rank()) == 2,
                    "{context}"
                );
                assert_eq!(
                    kind == MoveKind::EnPassant,
                    is_pawn && Some(to) == b.en_passant(),
                    "{context}"
                );
                assert_eq!(
                    kind == MoveKind::Castle,
                    piece == PieceKind::King && from.file().abs_diff(to.file()) == 2,
                    "{context}"
                );
            }
        }
    }
}
