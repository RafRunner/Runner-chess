use std::fmt::Display;

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

    pub fn to_uci(&self) -> String {
        let mut buffer = String::new();
        buffer.push_str(&self.from.to_algebraic());
        buffer.push_str(&self.to.to_algebraic());

        if let MoveKind::Promotion(kind) = self.kind {
            buffer.push(kind.to_char());
        }

        buffer
    }
}

impl Display for Move {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_uci())
    }
}
