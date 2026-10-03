use std::{fmt::Display, ops};

use crate::square::Square;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CastlingRights(u8);

#[derive(Debug)]
pub struct CastlingParseError;

impl CastlingRights {
    pub const ALL: Self = Self(0b1111);
    pub const NONE: Self = Self(0b0000);
    pub const WK: Self = Self(0b0001);
    pub const WQ: Self = Self(0b0010);
    pub const BK: Self = Self(0b0100);
    pub const BQ: Self = Self(0b1000);

    const CASTLE_MASK: [u8; 64] = {
        let mut m = [0b1111u8; 64];
        m[Square::A1.index()] = !Self::WQ.0 & 0b1111; // a1
        m[Square::H1.index()] = !Self::WK.0 & 0b1111; // h1
        m[Square::E1.index()] = !(Self::WK.0 | Self::WQ.0) & 0b1111; // e1
        m[Square::A8.index()] = !Self::BQ.0 & 0b1111; // a8
        m[Square::H8.index()] = !Self::BK.0 & 0b1111; // h8
        m[Square::E8.index()] = !(Self::BK.0 | Self::BQ.0) & 0b1111; // e8
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
        if self.has(Self::WK) {
            fen.push('K');
        }
        if self.has(Self::WQ) {
            fen.push('Q');
        }
        if self.has(Self::BK) {
            fen.push('k');
        }
        if self.has(Self::BQ) {
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
                    'K' => Self::WK,
                    'Q' => Self::WQ,
                    'k' => Self::BK,
                    'q' => Self::BQ,
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

    const SINGLE: [CR; 4] = [CR::WK, CR::WQ, CR::BK, CR::BQ];

    const A1: usize = 0;
    const E1: usize = 4;
    const H1: usize = 7;
    const A8: usize = 56;
    const E8: usize = 60;
    const H8: usize = 63;
    const SPECIAL: [usize; 6] = [A1, E1, H1, A8, E8, H8];

    /// as 16 combinações possíveis de direitos, montadas só com `|`
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
                assert!(!a.has(b), "{a:?} não deveria conter {b:?}");
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
        assert!(!CR::WK.has(CR::WQ));
        assert!(!CR::WK.has(CR::BK));

        let white = CR::WK | CR::WQ;
        assert!(CR::ALL.has(white));
        // ter só um dos dois não basta
        assert!(!CR::WK.has(white));

        // NONE é subconjunto de qualquer coisa
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
        assert_eq!(CR::ALL - CR::WK, CR::WQ | CR::BK | CR::BQ);
        assert_eq!(CR::ALL - (CR::WK | CR::WQ), CR::BK | CR::BQ);
        assert_eq!(CR::ALL - CR::ALL, CR::NONE);
        // remover algo ausente não muda nada
        assert_eq!(CR::WK - CR::BQ, CR::WK);
        assert_eq!(CR::NONE - CR::WK, CR::NONE);

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
        assert_eq!(CR::WK.to_fen(), "K");
        assert_eq!(CR::WQ.to_fen(), "Q");
        assert_eq!(CR::BK.to_fen(), "k");
        assert_eq!(CR::BQ.to_fen(), "q");
        // a ordem do texto não depende da ordem em que os direitos foram somados
        assert_eq!((CR::BQ | CR::WK).to_fen(), "Kq");
        assert_eq!((CR::BK | CR::WQ).to_fen(), "Qk");
        assert_eq!(CR::ALL.to_string(), "KQkq");
    }

    #[test]
    fn update_move_on_king_and_rook_squares() {
        let white = CR::WK | CR::WQ;
        let black = CR::BK | CR::BQ;
        let cases = [
            // (de, para, direitos perdidos)
            (E1, 12, white),           // rei branco anda
            (E1, 6, white),            // roque curto branco
            (A1, 24, CR::WQ),          // torre a1 sai
            (H1, 31, CR::WK),          // torre h1 sai
            (E8, 52, black),           // rei preto anda
            (E8, 58, black),           // roque longo preto
            (A8, 32, CR::BQ),          // torre a8 sai
            (H8, 39, CR::BK),          // torre h8 sai
            (27, A8, CR::BQ),          // captura em a8
            (36, H1, CR::WK),          // captura em h1
            (A1, A8, CR::WQ | CR::BQ), // torre captura torre
            (H8, H1, CR::BK | CR::WK), // torre captura torre
        ];
        for (from, to, lost) in cases {
            for rights in every_combination() {
                assert_eq!(
                    rights.update_move(from, to),
                    rights - lost,
                    "{rights:?} em {from}->{to}"
                );
            }
        }
    }

    #[test]
    fn update_move_elsewhere_keeps_rights() {
        for from in (0..64).filter(|sq| !SPECIAL.contains(sq)) {
            for to in (0..64).filter(|sq| !SPECIAL.contains(sq)) {
                for rights in every_combination() {
                    assert_eq!(
                        rights.update_move(from, to),
                        rights,
                        "{rights:?} em {from}->{to}"
                    );
                }
            }
        }
    }
}
