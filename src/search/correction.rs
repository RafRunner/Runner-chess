use crate::{
    board::Board,
    piece::{Color, Piece},
    score::Score,
};

const ENTRIES: usize = 16_384;
/// A centipawn of correction is GRAIN in the table
const GRAIN: i32 = 256;
const MAX_CORRECTION: i32 = 64 * GRAIN;
const MAX_WEIGHT: i32 = 16;

/// Correction history: how far the searches of the positions with each pawn structure
/// ended from their static eval, by side to move. 64 KB
pub struct Corrections {
    pawns: Box<[[i16; ENTRIES]; 2]>,
}

impl Corrections {
    pub fn new() -> Self {
        Self {
            pawns: Box::new([[0; ENTRIES]; 2]),
        }
    }

    pub fn clear(&mut self) {
        *self.pawns = [[0; ENTRIES]; 2];
    }

    /// Nothing without pawns: there is no structure to tell those positions apart, and
    /// the dead ones among them have to stay an exact draw
    fn index(board: &Board) -> Option<(Color, usize)> {
        let white = board.pieces(Piece::WhitePawn).raw();
        let black = board.pieces(Piece::BlackPawn).raw();
        if white | black == 0 {
            return None;
        }
        let key = white.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ black.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        Some((board.side_to_move(), (key >> 50) as usize))
    }

    pub fn correct(&self, board: &Board, eval: Score) -> Score {
        let Some((color, idx)) = Self::index(board) else {
            return eval;
        };
        let correction = i32::from(self.pawns[color][idx]) / GRAIN;
        Score::new(i32::from(eval.centipawns().unwrap_or(0)) + correction)
    }

    /// A search of `depth` plies of `board` ended `error` centipawns above its eval
    pub fn update(&mut self, board: &Board, depth: u32, error: i32) {
        let Some((color, idx)) = Self::index(board) else {
            return;
        };
        let entry = &mut self.pawns[color][idx];
        let weight = (depth as i32 + 1).min(MAX_WEIGHT);
        let new = (i32::from(*entry) * (GRAIN - weight) + error * GRAIN * weight) / GRAIN;
        *entry = new.clamp(-MAX_CORRECTION, MAX_CORRECTION) as i16;
    }
}
