use std::fmt::{Display, Formatter};

use crate::{
    castling::CastlingRights,
    piece::{Color, Piece},
    square::Square,
};

#[derive(Debug)]
pub struct Board {
    pieces: [u64; 12],
    mailbox: [Option<Piece>; 64],
    side_to_move: Color,
    castling: CastlingRights,
    en_passant: Option<Square>,
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
                    let piece = Piece::from_fen(c).map_err(|_| FenParseError)?;
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
        board.castling = CastlingRights::from_fen(castle).map_err(|_| FenParseError)?;

        let en_passant = parts.next().ok_or(FenParseError)?;
        board.en_passant = match en_passant {
            "-" => None,
            square => {
                let square = Square::from_algebraic(square).map_err(|_| FenParseError)?;
                if board.side_to_move == Color::White && square.rank() != 5
                    || board.side_to_move == Color::Black && square.rank() != 2
                {
                    return Err(FenParseError);
                } else {
                    Some(square)
                }
            }
        };

        Ok(board)
    }

    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        self.mailbox[sq.index()]
    }
}

impl Display for Board {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        for rank in (0..8).rev() {
            write!(f, "{} ", rank + 1)?;
            for file in 0..8 {
                let c = self
                    .piece_at(Square::from_file_and_rank(file, rank))
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
        let wp = Piece::new(Color::White, PieceKind::Pawn).index();
        let bp = Piece::new(Color::Black, PieceKind::Pawn).index();
        assert_eq!(b.pieces[wp], 0x0000_0000_0000_FF00);
        assert_eq!(b.pieces[bp], 0x00FF_0000_0000_0000);
    }

    #[test]
    fn asymmetric_position_catches_flips() {
        let b = Board::from_fen("k7/8/8/8/8/8/8/7K w - - 0 1").unwrap();
        assert_eq!(b.piece_at(Square::A8), p(Color::Black, PieceKind::King));
        assert_eq!(b.piece_at(Square::H1), p(Color::White, PieceKind::King));
        assert_eq!(b.mailbox.iter().filter(|s| s.is_some()).count(), 2);
    }

    /// linhas do Display, sem os espaços no fim de cada fileira
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
            let b = Board::from_fen(fen).unwrap_or_else(|_| panic!("falhou: {fen}"));
            assert_consistent(&b);
        }
    }

    #[test]
    fn invalid_fens_are_rejected() {
        // cada uma é a posição "4k3/8/8/8/8/8/8/4K3 w - - 0 1" com um único defeito
        let invalid = [
            "",
            "4k3/8/8/8/8/8/4K3 w - - 0 1",           // 7 fileiras
            "4k3/8/8/8/8/8/8/8/4K3 w - - 0 1",       // 9 fileiras
            "4k3/8/8/8/7/8/8/4K3 w - - 0 1",         // fileira curta
            "4k3/8/8/8/9/8/8/4K3 w - - 0 1",         // fileira longa (dígito)
            "4k3/8/8/8/4p4/8/8/4K3 w - - 0 1",       // fileira longa (misto)
            "4k3/8/8/8/8/8/PPPPPPPPP/4K3 w - - 0 1", // fileira longa (peças)
            "4k3/8/8/8/08/8/8/4K3 w - - 0 1",        // dígito zero
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKXNR w KQkq - 0 1", // peça inválida
        ];
        for fen in invalid {
            assert!(Board::from_fen(fen).is_err(), "deveria rejeitar: {fen:?}");
        }
    }

    fn side_of(field: &str) -> Result<Color, FenParseError> {
        Board::from_fen(&format!("4k3/8/8/8/8/8/8/4K3 {field} - - 0 1")).map(|b| b.side_to_move)
    }

    /// reis e torres nas casas iniciais: qualquer combinação de direitos é possível
    fn castling_of(field: &str) -> Result<CastlingRights, FenParseError> {
        Board::from_fen(&format!("r3k2r/8/8/8/8/8/8/R3K2R w {field} - 0 1")).map(|b| b.castling)
    }

    fn ep_of(fen: &str) -> Result<Option<Square>, FenParseError> {
        Board::from_fen(fen).map(|b| b.en_passant)
    }

    #[test]
    fn side_to_move_parse() {
        assert_eq!(side_of("w").unwrap(), Color::White);
        assert_eq!(side_of("b").unwrap(), Color::Black);
        assert!(side_of("x").is_err());
        assert!(side_of("W").is_err());
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
        assert_eq!(castling_of("Kq").unwrap(), CR::WK | CR::BQ);
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
        assert_eq!(ep_of(STARTPOS).unwrap(), None);
        // o último lance foi um avanço duplo e há um peão inimigo do lado para capturar
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
        // posição em que "d6" seria válido: só o campo de en passant muda
        for field in ["e4", "i6", "e", "e66", "E6", "6e", "--"] {
            let fen = format!("4k3/8/8/3pP3/8/8/8/4K3 w - {field} 0 1");
            assert!(ep_of(&fen).is_err(), "deveria rejeitar: {field:?}");
        }
        // decisão de política: a fileira precisa bater com quem joga
        assert!(ep_of("4k3/8/8/3pP3/8/8/8/4K3 w - d3 0 1").is_err());
        assert!(ep_of("4k3/8/8/8/3pP3/8/8/4K3 b - e6 0 1").is_err());
    }

    #[test]
    fn missing_fields_rejected() {
        for fen in [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq",
        ] {
            assert!(Board::from_fen(fen).is_err(), "deveria rejeitar: {fen:?}");
        }
    }

    #[test]
    fn clocks_are_optional() {
        // decisão de política: aceitar FEN sem os relógios
        assert!(Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -").is_ok());
    }
}
