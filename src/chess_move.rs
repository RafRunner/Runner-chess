use std::fmt::Display;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Move(u16);

impl Move {
    const MOVE_MASK: u16 = 0b11_1111;
    const TO_SHIFT: u16 = 6;
    const FLAG_SHIFT: u16 = 12;

    const NORMAL: u16 = 0b0000;
    const DOUBLE_PUSH: u16 = 0b0001;
    const CASTLE: u16 = 0b0010;
    const CAPTURE: u16 = 0b0100;
    const EN_PASSANT: u16 = 0b0101;
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
