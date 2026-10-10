use std::ops::Div;

use crate::{
    bitboard::BitBoard,
    board::Board,
    piece::{Color, Piece},
};

pub const MATE: i16 = 32_000;
pub const MAX_PLY: u8 = 248;
pub const INF_SCORE: i16 = i16::MAX - 1;
pub const MAX_EVAL: i16 = MATE - MAX_PLY as i16 - 1;

const MAX_PHASE: i32 = 24;
const PIECE_VALUES: [i32; 6] = [100, 320, 330, 500, 900, 0];
const PIECE_PHASE: [i32; 6] = [0, 1, 1, 2, 4, 0];

#[rustfmt::skip]
pub static  PST_PAWN: [i32; 64] = [
      0,   0,   0,   0,   0,   0,   0,   0, // 1
      5,  10,  10, -20, -20,  10,  10,   5, // 2
      5,  -5, -10,   0,   0, -10,  -5,   5, // 3
      0,   0,   0,  20,  20,   0,   0,   0, // 4
      5,   5,  10,  25,  25,  10,   5,   5, // 5
     10,  10,  20,  30,  30,  20,  10,  10, // 6
     50,  50,  50,  50,  50,  50,  50,  50, // 7
      0,   0,   0,   0,   0,   0,   0,   0, // 8
];

#[rustfmt::skip]
pub static  PST_KNIGHT: [i32; 64] = [
    -50, -40, -30, -30, -30, -30, -40, -50, // 1
    -40, -20,   0,   5,   5,   0, -20, -40, // 2
    -30,   5,  10,  15,  15,  10,   5, -30, // 3
    -30,   0,  15,  20,  20,  15,   0, -30, // 4
    -30,   5,  15,  20,  20,  15,   5, -30, // 5
    -30,   0,  10,  15,  15,  10,   0, -30, // 6
    -40, -20,   0,   0,   0,   0, -20, -40, // 7
    -50, -40, -30, -30, -30, -30, -40, -50, // 8
];

#[rustfmt::skip]
pub static  PST_BISHOP: [i32; 64] = [
    -20, -10, -10, -10, -10, -10, -10, -20, // 1
    -10,   5,   0,   0,   0,   0,   5, -10, // 2
    -10,  10,  10,  10,  10,  10,  10, -10, // 3
    -10,   0,  10,  10,  10,  10,   0, -10, // 4
    -10,   5,   5,  10,  10,   5,   5, -10, // 5
    -10,   0,   5,  10,  10,   5,   0, -10, // 6
    -10,   0,   0,   0,   0,   0,   0, -10, // 7
    -20, -10, -10, -10, -10, -10, -10, -20, // 8
];

#[rustfmt::skip]
pub static  PST_ROOK: [i32; 64] = [
      0,   0,   0,   5,   5,   0,   0,   0, // 1
     -5,   0,   0,   0,   0,   0,   0,  -5, // 2
     -5,   0,   0,   0,   0,   0,   0,  -5, // 3
     -5,   0,   0,   0,   0,   0,   0,  -5, // 4
     -5,   0,   0,   0,   0,   0,   0,  -5, // 5
     -5,   0,   0,   0,   0,   0,   0,  -5, // 6
      5,  10,  10,  10,  10,  10,  10,   5, // 7
      0,   0,   0,   0,   0,   0,   0,   0, // 8
];

#[rustfmt::skip]
pub static  PST_QUEEN: [i32; 64] = [
    -20, -10, -10,  -5,  -5, -10, -10, -20, // 1
    -10,   0,   5,   0,   0,   0,   0, -10, // 2
    -10,   5,   5,   5,   5,   5,   0, -10, // 3
      0,   0,   5,   5,   5,   5,   0,  -5, // 4
     -5,   0,   5,   5,   5,   5,   0,  -5, // 5
    -10,   0,   5,   5,   5,   5,   0, -10, // 6
    -10,   0,   0,   0,   0,   0,   0, -10, // 7
    -20, -10, -10,  -5,  -5, -10, -10, -20, // 8
];

#[rustfmt::skip]
pub static  PST_KING_MG: [i32; 64] = [
     20,  30,  10,   0,   0,  10,  30,  20, // 1
     20,  20,   0,   0,   0,   0,  20,  20, // 2
    -10, -20, -20, -20, -20, -20, -20, -10, // 3
    -20, -30, -30, -40, -40, -30, -30, -20, // 4
    -30, -40, -40, -50, -50, -40, -40, -30, // 5
    -30, -40, -40, -50, -50, -40, -40, -30, // 6
    -30, -40, -40, -50, -50, -40, -40, -30, // 7
    -30, -40, -40, -50, -50, -40, -40, -30, // 8
];

#[rustfmt::skip]
pub static  PST_KING_EG: [i32; 64] = [
    -50, -30, -30, -30, -30, -30, -30, -50, // 1
    -30, -30,   0,   0,   0,   0, -30, -30, // 2
    -30, -10,  20,  30,  30,  20, -10, -30, // 3
    -30, -10,  30,  40,  40,  30, -10, -30, // 4
    -30, -10,  30,  40,  40,  30, -10, -30, // 5
    -30, -10,  20,  30,  30,  20, -10, -30, // 6
    -30, -20, -10,   0,   0, -10, -20, -30, // 7
    -50, -40, -30, -20, -20, -30, -40, -50, // 8
];

pub static MG_TABLES: [[i32; 64]; 6] = [
    PST_PAWN,
    PST_KNIGHT,
    PST_BISHOP,
    PST_ROOK,
    PST_QUEEN,
    PST_KING_MG,
];
pub static EG_TABLES: [[i32; 64]; 6] = [
    PST_PAWN,
    PST_KNIGHT,
    PST_BISHOP,
    PST_ROOK,
    PST_QUEEN,
    PST_KING_EG,
];

pub fn eval(board: &Board) -> i16 {
    let us = board.side_to_move();

    let mut our_score: i32 = 0;
    let mut their_score: i32 = 0;

    let mut phase: i32 = 0;

    for piece in Piece::ALL {
        phase += PIECE_PHASE[piece.kind()] * board.pieces(piece).count_ones() as i32;
    }

    phase = phase.min(MAX_PHASE);

    for sq in BitBoard::FULL {
        if let Some(piece) = board.piece_at(sq) {
            let piece_score = PIECE_VALUES[piece.kind()];

            let pst_index = if piece.color() == Color::White {
                sq
            } else {
                sq.mirror()
            };

            let mg_score = MG_TABLES[piece.kind()][pst_index] + piece_score;
            let lg_score = EG_TABLES[piece.kind()][pst_index] + piece_score;

            let score = mg_score * phase + lg_score * (MAX_PHASE - phase);

            if piece.color() == us {
                our_score += score;
            } else {
                their_score += score;
            }
        }
    }

    (our_score - their_score)
        .div(MAX_PHASE)
        .min(MAX_EVAL as i32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{board, mirror_fen, PERFT_POSITIONS};

    /// The same position with the other side to move.
    fn flip_side(fen: &str) -> String {
        let mut fields: Vec<&str> = fen.split_whitespace().collect();
        fields[1] = if fields[1] == "w" { "b" } else { "w" };
        fields[3] = "-";
        fields.join(" ")
    }

    /// `better` must evaluate higher than `worse` for the side to move, and so
    /// must their mirrors, so every rule is checked for both colors.
    fn assert_better(better: &str, worse: &str) {
        let (b, w) = (eval(&board(better)), eval(&board(worse)));
        assert!(b > w, "expected {better} ({b}) > {worse} ({w})");

        let (better, worse) = (mirror_fen(better), mirror_fen(worse));
        let (b, w) = (eval(&board(&better)), eval(&board(&worse)));
        assert!(b > w, "expected {better} ({b}) > {worse} ({w})");
    }

    // properties

    #[test]
    fn startpos_is_balanced() {
        assert_eq!(eval(&board(Board::STARTPOS)), 0);
    }

    #[test]
    fn mirrored_positions_have_the_same_eval() {
        for fen in PERFT_POSITIONS {
            let mirrored = mirror_fen(fen);
            assert_eq!(
                eval(&board(fen)),
                eval(&board(&mirrored)),
                "{fen} vs {mirrored}"
            );
        }
    }

    #[test]
    fn eval_is_relative_to_side_to_move() {
        for fen in PERFT_POSITIONS {
            // passing the move to a side that is giving check is not a legal position
            let Ok(flipped) = Board::from_fen(&flip_side(fen)) else {
                continue;
            };
            assert_eq!(eval(&board(fen)), -eval(&flipped), "{fen}");
        }
    }

    // material

    #[test]
    fn any_extra_piece_is_an_advantage() {
        for piece in ["P", "N", "B", "R", "Q"] {
            assert_better(
                &format!("4k3/8/8/8/8/3{piece}4/8/4K3 w - - 0 1"),
                "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
            );
        }
    }

    // piece placement

    #[test]
    fn knight_prefers_the_center_to_the_corner() {
        assert_better(
            "4k3/8/8/8/3N4/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/8/8/8/N3K3 w - - 0 1",
        );
    }

    #[test]
    fn advanced_pawn_is_better() {
        assert_better(
            "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/8/8/P7/4K3 w - - 0 1",
        );
    }

    #[test]
    fn rook_prefers_the_seventh_rank() {
        assert_better(
            "4k3/R7/8/8/8/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/8/8/8/R3K3 w - - 0 1",
        );
    }

    // king safety and activity

    #[test]
    fn castled_king_is_safer_in_the_middlegame() {
        assert_better(
            "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPBPPP/RNBQ1RK1 w kq - 0 1",
            "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPBPPP/RNBQK2R w KQkq - 0 1",
        );
    }

    #[test]
    fn king_in_the_corner_is_bad_in_the_endgame() {
        assert_better(
            "4k3/8/8/8/3K4/8/8/8 w - - 0 1",
            "4k3/8/8/8/8/8/8/K7 w - - 0 1",
        );
    }

    #[test]
    fn king_preference_flips_as_material_comes_off() {
        // with all the material a central king is exposed...
        assert_better(
            "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPBPPP/RNBQ1RK1 w kq - 0 1",
            "rnbqkbnr/pppppppp/8/8/3K4/5N2/PPPPBPPP/RNBQ1R2 w kq - 0 1",
        );
        // ...but once it is gone, the same king belongs in the center
        assert_better(
            "4k3/pppppppp/8/8/3K4/8/PPPPPPPP/8 w - - 0 1",
            "4k3/pppppppp/8/8/8/8/PPPPPPPP/6K1 w - - 0 1",
        );
    }
}
