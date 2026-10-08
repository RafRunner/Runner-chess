use std::fmt::{Debug, Display};

use crate::{piece::PieceKind, square::Square};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PromotionPiece {
    Knight = 0,
    Bishop,
    Rook,
    Queen,
}

impl PromotionPiece {
    pub const ALL: [PromotionPiece; 4] = [Self::Knight, Self::Bishop, Self::Rook, Self::Queen];

    pub fn to_piece_kind(self) -> PieceKind {
        match self {
            PromotionPiece::Knight => PieceKind::Knight,
            PromotionPiece::Bishop => PieceKind::Bishop,
            PromotionPiece::Rook => PieceKind::Rook,
            PromotionPiece::Queen => PieceKind::Queen,
        }
    }

    pub fn to_char(self) -> char {
        self.to_piece_kind().to_char()
    }

    pub fn from_char(ch: char) -> Option<Self> {
        match ch {
            'n' => Some(Self::Knight),
            'b' => Some(Self::Bishop),
            'r' => Some(Self::Rook),
            'q' => Some(Self::Queen),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MoveKind {
    Normal,
    Capture,
    DoublePush,
    EnPassant,
    Castle,
    Promotion(PromotionPiece),
    PromotionCapture(PromotionPiece),
}

#[derive(Debug)]
pub struct MoveParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbstractMove {
    pub from: Square,
    pub to: Square,
    pub promotion_piece: Option<PromotionPiece>,
}

impl AbstractMove {
    pub fn new(from: Square, to: Square, promotion_piece: Option<PromotionPiece>) -> Self {
        Self {
            from,
            to,
            promotion_piece,
        }
    }

    pub fn from_uci(uci: &str) -> Result<Self, MoveParseError> {
        if !(4..=5).contains(&uci.len()) {
            Err(MoveParseError)
        } else {
            let mut chars = uci.chars();
            let chars = chars.by_ref();
            let from: String = chars.take(2).collect();
            let to: String = chars.take(2).collect();

            let promotion_piece = match chars.take(1).next() {
                Some(kind) => Some(PromotionPiece::from_char(kind).ok_or(MoveParseError)?),
                None => None,
            };

            Ok(Self::new(
                Square::from_algebraic(&from).map_err(|_| MoveParseError)?,
                Square::from_algebraic(&to).map_err(|_| MoveParseError)?,
                promotion_piece,
            ))
        }
    }

    pub fn is_move(self, mv: Move) -> bool {
        if !(self.from == mv.from() && self.to == mv.to()) {
            return false;
        }

        if let Some(self_pp) = self.promotion_piece {
            if let MoveKind::Promotion(mv_pp) | MoveKind::PromotionCapture(mv_pp) = mv.kind() {
                return self_pp == mv_pp;
            } else {
                return false;
            }
        } else if mv.is_promotion() {
            return false;
        }

        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Move(u16);

impl Move {
    const MOVE_MASK: u16 = 0b11_1111;
    const TO_SHIFT: u16 = 6;
    const FLAG_SHIFT: u16 = 12;

    const NORMAL: u16 = 0b0000;
    const DOUBLE_PUSH: u16 = 0b0001;
    const CASTLE: u16 = 0b0010;
    const CAPTURE: u16 = 0b0100;
    const EN_PASSANT: u16 = Self::DOUBLE_PUSH | Self::CAPTURE;
    const PROMOTION: u16 = 0b1000;
    const PROMOTION_CAPTURE: u16 = Self::PROMOTION | Self::CAPTURE;

    pub const fn new(from: Square, to: Square, kind: MoveKind) -> Self {
        let mut raw = from.index() as u16;
        raw |= (to.index() as u16) << Self::TO_SHIFT;

        let flags = match kind {
            MoveKind::Normal => Self::NORMAL,
            MoveKind::Capture => Self::CAPTURE,
            MoveKind::DoublePush => Self::DOUBLE_PUSH,
            MoveKind::EnPassant => Self::EN_PASSANT,
            MoveKind::Castle => Self::CASTLE,
            MoveKind::Promotion(piece) => Self::PROMOTION | piece as u16,
            MoveKind::PromotionCapture(piece) => Self::PROMOTION_CAPTURE | piece as u16,
        };

        raw |= flags << Self::FLAG_SHIFT;

        Self(raw)
    }

    pub const fn from(self) -> Square {
        Square::new((self.0 & Self::MOVE_MASK) as u8)
    }

    pub const fn to(self) -> Square {
        let to = self.0 & (Self::MOVE_MASK << Self::TO_SHIFT);
        Square::new((to >> Self::TO_SHIFT) as u8)
    }

    pub const fn kind(self) -> MoveKind {
        let flags = self.flags();

        if flags & Self::PROMOTION == Self::PROMOTION {
            let piece = PromotionPiece::ALL[(flags & 0b11) as usize];

            return if flags & Self::PROMOTION_CAPTURE == Self::PROMOTION_CAPTURE {
                MoveKind::PromotionCapture(piece)
            } else {
                MoveKind::Promotion(piece)
            };
        }

        match flags {
            Self::NORMAL => MoveKind::Normal,
            Self::DOUBLE_PUSH => MoveKind::DoublePush,
            Self::CASTLE => MoveKind::Castle,
            Self::CAPTURE => MoveKind::Capture,
            Self::EN_PASSANT => MoveKind::EnPassant,
            _ => panic!("invalid move flag"),
        }
    }

    pub const fn is_capture(self) -> bool {
        self.flags() & Self::CAPTURE == Self::CAPTURE
    }

    pub const fn is_promotion(self) -> bool {
        self.flags() & Self::PROMOTION == Self::PROMOTION
    }

    pub fn to_uci(&self) -> String {
        let piece =
            if let MoveKind::Promotion(piece) | MoveKind::PromotionCapture(piece) = self.kind() {
                &piece.to_char().to_string()
            } else {
                ""
            };
        format!(
            "{}{}{}",
            self.from().to_algebraic(),
            self.to().to_algebraic(),
            piece
        )
    }

    const fn flags(self) -> u16 {
        self.0 >> Self::FLAG_SHIFT
    }
}

impl Display for Move {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_uci())
    }
}

impl Debug for Move {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}", self.to_uci(), self.kind())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Square as S;

    /// every kind a move can have, each promotion piece included
    fn all_kinds() -> Vec<MoveKind> {
        let mut kinds = vec![
            MoveKind::Normal,
            MoveKind::Capture,
            MoveKind::DoublePush,
            MoveKind::EnPassant,
            MoveKind::Castle,
        ];
        for piece in PromotionPiece::ALL {
            kinds.push(MoveKind::Promotion(piece));
            kinds.push(MoveKind::PromotionCapture(piece));
        }
        kinds
    }

    #[test]
    fn move_is_two_bytes() {
        assert_eq!(std::mem::size_of::<Move>(), 2);
    }

    /// decoding gives back exactly what was encoded for every combination,
    /// so no field leaks into another and no two moves share an encoding
    #[test]
    fn fields_roundtrip() {
        let kinds = all_kinds();
        assert_eq!(kinds.len(), 13);
        for from in (0..64).map(Square::new) {
            for to in (0..64).map(Square::new) {
                for &kind in &kinds {
                    let mv = Move::new(from, to, kind);
                    assert_eq!(
                        (mv.from(), mv.to(), mv.kind()),
                        (from, to, kind),
                        "{from}{to} {kind:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn is_capture_matches_kind() {
        for kind in all_kinds() {
            let expected = matches!(
                kind,
                MoveKind::Capture | MoveKind::EnPassant | MoveKind::PromotionCapture(_)
            );
            assert_eq!(
                Move::new(S::E2, S::E4, kind).is_capture(),
                expected,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn promotion_pieces() {
        let kinds = PromotionPiece::ALL.map(PromotionPiece::to_piece_kind);
        assert_eq!(
            kinds,
            [
                PieceKind::Knight,
                PieceKind::Bishop,
                PieceKind::Rook,
                PieceKind::Queen
            ]
        );
        // the discriminant is what goes into the flags, and decoding indexes ALL with it
        for (i, piece) in PromotionPiece::ALL.into_iter().enumerate() {
            assert_eq!(piece as usize, i, "{piece:?}");
        }
    }

    #[test]
    fn uci_notation() {
        use PromotionPiece as P;
        #[rustfmt::skip]
        let cases = [
            (Move::new(S::G1, S::F3, MoveKind::Normal),                  "g1f3"),
            (Move::new(S::E4, S::D5, MoveKind::Capture),                 "e4d5"),
            (Move::new(S::E2, S::E4, MoveKind::DoublePush),              "e2e4"),
            (Move::new(S::E5, S::D6, MoveKind::EnPassant),               "e5d6"),
            (Move::new(S::E1, S::G1, MoveKind::Castle),                  "e1g1"),
            (Move::new(S::A7, S::A8, MoveKind::Promotion(P::Queen)),     "a7a8q"),
            (Move::new(S::A7, S::B8, MoveKind::PromotionCapture(P::Knight)), "a7b8n"),
            // black promotions are lowercase too
            (Move::new(S::H2, S::H1, MoveKind::Promotion(P::Rook)),      "h2h1r"),
            (Move::new(S::B2, S::A1, MoveKind::PromotionCapture(P::Bishop)), "b2a1b"),
        ];
        for (mv, uci) in cases {
            assert_eq!(mv.to_uci(), uci);
            assert_eq!(mv.to_string(), uci);
        }
    }

    #[test]
    fn uci_notation_abstract() {
        assert_eq!(
            AbstractMove::from_uci("e2e4").unwrap(),
            AbstractMove::new(S::E2, S::E4, None)
        );

        assert_eq!(
            AbstractMove::from_uci("a7a8q").unwrap(),
            AbstractMove::new(S::A7, S::A8, Some(PromotionPiece::Queen))
        );

        assert!(AbstractMove::from_uci("g1m4").is_err());
        assert!(AbstractMove::from_uci("h1h").is_err());
        assert!(AbstractMove::from_uci("g4g1h7").is_err());
    }

    #[test]
    fn abstract_board_corners() {
        assert_eq!(
            AbstractMove::from_uci("a1h8").unwrap(),
            AbstractMove::new(S::A1, S::H8, None)
        );
        assert_eq!(
            AbstractMove::from_uci("h8a1").unwrap(),
            AbstractMove::new(S::H8, S::A1, None)
        );
    }

    #[test]
    fn abstract_every_promotion_piece() {
        for piece in PromotionPiece::ALL {
            let uci = format!("b7b8{}", piece.to_char());
            assert_eq!(
                AbstractMove::from_uci(&uci).unwrap(),
                AbstractMove::new(S::B7, S::B8, Some(piece)),
                "{uci}"
            );
        }
    }

    #[test]
    fn abstract_rejects_malformed() {
        #[rustfmt::skip]
        let cases = [
            "",
            "e2",
            "e2e",
            "e2e4qq",
            // off the board
            "i1a1",
            "a1i1",
            "a0a1",
            "a1a9",
            // not something a pawn can promote to
            "e7e8k",
            "e7e8p",
            "e7e8x",
            // UCI promotions are always lowercase
            "e7e8Q",
            // trailing garbage takes the promotion slot
            "e2e4 ",
            // 5 bytes but only 4 chars, the last one multibyte
            "e2eé",
        ];
        for uci in cases {
            assert!(AbstractMove::from_uci(uci).is_err(), "{uci:?}");
        }
    }

    /// whatever a Move prints as UCI parses back into an AbstractMove that matches it
    #[test]
    fn abstract_roundtrips_every_move() {
        for from in (0..64).map(Square::new) {
            for to in (0..64).map(Square::new) {
                for kind in all_kinds() {
                    let mv = Move::new(from, to, kind);
                    let parsed = AbstractMove::from_uci(&mv.to_uci())
                        .unwrap_or_else(|_| panic!("{mv} {kind:?}"));
                    assert!(parsed.is_move(mv), "{mv} {kind:?}");
                }
            }
        }
    }

    #[test]
    fn is_move_compares_squares() {
        let mv = Move::new(S::E2, S::E4, MoveKind::DoublePush);
        assert!(AbstractMove::new(S::E2, S::E4, None).is_move(mv));
        assert!(!AbstractMove::new(S::E2, S::E3, None).is_move(mv));
        assert!(!AbstractMove::new(S::D2, S::E4, None).is_move(mv));
        // direction matters
        assert!(!AbstractMove::new(S::E4, S::E2, None).is_move(mv));
    }

    #[test]
    fn is_move_ignores_kind_for_non_promotions() {
        for kind in [
            MoveKind::Normal,
            MoveKind::Capture,
            MoveKind::DoublePush,
            MoveKind::EnPassant,
            MoveKind::Castle,
        ] {
            let mv = Move::new(S::E1, S::G1, kind);
            assert!(
                AbstractMove::new(S::E1, S::G1, None).is_move(mv),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn is_move_requires_matching_promotion() {
        use PromotionPiece as P;
        for promoted in P::ALL {
            for kind in [
                MoveKind::Promotion(promoted),
                MoveKind::PromotionCapture(promoted),
            ] {
                let mv = Move::new(S::A7, S::A8, kind);

                // no piece given never matches a promotion, otherwise the first
                // promotion found would be picked
                assert!(
                    !AbstractMove::new(S::A7, S::A8, None).is_move(mv),
                    "{kind:?}"
                );

                for asked in P::ALL {
                    assert_eq!(
                        AbstractMove::new(S::A7, S::A8, Some(asked)).is_move(mv),
                        asked == promoted,
                        "asked {asked:?} for {kind:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn is_move_rejects_promotion_piece_on_plain_move() {
        let mv = Move::new(S::A7, S::A8, MoveKind::Normal);
        assert!(!AbstractMove::new(S::A7, S::A8, Some(PromotionPiece::Queen)).is_move(mv));
    }
}
