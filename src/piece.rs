use std::{fmt::Display, ops};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Color {
    White = 0,
    Black = 1,
}

impl Color {
    pub const ALL: [Color; 2] = [Color::White, Color::Black];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn oposite(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }

    pub const fn to_char(self) -> char {
        match self {
            Color::White => 'w',
            Color::Black => 'b',
        }
    }
}

impl<T> ops::Index<Color> for [T; 2] {
    type Output = T;

    fn index(&self, index: Color) -> &Self::Output {
        &self[index.index()]
    }
}

impl<T> ops::IndexMut<Color> for [T; 2] {
    fn index_mut(&mut self, index: Color) -> &mut Self::Output {
        &mut self[index.index()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PieceKind {
    Pawn = 0,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceKind {
    pub const ALL: [PieceKind; 6] = [
        Self::Pawn,
        Self::Knight,
        Self::Bishop,
        Self::Rook,
        Self::Queen,
        Self::King,
    ];
    const CHARS: &'static [u8; 6] = b"pnbrqk";

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn to_char(self) -> char {
        Self::CHARS[self.index()] as char
    }
}

#[derive(Debug)]
pub struct PieceParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Piece {
    WhitePawn = 0,
    WhiteKnight,
    WhiteBishop,
    WhiteRook,
    WhiteQueen,
    WhiteKing,

    BlackPawn,
    BlackKnight,
    BlackBishop,
    BlackRook,
    BlackQueen,
    BlackKing,
}

impl Piece {
    pub const ALL: [Self; 12] = [
        Self::WhitePawn,
        Self::WhiteKnight,
        Self::WhiteBishop,
        Self::WhiteRook,
        Self::WhiteQueen,
        Self::WhiteKing,
        Self::BlackPawn,
        Self::BlackKnight,
        Self::BlackBishop,
        Self::BlackRook,
        Self::BlackQueen,
        Self::BlackKing,
    ];
    const CHARS: &'static [u8; 12] = b"PNBRQKpnbrqk";

    pub const fn new(color: Color, kind: PieceKind) -> Self {
        Self::ALL[color.index() * 6 + kind.index()]
    }

    pub fn from_fen(p: char) -> Result<Self, PieceParseError> {
        if !p.is_ascii() {
            return Err(PieceParseError);
        }
        let index = Self::CHARS
            .iter()
            .position(|c| c == &(p as u8))
            .ok_or(PieceParseError)?;

        Ok(Self::ALL[index])
    }

    pub const fn kind(self) -> PieceKind {
        match self {
            Self::WhitePawn | Self::BlackPawn => PieceKind::Pawn,
            Self::WhiteKnight | Self::BlackKnight => PieceKind::Knight,
            Self::WhiteBishop | Self::BlackBishop => PieceKind::Bishop,
            Self::WhiteRook | Self::BlackRook => PieceKind::Rook,
            Self::WhiteQueen | Self::BlackQueen => PieceKind::Queen,
            Self::WhiteKing | Self::BlackKing => PieceKind::King,
        }
    }

    pub const fn color(self) -> Color {
        if self as u8 > 5 {
            Color::Black
        } else {
            Color::White
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn to_char(self) -> char {
        Self::CHARS[self as usize] as char
    }
}

impl<T> ops::Index<Piece> for [T; 12] {
    type Output = T;

    fn index(&self, index: Piece) -> &Self::Output {
        &self[index.index()]
    }
}

impl<T> ops::IndexMut<Piece> for [T; 12] {
    fn index_mut(&mut self, index: Piece) -> &mut Self::Output {
        &mut self[index.index()]
    }
}

impl Display for Piece {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_char())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_roundtrips_color_and_kind() {
        for color in Color::ALL {
            for kind in PieceKind::ALL {
                let p = Piece::new(color, kind);
                assert_eq!(p.color(), color, "{p:?}");
                assert_eq!(p.kind(), kind, "{p:?}");
            }
        }
    }

    #[test]
    fn indices_are_unique_and_in_range() {
        let mut seen = [false; 12];
        for color in Color::ALL {
            for kind in PieceKind::ALL {
                let i = Piece::new(color, kind).index();
                assert!(i < 12, "index {i} out of range");
                assert!(!seen[i], "index {i} repeated");
                seen[i] = true;
            }
        }
    }

    #[test]
    fn index_layout_is_stable() {
        assert_eq!(Piece::WhitePawn.index(), 0);
        assert_eq!(Piece::WhiteKing.index(), 5);
        assert_eq!(Piece::BlackPawn.index(), 6);
        assert_eq!(Piece::BlackKing.index(), 11);
    }

    #[test]
    fn piece_is_one_byte() {
        assert_eq!(size_of::<Piece>(), 1);
        assert_eq!(size_of::<Option<Piece>>(), 1);
    }

    #[test]
    fn to_char_uses_fen_letters() {
        let letters = [
            (PieceKind::Pawn, 'P'),
            (PieceKind::Knight, 'N'),
            (PieceKind::Bishop, 'B'),
            (PieceKind::Rook, 'R'),
            (PieceKind::Queen, 'Q'),
            (PieceKind::King, 'K'),
        ];
        for (kind, upper) in letters {
            let white = Piece::new(Color::White, kind);
            let black = Piece::new(Color::Black, kind);
            assert_eq!(white.to_char(), upper);
            assert_eq!(black.to_char(), upper.to_ascii_lowercase());
            assert_eq!(white.to_string(), upper.to_string());
        }
    }

    #[test]
    fn from_fen_roundtrips_to_char() {
        for color in Color::ALL {
            for kind in PieceKind::ALL {
                let p = Piece::new(color, kind);
                assert_eq!(Piece::from_fen(p.to_char()).unwrap(), p, "{p:?}");
            }
        }
    }

    #[test]
    fn from_fen_rejects_invalid_chars() {
        // 'Ő' is U+0150: `as u8` truncates it to 0x50, which is b'P'
        for c in ['x', 'X', '1', '.', ' ', 'Ő'] {
            assert!(Piece::from_fen(c).is_err(), "should reject: {c:?}");
        }
    }
}
