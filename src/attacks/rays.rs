//! The attacks of the sliders, ray by ray: the slow way, which builds the magic tables
//! and checks them

use std::ops;

use crate::{
    bitboard::BitBoard,
    square::{Delta, Square},
};

static RAYS: [[BitBoard; 64]; 8] = calculate_rays();

pub(super) fn bishop_attacks(sq: Square, occupied: BitBoard) -> BitBoard {
    positive_ray(sq, occupied, Direction::NortEast)
        | positive_ray(sq, occupied, Direction::NothWest)
        | negative_ray(sq, occupied, Direction::SouthEast)
        | negative_ray(sq, occupied, Direction::SouthWest)
}

pub(super) fn rook_attacks(sq: Square, occupied: BitBoard) -> BitBoard {
    positive_ray(sq, occupied, Direction::North)
        | positive_ray(sq, occupied, Direction::East)
        | negative_ray(sq, occupied, Direction::South)
        | negative_ray(sq, occupied, Direction::West)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Direction {
    North,
    South,
    East,
    West,
    NortEast,
    NothWest,
    SouthEast,
    SouthWest,
}

impl Direction {
    const ALL: [Direction; 8] = [
        Self::North,
        Self::South,
        Self::East,
        Self::West,
        Self::NortEast,
        Self::NothWest,
        Self::SouthEast,
        Self::SouthWest,
    ];

    const fn to_delta(self) -> Delta {
        match self {
            Direction::North => Delta::NORTH,
            Direction::South => Delta::SOUTH,
            Direction::East => Delta::EAST,
            Direction::West => Delta::WEST,
            Direction::NortEast => Delta::NORTH_EAST,
            Direction::NothWest => Delta::NORTH_WEST,
            Direction::SouthEast => Delta::SOUTH_EAST,
            Direction::SouthWest => Delta::SOUTH_WEST,
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

impl<T> ops::Index<Direction> for [T; 8] {
    type Output = T;

    fn index(&self, index: Direction) -> &Self::Output {
        &self[index.index()]
    }
}

/// h8 is the highest possible blocker, so it stops overflows
#[inline(always)]
pub(super) fn positive_ray(sq: Square, occupied: BitBoard, direction: Direction) -> BitBoard {
    let ray = RAYS[direction][sq];
    let blockers = (ray & occupied) | Square::H8.bb();
    let first = Square::new(blockers.trailing_zeros() as u8);
    ray ^ RAYS[direction][first]
}

/// a1 is the smallest negative blocker, so it stops underflows
#[inline(always)]
pub(super) fn negative_ray(sq: Square, occupied: BitBoard, direction: Direction) -> BitBoard {
    let ray = RAYS[direction][sq];
    let blockers = (ray & occupied) | Square::A1.bb();
    let first = Square::new(63 - blockers.leading_zeros() as u8);
    ray ^ RAYS[direction][first]
}

const fn directional_attacks(mut sq: Square, occupied: BitBoard, delta: Delta) -> BitBoard {
    let mut bb = 0u64;

    while let Some(new_sq) = sq.offset(delta) {
        let has_piece = occupied.raw() & new_sq.bb().raw() != 0;
        bb |= new_sq.bb().raw();

        if has_piece {
            break;
        }
        sq = new_sq;
    }

    BitBoard::new(bb)
}

const fn calculate_rays() -> [[BitBoard; 64]; 8] {
    let mut rays = [[BitBoard::EMPTY; 64]; 8];

    let mut direction_idx = 0;

    while direction_idx < 8 {
        let mut square_idx = 0;
        let direction = Direction::ALL[direction_idx];

        while square_idx < 64 {
            let sq = Square::new(square_idx);

            rays[direction_idx][sq.index()] =
                directional_attacks(sq, BitBoard::EMPTY, direction.to_delta());

            square_idx += 1;
        }
        direction_idx += 1;
    }

    rays
}
