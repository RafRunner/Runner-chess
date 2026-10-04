use std::{fmt::Display, ops};

use crate::square::Square;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BitBoard(u64);

impl BitBoard {
    pub const EMPTY: Self = Self(0);

    pub const RANK_1: Self = Self(0xFF);
    pub const RANK_2: Self = Self(Self::RANK_1.0 << 8);
    pub const RANK_3: Self = Self(Self::RANK_1.0 << 16);
    pub const RANK_4: Self = Self(Self::RANK_1.0 << 24);
    pub const RANK_5: Self = Self(Self::RANK_1.0 << 32);
    pub const RANK_6: Self = Self(Self::RANK_1.0 << 40);
    pub const RANK_7: Self = Self(Self::RANK_1.0 << 48);
    pub const RANK_8: Self = Self(Self::RANK_1.0 << 56);

    pub const RANKS: [Self; 8] = [
        Self::RANK_1,
        Self::RANK_2,
        Self::RANK_3,
        Self::RANK_4,
        Self::RANK_5,
        Self::RANK_6,
        Self::RANK_7,
        Self::RANK_8,
    ];

    pub const FILE_A: Self = Self(0x0101_0101_0101_0101);
    pub const FILE_B: Self = Self(Self::FILE_A.0 << 1);
    pub const FILE_C: Self = Self(Self::FILE_A.0 << 2);
    pub const FILE_D: Self = Self(Self::FILE_A.0 << 3);
    pub const FILE_E: Self = Self(Self::FILE_A.0 << 4);
    pub const FILE_F: Self = Self(Self::FILE_A.0 << 5);
    pub const FILE_G: Self = Self(Self::FILE_A.0 << 6);
    pub const FILE_H: Self = Self(Self::FILE_A.0 << 7);

    pub const FILES: [Self; 8] = [
        Self::FILE_A,
        Self::FILE_B,
        Self::FILE_C,
        Self::FILE_D,
        Self::FILE_E,
        Self::FILE_F,
        Self::FILE_G,
        Self::FILE_H,
    ];

    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn count_ones(self) -> u32 {
        self.0.count_ones()
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl Iterator for BitBoard {
    type Item = Square;

    fn next(&mut self) -> Option<Self::Item> {
        let lsb = self.0.trailing_zeros();

        if lsb == 64 {
            None
        } else {
            self.0 &= self.0 - 1;
            Some(Square::new(lsb as u8))
        }
    }
}

impl ops::BitAnd for BitBoard {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl ops::BitAndAssign for BitBoard {
    fn bitand_assign(&mut self, rhs: Self) {
        *self = Self(self.0 & rhs.0)
    }
}

impl ops::BitOr for BitBoard {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl ops::BitOrAssign for BitBoard {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = Self(self.0 | rhs.0)
    }
}

impl Display for BitBoard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for rank in (0..8).rev() {
            write!(f, "{} ", rank + 1)?;
            for file in 0..8 {
                let test = self.0 & 1 << (rank * 8 + file) != 0;
                let c = if test { '1' } else { '0' };
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
    use BitBoard as BB;

    /// each constant must hold exactly the 8 squares its name says
    #[test]
    fn ranks_and_files_match_their_names() {
        let ranks = BB::RANKS;
        for (i, rank) in ranks.into_iter().enumerate() {
            let name = format!("RANK_{}", i + 1);
            assert_eq!(rank.count_ones(), 8, "{name}");
            for sq in rank {
                assert_eq!(sq.rank() as usize, i, "{sq} in {name}");
            }
        }

        let files = BB::FILES;
        for (i, file) in files.into_iter().enumerate() {
            let name = format!("FILE_{}", (b'A' + i as u8) as char);
            assert_eq!(file.count_ones(), 8, "{name}");
            for sq in file {
                assert_eq!(sq.file() as usize, i, "{sq} in {name}");
            }
        }
    }
}
