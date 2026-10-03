use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Color {
    White = 0,
    Black = 1,
}

impl Color {
    pub const ALL: [Color; 2] = [Color::White, Color::Black];
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
}

#[derive(Debug)]
pub struct PieceParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Piece(u8);

impl Piece {
    const CHARS: &'static [u8; 12] = b"PNBRQKpnbrqk";

    pub const fn new(color: Color, kind: PieceKind) -> Self {
        Self(color as u8 * 6 + kind as u8)
    }

    pub fn from_fen(p: char) -> Result<Self, PieceParseError> {
        if !p.is_ascii() {
            return Err(PieceParseError);
        }
        let index = Self::CHARS
            .iter()
            .position(|c| c == &(p as u8))
            .ok_or(PieceParseError)?;

        Ok(Self(index as u8))
    }

    pub const fn kind(self) -> PieceKind {
        unsafe { std::mem::transmute(self.0 % 6) }
    }

    pub const fn color(self) -> Color {
        if self.0 > 5 {
            Color::Black
        } else {
            Color::White
        }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn to_char(self) -> char {
        Self::CHARS[self.0 as usize] as char
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
        assert_eq!(Piece::new(Color::White, PieceKind::Pawn).index(), 0);
        assert_eq!(Piece::new(Color::White, PieceKind::King).index(), 5);
        assert_eq!(Piece::new(Color::Black, PieceKind::Pawn).index(), 6);
        assert_eq!(Piece::new(Color::Black, PieceKind::King).index(), 11);
    }

    #[test]
    fn piece_is_one_byte() {
        assert_eq!(std::mem::size_of::<Piece>(), 1);
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
        // 'Ő' é U+0150: um `as u8` trunca para 0x50, que é b'P'
        for c in ['x', 'X', '1', '.', ' ', 'Ő'] {
            assert!(Piece::from_fen(c).is_err(), "deveria rejeitar: {c:?}");
        }
    }
}
