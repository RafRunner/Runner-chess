mod nnue;

pub use nnue::{AccumulatorStack, Accumulators, NETWORK_NAME};

use crate::{
    bitboard::BitBoard,
    board::Board,
    piece::{Color, Piece, PieceKind},
    score::Score,
    square::Square,
};

/// net-008 gives about 1.21x the scores it learned from, as half of what it learned is
/// the result of the game
const NETWORK_INFLATION: i32 = 121;

/// For the side to move: the network, except in dead positions
pub fn evaluate(board: &Board, accumulators: &Accumulators) -> Score {
    let eval = || Score::new(accumulators.evaluate(board) * 100 / NETWORK_INFLATION);

    match board_state(board) {
        BoardState::WithMajorOrPawns => eval(),
        BoardState::OnlyMinor => {
            let base = eval();
            if base.is_mate() {
                return base;
            }

            if let Some(bonus) = bishop_and_knight_mate(board) {
                Score::new(base.centipawns().expect("we checked mate") as i32 + bonus)
            } else {
                base
            }
        }
        BoardState::Dead => Score::DRAW,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BoardState {
    WithMajorOrPawns,
    OnlyMinor,
    Dead,
}

/// No sequence of legal moves can end in mate: besides the kings, at most one minor
/// piece, or only bishops, all on squares of the same color.
pub fn board_state(board: &Board) -> BoardState {
    let both = |kind: PieceKind| board.both(kind);

    if both(PieceKind::Pawn) | both(PieceKind::Rook) | both(PieceKind::Queen) != BitBoard::EMPTY {
        return BoardState::WithMajorOrPawns;
    }

    let (knights, bishops) = (both(PieceKind::Knight), both(PieceKind::Bishop));
    let one_color = (bishops & BitBoard::LIGHT_SQUARES).is_empty()
        || (bishops & !BitBoard::LIGHT_SQUARES).is_empty();

    if (knights | bishops).count_ones() <= 1 || (knights.is_empty() && one_color) {
        BoardState::Dead
    } else {
        BoardState::OnlyMinor
    }
}

/// Returns a bonus in cases where it's B+N vs K to guide the engine into checkmate.
/// We assume we're not in a dead position
fn bishop_and_knight_mate(board: &Board) -> Option<i32> {
    Color::ALL
        .iter()
        .cloned()
        .find(|&color| {
            (board.by_color(color) & !board.pieces(Piece::new(color, PieceKind::King))).is_empty()
        })
        .and_then(|mated| {
            let bishops = board.pieces(Piece::new(mated.oposite(), PieceKind::Bishop));
            if bishops.count_ones() == 1 {
                Some((
                    mated,
                    bishops.into_iter().next().expect("we checked there is one"),
                ))
            } else {
                None
            }
        })
        .map(|(mated, bishop_sq)| {
            // Here, it must be B+N vs K
            let mating = mated.oposite();
            let our_perspective = mating == board.side_to_move();

            let mating_king = board.king(mating);
            let mated_king = board.king(mated);

            let (corner_1, corner_2) = if bishop_sq.is_light() {
                (Square::H1, Square::A8)
            } else {
                (Square::A1, Square::H8)
            };
            let corner_dst = mated_king
                .manhattan_distance(corner_1)
                .min(mated_king.manhattan_distance(corner_2)) as i32;
            let king_dst = mating_king.distance(mated_king) as i32;

            let bonus = 30 * (7 - corner_dst) + 15 * (7 - king_dst);

            if our_perspective { bonus } else { -bonus }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{PERFT_POSITIONS, board, both_colors, mirror_fen, static_eval};

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
        for (better, worse) in both_colors(better).iter().zip(&both_colors(worse)) {
            let (b, w) = (static_eval(&board(better)), static_eval(&board(worse)));
            assert!(b > w, "expected {better} ({b:?}) > {worse} ({w:?})");
        }
    }

    // properties

    #[test]
    fn mirrored_positions_have_the_same_eval() {
        for fen in PERFT_POSITIONS {
            let mirrored = mirror_fen(fen);
            assert_eq!(
                static_eval(&board(fen)),
                static_eval(&board(&mirrored)),
                "{fen} vs {mirrored}"
            );
        }
    }

    // material

    #[test]
    fn any_extra_piece_is_an_advantage() {
        // with a pawn on the board, since a lone minor piece is a dead draw
        for piece in ["P", "N", "B", "R", "Q"] {
            assert_better(
                &format!("4k3/7p/8/8/8/3{piece}4/8/4K3 w - - 0 1"),
                "4k3/7p/8/8/8/8/8/4K3 w - - 0 1",
            );
        }
    }

    // draws by insufficient material

    /// `fen` for both colors and both sides to move.
    fn every_side(fen: &str) -> [String; 4] {
        let mirrored = mirror_fen(fen);
        [
            fen.to_string(),
            flip_side(fen),
            flip_side(&mirrored),
            mirrored,
        ]
    }

    #[test]
    fn dead_positions_are_draws() {
        // no sequence of moves ends in mate, which the network alone doesn't know
        #[rustfmt::skip]
        let dead = [
            "k7/8/8/8/3K4/8/8/8 w - - 0 1",    // K vs K
            "k7/8/8/8/3K4/8/5N2/8 w - - 0 1",  // K+N vs K
            "k7/8/8/8/3K4/8/5B2/8 w - - 0 1",  // K+B vs K
            "kb6/8/8/8/3K4/8/5B2/8 w - - 0 1", // K+B vs K+B, both on dark squares
        ];
        for fen in dead.iter().flat_map(|fen| every_side(fen)) {
            assert_eq!(static_eval(&board(&fen)), Score::DRAW, "{fen}");
        }
    }

    #[test]
    fn mate_is_still_possible() {
        // even if only with the help of the other side
        #[rustfmt::skip]
        let alive = [
            "k7/8/8/8/3K4/8/5P2/8 w - - 0 1",  // K+P vs K
            "k7/8/8/8/3K4/8/5R2/8 w - - 0 1",  // K+R vs K
            "k7/8/8/8/3K4/8/5Q2/8 w - - 0 1",  // K+Q vs K
            "k7/8/8/8/3K4/8/5BN1/8 w - - 0 1", // K+B+N vs K
            "k7/8/8/8/3K4/8/4BB2/8 w - - 0 1", // K+B+B vs K, one bishop on each color
            "kb6/8/8/8/3K4/8/4B3/8 w - - 0 1", // K+B vs K+B on opposite colors
            "k7/p7/8/8/3K4/8/5N2/8 w - - 0 1", // K+N vs K+P: the pawn can box its king in
        ];
        for fen in alive.iter().flat_map(|fen| every_side(fen)) {
            assert_ne!(static_eval(&board(&fen)), Score::DRAW, "{fen}");
        }
    }

    #[test]
    fn the_material_tells_the_state_of_the_board() {
        #[rustfmt::skip]
        let cases = [
            ("k7/8/8/8/3K4/8/5P2/8 w - - 0 1", BoardState::WithMajorOrPawns), // K+P vs K
            ("k7/8/8/8/3K4/8/5R2/8 w - - 0 1", BoardState::WithMajorOrPawns), // K+R vs K
            ("k7/8/8/8/3K4/8/5BN1/8 w - - 0 1", BoardState::OnlyMinor),       // K+B+N vs K
            ("k7/8/8/8/3K4/8/5NN1/8 w - - 0 1", BoardState::OnlyMinor),       // K+N+N vs K
            ("kb6/8/8/8/3K4/8/4B3/8 w - - 0 1", BoardState::OnlyMinor),       // bishops of both colors
            ("k7/8/8/8/3K4/8/8/8 w - - 0 1", BoardState::Dead),               // K vs K
            ("kb6/8/8/8/3K4/8/5B2/8 w - - 0 1", BoardState::Dead),            // dark bishops only
        ];
        for (fen, state) in cases {
            for fen in every_side(fen) {
                assert_eq!(board_state(&board(&fen)), state, "{fen}");
            }
        }
    }

    // the mate with bishop and knight

    // White's king on e4 is as far from a1 as from a8, the knight on g1
    const DARK_BISHOP_RIGHT_CORNER: &str = "8/8/8/8/4K3/8/8/k1B3N1 w - - 0 1";
    const DARK_BISHOP_WRONG_CORNER: &str = "k7/8/8/8/4K3/8/8/2B3N1 w - - 0 1";
    const LIGHT_BISHOP_RIGHT_CORNER: &str = "k7/8/8/8/4K3/8/8/5BN1 w - - 0 1";
    const LIGHT_BISHOP_WRONG_CORNER: &str = "8/8/8/8/4K3/8/8/k4BN1 w - - 0 1";

    fn bonus(fen: &str) -> i32 {
        bishop_and_knight_mate(&board(fen)).expect("bishop and knight against a lone king")
    }

    #[test]
    fn the_bonus_goes_to_the_side_that_mates() {
        for fen in [
            DARK_BISHOP_RIGHT_CORNER,
            DARK_BISHOP_WRONG_CORNER,
            LIGHT_BISHOP_RIGHT_CORNER,
            LIGHT_BISHOP_WRONG_CORNER,
        ] {
            let mating = bonus(fen);
            assert!(mating > 0, "{fen}");
            for fen in both_colors(fen) {
                assert_eq!(bonus(&fen), mating, "{fen}");
                assert_eq!(bonus(&flip_side(&fen)), -mating, "{fen}");
            }
        }
    }

    #[test]
    fn the_bonus_pushes_the_king_to_a_corner_of_the_bishops_color() {
        for (right, wrong) in [
            (DARK_BISHOP_RIGHT_CORNER, DARK_BISHOP_WRONG_CORNER),
            (LIGHT_BISHOP_RIGHT_CORNER, LIGHT_BISHOP_WRONG_CORNER),
        ] {
            for (right, wrong) in both_colors(right).iter().zip(&both_colors(wrong)) {
                assert!(bonus(right) > bonus(wrong), "{right} against {wrong}");
            }
        }
    }

    #[test]
    fn the_bonus_brings_the_kings_together() {
        // White's king on c3 instead of e4
        let closer = "8/8/8/8/8/2K5/8/k1B3N1 w - - 0 1";
        for (closer, farther) in both_colors(closer)
            .iter()
            .zip(&both_colors(DARK_BISHOP_RIGHT_CORNER))
        {
            assert!(bonus(closer) > bonus(farther), "{closer} against {farther}");
        }
    }

    #[test]
    fn the_bonus_is_only_for_bishop_and_knight_against_a_lone_king() {
        #[rustfmt::skip]
        let others = [
            "k7/8/8/8/4K3/8/8/2N3N1 w - - 0 1",  // two knights
            "k7/8/8/8/4K3/8/8/2B2B2 w - - 0 1",  // two bishops
            "kn6/8/8/8/4K3/8/8/2B3N1 w - - 0 1", // the other king isn't alone
        ];
        for fen in others.iter().flat_map(|fen| every_side(fen)) {
            assert_eq!(bishop_and_knight_mate(&board(&fen)), None, "{fen}");
        }
    }
}
