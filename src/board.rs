use std::fmt::{Display, Formatter};

use crate::{
    castling::CastlingRights,
    piece::{Color, Piece, PieceKind},
};

#[derive(Debug)]
pub struct Board {
    pieces: [u64; 12],
    mailbox: [Option<Piece>; 64],
    side_to_move: Color,
    castling: CastlingRights,
    en_passant: Option<u8>,
}

#[derive(Debug)]
pub struct FenParseError;

impl Board {
    pub fn empty() -> Self {
        Board {
            pieces: [0; 12],
            mailbox: [None; 64],
            side_to_move: Color::White,
            castling: CastlingRights::NONE,
            en_passant: None,
        }
    }

    pub fn from_fen(fen: &str) -> Result<Self, FenParseError> {
        let mut board = Self::empty();
        let mut parts = fen.split_whitespace();
        let pieces = parts.next().ok_or(FenParseError)?;

        let ranks: Vec<&str> = pieces.split("/").collect();
        if ranks.len() != 8 {
            return Err(FenParseError);
        }

        for (rank_idx, rank) in ranks.iter().rev().enumerate() {
            let mut file: usize = 0;
            for c in rank.chars() {
                if file > 7 {
                    return Err(FenParseError);
                }

                if let Some(n) = c.to_digit(10) {
                    if n == 0 {
                        return Err(FenParseError);
                    }
                    file += n as usize;
                } else {
                    let color = if c.is_ascii_lowercase() {
                        Color::Black
                    } else {
                        Color::White
                    };

                    let kind = match c.to_ascii_lowercase() {
                        'p' => PieceKind::Pawn,
                        'n' => PieceKind::Knight,
                        'b' => PieceKind::Bishop,
                        'r' => PieceKind::Rook,
                        'q' => PieceKind::Queen,
                        'k' => PieceKind::King,
                        _ => return Err(FenParseError),
                    };

                    let piece = Piece::new(color, kind);
                    board.pieces[piece.index()] |= 1 << (rank_idx * 8 + file);
                    board.mailbox[rank_idx * 8 + file] = Some(piece);
                    file += 1;
                }
            }

            if file != 8 {
                return Err(FenParseError);
            }
        }

        let side_to_move = parts.next().ok_or(FenParseError)?;
        board.side_to_move = match side_to_move {
            "w" => Color::White,
            "b" => Color::Black,
            _ => return Err(FenParseError),
        };

        let castle = parts.next().ok_or(FenParseError)?;
        let mut castling = CastlingRights::NONE;
        if castle != "-" {
            for c in castle.chars() {
                let right = match c {
                    'K' => CastlingRights::WK,
                    'Q' => CastlingRights::WQ,
                    'k' => CastlingRights::BK,
                    'q' => CastlingRights::BQ,
                    _ => return Err(FenParseError),
                };
                if castling.has(right) {
                    return Err(FenParseError);
                }
                castling = castling.add(right);
            }
        }
        board.castling = castling;

        let en_passant = parts.next().ok_or(FenParseError)?;
        board.en_passant = match en_passant {
            "-" => None,
            square => {
                if let [file, rank] = square.as_bytes() {
                    if board.side_to_move == Color::Black && rank != &b'3'
                        || board.side_to_move == Color::White && rank != &b'6'
                    {
                        return Err(FenParseError);
                    }
                    if file < &b'a' || file > &b'h' {
                        return Err(FenParseError);
                    }
                    Some((rank - b'1') * 8 + file - b'a')
                } else {
                    return Err(FenParseError);
                }
            }
        };

        Ok(board)
    }
}

impl Display for Board {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        const CHARS: &[u8; 12] = b"PNBRQKpnbrqk";
        for rank in (0..8).rev() {
            write!(f, "{} ", rank + 1)?;
            for file in 0..8 {
                let c = match self.mailbox[rank * 8 + file] {
                    Some(p) => CHARS[p.index()] as char,
                    None => '.',
                };
                write!(f, "{c} ")?;
            }
            writeln!(f)?;
        }
        writeln!(f, "  a b c d e f g h")
    }
}

#[cfg(test)]
mod tests {
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

    /// mailbox e bitboards precisam descrever exatamente a mesma posição
    fn assert_consistent(b: &Board) {
        for sq in 0..64 {
            let bit = 1u64 << sq;
            let owners: Vec<usize> = (0..12).filter(|&i| b.pieces[i] & bit != 0).collect();
            match b.mailbox[sq] {
                Some(piece) => assert_eq!(owners, vec![piece.index()], "casa {sq}"),
                None => assert!(
                    owners.is_empty(),
                    "casa {sq} vazia no mailbox, mas em {owners:?}"
                ),
            }
        }
    }

    #[test]
    fn startpos_pieces_on_expected_squares() {
        let b = Board::from_fen(STARTPOS).unwrap();
        assert_eq!(b.mailbox[0], p(Color::White, PieceKind::Rook)); // a1
        assert_eq!(b.mailbox[4], p(Color::White, PieceKind::King)); // e1
        assert_eq!(b.mailbox[12], p(Color::White, PieceKind::Pawn)); // e2
        assert_eq!(b.mailbox[28], None); // e4
        assert_eq!(b.mailbox[59], p(Color::Black, PieceKind::Queen)); // d8
        assert_eq!(b.mailbox[60], p(Color::Black, PieceKind::King)); // e8
        assert_eq!(b.mailbox.iter().filter(|s| s.is_some()).count(), 32);
    }

    #[test]
    fn startpos_pawn_bitboards() {
        let b = Board::from_fen(STARTPOS).unwrap();
        let wp = Piece::new(Color::White, PieceKind::Pawn).index();
        let bp = Piece::new(Color::Black, PieceKind::Pawn).index();
        assert_eq!(b.pieces[wp], 0x0000_0000_0000_FF00);
        assert_eq!(b.pieces[bp], 0x00FF_0000_0000_0000);
    }

    #[test]
    fn asymmetric_position_catches_flips() {
        let b = Board::from_fen("k7/8/8/8/8/8/8/7K w - - 0 1").unwrap();
        assert_eq!(b.mailbox[56], p(Color::Black, PieceKind::King)); // a8
        assert_eq!(b.mailbox[7], p(Color::White, PieceKind::King)); // h1
        assert_eq!(b.mailbox.iter().filter(|s| s.is_some()).count(), 2);
    }

    #[test]
    fn valid_fens_are_consistent() {
        for fen in VALID {
            let b = Board::from_fen(fen).unwrap_or_else(|_| panic!("falhou: {fen}"));
            assert_consistent(&b);
        }
    }

    #[test]
    fn invalid_fens_are_rejected() {
        let invalid = [
            "",
            "8/8/8/8/8/8/8 w - - 0 1",           // 7 fileiras
            "8/8/8/8/8/8/8/8/8 w - - 0 1",       // 9 fileiras
            "7/8/8/8/8/8/8/8 w - - 0 1",         // fileira curta
            "9/8/8/8/8/8/8/8 w - - 0 1",         // fileira longa (dígito)
            "4p4/8/8/8/8/8/8/8 w - - 0 1",       // fileira longa (misto)
            "ppppppppp/8/8/8/8/8/8/8 w - - 0 1", // fileira longa (peças)
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKXNR w KQkq - 0 1", // peça inválida
        ];
        for fen in invalid {
            assert!(Board::from_fen(fen).is_err(), "deveria rejeitar: {fen:?}");
        }
    }

    fn castling_of(field: &str) -> Result<CastlingRights, FenParseError> {
        Board::from_fen(&format!("8/8/8/8/8/8/8/8 w {field} - 0 1")).map(|b| b.castling)
    }

    fn ep_of(side: &str, field: &str) -> Result<Option<u8>, FenParseError> {
        Board::from_fen(&format!("8/8/8/8/8/8/8/8 {side} - {field} 0 1")).map(|b| b.en_passant)
    }

    #[test]
    fn side_to_move_parse() {
        assert_eq!(
            Board::from_fen("8/8/8/8/8/8/8/8 w - - 0 1")
                .unwrap()
                .side_to_move,
            Color::White
        );
        assert_eq!(
            Board::from_fen("8/8/8/8/8/8/8/8 b - - 0 1")
                .unwrap()
                .side_to_move,
            Color::Black
        );
        assert!(Board::from_fen("8/8/8/8/8/8/8/8 x - - 0 1").is_err());
        assert!(Board::from_fen("8/8/8/8/8/8/8/8 W - - 0 1").is_err());
    }

    #[test]
    fn castling_parse() {
        use CastlingRights as CR;
        assert_eq!(castling_of("-").unwrap(), CR::NONE);
        assert_eq!(castling_of("KQkq").unwrap(), CR::ALL);
        assert_eq!(castling_of("K").unwrap(), CR::WK);
        assert_eq!(castling_of("Q").unwrap(), CR::WQ);
        assert_eq!(castling_of("k").unwrap(), CR::BK);
        assert_eq!(castling_of("q").unwrap(), CR::BQ);
        assert_eq!(castling_of("Kq").unwrap(), CR::WK.add(CR::BQ));
    }

    #[test]
    fn castling_invalid() {
        for field in ["X", "K-", "KQkqX", "kK2"] {
            assert!(castling_of(field).is_err(), "deveria rejeitar: {field:?}");
        }
        // decisão de política: rejeitar direitos repetidos
        assert!(castling_of("KK").is_err());
    }

    #[test]
    fn en_passant_parse() {
        assert_eq!(ep_of("w", "-").unwrap(), None);
        assert_eq!(ep_of("b", "a3").unwrap(), Some(16));
        assert_eq!(ep_of("b", "e3").unwrap(), Some(20));
        assert_eq!(ep_of("w", "d6").unwrap(), Some(43));
        assert_eq!(ep_of("w", "h6").unwrap(), Some(47));
    }

    #[test]
    fn en_passant_invalid() {
        for field in ["e4", "i6", "e", "e66", "E6", "6e", "--"] {
            assert!(ep_of("w", field).is_err(), "deveria rejeitar: {field:?}");
        }
        // decisão de política: a fileira precisa bater com quem joga
        assert!(ep_of("w", "e3").is_err());
        assert!(ep_of("b", "e6").is_err());
    }

    #[test]
    fn missing_fields_rejected() {
        for fen in [
            "8/8/8/8/8/8/8/8",
            "8/8/8/8/8/8/8/8 w",
            "8/8/8/8/8/8/8/8 w -",
        ] {
            assert!(Board::from_fen(fen).is_err(), "deveria rejeitar: {fen:?}");
        }
    }

    #[test]
    fn clocks_are_optional() {
        // decisão de política: aceitar FEN sem os relógios
        assert!(Board::from_fen("8/8/8/8/8/8/8/8 w - -").is_ok());
    }
}
