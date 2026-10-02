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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Piece(u8);

impl Piece {
    pub const fn new(color: Color, kind: PieceKind) -> Self {
        Self(color as u8 * 6 + kind as u8)
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
}
