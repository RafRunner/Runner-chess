use std::{fmt::Display, ops};

use crate::square::Square;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CastlingRights(u8);

#[derive(Debug)]
pub struct CastlingParseError;

impl CastlingRights {
    pub const ALL: Self = Self(0b1111);
    pub const NONE: Self = Self(0b0000);
    pub const WHITE_SHORT: Self = Self(0b0001);
    pub const WHITE_LONG: Self = Self(0b0010);
    pub const BLACK_SHORT: Self = Self(0b0100);
    pub const BLACK_LONG: Self = Self(0b1000);

    const CASTLE_MASK: [u8; 64] = {
        let mut m = [0b1111u8; 64];
        m[Square::A1.index()] = !Self::WHITE_LONG.0 & 0b1111; // a1
        m[Square::H1.index()] = !Self::WHITE_SHORT.0 & 0b1111; // h1
        m[Square::E1.index()] = !(Self::WHITE_SHORT.0 | Self::WHITE_LONG.0) & 0b1111; // e1
        m[Square::A8.index()] = !Self::BLACK_LONG.0 & 0b1111; // a8
        m[Square::H8.index()] = !Self::BLACK_SHORT.0 & 0b1111; // h8
        m[Square::E8.index()] = !(Self::BLACK_SHORT.0 | Self::BLACK_LONG.0) & 0b1111; // e8
        m
    };

    pub fn has(self, right: Self) -> bool {
        self.0 | right.0 == self.0
    }

    pub fn update_move(self, from: usize, to: usize) -> Self {
        Self(self.0 & Self::CASTLE_MASK[from] & Self::CASTLE_MASK[to])
    }

    pub fn to_fen(self) -> String {
        let mut fen = String::new();
        if self.has(Self::WHITE_SHORT) {
            fen.push('K');
        }
        if self.has(Self::WHITE_LONG) {
            fen.push('Q');
        }
        if self.has(Self::BLACK_SHORT) {
            fen.push('k');
        }
        if self.has(Self::BLACK_LONG) {
            fen.push('q');
        }
        if fen.is_empty() {
            fen.push('-');
        }

        fen
    }

    pub fn from_fen(fen: &str) -> Result<Self, CastlingParseError> {
        let mut castling = Self::NONE;
        if fen != "-" {
            for c in fen.chars() {
                let right = match c {
                    'K' => Self::WHITE_SHORT,
                    'Q' => Self::WHITE_LONG,
                    'k' => Self::BLACK_SHORT,
                    'q' => Self::BLACK_LONG,
                    _ => return Err(CastlingParseError),
                };
                if castling.has(right) {
                    return Err(CastlingParseError);
                }
                castling = castling | right;
            }
        }

        Ok(castling)
    }
}

impl Display for CastlingRights {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_fen())
    }
}

impl ops::BitOr for CastlingRights {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl ops::Sub for CastlingRights {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 & !rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use CastlingRights as CR;
    use Square as S;

    const SINGLE: [CR; 4] = [
        CR::WHITE_SHORT,
        CR::WHITE_LONG,
        CR::BLACK_SHORT,
        CR::BLACK_LONG,
    ];

    /// initial king and rook squares, the only ones that affect castling rights
    const SPECIAL: [Square; 6] = [S::A1, S::E1, S::H1, S::A8, S::E8, S::H8];

    /// all 16 possible combinations of rights, built only with `|`
    fn every_combination() -> impl Iterator<Item = CR> {
        (0..16u8).map(|subset| {
            SINGLE
                .iter()
                .enumerate()
                .filter(|&(i, _)| subset & (1 << i) != 0)
                .fold(CR::NONE, |acc, (_, &r)| acc | r)
        })
    }

    #[test]
    fn rights_are_independent() {
        for a in SINGLE {
            for b in SINGLE.into_iter().filter(|&b| b != a) {
                assert!(!a.has(b), "{a:?} should not contain {b:?}");
            }
        }
        let distinct: HashSet<CR> = every_combination().collect();
        assert_eq!(distinct.len(), 16);
    }

    #[test]
    fn all_is_union_of_single_rights() {
        let union = SINGLE.iter().fold(CR::NONE, |acc, &r| acc | r);
        assert_eq!(union, CR::ALL);
    }

    #[test]
    fn has_requires_every_right_in_argument() {
        for r in SINGLE {
            assert!(CR::ALL.has(r));
            assert!(!CR::NONE.has(r));
            assert!(r.has(r));
        }
        assert!(!CR::WHITE_SHORT.has(CR::WHITE_LONG));
        assert!(!CR::WHITE_SHORT.has(CR::BLACK_SHORT));

        let white = CR::WHITE_SHORT | CR::WHITE_LONG;
        assert!(CR::ALL.has(white));
        // having only one of the two is not enough
        assert!(!CR::WHITE_SHORT.has(white));

        // NONE is a subset of everything
        for rights in every_combination() {
            assert!(rights.has(CR::NONE), "{rights:?}");
        }
    }

    #[test]
    fn union_laws() {
        for a in every_combination() {
            assert_eq!(a | CR::NONE, a);
            assert_eq!(a | a, a);
            for b in every_combination() {
                let union = a | b;
                assert_eq!(union, b | a, "{a:?} | {b:?}");
                assert!(union.has(a) && union.has(b), "{a:?} | {b:?}");
            }
        }
    }

    #[test]
    fn remove_clears_only_given_rights() {
        assert_eq!(
            CR::ALL - CR::WHITE_SHORT,
            CR::WHITE_LONG | CR::BLACK_SHORT | CR::BLACK_LONG
        );
        assert_eq!(
            CR::ALL - (CR::WHITE_SHORT | CR::WHITE_LONG),
            CR::BLACK_SHORT | CR::BLACK_LONG
        );
        assert_eq!(CR::ALL - CR::ALL, CR::NONE);
        // removing something absent changes nothing
        assert_eq!(CR::WHITE_SHORT - CR::BLACK_LONG, CR::WHITE_SHORT);
        assert_eq!(CR::NONE - CR::WHITE_SHORT, CR::NONE);

        for a in every_combination() {
            assert_eq!(a - CR::NONE, a);
            for r in SINGLE {
                let rest = a - r;
                assert!(!rest.has(r), "{a:?} - {r:?}");
                for other in SINGLE.into_iter().filter(|&o| o != r) {
                    assert_eq!(rest.has(other), a.has(other), "{a:?} - {r:?}");
                }
            }
        }
    }

    #[test]
    fn to_fen_uses_canonical_order() {
        assert_eq!(CR::NONE.to_fen(), "-");
        assert_eq!(CR::ALL.to_fen(), "KQkq");
        assert_eq!(CR::WHITE_SHORT.to_fen(), "K");
        assert_eq!(CR::WHITE_LONG.to_fen(), "Q");
        assert_eq!(CR::BLACK_SHORT.to_fen(), "k");
        assert_eq!(CR::BLACK_LONG.to_fen(), "q");
        // the text order does not depend on the order the rights were combined in
        assert_eq!((CR::BLACK_LONG | CR::WHITE_SHORT).to_fen(), "Kq");
        assert_eq!((CR::BLACK_SHORT | CR::WHITE_LONG).to_fen(), "Qk");
        assert_eq!(CR::ALL.to_string(), "KQkq");
    }

    #[test]
    fn update_move_on_king_and_rook_squares() {
        let white = CR::WHITE_SHORT | CR::WHITE_LONG;
        let black = CR::BLACK_SHORT | CR::BLACK_LONG;
        let cases = [
            // (from, to, lost rights)
            (S::E1, S::E2, white),                           // white king moves
            (S::E1, S::G1, white),                           // white castles kingside
            (S::A1, S::A4, CR::WHITE_LONG),                  // a1 rook moves
            (S::H1, S::H4, CR::WHITE_SHORT),                 // h1 rook moves
            (S::E8, S::E7, black),                           // black king moves
            (S::E8, S::C8, black),                           // black castles queenside
            (S::A8, S::A5, CR::BLACK_LONG),                  // a8 rook moves
            (S::H8, S::H5, CR::BLACK_SHORT),                 // h8 rook moves
            (S::D5, S::A8, CR::BLACK_LONG),                  // capture on a8
            (S::E4, S::H1, CR::WHITE_SHORT),                 // capture on h1
            (S::A1, S::A8, CR::WHITE_LONG | CR::BLACK_LONG), // rook takes rook
            (S::H8, S::H1, CR::BLACK_SHORT | CR::WHITE_SHORT), // rook takes rook
        ];
        for (from, to, lost) in cases {
            for rights in every_combination() {
                assert_eq!(
                    rights.update_move(from.index(), to.index()),
                    rights - lost,
                    "{rights:?} on {from}->{to}"
                );
            }
        }
    }

    #[test]
    fn update_move_elsewhere_keeps_rights() {
        let ordinary = || (0..64).map(S::new).filter(|sq| !SPECIAL.contains(sq));
        for from in ordinary() {
            // a piece can't move to its own square
            for to in ordinary().filter(|&to| to != from) {
                for rights in every_combination() {
                    assert_eq!(
                        rights.update_move(from.index(), to.index()),
                        rights,
                        "{rights:?} on {from}->{to}"
                    );
                }
            }
        }
    }
}
