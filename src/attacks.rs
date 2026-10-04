use crate::{bitboard::BitBoard, piece::Color, square::Square};

pub static KNIGHT_ATTACKS: [BitBoard; 64] = knight_table();
pub static KING_ATTACKS: [BitBoard; 64] = king_table();
pub static PAWN_ATTACKS: [[BitBoard; 64]; 2] = [pawn_table(Color::White), pawn_table(Color::Black)];

const fn knight_table() -> [BitBoard; 64] {
    leaper_table(&[
        [2, 1],
        [2, -1],
        [-2, 1],
        [-2, -1],
        [1, 2],
        [1, -2],
        [-1, 2],
        [-1, -2],
    ])
}

const fn king_table() -> [BitBoard; 64] {
    leaper_table(&[
        [1, 0],
        [1, 1],
        [0, 1],
        [-1, 1],
        [-1, 0],
        [-1, -1],
        [0, -1],
        [1, -1],
    ])
}

const fn pawn_table(color: Color) -> [BitBoard; 64] {
    match color {
        Color::White => leaper_table(&[[1, 1], [-1, 1]]),
        Color::Black => leaper_table(&[[1, -1], [-1, -1]]),
    }
}

const fn leaper_table(deltas: &[[i16; 2]]) -> [BitBoard; 64] {
    let mut bbs = [BitBoard::EMPTY; 64];

    let mut rank: i16 = 0;

    while rank < 8 {
        let mut file: i16 = 0;

        while file < 8 {
            let sq = Square::from_file_and_rank(file as u8, rank as u8);
            let mut raw: u64 = 0;

            let mut idx = 0;
            while idx < deltas.len() {
                let [file_diff, rank_diff] = deltas[idx];

                let new_file = file + file_diff;
                let new_rank = rank + rank_diff;

                if new_file >= 0 && new_file < 8 && new_rank >= 0 && new_rank < 8 {
                    let new_sq = Square::from_file_and_rank(new_file as u8, new_rank as u8);

                    raw |= new_sq.bb().raw()
                }
                idx += 1;
            }

            bbs[sq.index()] = BitBoard::new(raw);
            file += 1;
        }
        rank += 1
    }

    bbs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::Color;
    use Square as S;

    /// the squares of a bitboard, in index order (a1, b1, ..., h8)
    fn squares(bb: BitBoard) -> Vec<Square> {
        bb.collect()
    }

    /// each square's attacks must be exactly the expected targets, in any order
    fn assert_attacks(table: &[BitBoard; 64], cases: &[(Square, &[Square])]) {
        for &(from, targets) in cases {
            let mut expected = targets.to_vec();
            expected.sort_by_key(|sq| sq.index());
            assert_eq!(squares(table[from]), expected, "attacks from {from}");
        }
    }

    /// every (from, to) pair in the table
    fn pairs(table: &[BitBoard; 64]) -> impl Iterator<Item = (Square, Square)> + '_ {
        (0..64)
            .map(S::new)
            .flat_map(move |from| table[from].map(move |to| (from, to)))
    }

    /// file and rank distance between two squares
    fn distance(a: Square, b: Square) -> (i8, i8) {
        let df = (a.file() as i8 - b.file() as i8).abs();
        let dr = (a.rank() as i8 - b.rank() as i8).abs();
        (df, dr)
    }

    fn total(table: &[BitBoard; 64]) -> u32 {
        table.iter().map(|bb| bb.count_ones()).sum()
    }

    #[test]
    fn knight_attacks_on_known_squares() {
        assert_attacks(
            &KNIGHT_ATTACKS,
            &[
                (S::A1, &[S::C2, S::B3]),
                (S::H1, &[S::F2, S::G3]),
                (S::A8, &[S::C7, S::B6]),
                (S::H8, &[S::F7, S::G6]),
                (
                    S::D4,
                    &[S::C2, S::E2, S::B3, S::F3, S::B5, S::F5, S::C6, S::E6],
                ),
                // a1 and h8 are indices 0 and 63, the easiest squares to leave out
                (S::B3, &[S::A1, S::C1, S::D2, S::D4, S::A5, S::C5]),
                (S::G6, &[S::H8, S::F8, S::E7, S::E5, S::F4, S::H4]),
            ],
        );
    }

    /// every target is one square away in one direction and two in the other,
    /// so a jump that wraps around the board edge (h1 -> b3) fails here
    #[test]
    fn knight_targets_are_knight_jumps() {
        for (from, to) in pairs(&KNIGHT_ATTACKS) {
            let d = distance(from, to);
            assert!(d == (1, 2) || d == (2, 1), "{from} -> {to}");
        }
    }

    /// there are 336 knight jumps on an empty board; with the test above,
    /// this means the table has every jump and nothing else
    #[test]
    fn knight_attack_count() {
        assert_eq!(total(&KNIGHT_ATTACKS), 336);
    }

    #[test]
    fn king_attacks_on_known_squares() {
        assert_attacks(
            &KING_ATTACKS,
            &[
                (S::A1, &[S::B1, S::A2, S::B2]),
                (S::H1, &[S::G1, S::G2, S::H2]),
                (S::A8, &[S::A7, S::B7, S::B8]),
                (S::H8, &[S::G7, S::H7, S::G8]),
                (S::E1, &[S::D1, S::F1, S::D2, S::E2, S::F2]),
                (
                    S::D4,
                    &[S::C3, S::D3, S::E3, S::C4, S::E4, S::C5, S::D5, S::E5],
                ),
                // a1 and h8 are indices 0 and 63, the easiest squares to leave out
                (
                    S::B2,
                    &[S::A1, S::B1, S::C1, S::A2, S::C2, S::A3, S::B3, S::C3],
                ),
                (
                    S::G7,
                    &[S::F6, S::G6, S::H6, S::F7, S::H7, S::F8, S::G8, S::H8],
                ),
            ],
        );
    }

    /// every target is an adjacent square, so a step that wraps around the
    /// board edge (h1 -> a2) fails here
    #[test]
    fn king_targets_are_adjacent() {
        for (from, to) in pairs(&KING_ATTACKS) {
            let (df, dr) = distance(from, to);
            assert_eq!(df.max(dr), 1, "{from} -> {to}");
        }
    }

    /// 4 corners * 3 + 24 edge squares * 5 + 36 inner squares * 8
    #[test]
    fn king_attack_count() {
        assert_eq!(total(&KING_ATTACKS), 420);
    }

    #[test]
    fn pawn_attacks_on_known_squares() {
        assert_attacks(
            &PAWN_ATTACKS[Color::White],
            &[
                (S::E4, &[S::D5, S::F5]),
                (S::A2, &[S::B3]),
                (S::H2, &[S::G3]),
                (S::G7, &[S::F8, S::H8]),
                // no white pawn stands on e1, but the table is also read backwards:
                // "is e1 attacked by a black pawn?" looks at white's attacks from e1
                (S::E1, &[S::D2, S::F2]),
                (S::E8, &[]),
            ],
        );
        assert_attacks(
            &PAWN_ATTACKS[Color::Black],
            &[
                (S::E5, &[S::D4, S::F4]),
                (S::A7, &[S::B6]),
                (S::H7, &[S::G6]),
                (S::B2, &[S::A1, S::C1]),
                (S::E8, &[S::D7, S::F7]),
                (S::E1, &[]),
            ],
        );
    }

    /// every target is one rank forward (for the pawn's color) and one file to the side
    #[test]
    fn pawn_targets_are_diagonal_steps_forward() {
        for (color, forward) in [(Color::White, 1), (Color::Black, -1)] {
            for (from, to) in pairs(&PAWN_ATTACKS[color]) {
                let df = (from.file() as i8 - to.file() as i8).abs();
                let dr = to.rank() as i8 - from.rank() as i8;
                assert_eq!((df, dr), (1, forward), "{color:?} pawn {from} -> {to}");
            }
        }
    }

    /// a white pawn on `a` attacks `b` exactly when a black pawn on `b` attacks `a`,
    /// which is what lets `is_square_attacked` read the tables backwards
    #[test]
    fn pawn_attacks_are_mirrored_between_colors() {
        let white = &PAWN_ATTACKS[Color::White];
        let black = &PAWN_ATTACKS[Color::Black];
        let attacks = |table: &[BitBoard; 64], from: Square, to: Square| {
            table[from] & to.bb() != BitBoard::EMPTY
        };
        for (from, to) in pairs(white) {
            assert!(attacks(black, to, from), "white {from} -> {to}");
        }
        for (from, to) in pairs(black) {
            assert!(attacks(white, to, from), "black {from} -> {to}");
        }
    }

    /// 7 ranks * 14 attacks (2 per square, 1 on the a and h files), none past the last rank
    #[test]
    fn pawn_attack_count() {
        for color in Color::ALL {
            assert_eq!(total(&PAWN_ATTACKS[color]), 98, "{color:?}");
        }
    }
}
