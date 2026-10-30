//! The network, `(768 -> 256)x2 -> 1`, from `trainer/` (see `docs/nnue.md`).

use crate::{
    board::Board,
    chess_move::{Move, MoveKind},
    piece::{Color, Piece, PieceKind},
    square::Square,
};

// as in trainer/src/main.rs, checked by the file's size and the trainer test below
const HIDDEN_SIZE: usize = 256;
const QA: i32 = 255;
const QB: i32 = 64;
const SCALE: i32 = 400;

/// The trainer clips weights to ±1.98, 127 in `QB`: each view's half of the output layer
/// always fits an i32, so its sum stays in the lanes that SIMD handles best. Both halves
/// together may not
const MAX_OUTPUT_WEIGHT: i64 = 127;
const _: () = assert!(HIDDEN_SIZE as i64 * (QA * QA) as i64 * MAX_OUTPUT_WEIGHT <= i32::MAX as i64);

macro_rules! network_name {
    () => {
        "net-007"
    };
}
pub const NETWORK_NAME: &str = network_name!();

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

static NNUE: Network = unsafe {
    std::mem::transmute(*include_bytes!(concat!(
        "../../nets/",
        network_name!(),
        ".bin"
    )))
};

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
            let piece = board.piece_at(sq).expect("an occupied square");
            for view in Color::ALL {
                let column = &NNUE.feature_weights[input(view, piece, sq)].0;
                for (value, weight) in accumulators.0[view].0.iter_mut().zip(column) {
                    *value += weight;
                }
            }
        }
        accumulators
    }

    /// For the side to move, at the trainer's scale
    pub fn evaluate(&self, side_to_move: Color) -> i32 {
        let us = &self.0[side_to_move];
        let them = &self.0[side_to_move.oposite()];
        let (our_weights, their_weights) = NNUE.output_weights.split_at(HIDDEN_SIZE);

        // in QA * QA * QB, since screlu squares what is in QA
        let output = i64::from(activated_dot(us, our_weights))
            + i64::from(activated_dot(them, their_weights));
        // to QA * QB, the bias's, then to centipawns
        let output = output / i64::from(QA) + i64::from(NNUE.output_bias);
        (output * i64::from(SCALE) / i64::from(QA * QB)) as i32
    }

    /// Writes into `child` these accumulators changed by `update`, in one pass over each
    /// view: reading the parent and the columns once is most of the cost
    fn update_into(&self, update: Update, child: &mut Self) {
        for view in Color::ALL {
            let (parent, child) = (&self.0[view], &mut child.0[view]);
            let column = |(piece, sq)| &NNUE.feature_weights[input(view, piece, sq)];
            match update {
                Update::Pass => *child = *parent,
                Update::Move { add, remove } => add_sub(child, parent, column(add), column(remove)),
                Update::Capture {
                    add,
                    remove,
                    captured,
                } => add_sub_sub(child, parent, column(add), column(remove), column(captured)),
                Update::Castle { add, remove } => add_add_sub_sub(
                    child,
                    parent,
                    [column(add[0]), column(add[1])],
                    [column(remove[0]), column(remove[1])],
                ),
            }
        }
    }
}

/// What a move changes in the inputs: the pieces it puts on squares and takes off them
#[derive(Clone, Copy, Debug)]
enum Update {
    /// A null move
    Pass,
    Move {
        add: (Piece, Square),
        remove: (Piece, Square),
    },
    Capture {
        add: (Piece, Square),
        remove: (Piece, Square),
        captured: (Piece, Square),
    },
    Castle {
        add: [(Piece, Square); 2],
        remove: [(Piece, Square); 2],
    },
}

impl Update {
    fn new(parent: &Board, mv: Move) -> Self {
        let us = parent.side_to_move();
        let moving = parent.piece_at(mv.from()).expect("a piece moves");
        let remove = (moving, mv.from());
        let captured = || (parent.piece_at(mv.to()).expect("a capture"), mv.to());

        match mv.kind() {
            MoveKind::Normal | MoveKind::DoublePush => Self::Move {
                add: (moving, mv.to()),
                remove,
            },
            MoveKind::Capture => Self::Capture {
                add: (moving, mv.to()),
                remove,
                captured: captured(),
            },
            MoveKind::EnPassant => Self::Capture {
                add: (moving, mv.to()),
                remove,
                captured: (
                    Piece::new(us.oposite(), PieceKind::Pawn),
                    mv.get_ep_square(us),
                ),
            },
            MoveKind::Castle => {
                let (rook_from, rook_to) = mv.get_rook_squares();
                let rook = Piece::new(us, PieceKind::Rook);
                Self::Castle {
                    add: [(moving, mv.to()), (rook, rook_to)],
                    remove: [remove, (rook, rook_from)],
                }
            }
            MoveKind::Promotion(promoted) => Self::Move {
                add: (Piece::new(us, promoted.to_piece_kind()), mv.to()),
                remove,
            },
            MoveKind::PromotionCapture(promoted) => Self::Capture {
                add: (Piece::new(us, promoted.to_piece_kind()), mv.to()),
                remove,
                captured: captured(),
            },
        }
    }
}

/// The accumulators along the line being searched, the current node's last. A child's
/// are written straight from its parent's, which sit next to them
pub struct AccumulatorStack {
    stack: Box<[Accumulators]>,
    len: usize,
}

impl AccumulatorStack {
    /// Room for the root and `plies` below it
    pub fn new(plies: usize) -> Self {
        let empty = Accumulators([Accumulator([0; HIDDEN_SIZE]); 2]);
        Self {
            stack: vec![empty; plies + 1].into_boxed_slice(),
            len: 0,
        }
    }

    /// Starts over with only `root`
    pub fn reset(&mut self, root: &Board) {
        self.stack[0] = Accumulators::new(root);
        self.len = 1;
    }

    /// Enters the child of `parent` that `mv` reaches (`None` passes)
    pub fn push(&mut self, parent: &Board, mv: Option<Move>) {
        let update = mv.map_or(Update::Pass, |mv| Update::new(parent, mv));
        let (line, rest) = self.stack.split_at_mut(self.len);
        line[self.len - 1].update_into(update, &mut rest[0]);
        self.len += 1;
    }

    pub fn pop(&mut self) {
        self.len -= 1;
    }

    pub fn current(&self) -> &Accumulators {
        &self.stack[self.len - 1]
    }
}

fn add_sub(child: &mut Accumulator, parent: &Accumulator, add: &Accumulator, sub: &Accumulator) {
    for i in 0..HIDDEN_SIZE {
        child.0[i] = parent.0[i] + add.0[i] - sub.0[i];
    }
}

fn add_sub_sub(
    child: &mut Accumulator,
    parent: &Accumulator,
    add: &Accumulator,
    sub: &Accumulator,
    sub2: &Accumulator,
) {
    for i in 0..HIDDEN_SIZE {
        child.0[i] = parent.0[i] + add.0[i] - sub.0[i] - sub2.0[i];
    }
}

fn add_add_sub_sub(
    child: &mut Accumulator,
    parent: &Accumulator,
    [add, add2]: [&Accumulator; 2],
    [sub, sub2]: [&Accumulator; 2],
) {
    for i in 0..HIDDEN_SIZE {
        child.0[i] = parent.0[i] + add.0[i] + add2.0[i] - sub.0[i] - sub2.0[i];
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
        test_utils::{KIWIPETE, PERFT_POSITIONS, board, both_colors, mirror_fen},
    };

    /// The 768 inputs
    #[test]
    fn the_network_is_the_file_its_name_says() {
        // `include_bytes!` only takes a literal, so the name is written there again
        let path = format!("{}/nets/{NETWORK_NAME}.bin", env!("CARGO_MANIFEST_DIR"));
        let file = std::fs::read(&path).unwrap_or_else(|err| panic!("{path}: {err}"));
        let in_file: Vec<i16> = file
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| i16::from_le_bytes(pair))
            .collect();

        // in the order of the file, which ends in the padding
        let embedded: Vec<i16> = NNUE
            .feature_weights
            .iter()
            .chain([&NNUE.feature_bias])
            .flat_map(|accumulator| accumulator.0)
            .chain(NNUE.output_weights)
            .chain([NNUE.output_bias])
            .collect();
        assert!(
            in_file[..embedded.len()] == embedded[..],
            "the network embedded isn't {path}"
        );
    }

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
            accumulators.update_into(Update::new(board, mv), &mut updated);
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
        for fen in PERFT_POSITIONS.into_iter().flat_map(both_colors) {
            let board = board(&fen);
            assert_updates_match(&board, Accumulators::new(&board), 2, &mut kinds);
        }
        assert_eq!(kinds.len(), 7, "not every kind of move got played");
    }

    #[test]
    fn the_stack_follows_the_line_being_searched() {
        let root = board(KIWIPETE);
        let mut stack = AccumulatorStack::new(2);
        stack.reset(&root);

        let (mv, child) = root
            .legal_successors()
            .find(|(mv, _)| mv.is_capture())
            .expect("a capture");
        stack.push(&root, Some(mv));
        assert_eq!(*stack.current(), Accumulators::new(&child));

        // passing moves no piece
        stack.push(&child, None);
        assert_eq!(*stack.current(), Accumulators::new(&child));

        stack.pop();
        stack.pop();
        assert_eq!(*stack.current(), Accumulators::new(&root));
    }

    /// `trainer/checkpoints/net-007-evals.txt`, before quantising
    const TRAINER_EVALS: [(&str, f32); 7] = [
        (Board::STARTPOS, 40.4),
        (
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
            -11.2,
        ),
        (KIWIPETE, -143.1),
        (
            "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
            57.9,
        ),
        (
            "rnbqkb1r/pppp1ppp/5n2/4p3/4P3/2N5/PPPP1PPP/R1BQKBNR b KQkq - 2 3",
            57.9,
        ),
        ("4k3/8/8/8/8/8/PPP5/4K2R w K - 0 1", 2496.8),
        ("4k2r/ppp5/8/8/8/8/8/4K3 b k - 0 1", 2496.8),
    ];

    /// Rounding net-007's weights moves these positions by up to 15.6, most of it in the
    /// output layer. It depends on the network: net-006's moved by up to 2.7
    const QUANTISATION_ERROR: f32 = 20.0;

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
