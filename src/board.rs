use std::fmt::{Display, Formatter};

use crate::{
    attacks::{bishop_attacks, rook_attacks, KING_ATTACKS, KNIGHT_ATTACKS, PAWN_ATTACKS},
    bitboard::BitBoard,
    castling::CastlingRights,
    chess_move::{Move, MoveKind},
    piece::{Color, Piece, PieceKind},
    square::{Delta, Square},
};

#[derive(Debug, Clone)]
pub struct Board {
    pieces: [BitBoard; 12],
    by_color: [BitBoard; 2],
    mailbox: [Option<Piece>; 64],
    side_to_move: Color,
    castling: CastlingRights,
    en_passant: Option<Square>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenParseError {
    MissingField(&'static str),
    WrongRankCount(usize),
    WrongRankLength(u8),
    ZeroDigit,
    InvalidPiece(char),
    InvalidSideToMove,
    InvalidCastling,
    InvalidEnPassant,
    EnPassantSideMismatch(Square),
    IllegalPosition(BoardInconsistencyError),
}

impl From<BoardInconsistencyError> for FenParseError {
    fn from(err: BoardInconsistencyError) -> Self {
        Self::IllegalPosition(err)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardInconsistencyError {
    KingCount(Color),
    KingCanBeCaptured(Color),
    PawnOnBackRank,
    CastlingWithoutPieces(CastlingRights),
    EnPassantWrongRank(Square),
    EnPassantBlocked(Square),
    EnPassantWithoutPawn(Square),
}

impl Board {
    pub fn empty() -> Self {
        Board {
            pieces: [BitBoard::EMPTY; 12],
            by_color: [BitBoard::EMPTY; 2],
            mailbox: [None; 64],
            side_to_move: Color::White,
            castling: CastlingRights::NONE,
            en_passant: None,
        }
    }

    pub fn from_fen(fen: &str) -> Result<Self, FenParseError> {
        let mut board = Self::empty();
        let mut parts = fen.split_whitespace();
        let pieces = parts
            .next()
            .ok_or(FenParseError::MissingField("piece placement"))?;

        let ranks: Vec<&str> = pieces.split("/").collect();
        if ranks.len() != 8 {
            return Err(FenParseError::WrongRankCount(ranks.len()));
        }

        for (rank_idx, rank) in ranks.iter().rev().enumerate() {
            let mut file: usize = 0;
            for c in rank.chars() {
                if file > 7 {
                    return Err(FenParseError::WrongRankLength(rank_idx as u8 + 1));
                }

                if let Some(n) = c.to_digit(10) {
                    if n == 0 {
                        return Err(FenParseError::ZeroDigit);
                    }
                    file += n as usize;
                } else {
                    let square = Square::from_file_and_rank(file as u8, rank_idx as u8);
                    let piece = Piece::from_fen(c).map_err(|_| FenParseError::InvalidPiece(c))?;

                    board.pieces[piece] |= square.bb();
                    board.by_color[piece.color()] |= square.bb();
                    board.mailbox[square] = Some(piece);

                    file += 1;
                }
            }

            if file != 8 {
                return Err(FenParseError::WrongRankLength(rank_idx as u8 + 1));
            }
        }

        let side_to_move = parts
            .next()
            .ok_or(FenParseError::MissingField("side to move"))?;
        board.side_to_move = match side_to_move {
            "w" => Color::White,
            "b" => Color::Black,
            _ => return Err(FenParseError::InvalidSideToMove),
        };

        let castle = parts
            .next()
            .ok_or(FenParseError::MissingField("castling"))?;
        board.castling =
            CastlingRights::from_fen(castle).map_err(|_| FenParseError::InvalidCastling)?;

        let en_passant = parts
            .next()
            .ok_or(FenParseError::MissingField("en passant"))?;
        board.en_passant = match en_passant {
            "-" => None,
            square => {
                let square =
                    Square::from_algebraic(square).map_err(|_| FenParseError::InvalidEnPassant)?;
                if board.side_to_move == Color::White && square.rank() != 5
                    || board.side_to_move == Color::Black && square.rank() != 2
                {
                    return Err(FenParseError::EnPassantSideMismatch(square));
                } else {
                    Some(square)
                }
            }
        };

        board.sanity_check()?;

        Ok(board)
    }

    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        self.mailbox[sq]
    }

    pub fn pieces(&self, piece: Piece) -> BitBoard {
        self.pieces[piece]
    }

    pub fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    pub fn castling(&self) -> CastlingRights {
        self.castling
    }

    pub fn en_passant(&self) -> Option<Square> {
        self.en_passant
    }

    pub fn all_pieces_bb(&self) -> BitBoard {
        self.by_color[Color::White] | self.by_color[Color::Black]
    }

    /// Needs to be called on the board BEFORE the move is made
    pub fn is_capture(&self, mv: Move) -> bool {
        self.mailbox[mv.to()].is_some() || mv.kind() == MoveKind::EnPassant
    }

    pub fn attackers_by(&self, sq: Square, by: Color) -> BitBoard {
        let pawns = self.pieces[Piece::new(by, PieceKind::Pawn)];
        let knights = self.pieces[Piece::new(by, PieceKind::Knight)];
        let bishops = self.pieces[Piece::new(by, PieceKind::Bishop)];
        let rooks = self.pieces[Piece::new(by, PieceKind::Rook)];
        let queens = self.pieces[Piece::new(by, PieceKind::Queen)];
        let king = self.pieces[Piece::new(by, PieceKind::King)];

        let occupied = self.all_pieces_bb();

        (PAWN_ATTACKS[by.oposite()][sq] & pawns)
            | (KNIGHT_ATTACKS[sq] & knights)
            | (KING_ATTACKS[sq] & king)
            | (bishop_attacks(sq, occupied) & (bishops | queens))
            | (rook_attacks(sq, occupied) & (rooks | queens))
    }

    pub fn is_square_attacked(&self, sq: Square, by: Color) -> bool {
        !self.attackers_by(sq, by).is_empty()
    }

    /// Rejects positions that would break the engine: desynced mailbox and
    /// bitboards, or out-of-bounds indexing. Positions that are unreachable but
    /// harmless (9 pawns, same-colored bishops, ...) are accepted on purpose.
    fn sanity_check(&self) -> Result<(), BoardInconsistencyError> {
        let us = self.side_to_move();
        let them = us.oposite();
        let white_to_move = us == Color::White;

        if self.pieces(Piece::WhiteKing).count_ones() != 1 {
            return Err(BoardInconsistencyError::KingCount(Color::White));
        }
        if self.pieces(Piece::BlackKing).count_ones() != 1 {
            return Err(BoardInconsistencyError::KingCount(Color::Black));
        }

        let white_pawns = self.pieces[Piece::WhitePawn];
        let black_pawns = self.pieces[Piece::BlackPawn];

        if (white_pawns | black_pawns) & (BitBoard::RANK_1 | BitBoard::RANK_8) != BitBoard::EMPTY {
            return Err(BoardInconsistencyError::PawnOnBackRank);
        }

        // TODO (only if the move list has a fixed capacity): at most 16 pieces
        // per side and 8 pawns. Reachable positions have at most 218 legal
        // moves, but a FEN with a dozen queens can overflow the list.
        // TODO (only if check evasion assumes it): at most 2 checkers on the
        // side to move.
        use CastlingRights as CR;
        let rules = [
            (CR::WHITE_SHORT, Color::White, Square::E1, Square::H1),
            (CR::WHITE_LONG, Color::White, Square::E1, Square::A1),
            (CR::BLACK_SHORT, Color::Black, Square::E8, Square::H8),
            (CR::BLACK_LONG, Color::Black, Square::E8, Square::A8),
        ];
        for (right, color, king_sq, rook_sq) in rules {
            let king = Some(Piece::new(color, PieceKind::King));
            let rook = Some(Piece::new(color, PieceKind::Rook));
            if self.castling.has(right)
                && (self.mailbox[king_sq] != king || self.mailbox[rook_sq] != rook)
            {
                return Err(BoardInconsistencyError::CastlingWithoutPieces(right));
            }
        }

        if let Some(sq) = self.en_passant {
            let (ep_rank, delta, ranks) = if white_to_move {
                (5, Delta::SOUTH, BitBoard::RANK_6 | BitBoard::RANK_7)
            } else {
                (2, Delta::NORTH, BitBoard::RANK_2 | BitBoard::RANK_3)
            };
            if ep_rank != sq.rank() {
                return Err(BoardInconsistencyError::EnPassantWrongRank(sq));
            }
            let all_pieces = self.all_pieces_bb();

            let file = BitBoard::FILES[sq.file() as usize];
            let mask = ranks & file;

            if all_pieces & mask != BitBoard::EMPTY {
                return Err(BoardInconsistencyError::EnPassantBlocked(sq));
            }

            let pawn_square = sq.offset(delta).expect("en passant rank was checked above");

            if self.mailbox[pawn_square] != Some(Piece::new(them, PieceKind::Pawn)) {
                return Err(BoardInconsistencyError::EnPassantWithoutPawn(sq));
            }
        }

        let mut enemy_king = self.pieces[Piece::new(them, PieceKind::King)];
        if self.is_square_attacked(enemy_king.next().unwrap(), us) {
            return Err(BoardInconsistencyError::KingCanBeCaptured(them));
        }

        Ok(())
    }
}

impl Display for Board {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        for rank in (0..8).rev() {
            write!(f, "{} ", rank + 1)?;
            for file in 0..8 {
                let c = self.mailbox[Square::from_file_and_rank(file, rank)]
                    .map_or('.', Piece::to_char);
                write!(f, "{c} ")?;
            }
            writeln!(f)?;
        }
        writeln!(f, "  a b c d e f g h")?;
        writeln!(f, "Castling: {}", self.castling)?;
        writeln!(
            f,
            "En-Passant: {}",
            self.en_passant
                .map_or(String::from("-"), |s| s.to_algebraic())
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::piece::PieceKind;

    use super::*;

    const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    const VALID: [&str; 4] = [
        STARTPOS,
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    ];

    fn p(color: Color, kind: PieceKind) -> Option<Piece> {
        Some(Piece::new(color, kind))
    }

    /// mailbox and bitboards must describe exactly the same position
    fn assert_consistent(b: &Board) {
        for sq in 0..64 {
            let square = Square::new(sq as u8);
            let owners: Vec<usize> = (0..12)
                .filter(|&i| b.pieces[i] & square.bb() != BitBoard::EMPTY)
                .collect();
            match b.mailbox[sq] {
                Some(piece) => assert_eq!(owners, vec![piece.index()], "square {square}"),
                None => assert!(
                    owners.is_empty(),
                    "square {square} empty in mailbox, but set in {owners:?}"
                ),
            }
        }

        // by_color must be the union of that color's piece bitboards
        for color in Color::ALL {
            let union = PieceKind::ALL
                .into_iter()
                .fold(BitBoard::EMPTY, |acc, kind| {
                    acc | b.pieces[Piece::new(color, kind)]
                });
            assert_eq!(b.by_color[color], union, "{color:?}");
        }
    }

    #[test]
    fn startpos_pieces_on_expected_squares() {
        let b = Board::from_fen(STARTPOS).unwrap();
        assert_eq!(b.piece_at(Square::A1), p(Color::White, PieceKind::Rook));
        assert_eq!(b.piece_at(Square::E1), p(Color::White, PieceKind::King));
        assert_eq!(b.piece_at(Square::E2), p(Color::White, PieceKind::Pawn));
        assert_eq!(b.piece_at(Square::E4), None);
        assert_eq!(b.piece_at(Square::D8), p(Color::Black, PieceKind::Queen));
        assert_eq!(b.piece_at(Square::E8), p(Color::Black, PieceKind::King));
        assert_eq!(b.mailbox.iter().filter(|s| s.is_some()).count(), 32);
    }

    #[test]
    fn startpos_pawn_bitboards() {
        let b = Board::from_fen(STARTPOS).unwrap();
        let wp = Piece::WhitePawn;
        let bp = Piece::BlackPawn;
        assert_eq!(b.pieces(wp), BitBoard::new(0x0000_0000_0000_FF00));
        assert_eq!(b.pieces(bp), BitBoard::new(0x00FF_0000_0000_0000));
    }

    #[test]
    fn asymmetric_position_catches_flips() {
        let b = Board::from_fen("k7/8/8/8/8/8/8/7K w - - 0 1").unwrap();
        assert_eq!(b.piece_at(Square::A8), p(Color::Black, PieceKind::King));
        assert_eq!(b.piece_at(Square::H1), p(Color::White, PieceKind::King));
        assert_eq!(b.mailbox.iter().filter(|s| s.is_some()).count(), 2);
    }

    /// Display lines, without the trailing spaces of each rank
    fn display_lines(fen: &str) -> Vec<String> {
        let b = Board::from_fen(fen).unwrap();
        b.to_string()
            .lines()
            .map(|l| l.trim_end().to_string())
            .collect()
    }

    #[test]
    fn display_startpos() {
        assert_eq!(
            display_lines(STARTPOS),
            [
                "8 r n b q k b n r",
                "7 p p p p p p p p",
                "6 . . . . . . . .",
                "5 . . . . . . . .",
                "4 . . . . . . . .",
                "3 . . . . . . . .",
                "2 P P P P P P P P",
                "1 R N B Q K B N R",
                "  a b c d e f g h",
                "Castling: KQkq",
                "En-Passant: -",
            ]
        );
    }

    #[test]
    fn display_asymmetric_position() {
        assert_eq!(
            display_lines("k7/8/8/8/3pP3/8/8/7K b - e3 0 1"),
            [
                "8 k . . . . . . .",
                "7 . . . . . . . .",
                "6 . . . . . . . .",
                "5 . . . . . . . .",
                "4 . . . p P . . .",
                "3 . . . . . . . .",
                "2 . . . . . . . .",
                "1 . . . . . . . K",
                "  a b c d e f g h",
                "Castling: -",
                "En-Passant: e3",
            ]
        );
    }

    #[test]
    fn valid_fens_are_consistent() {
        for fen in VALID {
            let b = Board::from_fen(fen).unwrap_or_else(|err| panic!("{fen}: {err:?}"));
            assert_consistent(&b);
        }
    }

    #[test]
    fn invalid_fens_are_rejected() {
        use FenParseError as E;
        // each one is the position "4k3/8/8/8/8/8/8/4K3 w - - 0 1" with a single defect
        #[rustfmt::skip]
        let invalid = [
            ("", E::MissingField("piece placement")),
            ("4k3/8/8/8/8/8/4K3 w - - 0 1", E::WrongRankCount(7)),
            ("4k3/8/8/8/8/8/8/8/4K3 w - - 0 1", E::WrongRankCount(9)),
            ("4k3/8/8/8/7/8/8/4K3 w - - 0 1", E::WrongRankLength(4)),         // short rank
            ("4k3/8/8/8/9/8/8/4K3 w - - 0 1", E::WrongRankLength(4)),         // long rank (digit)
            ("4k3/8/8/8/4p4/8/8/4K3 w - - 0 1", E::WrongRankLength(4)),       // long rank (mixed)
            ("4k3/8/8/8/8/8/PPPPPPPPP/4K3 w - - 0 1", E::WrongRankLength(2)), // long rank (pieces)
            ("4k3/8/8/8/08/8/8/4K3 w - - 0 1", E::ZeroDigit),
            ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKXNR w KQkq - 0 1", E::InvalidPiece('X')),
        ];
        for (fen, err) in invalid {
            assert_eq!(Board::from_fen(fen).unwrap_err(), err, "{fen:?}");
        }
    }

    fn side_of(field: &str) -> Result<Color, FenParseError> {
        Board::from_fen(&format!("4k3/8/8/8/8/8/8/4K3 {field} - - 0 1")).map(|b| b.side_to_move())
    }

    /// kings and rooks on their initial squares: any combination of rights is possible
    fn castling_of(field: &str) -> Result<CastlingRights, FenParseError> {
        Board::from_fen(&format!("r3k2r/8/8/8/8/8/8/R3K2R w {field} - 0 1")).map(|b| b.castling())
    }

    fn ep_of(fen: &str) -> Result<Option<Square>, FenParseError> {
        Board::from_fen(fen).map(|b| b.en_passant())
    }

    /// the rule `sanity_check` rejected the position for
    fn inconsistency_of(fen: &str) -> BoardInconsistencyError {
        match Board::from_fen(fen) {
            Err(FenParseError::IllegalPosition(err)) => err,
            other => panic!("{fen:?}: expected an illegal position, got {other:?}"),
        }
    }

    #[test]
    fn side_to_move_parse() {
        assert_eq!(side_of("w").unwrap(), Color::White);
        assert_eq!(side_of("b").unwrap(), Color::Black);
        assert_eq!(side_of("x").unwrap_err(), FenParseError::InvalidSideToMove);
        assert_eq!(side_of("W").unwrap_err(), FenParseError::InvalidSideToMove);
    }

    #[test]
    fn castling_parse() {
        use CastlingRights as CR;
        assert_eq!(castling_of("-").unwrap(), CR::NONE);
        assert_eq!(castling_of("KQkq").unwrap(), CR::ALL);
        assert_eq!(castling_of("K").unwrap(), CR::WHITE_SHORT);
        assert_eq!(castling_of("Q").unwrap(), CR::WHITE_LONG);
        assert_eq!(castling_of("k").unwrap(), CR::BLACK_SHORT);
        assert_eq!(castling_of("q").unwrap(), CR::BLACK_LONG);
        assert_eq!(castling_of("Kq").unwrap(), CR::WHITE_SHORT | CR::BLACK_LONG);
    }

    #[test]
    fn castling_invalid() {
        for field in ["X", "K-", "KQkqX", "kK2"] {
            assert_eq!(
                castling_of(field).unwrap_err(),
                FenParseError::InvalidCastling,
                "{field:?}"
            );
        }
        // policy decision: reject repeated rights
        assert_eq!(
            castling_of("KK").unwrap_err(),
            FenParseError::InvalidCastling
        );
    }

    #[test]
    fn en_passant_parse() {
        assert_eq!(ep_of(STARTPOS).unwrap(), None);
        // the last move was a double push and an enemy pawn beside it can capture
        let cases = [
            ("4k3/8/8/8/Pp6/8/8/4K3 b - a3 0 1", Square::A3), // a2-a4
            ("4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1", Square::E3), // e2-e4
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", Square::D6), // d7-d5
            ("4k3/8/8/6Pp/8/8/8/4K3 w - h6 0 1", Square::H6), // h7-h5
        ];
        for (fen, sq) in cases {
            assert_eq!(ep_of(fen).unwrap(), Some(sq), "{fen}");
        }
    }

    #[test]
    fn en_passant_invalid() {
        // position where "d6" would be valid: only the en passant field changes
        for field in ["i6", "e", "e66", "E6", "6e", "--"] {
            let fen = format!("4k3/8/8/3pP3/8/8/8/4K3 w - {field} 0 1");
            assert_eq!(
                ep_of(&fen).unwrap_err(),
                FenParseError::InvalidEnPassant,
                "{field:?}"
            );
        }
        // policy decision: the rank must match the side to move
        let mismatch = [
            ("4k3/8/8/3pP3/8/8/8/4K3 w - e4 0 1", Square::E4),
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d3 0 1", Square::D3),
            ("4k3/8/8/8/3pP3/8/8/4K3 b - e6 0 1", Square::E6),
        ];
        for (fen, sq) in mismatch {
            assert_eq!(
                ep_of(fen).unwrap_err(),
                FenParseError::EnPassantSideMismatch(sq),
                "{fen}"
            );
        }
    }

    #[test]
    fn en_passant_must_match_the_board() {
        use BoardInconsistencyError::{EnPassantBlocked, EnPassantWithoutPawn};

        // each one is a valid en passant position with a single defect
        let without_pawn = [
            "4k3/8/8/8/3p4/8/8/4K3 b - e3 0 1",  // no pawn on e4
            "4k3/8/8/8/3pp3/8/8/4K3 b - e3 0 1", // pawn on e4 has the wrong color
            "4k3/8/8/8/3pN3/8/8/4K3 b - e3 0 1", // knight on e4 instead of a pawn
            "4k3/8/8/4P3/8/8/8/4K3 w - d6 0 1",  // no pawn on d5
            "4k3/8/8/3PP3/8/8/8/4K3 w - d6 0 1", // pawn on d5 has the wrong color
        ];
        for fen in without_pawn {
            assert!(
                matches!(inconsistency_of(fen), EnPassantWithoutPawn(_)),
                "{fen:?}"
            );
        }

        let blocked = [
            "4k3/8/8/8/3pP3/4N3/8/4K3 b - e3 0 1", // target square e3 occupied
            "4k3/8/8/8/3pP3/8/4N3/4K3 b - e3 0 1", // origin square e2 occupied
            "4k3/8/3n4/3pP3/8/8/8/4K3 w - d6 0 1", // target square d6 occupied
            "4k3/3n4/8/3pP3/8/8/8/4K3 w - d6 0 1", // origin square d7 occupied
        ];
        for fen in blocked {
            assert!(
                matches!(inconsistency_of(fen), EnPassantBlocked(_)),
                "{fen:?}"
            );
        }

        // only the target and origin squares must be empty, the rest of the file doesn't matter
        let valid = [
            "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1",  // white king on e1
            "3qk3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", // black queen on d8
        ];
        for fen in valid {
            assert!(Board::from_fen(fen).is_ok(), "should accept: {fen:?}");
        }
    }

    /// boards built by `make_move` skip `from_fen`'s checks, so `sanity_check`
    /// must reject an en passant square on the wrong rank by itself
    #[test]
    fn en_passant_rank_must_match_side_to_move() {
        use BoardInconsistencyError::EnPassantWrongRank;

        // every other en passant check passes: e2 and e3 are empty and there's
        // an enemy pawn on e4, but with white to move e3 is impossible
        let mut b = Board::from_fen("4k3/8/8/8/4p3/8/8/4K3 w - - 0 1").unwrap();
        b.en_passant = Some(Square::E3);
        assert_eq!(b.sanity_check(), Err(EnPassantWrongRank(Square::E3)));

        // the same for black to move: d6 and d7 are empty and there's a white pawn on d5
        let mut b = Board::from_fen("4k3/8/8/3P4/8/8/8/4K3 b - - 0 1").unwrap();
        b.en_passant = Some(Square::D6);
        assert_eq!(b.sanity_check(), Err(EnPassantWrongRank(Square::D6)));
    }

    #[test]
    fn king_count() {
        use BoardInconsistencyError::KingCount;
        let cases = [
            ("4k3/8/8/8/8/8/8/8 w - - 0 1", KingCount(Color::White)), // no white king
            ("8/8/8/8/8/8/8/4K3 w - - 0 1", KingCount(Color::Black)), // no black king
            ("4k3/8/8/8/8/8/8/3KK3 w - - 0 1", KingCount(Color::White)), // two white kings
            ("3kk3/8/8/8/8/8/8/4K3 w - - 0 1", KingCount(Color::Black)), // two black kings
        ];
        for (fen, err) in cases {
            assert_eq!(inconsistency_of(fen), err, "{fen:?}");
        }
    }

    #[test]
    fn pawns_on_back_rank() {
        for fen in [
            "4k2P/8/8/8/8/8/8/4K3 w - - 0 1", // white pawn on h8
            "4k3/8/8/8/8/8/8/P3K3 w - - 0 1", // white pawn on a1
            "4k3/8/8/8/8/8/8/4K2p w - - 0 1", // black pawn on h1
            "p3k3/8/8/8/8/8/8/4K3 w - - 0 1", // black pawn on a8
        ] {
            assert_eq!(
                inconsistency_of(fen),
                BoardInconsistencyError::PawnOnBackRank,
                "{fen:?}"
            );
        }
    }

    #[test]
    fn castling_needs_king_and_rook_on_initial_squares() {
        use BoardInconsistencyError::CastlingWithoutPieces;
        use CastlingRights as CR;

        // each one is "r3k2r/8/8/8/8/8/8/R3K2R" with a single piece missing, moved or replaced
        let invalid = [
            ("r3k2r/8/8/8/8/8/8/R3K3 w K - 0 1", CR::WHITE_SHORT), // no rook on h1
            ("r3k2r/8/8/8/8/8/8/4K2R w Q - 0 1", CR::WHITE_LONG),  // no rook on a1
            ("r3k3/8/8/8/8/8/8/R3K2R w k - 0 1", CR::BLACK_SHORT), // no rook on h8
            ("4k2r/8/8/8/8/8/8/R3K2R w q - 0 1", CR::BLACK_LONG),  // no rook on a8
            ("r3k2r/8/8/8/8/8/8/R4K1R w K - 0 1", CR::WHITE_SHORT), // white king on f1
            ("r2k3r/8/8/8/8/8/8/R3K2R w q - 0 1", CR::BLACK_LONG), // black king on d8
            ("r3k2r/8/8/8/8/8/8/R3K2r w K - 0 1", CR::WHITE_SHORT), // black rook on h1
        ];
        for (fen, right) in invalid {
            assert_eq!(
                inconsistency_of(fen),
                CastlingWithoutPieces(right),
                "{fen:?}"
            );
        }

        // a missing rook only matters for its own right
        assert!(Board::from_fen("r3k2r/8/8/8/8/8/8/R3K3 w Qkq - 0 1").is_ok());
    }

    #[test]
    fn side_not_to_move_cannot_be_in_check() {
        use BoardInconsistencyError::KingCanBeCaptured;

        // legal with the checked side to move, illegal with the other one,
        // since it could capture the king
        #[rustfmt::skip]
        let cases = [
            ("4k3/8/8/8/8/8/4R3/4K3", Color::Black), // rook e2
            ("4k3/3P4/8/8/8/8/8/4K3", Color::Black), // pawn d7
            ("4k3/8/3N4/8/8/8/8/4K3", Color::Black), // knight d6
            ("4k3/8/8/1B6/8/8/8/4K3", Color::Black), // bishop b5
            ("4k3/8/8/8/Q7/8/8/4K3",  Color::Black), // queen a4
            ("4k3/8/8/8/8/8/3p4/4K3", Color::White), // pawn d2
            ("4k3/4r3/8/8/8/8/8/4K3", Color::White), // rook e7
        ];
        let side = |color: Color| match color {
            Color::White => "w",
            Color::Black => "b",
        };
        for (placement, checked) in cases {
            let legal = format!("{placement} {} - - 0 1", side(checked));
            assert!(Board::from_fen(&legal).is_ok(), "should accept: {legal:?}");

            let illegal = format!("{placement} {} - - 0 1", side(checked.oposite()));
            assert_eq!(
                inconsistency_of(&illegal),
                KingCanBeCaptured(checked),
                "{illegal:?}"
            );
        }

        // adjacent kings attack each other, whoever is to move
        let adjacent = "8/8/8/8/8/8/3k4/4K3";
        assert_eq!(
            inconsistency_of(&format!("{adjacent} w - - 0 1")),
            KingCanBeCaptured(Color::Black)
        );
        assert_eq!(
            inconsistency_of(&format!("{adjacent} b - - 0 1")),
            KingCanBeCaptured(Color::White)
        );

        // not a check: the rook is blocked, and a pawn doesn't attack straight ahead
        for fen in [
            "4k3/8/8/4n3/8/8/4R3/4K3 w - - 0 1",
            "4k3/4P3/8/8/8/8/8/4K3 w - - 0 1",
        ] {
            assert!(Board::from_fen(fen).is_ok(), "should accept: {fen:?}");
        }
    }

    /// squares in index order, so failures print as a readable list
    fn squares_of(bb: BitBoard) -> Vec<Square> {
        bb.collect()
    }

    #[test]
    fn attackers_by_known_squares() {
        use Color::{Black, White};
        use Square as S;

        #[rustfmt::skip]
        let cases: [(&str, Square, Color, &[Square]); 23] = [
            // one attacker of each kind, all aiming at d4
            ("k7/8/8/8/8/2P5/8/7K w - - 0 1",     S::D4, White, &[S::C3]),
            ("k7/8/8/4p3/8/8/8/7K w - - 0 1",     S::D4, Black, &[S::E5]),
            ("k7/8/8/8/8/5N2/8/7K w - - 0 1",     S::D4, White, &[S::F3]),
            ("k7/8/8/8/8/8/8/6BK w - - 0 1",      S::D4, White, &[S::G1]),
            ("k7/8/8/8/8/8/8/3R3K w - - 0 1",     S::D4, White, &[S::D1]),
            ("k7/8/8/8/8/8/8/3Q3K w - - 0 1",     S::D4, White, &[S::D1]), // queen as a rook
            ("k7/8/8/8/8/8/8/6QK w - - 0 1",      S::D4, White, &[S::G1]), // queen as a bishop
            ("k7/8/8/8/8/4K3/8/8 w - - 0 1",      S::D4, White, &[S::E3]),
            // all of them together, and none of them is black
            ("k7/8/8/8/8/2P2N2/8/3R2BK w - - 0 1", S::D4, White, &[S::C3, S::F3, S::D1, S::G1]),
            ("k7/8/8/8/8/2P2N2/8/3R2BK w - - 0 1", S::D4, Black, &[]),
            // pawns only attack diagonally forward
            ("k7/8/8/8/8/3P4/8/7K w - - 0 1",     S::D4, White, &[]), // d3 pushes to d4
            ("k7/8/8/2P5/8/8/8/7K w - - 0 1",     S::D4, White, &[]), // c5 is past d4
            ("k7/8/8/2P5/8/8/8/7K w - - 0 1",     S::D6, White, &[S::C5]),
            ("k7/8/8/8/8/2p5/8/7K w - - 0 1",     S::D4, Black, &[]), // c3 is past d4 for black
            ("k7/8/8/8/8/2p5/8/7K w - - 0 1",     S::D2, Black, &[S::C3]),
            // sliders stop at the first piece, of either color
            ("k7/8/8/8/8/3p4/8/3R3K w - - 0 1",   S::D4, White, &[]),
            ("k7/8/8/8/8/3p4/8/3R3K w - - 0 1",   S::D3, White, &[S::D1]), // the blocker itself
            ("k7/8/8/8/8/4N3/8/6BK w - - 0 1",    S::D4, White, &[]),
            // an occupied target is still attacked: that's a capture, or a check
            ("k7/8/8/8/3n4/8/8/3R3K w - - 0 1",   S::D4, White, &[S::D1]),
            (STARTPOS, S::F3, White, &[S::E2, S::G2, S::G1]),
            (STARTPOS, S::C6, Black, &[S::B7, S::D7, S::B8]),
            (STARTPOS, S::F3, Black, &[]),
            (STARTPOS, S::E4, White, &[]),
        ];
        for (fen, sq, by, expected) in cases {
            let b = Board::from_fen(fen).unwrap_or_else(|err| panic!("{fen}: {err:?}"));
            let expected = expected.iter().fold(BitBoard::EMPTY, |acc, s| acc | s.bb());
            assert_eq!(
                squares_of(b.attackers_by(sq, by)),
                squares_of(expected),
                "{fen} {sq} by {by:?}"
            );
            assert_eq!(
                b.is_square_attacked(sq, by),
                !expected.is_empty(),
                "{fen} {sq} by {by:?}"
            );
        }
    }

    /// `attackers_by` looks from the target square; this looks from each piece instead
    fn attackers_by_definition(b: &Board, sq: Square, by: Color) -> BitBoard {
        let occupied = b.all_pieces_bb();
        b.by_color[by]
            .filter(|&from| {
                let attacks = match b.mailbox[from].unwrap().kind() {
                    PieceKind::Pawn => PAWN_ATTACKS[by][from],
                    PieceKind::Knight => KNIGHT_ATTACKS[from],
                    PieceKind::Bishop => bishop_attacks(from, occupied),
                    PieceKind::Rook => rook_attacks(from, occupied),
                    PieceKind::Queen => {
                        bishop_attacks(from, occupied) | rook_attacks(from, occupied)
                    }
                    PieceKind::King => KING_ATTACKS[from],
                };
                !(attacks & sq.bb()).is_empty()
            })
            .fold(BitBoard::EMPTY, |acc, from| acc | from.bb())
    }

    #[test]
    fn attackers_by_matches_definition() {
        for fen in VALID {
            let b = Board::from_fen(fen).unwrap();
            for sq in (0..64).map(Square::new) {
                for by in Color::ALL {
                    assert_eq!(
                        squares_of(b.attackers_by(sq, by)),
                        squares_of(attackers_by_definition(&b, sq, by)),
                        "{fen} {sq} by {by:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn missing_fields_rejected() {
        let placement = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR";
        let cases = [
            ("", "side to move"),
            (" w", "castling"),
            (" w KQkq", "en passant"),
        ];
        for (rest, field) in cases {
            let fen = format!("{placement}{rest}");
            assert_eq!(
                Board::from_fen(&fen).unwrap_err(),
                FenParseError::MissingField(field),
                "{fen:?}"
            );
        }
    }

    #[test]
    fn clocks_are_optional() {
        // policy decision: accept FEN without the clocks
        assert!(Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -").is_ok());
    }
}
