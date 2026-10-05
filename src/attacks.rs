use crate::{
    bitboard::BitBoard,
    piece::{Color, Piece, PieceKind},
    square::{Delta, Square},
};

pub static KNIGHT_ATTACKS: [BitBoard; 64] = knight_table();
pub static KING_ATTACKS: [BitBoard; 64] = king_table();
pub static PAWN_ATTACKS: [[BitBoard; 64]; 2] = [pawn_table(Color::White), pawn_table(Color::Black)];

pub fn bishop_attacks(sq: Square, occupied: BitBoard) -> BitBoard {
    directional_attacks(sq, occupied, Delta::NORTH_EAST)
        | directional_attacks(sq, occupied, Delta::NORTH_WEST)
        | directional_attacks(sq, occupied, Delta::SOUTH_EAST)
        | directional_attacks(sq, occupied, Delta::SOUTH_WEST)
}

pub fn rook_attacks(sq: Square, occupied: BitBoard) -> BitBoard {
    directional_attacks(sq, occupied, Delta::NORTH)
        | directional_attacks(sq, occupied, Delta::SOUTH)
        | directional_attacks(sq, occupied, Delta::EAST)
        | directional_attacks(sq, occupied, Delta::WEST)
}

pub fn attacks(piece: Piece, sq: Square, occupied: BitBoard) -> BitBoard {
    match piece.kind() {
        PieceKind::Pawn => PAWN_ATTACKS[piece.color()][sq],
        PieceKind::Knight => KNIGHT_ATTACKS[sq],
        PieceKind::Bishop => bishop_attacks(sq, occupied),
        PieceKind::Rook => rook_attacks(sq, occupied),
        PieceKind::Queen => bishop_attacks(sq, occupied) | rook_attacks(sq, occupied),
        PieceKind::King => KING_ATTACKS[sq],
    }
}

fn directional_attacks(mut sq: Square, occupied: BitBoard, delta: Delta) -> BitBoard {
    let mut bb = BitBoard::EMPTY;

    while let Some(new_sq) = sq.offset(delta) {
        let has_piece = occupied & new_sq.bb() != BitBoard::EMPTY;
        bb |= new_sq.bb();

        if has_piece {
            break;
        }
        sq = new_sq;
    }

    bb
}

const fn knight_table() -> [BitBoard; 64] {
    leaper_table(&[
        Delta::new(2, 1),
        Delta::new(2, -1),
        Delta::new(-2, 1),
        Delta::new(-2, -1),
        Delta::new(1, 2),
        Delta::new(1, -2),
        Delta::new(-1, 2),
        Delta::new(-1, -2),
    ])
}

const fn king_table() -> [BitBoard; 64] {
    leaper_table(&[
        Delta::NORTH,
        Delta::SOUTH,
        Delta::EAST,
        Delta::WEST,
        Delta::NORTH_EAST,
        Delta::NORTH_WEST,
        Delta::SOUTH_EAST,
        Delta::SOUTH_WEST,
    ])
}

const fn pawn_table(color: Color) -> [BitBoard; 64] {
    match color {
        Color::White => leaper_table(&[Delta::NORTH_EAST, Delta::NORTH_WEST]),
        Color::Black => leaper_table(&[Delta::SOUTH_EAST, Delta::SOUTH_WEST]),
    }
}

const fn leaper_table(deltas: &[Delta]) -> [BitBoard; 64] {
    let mut bbs = [BitBoard::EMPTY; 64];

    let mut square_idx: u8 = 0;

    while square_idx < 64 {
        let sq = Square::new(square_idx);
        let mut raw: u64 = 0;
        let mut idx = 0;

        while idx < deltas.len() {
            let delta = deltas[idx];
            if let Some(new_sq) = sq.offset(delta) {
                raw |= new_sq.bb().raw()
            }

            idx += 1;
        }

        bbs[sq.index()] = BitBoard::new(raw);
        square_idx += 1;
    }

    bbs
}

#[cfg(test)]
mod tests {
    use super::*;
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

    type SliderAttacks = fn(Square, BitBoard) -> BitBoard;

    /// attacks from every square with the given pieces on the board
    fn slider_table(attacks: SliderAttacks, occupied: BitBoard) -> [BitBoard; 64] {
        std::array::from_fn(|i| attacks(S::new(i as u8), occupied))
    }

    /// a square is attacked exactly when it's on one of the piece's lines and
    /// every square between them is empty, whatever is on the square itself
    fn assert_matches_definition(attacks: SliderAttacks, on_line: fn(i8, i8) -> bool) {
        for occupied in sample_occupancies() {
            let table = slider_table(attacks, occupied);
            for i in 0..64 {
                let from = S::new(i);
                for j in 0..64 {
                    let to = S::new(j);
                    let (df, dr) = distance(from, to);
                    let expected = on_line(df, dr) && is_clear(from, to, occupied);
                    let attacked = table[from] & to.bb() != BitBoard::EMPTY;
                    assert_eq!(attacked, expected, "{from} -> {to}, occupied:\n{occupied}");
                }
            }
        }
    }

    /// every square strictly between `from` and `to` is empty (they must be on the same line)
    fn is_clear(from: Square, to: Square, occupied: BitBoard) -> bool {
        let step_f = (to.file() as i8 - from.file() as i8).signum();
        let step_r = (to.rank() as i8 - from.rank() as i8).signum();
        let (mut f, mut r) = (from.file() as i8 + step_f, from.rank() as i8 + step_r);
        while (f, r) != (to.file() as i8, to.rank() as i8) {
            if occupied & S::from_file_and_rank(f as u8, r as u8).bb() != BitBoard::EMPTY {
                return false;
            }
            f += step_f;
            r += step_r;
        }
        true
    }

    /// a few fixed boards plus pseudo-random ones, from a fixed seed so failures repeat
    fn sample_occupancies() -> Vec<BitBoard> {
        let mut boards = vec![
            BitBoard::EMPTY,
            BitBoard::new(u64::MAX),
            BitBoard::RANK_2 | BitBoard::RANK_7,
        ];
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            // xorshift64
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..16 {
            let (a, b) = (next(), next());
            boards.push(BitBoard::new(a)); // about half the squares
            boards.push(BitBoard::new(a & b)); // about a quarter
        }
        boards
    }

    #[test]
    fn bishop_attacks_on_empty_board() {
        assert_attacks(
            &slider_table(bishop_attacks, BitBoard::EMPTY),
            &[
                (S::A1, &[S::B2, S::C3, S::D4, S::E5, S::F6, S::G7, S::H8]),
                (S::H1, &[S::G2, S::F3, S::E4, S::D5, S::C6, S::B7, S::A8]),
                (
                    S::D4,
                    &[
                        S::A1,
                        S::B2,
                        S::C3,
                        S::E5,
                        S::F6,
                        S::G7,
                        S::H8, // a1-h8
                        S::G1,
                        S::F2,
                        S::E3,
                        S::C5,
                        S::B6,
                        S::A7, // g1-a7
                    ],
                ),
            ],
        );
    }

    /// the ray includes the first occupied square, whatever its color, and stops there
    #[test]
    fn bishop_attacks_stop_at_blockers() {
        let occupied = S::F6.bb() | S::B2.bb();
        assert_attacks(
            &slider_table(bishop_attacks, occupied),
            &[(
                S::D4,
                &[
                    S::E5,
                    S::F6, // stops at f6, g7 and h8 are hidden
                    S::C3,
                    S::B2, // stops at b2, a1 is hidden
                    S::C5,
                    S::B6,
                    S::A7,
                    S::E3,
                    S::F2,
                    S::G1,
                ],
            )],
        );
    }

    /// 560 diagonal moves from all squares of an empty board
    #[test]
    fn bishop_attack_count_on_empty_board() {
        assert_eq!(total(&slider_table(bishop_attacks, BitBoard::EMPTY)), 560);
    }

    #[test]
    fn bishop_attacks_match_definition() {
        // same diagonal: as many files as ranks away
        assert_matches_definition(bishop_attacks, |df, dr| df == dr && df != 0);
    }

    #[test]
    fn rook_attacks_on_empty_board() {
        assert_attacks(
            &slider_table(rook_attacks, BitBoard::EMPTY),
            &[
                (
                    S::A1,
                    &[
                        S::B1,
                        S::C1,
                        S::D1,
                        S::E1,
                        S::F1,
                        S::G1,
                        S::H1, // rank 1
                        S::A2,
                        S::A3,
                        S::A4,
                        S::A5,
                        S::A6,
                        S::A7,
                        S::A8, // a file
                    ],
                ),
                (
                    S::D4,
                    &[
                        S::A4,
                        S::B4,
                        S::C4,
                        S::E4,
                        S::F4,
                        S::G4,
                        S::H4, // rank 4
                        S::D1,
                        S::D2,
                        S::D3,
                        S::D5,
                        S::D6,
                        S::D7,
                        S::D8, // d file
                    ],
                ),
            ],
        );
    }

    /// the ray includes the first occupied square, whatever its color, and stops there
    #[test]
    fn rook_attacks_stop_at_blockers() {
        let occupied = S::D6.bb() | S::B4.bb();
        assert_attacks(
            &slider_table(rook_attacks, occupied),
            &[(
                S::D4,
                &[
                    S::D5,
                    S::D6, // stops at d6, d7 and d8 are hidden
                    S::C4,
                    S::B4, // stops at b4, a4 is hidden
                    S::D3,
                    S::D2,
                    S::D1,
                    S::E4,
                    S::F4,
                    S::G4,
                    S::H4,
                ],
            )],
        );
    }

    /// on an empty board a rook always sees its whole rank and file: 64 * 14
    #[test]
    fn rook_attack_count_on_empty_board() {
        assert_eq!(total(&slider_table(rook_attacks, BitBoard::EMPTY)), 896);
    }

    #[test]
    fn rook_attacks_match_definition() {
        // same rank or same file, but not the same square
        assert_matches_definition(rook_attacks, |df, dr| (df == 0) != (dr == 0));
    }
}
