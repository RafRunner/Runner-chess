use crate::{piece::PieceKind, square::Square};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MoveKind {
    Normal,
    DoublePush,
    EnPassant,
    Castle,
    Promotion(PieceKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Move {
    from: Square,
    to: Square,
    kind: MoveKind,
}

impl Move {
    pub fn new(from: Square, to: Square, kind: MoveKind) -> Self {
        Self { from, to, kind }
    }

    pub fn from(&self) -> Square {
        self.from
    }

    pub fn to(&self) -> Square {
        self.to
    }

    pub fn kind(&self) -> MoveKind {
        self.kind
    }
}
