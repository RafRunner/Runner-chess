use std::sync::LazyLock;

use super::rays::{self, Direction, negative_ray, positive_ray};
use crate::{bitboard::BitBoard, rng::Rng, square::Square};

pub(super) static MAGIC_BISHOPS: LazyLock<MagicBBs> = LazyLock::new(|| MagicBBs::new_bishops(1337));
pub(super) static MAGIC_ROOKS: LazyLock<MagicBBs> = LazyLock::new(|| MagicBBs::new_rooks(1337));

fn bishop_mask_and_bits() -> ([BitBoard; 64], [u8; 64]) {
    let border = BitBoard::RANK_1 | BitBoard::FILE_A | BitBoard::RANK_8 | BitBoard::FILE_H;
    let mut mask = [BitBoard::EMPTY; 64];
    let mut bits = [0u8; 64];

    for sq in BitBoard::FULL {
        mask[sq] = rays::bishop_attacks(sq, BitBoard::EMPTY) & !border;
        bits[sq] = mask[sq].count_ones() as u8 // Safe, mask never has more than 9 ones
    }

    (mask, bits)
}

fn rook_mask_and_bits() -> ([BitBoard; 64], [u8; 64]) {
    let mut mask = [BitBoard::EMPTY; 64];
    let mut bits = [0u8; 64];

    for sq in BitBoard::FULL {
        let file_atk = (positive_ray(sq, BitBoard::EMPTY, Direction::North) & !BitBoard::RANK_8)
            | (negative_ray(sq, BitBoard::EMPTY, Direction::South) & !BitBoard::RANK_1);
        let rank_atk = (positive_ray(sq, BitBoard::EMPTY, Direction::East) & !BitBoard::FILE_H)
            | (negative_ray(sq, BitBoard::EMPTY, Direction::West) & !BitBoard::FILE_A);

        mask[sq] = file_atk | rank_atk;
        bits[sq] = mask[sq].count_ones() as u8 // Safe, mask never has more than 12 ones
    }

    (mask, bits)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Magic {
    mask: BitBoard,
    bits: u8,
    magic: u64,
    offset: usize,
}

impl Magic {
    pub fn empty() -> Self {
        Self {
            mask: BitBoard::EMPTY,
            bits: 0,
            magic: 0,
            offset: 0,
        }
    }
}

pub(super) struct MagicBBs {
    magics: [Magic; 64],
    tables: Vec<BitBoard>,
}

impl MagicBBs {
    pub(super) fn new_bishops(seed: u64) -> Self {
        let (masks, bits) = bishop_mask_and_bits();
        let mut tables = Vec::with_capacity(5_248);
        let magics = find_magic(Rng(seed), masks, bits, rays::bishop_attacks, &mut tables);
        Self { magics, tables }
    }

    pub(super) fn new_rooks(seed: u64) -> Self {
        let (masks, bits) = rook_mask_and_bits();
        let mut tables = Vec::with_capacity(102_400);
        let magics = find_magic(Rng(seed), masks, bits, rays::rook_attacks, &mut tables);
        Self { magics, tables }
    }

    pub(super) fn attacks(&self, sq: Square, occupied: BitBoard) -> BitBoard {
        let Magic {
            mask,
            bits,
            magic,
            offset,
        } = self.magics[sq];
        let blockers = (occupied & mask).raw();
        let index = blockers.wrapping_mul(magic) >> (64 - bits);
        self.tables[offset + index as usize]
    }
}

fn find_magic(
    mut rng: Rng,
    masks: [BitBoard; 64],
    bits: [u8; 64],
    attack_fn: fn(Square, BitBoard) -> BitBoard,
    tables: &mut Vec<BitBoard>,
) -> [Magic; 64] {
    let mut magics = [Magic::empty(); 64];

    for sq in BitBoard::FULL {
        let mask = masks[sq];
        let this_bits = bits[sq];
        let mut occupied = Vec::new();
        let mut attacks = Vec::new();
        let mut subset = BitBoard::EMPTY;

        loop {
            occupied.push(subset);
            attacks.push(attack_fn(sq, subset));
            subset = BitBoard::new(subset.raw().wrapping_sub(mask.raw())) & mask;

            if subset == BitBoard::EMPTY {
                break;
            }
        }

        loop {
            let magic = rng.next_u64() & rng.next_u64() & rng.next_u64();
            // Bad magic
            if (mask.raw().wrapping_mul(magic) >> 56).count_ones() < 6 {
                continue;
            }
            let mut table = vec![BitBoard::EMPTY; 1 << this_bits];

            let mut ok = true;

            for idx in 0..occupied.len() {
                let index = (occupied[idx].raw().wrapping_mul(magic) >> (64 - this_bits)) as usize;

                if table[index] == BitBoard::EMPTY {
                    table[index] = attacks[idx];
                    // destructive conflict
                } else if table[index] != attacks[idx] {
                    ok = false;
                    break;
                }
            }

            if ok {
                let offset = tables.len();
                magics[sq] = Magic {
                    mask,
                    bits: this_bits,
                    magic,
                    offset,
                };
                tables.extend_from_slice(&table);
                break;
            }
        }
    }

    magics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::square::Square;
    use Square as S;

    /// The size of the attack tables of a piece: every square has a slot for each of
    /// the 2^bits blockers its mask can have
    fn table_size(bits: [u8; 64]) -> usize {
        bits.iter().map(|&bits| 1 << bits).sum()
    }

    #[test]
    fn the_tables_have_the_known_sizes() {
        assert_eq!(table_size(rook_mask_and_bits().1), 102_400);
        assert_eq!(table_size(bishop_mask_and_bits().1), 5_248);
    }

    /// A square is in the mask exactly when a lone piece on it changes the attacks:
    /// a blocker on the last square of a ray, or off the rays, changes nothing
    fn assert_masks(masks: [BitBoard; 64], attacks: fn(Square, BitBoard) -> BitBoard) {
        for sq in BitBoard::FULL {
            let open = attacks(sq, BitBoard::EMPTY);
            let expected: BitBoard = BitBoard::FULL
                .filter(|&blocker| blocker != sq && attacks(sq, blocker.bb()) != open)
                .fold(BitBoard::EMPTY, |mask, blocker| mask | blocker.bb());
            assert_eq!(masks[sq], expected, "{sq}");
        }
    }

    #[test]
    fn masks_hold_the_squares_where_a_blocker_matters() {
        assert_masks(rook_mask_and_bits().0, rays::rook_attacks);
        assert_masks(bishop_mask_and_bits().0, rays::bishop_attacks);
    }

    #[test]
    fn bits_count_the_squares_of_the_mask() {
        let (rook_masks, rook_bits) = rook_mask_and_bits();
        let (bishop_masks, bishop_bits) = bishop_mask_and_bits();
        for sq in BitBoard::FULL {
            assert_eq!(
                u32::from(rook_bits[sq]),
                rook_masks[sq].count_ones(),
                "{sq}"
            );
            assert_eq!(
                u32::from(bishop_bits[sq]),
                bishop_masks[sq].count_ones(),
                "{sq}"
            );
        }

        // in the corner a rook sees the most, and a bishop in the middle
        assert_eq!(rook_bits[S::A1], 12);
        assert_eq!(rook_bits[S::D4], 10);
        assert_eq!(bishop_bits[S::D4], 9);
        assert_eq!(bishop_bits[S::A4], 5);
    }

    type Slow = fn(Square, BitBoard) -> BitBoard;

    /// The `n`th of the 2^bits ways to put pieces on `mask`: bit `i` of `n` says whether
    /// its `i`th square has one. Counted apart from the generator's own enumeration
    fn filling(mask: BitBoard, n: usize) -> BitBoard {
        mask.enumerate()
            .filter(|&(i, _)| n >> i & 1 == 1)
            .fold(BitBoard::EMPTY, |filled, (_, sq)| filled | sq.bb())
    }

    /// The masks and bits come from where the magics got them, not from the magics
    fn assert_every_filling(
        magics: &MagicBBs,
        (masks, bits): ([BitBoard; 64], [u8; 64]),
        slow: Slow,
    ) {
        for sq in BitBoard::FULL {
            for n in 0..1 << bits[sq] {
                let occupied = filling(masks[sq], n);
                assert_eq!(
                    magics.attacks(sq, occupied),
                    slow(sq, occupied),
                    "{sq}, occupied {occupied:?}"
                );
            }
        }
    }

    #[test]
    fn every_filling_of_the_mask_finds_its_attacks() {
        assert_every_filling(&MAGIC_BISHOPS, bishop_mask_and_bits(), rays::bishop_attacks);
        assert_every_filling(&MAGIC_ROOKS, rook_mask_and_bits(), rays::rook_attacks);
    }

    /// Whole boards, with pieces on the edges, on the slider's own square and off its
    /// lines, as the engine passes them
    fn assert_whole_boards(magics: &MagicBBs, slow: Slow) {
        let mut rng = Rng(7);
        for sq in BitBoard::FULL {
            for _ in 0..1000 {
                let occupied = BitBoard::new(rng.next_u64() & rng.next_u64()) | sq.bb();
                assert_eq!(
                    magics.attacks(sq, occupied),
                    slow(sq, occupied),
                    "{sq}, occupied {occupied:?}"
                );
            }
        }
    }

    #[test]
    fn pieces_off_the_mask_change_nothing() {
        assert_whole_boards(&MAGIC_BISHOPS, rays::bishop_attacks);
        assert_whole_boards(&MAGIC_ROOKS, rays::rook_attacks);
    }

    #[test]
    fn each_square_gets_a_slot_for_each_filling() {
        for magics in [&*MAGIC_BISHOPS, &*MAGIC_ROOKS] {
            let mut next = 0;
            for sq in BitBoard::FULL {
                assert_eq!(magics.magics[sq].offset, next, "{sq}");
                next += 1 << magics.magics[sq].bits;
            }
            assert_eq!(magics.tables.len(), next);
        }
    }

    // the search for the magics

    const SEEDS: [u64; 3] = [1, 2, 0x5eed];

    #[test]
    fn any_seed_finds_magics_that_work() {
        // the bishops only, as the rooks take a while
        for seed in SEEDS {
            let bishops = MagicBBs::new_bishops(seed);
            assert_every_filling(&bishops, bishop_mask_and_bits(), rays::bishop_attacks);
        }
    }

    #[test]
    fn the_same_seed_finds_the_same_magics() {
        // so the tables built at startup are the same in every run
        assert_eq!(
            MagicBBs::new_bishops(SEEDS[0]).magics,
            MagicBBs::new_bishops(SEEDS[0]).magics
        );
        assert_ne!(
            MagicBBs::new_bishops(SEEDS[0]).magics,
            MagicBBs::new_bishops(SEEDS[1]).magics
        );
    }
}
