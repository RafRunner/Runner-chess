//! What the trainer and `validate` share: the shape of the networks, which the engine's
//! inference has to match.

use bullet::game::{
    inputs::get_num_buckets,
    outputs::{MaterialCount, OutputBuckets},
};
use bulletformat::ChessBoard;

pub const HIDDEN_SIZE: usize = 256;
/// The output layer of a position is the one of its bucket: (pieces - 2) / 4
pub const OUTPUT_BUCKETS: usize = 8;
pub const SCALE: i32 = 400;
pub const QA: i16 = 255;
pub const QB: i16 = 64;

/// The region of the own king, by rank from its side and by file from a to d, mirrored:
/// castled in the corner, at home in the middle, on the second rank, further up
#[rustfmt::skip]
pub const KING_BUCKETS: [usize; 32] = [
    0, 0, 1, 1,
    2, 2, 2, 2,
    3, 3, 3, 3,
    3, 3, 3, 3,
    3, 3, 3, 3,
    3, 3, 3, 3,
    3, 3, 3, 3,
    3, 3, 3, 3,
];
pub const INPUT_BUCKETS: usize = get_num_buckets(&KING_BUCKETS);

/// `MaterialCount`, then 8 more for the kings on opposite wings, a to d and e to h
#[derive(Clone, Copy, Default)]
pub struct MaterialAndWings;

impl OutputBuckets<ChessBoard> for MaterialAndWings {
    const BUCKETS: usize = 2 * OUTPUT_BUCKETS;

    fn bucket(&self, pos: &ChessBoard) -> u8 {
        let material = MaterialCount::<OUTPUT_BUCKETS>.bucket(pos);
        let opposite = (pos.our_ksq() % 8 > 3) != (pos.opp_ksq() % 8 > 3);
        material + OUTPUT_BUCKETS as u8 * u8::from(opposite)
    }
}
