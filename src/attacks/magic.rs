use super::rays::{self, Direction, negative_ray, positive_ray};
use crate::{bitboard::BitBoard, rng::Rng};

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

fn bishop_magic(mut rng: Rng) -> ([u64; 64], Vec<Vec<BitBoard>>) {
    let mut magics = [0; 64];
    let mut tables = Vec::with_capacity(512);
    let (masks, bits) = bishop_mask_and_bits();

    for sq in BitBoard::FULL {
        let mask = masks[sq];
        let this_bits = bits[sq];
        let mut occupied = Vec::new();
        let mut attacks = Vec::new();
        let mut subset = BitBoard::EMPTY;

        loop {
            occupied.push(subset);
            attacks.push(rays::bishop_attacks(sq, subset));
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
                magics[sq] = magic;
                tables.push(table);
                break;
            }
        }
    }

    (magics, tables)
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

    // the bishop's magics

    /// The bishop's magics found from `seed`, with what a lookup needs, looked up the
    /// way the engine will
    struct Bishops {
        masks: [BitBoard; 64],
        bits: [u8; 64],
        magics: [u64; 64],
        tables: Vec<Vec<BitBoard>>,
    }

    impl Bishops {
        fn new(seed: u64) -> Self {
            let (masks, bits) = bishop_mask_and_bits();
            let (magics, tables) = bishop_magic(Rng(seed));
            Self {
                masks,
                bits,
                magics,
                tables,
            }
        }

        fn attacks(&self, sq: Square, occupied: BitBoard) -> BitBoard {
            let blockers = (occupied & self.masks[sq]).raw();
            let index = blockers.wrapping_mul(self.magics[sq]) >> (64 - self.bits[sq]);
            self.tables[sq.index()][index as usize]
        }
    }

    const SEEDS: [u64; 3] = [1, 2, 0x5eed];

    /// The `n`th of the 2^bits ways to put pieces on `mask`: bit `i` of `n` says whether
    /// its `i`th square has one. Counted apart from the generator's own enumeration
    fn filling(mask: BitBoard, n: usize) -> BitBoard {
        mask.enumerate()
            .filter(|&(i, _)| n >> i & 1 == 1)
            .fold(BitBoard::EMPTY, |filled, (_, sq)| filled | sq.bb())
    }

    #[test]
    fn every_filling_of_the_mask_finds_its_attacks() {
        for seed in SEEDS {
            let bishops = Bishops::new(seed);
            for sq in BitBoard::FULL {
                let mask = bishops.masks[sq];
                for n in 0..1 << bishops.bits[sq] {
                    let occupied = filling(mask, n);
                    assert_eq!(
                        bishops.attacks(sq, occupied),
                        rays::bishop_attacks(sq, occupied),
                        "seed {seed}, {sq}, occupied {occupied:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn pieces_off_the_mask_change_nothing() {
        // whole boards, with pieces on the edges, on the bishop's own square and off
        // its diagonals, as the engine will pass them
        let bishops = Bishops::new(SEEDS[0]);
        let mut rng = Rng(7);
        for sq in BitBoard::FULL {
            for _ in 0..1000 {
                let occupied = BitBoard::new(rng.next_u64() & rng.next_u64()) | sq.bb();
                assert_eq!(
                    bishops.attacks(sq, occupied),
                    rays::bishop_attacks(sq, occupied),
                    "{sq}, occupied {occupied:?}"
                );
            }
        }
    }

    #[test]
    fn each_square_gets_a_slot_for_each_filling() {
        let bishops = Bishops::new(SEEDS[0]);
        assert_eq!(bishops.tables.len(), 64);
        for sq in BitBoard::FULL {
            let slots = bishops.tables[sq.index()].len();
            assert_eq!(slots, 1 << bishops.bits[sq], "{sq}");
        }
    }

    #[test]
    fn the_same_seed_finds_the_same_magics() {
        // so the tables built at startup are the same in every run
        assert_eq!(Bishops::new(SEEDS[0]).magics, Bishops::new(SEEDS[0]).magics);
    }
}
