//! The network, `(768x4hm -> 256)x2 -> 1x8`, from `trainer/` (see `docs/nnue.md`): the
//! inputs of 4 regions of each view's own king, the board mirrored to keep it on files a
//! to d, and 8 output layers, one for each bucket of numbers of pieces. `KING_BUCKETS`
//! and `WING_OUTPUTS` give it the trainer's `--king-buckets` and `--wing-outputs`.

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
/// The output layer of a position is the one of its bucket, by its number of pieces
const MATERIAL_BUCKETS: usize = 8;
/// 8 more output buckets for the kings on opposite wings, a to d and e to h
const WING_OUTPUTS: bool = false;
const OUTPUT_BUCKETS: usize = MATERIAL_BUCKETS * if WING_OUTPUTS { 2 } else { 1 };
/// The region of each view's own king, by rank from its side and file from a to d, the
/// board mirrored to keep that king there; `None` for 768 inputs only
const KING_BUCKETS: Option<[usize; 32]> = Some(KING_BUCKETS_LAYOUT);
/// The trainer's `KING_BUCKETS`: castled in the corner, at home in the middle, on the
/// second rank, further up
#[rustfmt::skip]
const KING_BUCKETS_LAYOUT: [usize; 32] = [
    0, 0, 1, 1,
    2, 2, 2, 2,
    3, 3, 3, 3,
    3, 3, 3, 3,
    3, 3, 3, 3,
    3, 3, 3, 3,
    3, 3, 3, 3,
    3, 3, 3, 3,
];
const INPUT_BUCKETS: usize = match KING_BUCKETS {
    Some(buckets) => {
        let mut most = 0;
        let mut i = 0;
        while i < buckets.len() {
            if buckets[i] > most {
                most = buckets[i];
            }
            i += 1;
        }
        most + 1
    }
    None => 1,
};

/// The trainer clips weights to ±1.98, 127 in `QB`: each view's half of the output layer
/// always fits an i32, so its sum stays in the lanes that SIMD handles best. Both halves
/// together may not
const MAX_OUTPUT_WEIGHT: i64 = 127;
const _: () = assert!(HIDDEN_SIZE as i64 * (QA * QA) as i64 * MAX_OUTPUT_WEIGHT <= i32::MAX as i64);

macro_rules! network_name {
    () => {
        "net-012"
    };
}
pub const NETWORK_NAME: &str = network_name!();

/// bullet's layout, padded to a multiple of 64 bytes, as `align(64)` pads the struct
#[repr(C)]
struct Network {
    /// One column per input, 768 for each king bucket, in `QA`
    feature_weights: [Accumulator; 768 * INPUT_BUCKETS],
    feature_bias: Accumulator,
    /// For each bucket, the side to move's half first, in `QB`
    output_weights: [[i16; 2 * HIDDEN_SIZE]; OUTPUT_BUCKETS],
    /// In `QA * QB`
    output_bias: [i16; OUTPUT_BUCKETS],
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
        Self(Color::ALL.map(|view| refresh(board, view)))
    }

    /// Of `board`, whose accumulators these are, for the side to move, at the trainer's
    /// scale
    pub fn evaluate(&self, board: &Board) -> i32 {
        let us = &self.0[board.side_to_move()];
        let them = &self.0[board.side_to_move().oposite()];
        let bucket = output_bucket(board);
        let (our_weights, their_weights) = NNUE.output_weights[bucket].split_at(HIDDEN_SIZE);

        // in QA * QA * QB, since screlu squares what is in QA
        let output = i64::from(activated_dot(us, our_weights))
            + i64::from(activated_dot(them, their_weights));
        // to QA * QB, the bias's, then to centipawns
        let output = output / i64::from(QA) + i64::from(NNUE.output_bias[bucket]);
        (output * i64::from(SCALE) / i64::from(QA * QB)) as i32
    }

    /// Writes into `child` these accumulators, of `parent_board`, changed by `update` into
    /// those of `child_board`, in one pass over each view: reading the parent and the
    /// columns once is most of the cost. A view whose king changes bucket starts over
    fn update_into(
        &self,
        update: Update,
        child: &mut Self,
        parent_board: &Board,
        child_board: &Board,
    ) {
        for view in Color::ALL {
            let king = KingView::new(child_board, view);
            if king != KingView::new(parent_board, view) {
                child.0[view] = refresh(child_board, view);
                continue;
            }
            let (parent, child) = (&self.0[view], &mut child.0[view]);
            let column = |(piece, sq)| &NNUE.feature_weights[king.input(input(view, piece, sq))];
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

    /// Enters `child`, the child of `parent` that `mv` reaches (`None` passes)
    pub fn push(&mut self, parent: &Board, child: &Board, mv: Option<Move>) {
        let update = mv.map_or(Update::Pass, |mv| Update::new(parent, mv));
        let (line, rest) = self.stack.split_at_mut(self.len);
        line[self.len - 1].update_into(update, &mut rest[0], parent, child);
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

/// bullet's `MaterialCount`: 4 numbers of pieces in each, from the kings alone in 0 to
/// the full board in 7, then 8 more for the kings on opposite wings with `WING_OUTPUTS`
fn output_bucket(board: &Board) -> usize {
    let material =
        (board.all_pieces_bb().count_ones() as usize - 2) / 32usize.div_ceil(MATERIAL_BUCKETS);
    let wing = |color| board.king(color).file() > 3;
    let opposite = WING_OUTPUTS && wing(Color::White) != wing(Color::Black);
    material + MATERIAL_BUCKETS * usize::from(opposite)
}

/// Where a view's inputs are, from the square of its own king: the 768 of its bucket, and
/// the board mirrored when that king is on files e to h, as bullet's `ChessBucketsMirrored`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct KingView {
    first: usize,
    /// 7 mirrors the file of an input's square
    flip: usize,
}

impl KingView {
    fn new(board: &Board, view: Color) -> Self {
        let Some(buckets) = KING_BUCKETS else {
            return Self { first: 0, flip: 0 };
        };
        Self::of(&buckets, board, view)
    }

    fn of(buckets: &[usize; 32], board: &Board, view: Color) -> Self {
        let king = board.king(view);
        let king = if view == Color::White {
            king
        } else {
            king.mirror()
        };
        let (file, rank) = (usize::from(king.file()), usize::from(king.rank()));
        let flip = if file > 3 { 7 } else { 0 };
        Self {
            first: 768 * buckets[4 * rank + file.min(7 - file)],
            flip,
        }
    }

    /// The input of the view for one of `input`'s
    fn input(self, input: usize) -> usize {
        self.first + (input ^ self.flip)
    }
}

/// The accumulator of a view, from every piece on the board
fn refresh(board: &Board, view: Color) -> Accumulator {
    let king = KingView::new(board, view);
    let mut accumulator = NNUE.feature_bias;
    for sq in board.all_pieces_bb() {
        let piece = board.piece_at(sq).expect("an occupied square");
        let column = &NNUE.feature_weights[king.input(input(view, piece, sq))].0;
        for (value, weight) in accumulator.0.iter_mut().zip(column) {
            *value += weight;
        }
    }
    accumulator
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

/// One view's half of the output layer: the sum of screlu(x) * w = (y * w) * y, with y
/// the value clamped to [0, QA]. y * w fits an i16, as y <= 255 and |w| <= 127, so the
/// first product takes 8 values at once, and only the second gets widened
fn activated_dot(accumulator: &Accumulator, weights: &[i16]) -> i32 {
    accumulator
        .0
        .iter()
        .zip(weights)
        .map(|(&x, &w)| {
            let y = x.clamp(0, QA as i16);
            i32::from(y * w) * i32::from(y)
        })
        .sum()
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
            .chain(NNUE.output_weights.into_iter().flatten())
            .chain(NNUE.output_bias)
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
    fn a_king_bucket_is_the_region_of_the_own_king_mirrored() {
        let view = |fen: &str, color| KingView::of(&KING_BUCKETS_LAYOUT, &board(fen), color);
        let at = |bucket: usize, flip| KingView {
            first: 768 * bucket,
            flip,
        };
        // castled on either wing, the same bucket, mirrored on the king's
        assert_eq!(
            view("6k1/8/8/8/8/8/8/6K1 w - - 0 1", Color::White),
            at(0, 7)
        );
        assert_eq!(
            view("1k6/8/8/8/8/8/8/1K6 w - - 0 1", Color::White),
            at(0, 0)
        );
        // Black's from its own side
        assert_eq!(
            view("6k1/8/8/8/8/8/8/6K1 w - - 0 1", Color::Black),
            at(0, 7)
        );
        assert_eq!(
            view("4k3/8/8/8/8/8/8/3K4 w - - 0 1", Color::White),
            at(1, 0)
        );
        assert_eq!(
            view("4k3/8/8/8/8/8/8/3K4 w - - 0 1", Color::Black),
            at(1, 7)
        );
        assert_eq!(
            view("8/4k3/8/8/8/8/4K3/8 w - - 0 1", Color::White),
            at(2, 7)
        );
        assert_eq!(view("8/8/8/3k4/8/8/8/K7 b - - 0 1", Color::Black), at(3, 0));
    }

    #[test]
    fn a_king_view_mirrors_the_file_of_an_input() {
        let mirrored = KingView {
            first: 768,
            flip: 7,
        };
        let knight = Piece::new(Color::White, PieceKind::Knight);
        // the knight on g1 seen as if on b1, in the second bucket
        assert_eq!(
            mirrored.input(input(Color::White, knight, Square::G1)),
            768 + input(Color::White, knight, Square::B1)
        );
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
            accumulators.update_into(Update::new(board, mv), &mut updated, board, &next);
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
        stack.push(&root, &child, Some(mv));
        assert_eq!(*stack.current(), Accumulators::new(&child));

        // passing moves no piece
        stack.push(&child, &child, None);
        assert_eq!(*stack.current(), Accumulators::new(&child));

        stack.pop();
        stack.pop();
        assert_eq!(*stack.current(), Accumulators::new(&root));
    }

    #[test]
    fn the_output_weights_fit_the_activated_product() {
        let weights = NNUE.output_weights.iter().flatten();
        assert!(
            weights
                .into_iter()
                .all(|w| i64::from(w.unsigned_abs()) <= MAX_OUTPUT_WEIGHT)
        );
    }

    /// `trainer/checkpoints/net-012-evals.txt`, before quantising
    const TRAINER_EVALS: [(&str, f32); 9] = [
        (Board::STARTPOS, 66.3),
        (
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
            -10.9,
        ),
        (KIWIPETE, -143.5),
        (
            "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
            59.9,
        ),
        (
            "rnbqkb1r/pppp1ppp/5n2/4p3/4P3/2N5/PPPP1PPP/R1BQKBNR b KQkq - 2 3",
            59.9,
        ),
        ("4k3/8/8/8/8/8/PPP5/4K2R w K - 0 1", 2707.7),
        ("4k2r/ppp5/8/8/8/8/8/4K3 b k - 0 1", 2707.7),
        // output buckets 4 and 0
        ("r4rk1/pp3ppp/8/8/8/8/PP3PPP/R4RK1 w - - 0 1", 42.2),
        ("8/8/8/4k3/8/8/2BN4/4K3 w - - 0 1", 340.5),
    ];

    /// Rounding net-012's weights moves these positions by up to 10.1, toward the side to
    /// move in the openings, and by 7.9 at most elsewhere. It depends on the network:
    /// net-010's moved by up to 20.3, net-006's by 2.7
    const QUANTISATION_ERROR: f32 = 25.0;

    #[test]
    fn the_output_matches_the_trainer() {
        for (fen, expected) in TRAINER_EVALS {
            let board = board(fen);
            let output = Accumulators::new(&board).evaluate(&board);
            // in the won endgames, a fraction of the score
            let tolerance = QUANTISATION_ERROR.max(expected.abs() / 100.0);
            assert!(
                (output as f32 - expected).abs() <= tolerance,
                "{fen}: {output} against {expected}"
            );
        }
    }
}
