use std::{
    fmt::{Debug, Display},
    ops,
};

use crate::bitboard::BitBoard;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Square(u8);

#[derive(Debug)]
pub struct SquareParseError;

/// a displacement in files and ranks, from white's point of view (north is towards rank 8)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delta {
    file: i8,
    rank: i8,
}

impl Delta {
    pub const NORTH: Self = Self::new(0, 1);
    pub const SOUTH: Self = Self::new(0, -1);
    pub const EAST: Self = Self::new(1, 0);
    pub const WEST: Self = Self::new(-1, 0);
    pub const NORTH_EAST: Self = Self::new(1, 1);
    pub const NORTH_WEST: Self = Self::new(-1, 1);
    pub const SOUTH_EAST: Self = Self::new(1, -1);
    pub const SOUTH_WEST: Self = Self::new(-1, -1);

    pub const fn new(file: i8, rank: i8) -> Self {
        Self { file, rank }
    }
}

impl Square {
    pub const A1: Self = Self(0);
    pub const B1: Self = Self(1);
    pub const C1: Self = Self(2);
    pub const D1: Self = Self(3);
    pub const E1: Self = Self(4);
    pub const F1: Self = Self(5);
    pub const G1: Self = Self(6);
    pub const H1: Self = Self(7);

    pub const A2: Self = Self(8);
    pub const B2: Self = Self(9);
    pub const C2: Self = Self(10);
    pub const D2: Self = Self(11);
    pub const E2: Self = Self(12);
    pub const F2: Self = Self(13);
    pub const G2: Self = Self(14);
    pub const H2: Self = Self(15);

    pub const A3: Self = Self(16);
    pub const B3: Self = Self(17);
    pub const C3: Self = Self(18);
    pub const D3: Self = Self(19);
    pub const E3: Self = Self(20);
    pub const F3: Self = Self(21);
    pub const G3: Self = Self(22);
    pub const H3: Self = Self(23);

    pub const A4: Self = Self(24);
    pub const B4: Self = Self(25);
    pub const C4: Self = Self(26);
    pub const D4: Self = Self(27);
    pub const E4: Self = Self(28);
    pub const F4: Self = Self(29);
    pub const G4: Self = Self(30);
    pub const H4: Self = Self(31);

    pub const A5: Self = Self(32);
    pub const B5: Self = Self(33);
    pub const C5: Self = Self(34);
    pub const D5: Self = Self(35);
    pub const E5: Self = Self(36);
    pub const F5: Self = Self(37);
    pub const G5: Self = Self(38);
    pub const H5: Self = Self(39);

    pub const A6: Self = Self(40);
    pub const B6: Self = Self(41);
    pub const C6: Self = Self(42);
    pub const D6: Self = Self(43);
    pub const E6: Self = Self(44);
    pub const F6: Self = Self(45);
    pub const G6: Self = Self(46);
    pub const H6: Self = Self(47);

    pub const A7: Self = Self(48);
    pub const B7: Self = Self(49);
    pub const C7: Self = Self(50);
    pub const D7: Self = Self(51);
    pub const E7: Self = Self(52);
    pub const F7: Self = Self(53);
    pub const G7: Self = Self(54);
    pub const H7: Self = Self(55);

    pub const A8: Self = Self(56);
    pub const B8: Self = Self(57);
    pub const C8: Self = Self(58);
    pub const D8: Self = Self(59);
    pub const E8: Self = Self(60);
    pub const F8: Self = Self(61);
    pub const G8: Self = Self(62);
    pub const H8: Self = Self(63);

    pub const fn new(raw: u8) -> Self {
        debug_assert!(raw < 64, "Square index must be between 0-63");

        Self(raw)
    }

    pub const fn from_file_and_rank(file: u8, rank: u8) -> Self {
        debug_assert!(file < 8, "file must be between 0-7");
        debug_assert!(rank < 8, "rank must be between 0-7");

        Self::new(rank * 8 + file)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn bb(self) -> BitBoard {
        BitBoard::new(1 << self.0)
    }

    pub const fn file(self) -> u8 {
        self.0 % 8
    }

    pub const fn rank(self) -> u8 {
        self.0 / 8
    }

    /// the square `delta` away, or `None` if it falls off the board
    pub const fn offset(self, delta: Delta) -> Option<Self> {
        let file = self.file() as i8 + delta.file;
        let rank = self.rank() as i8 + delta.rank;

        if file >= 0 && file < 8 && rank >= 0 && rank < 8 {
            Some(Self::from_file_and_rank(file as u8, rank as u8))
        } else {
            None
        }
    }

    pub fn from_algebraic(s: &str) -> Result<Self, SquareParseError> {
        if let [file, rank] = s.as_bytes() {
            if !(b'a'..=b'h').contains(file) {
                return Err(SquareParseError);
            }
            if !(b'1'..=b'8').contains(rank) {
                return Err(SquareParseError);
            }

            Ok(Self::from_file_and_rank(file - b'a', rank - b'1'))
        } else {
            Err(SquareParseError)
        }
    }

    pub fn to_algebraic(self) -> String {
        let file = self.file();
        let rank = self.rank();

        format!("{}{}", (file + b'a') as char, (rank + b'1') as char)
    }
}

impl<T> ops::Index<Square> for [T; 64] {
    type Output = T;

    fn index(&self, index: Square) -> &Self::Output {
        &self[index.index()]
    }
}

impl<T> ops::IndexMut<Square> for [T; 64] {
    fn index_mut(&mut self, index: Square) -> &mut Self::Output {
        &mut self[index.index()]
    }
}

impl Display for Square {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_algebraic())
    }
}

impl Debug for Square {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_algebraic())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_algebraic_test() {
        let cases = [
            // (file, rank, name)
            (0, 0, "a1"),
            (7, 0, "h1"),
            (0, 7, "a8"),
            (7, 7, "h8"),
            (4, 3, "e4"),
            (3, 4, "d5"), // file and rank swapped relative to e4
        ];
        for (file, rank, name) in cases {
            let sq = Square::from_file_and_rank(file, rank);
            assert_eq!(sq.to_algebraic(), name);
            assert_eq!(sq.to_string(), name);
        }

        // all 64 squares: the generated name maps back to the same square
        for i in 0..64 {
            let sq = Square::new(i);
            let name = sq.to_algebraic();
            assert_eq!(Square::from_algebraic(&name).unwrap(), sq, "{name}");
            assert_eq!(sq.to_string(), name);
        }
    }

    /// each constant must have the index its name says
    #[test]
    fn constants_match_their_names() {
        use Square as S;
        #[rustfmt::skip]
        let all = [
            S::A1, S::B1, S::C1, S::D1, S::E1, S::F1, S::G1, S::H1,
            S::A2, S::B2, S::C2, S::D2, S::E2, S::F2, S::G2, S::H2,
            S::A3, S::B3, S::C3, S::D3, S::E3, S::F3, S::G3, S::H3,
            S::A4, S::B4, S::C4, S::D4, S::E4, S::F4, S::G4, S::H4,
            S::A5, S::B5, S::C5, S::D5, S::E5, S::F5, S::G5, S::H5,
            S::A6, S::B6, S::C6, S::D6, S::E6, S::F6, S::G6, S::H6,
            S::A7, S::B7, S::C7, S::D7, S::E7, S::F7, S::G7, S::H7,
            S::A8, S::B8, S::C8, S::D8, S::E8, S::F8, S::G8, S::H8,
        ];
        for (i, sq) in all.into_iter().enumerate() {
            assert_eq!(sq.index(), i, "{}", sq.to_algebraic());
        }
        assert_eq!(S::E4.to_algebraic(), "e4");
        assert_eq!(S::from_algebraic("d5").unwrap(), S::D5);
    }

    #[test]
    fn offset_moves_by_files_and_ranks() {
        use Square as S;
        let cases = [
            (Delta::NORTH, S::E5),
            (Delta::SOUTH, S::E3),
            (Delta::EAST, S::F4),
            (Delta::WEST, S::D4),
            (Delta::NORTH_EAST, S::F5),
            (Delta::NORTH_WEST, S::D5),
            (Delta::SOUTH_EAST, S::F3),
            (Delta::SOUTH_WEST, S::D3),
            (Delta::new(2, -1), S::G3),
        ];
        for (delta, to) in cases {
            assert_eq!(S::E4.offset(delta), Some(to), "{delta:?}");
        }
    }

    /// some of these stay inside 0..64 when added to the index (h1 + east is a2),
    /// so the board edge has to come from the file and rank
    #[test]
    fn offset_off_the_board_is_none() {
        use Square as S;
        let cases = [
            (S::H1, Delta::EAST),
            (S::A1, Delta::WEST),
            (S::A1, Delta::SOUTH),
            (S::H8, Delta::NORTH),
            (S::A8, Delta::NORTH_WEST),
            (S::G1, Delta::new(2, 1)), // a knight jump that would wrap to a3
        ];
        for (from, delta) in cases {
            assert_eq!(from.offset(delta), None, "{from} + {delta:?}");
        }
    }
}
