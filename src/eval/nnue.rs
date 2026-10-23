//! The network, `(768 -> 128)x2 -> 1`, from `trainer/` (see `docs/nnue.md`).

use crate::{
    board::Board,
    chess_move::{Move, MoveKind},
    piece::{Color, Piece, PieceKind},
    square::Square,
};

// as in trainer/src/main.rs, checked by the file's size and the trainer test below
const HIDDEN_SIZE: usize = 128;
const QA: i32 = 255;
const QB: i32 = 64;
const SCALE: i32 = 400;

/// The trainer clips weights to ±1.98, 127 in `QB`: the output layer always fits an i32
const MAX_OUTPUT_WEIGHT: i64 = 127;
const _: () =
    assert!(2 * HIDDEN_SIZE as i64 * (QA * QA) as i64 * MAX_OUTPUT_WEIGHT <= i32::MAX as i64);

/// bullet's layout, padded to a multiple of 64 bytes, as `align(64)` pads the struct
#[repr(C)]
struct Network {
    /// One column per input, in `QA`
    feature_weights: [Accumulator; 768],
    feature_bias: Accumulator,
    /// The side to move's half first, in `QB`
    output_weights: [i16; 2 * HIDDEN_SIZE],
    /// In `QA * QB`
    output_bias: i16,
}

static NNUE: Network = unsafe { std::mem::transmute(*include_bytes!("../../nets/net-002.bin")) };

/// One view's hidden layer, before the activation
#[repr(C, align(64))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Accumulator([i16; HIDDEN_SIZE]);

/// Indexed by the color whose view it is
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Accumulators([Accumulator; 2]);

impl Accumulators {
    pub fn new(board: &Board) -> Self {
        let mut accumulators = Self([NNUE.feature_bias; 2]);
        for sq in board.all_pieces_bb() {
            accumulators.add(board.piece_at(sq).expect("an occupied square"), sq);
        }
        accumulators
    }

    /// From `parent`'s accumulators to those after `mv`
    pub fn apply(&mut self, parent: &Board, mv: Move) {
        let us = parent.side_to_move();
        let moving = parent.piece_at(mv.from()).expect("a piece moves");

        match mv.kind() {
            MoveKind::Normal | MoveKind::DoublePush => self.move_piece(moving, mv.from(), mv.to()),
            MoveKind::Capture => {
                self.remove(parent.piece_at(mv.to()).expect("a capture"), mv.to());
                self.move_piece(moving, mv.from(), mv.to());
            }
            MoveKind::EnPassant => {
                let taken = mv.get_ep_square(us);
                self.remove(Piece::new(us.oposite(), PieceKind::Pawn), taken);
                self.move_piece(moving, mv.from(), mv.to());
            }
            MoveKind::Castle => {
                let (rook_from, rook_to) = mv.get_rook_squares();
                self.move_piece(moving, mv.from(), mv.to());
                self.move_piece(Piece::new(us, PieceKind::Rook), rook_from, rook_to);
            }
            MoveKind::Promotion(promoted) | MoveKind::PromotionCapture(promoted) => {
                if mv.is_capture() {
                    self.remove(parent.piece_at(mv.to()).expect("a capture"), mv.to());
                }
                self.remove(moving, mv.from());
                self.add(Piece::new(us, promoted.to_piece_kind()), mv.to());
            }
        }
    }

    /// For the side to move, at the trainer's scale
    pub fn evaluate(&self, side_to_move: Color) -> i32 {
        let us = &self.0[side_to_move];
        let them = &self.0[side_to_move.oposite()];
        let (our_weights, their_weights) = NNUE.output_weights.split_at(HIDDEN_SIZE);

        // in QA * QA * QB, since screlu squares what is in QA
        let output = activated_dot(us, our_weights) + activated_dot(them, their_weights);
        // to QA * QB, the bias's, then to centipawns, in i64 against overflow
        let output = i64::from(output / QA + i32::from(NNUE.output_bias));
        (output * i64::from(SCALE) / i64::from(QA * QB)) as i32
    }

    fn add(&mut self, piece: Piece, sq: Square) {
        for view in Color::ALL {
            let column = &NNUE.feature_weights[input(view, piece, sq)].0;
            for (value, weight) in self.0[view].0.iter_mut().zip(column) {
                *value += weight;
            }
        }
    }

    fn remove(&mut self, piece: Piece, sq: Square) {
        for view in Color::ALL {
            let column = &NNUE.feature_weights[input(view, piece, sq)].0;
            for (value, weight) in self.0[view].0.iter_mut().zip(column) {
                *value -= weight;
            }
        }
    }

    fn move_piece(&mut self, piece: Piece, from: Square, to: Square) {
        self.remove(piece, from);
        self.add(piece, to);
    }
}

/// bullet's numbering: own pieces first, then by kind and by square, flipped for Black
fn input(view: Color, piece: Piece, sq: Square) -> usize {
    let side = if piece.color() == view { 0 } else { 384 };
    let sq = if view == Color::White {
        sq
    } else {
        sq.mirror()
    };
    side + 64 * piece.kind().index() + sq.index()
}

/// One view's half of the output layer
fn activated_dot(accumulator: &Accumulator, weights: &[i16]) -> i32 {
    accumulator
        .0
        .iter()
        .zip(weights)
        .map(|(&x, &w)| screlu(x) * i32::from(w))
        .sum()
}

fn screlu(x: i16) -> i32 {
    let y = i32::from(x).clamp(0, QA);
    y * y
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        mem::{self, Discriminant},
    };

    use super::*;
    use crate::{
        bitboard::BitBoard,
        test_utils::{PERFT_POSITIONS, board, mirror_fen},
    };

    /// The 768 inputs
    fn every_input() -> impl Iterator<Item = (Piece, Square)> {
        Piece::ALL
            .into_iter()
            .flat_map(|piece| BitBoard::FULL.into_iter().map(move |sq| (piece, sq)))
    }

    #[test]
    fn each_view_numbers_every_input_once() {
        for view in Color::ALL {
            let mut seen = [false; 768];
            for (piece, sq) in every_input() {
                let input = input(view, piece, sq);
                assert!(!seen[input], "{view:?}: {piece:?} on {sq} repeats {input}");
                seen[input] = true;
            }
        }
    }

    #[test]
    fn the_inputs_are_the_ones_of_bullet() {
        let knight = Piece::new(Color::White, PieceKind::Knight);
        assert_eq!(input(Color::White, knight, Square::G1), 64 + 6);
        assert_eq!(input(Color::Black, knight, Square::G1), 384 + 64 + 62);

        let king = Piece::new(Color::Black, PieceKind::King);
        assert_eq!(input(Color::Black, king, Square::E8), 5 * 64 + 4);
        assert_eq!(input(Color::White, king, Square::E8), 384 + 5 * 64 + 60);
    }

    #[test]
    fn each_view_sees_the_other_mirrored() {
        // for Black, the piece of the other color on the mirrored square
        for (piece, sq) in every_input() {
            let swapped = Piece::new(piece.color().oposite(), piece.kind());
            assert_eq!(
                input(Color::White, piece, sq),
                input(Color::Black, swapped, sq.mirror()),
                "{piece:?} on {sq}"
            );
        }
    }

    #[test]
    fn both_sides_see_the_starting_position_the_same() {
        let [white, black] = Accumulators::new(&board(Board::STARTPOS)).0;
        assert_eq!(white, black);
    }

    #[test]
    fn a_mirrored_position_swaps_the_views() {
        for fen in PERFT_POSITIONS {
            let [white, black] = Accumulators::new(&board(fen)).0;
            let [mirrored_white, mirrored_black] = Accumulators::new(&board(&mirror_fen(fen))).0;
            assert_eq!(white, mirrored_black, "{fen}");
            assert_eq!(black, mirrored_white, "{fen}");
        }
    }

    /// Along every line `depth` plies deep, the updated accumulators equal a refresh
    fn assert_updates_match(
        board: &Board,
        accumulators: Accumulators,
        depth: u32,
        kinds: &mut HashSet<Discriminant<MoveKind>>,
    ) {
        if depth == 0 {
            return;
        }
        for (mv, next) in board.legal_successors() {
            kinds.insert(mem::discriminant(&mv.kind()));
            let mut updated = accumulators;
            updated.apply(board, mv);
            assert_eq!(
                updated,
                Accumulators::new(&next),
                "{mv} in {}",
                board.to_fen()
            );
            assert_updates_match(&next, updated, depth - 1, kinds);
        }
    }

    #[test]
    fn updates_match_a_refresh_along_perft_trees() {
        let mut kinds = HashSet::new();
        // two plies already play every kind of move, and the mirrors with the other color
        for fen in PERFT_POSITIONS
            .into_iter()
            .flat_map(|fen| [fen.to_string(), mirror_fen(fen)])
        {
            let board = board(&fen);
            assert_updates_match(&board, Accumulators::new(&board), 2, &mut kinds);
        }
        assert_eq!(kinds.len(), 7, "not every kind of move got played");
    }

    /// `trainer/checkpoints/net-002-evals.txt`, before quantising
    const TRAINER_EVALS: [(&str, f32); 7] = [
        (Board::STARTPOS, 29.2),
        (
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
            -16.3,
        ),
        (
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            -142.4,
        ),
        (
            "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
            17.3,
        ),
        (
            "rnbqkb1r/pppp1ppp/5n2/4p3/4P3/2N5/PPPP1PPP/R1BQKBNR b KQkq - 2 3",
            17.3,
        ),
        ("4k3/8/8/8/8/8/PPP5/4K2R w K - 0 1", 2372.3),
        ("4k2r/ppp5/8/8/8/8/8/4K3 b k - 0 1", 2372.3),
    ];

    /// Rounding net-002's weights moves these positions by up to 7.3
    const QUANTISATION_ERROR: f32 = 8.0;

    #[test]
    fn the_output_matches_the_trainer() {
        for (fen, expected) in TRAINER_EVALS {
            let board = board(fen);
            let output = Accumulators::new(&board).evaluate(board.side_to_move());
            assert!(
                (output as f32 - expected).abs() <= QUANTISATION_ERROR,
                "{fen}: {output} against {expected}"
            );
        }
    }
}
