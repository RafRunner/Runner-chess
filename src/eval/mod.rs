mod nnue;

pub use nnue::Accumulators;

use crate::{bitboard::BitBoard, board::Board, piece::PieceKind, score::Score};

/// net-002 gives about twice the scores it learned from, as it mostly predicts results
const NETWORK_INFLATION: i32 = 2;

/// For the side to move: the network, except in dead positions
pub fn evaluate(board: &Board, accumulators: &Accumulators) -> Score {
    if is_dead_position(board) {
        return Score::DRAW;
    }
    Score::new(accumulators.evaluate(board.side_to_move()) / NETWORK_INFLATION)
}

/// No sequence of legal moves can end in mate: besides the kings, at most one minor
/// piece, or only bishops, all on squares of the same color.
pub fn is_dead_position(board: &Board) -> bool {
    let both = |kind: PieceKind| board.both(kind);

    if both(PieceKind::Pawn) | both(PieceKind::Rook) | both(PieceKind::Queen) != BitBoard::EMPTY {
        return false;
    }

    let (knights, bishops) = (both(PieceKind::Knight), both(PieceKind::Bishop));
    let one_color = (bishops & BitBoard::LIGHT_SQUARES).is_empty()
        || (bishops & !BitBoard::LIGHT_SQUARES).is_empty();

    (knights | bishops).count_ones() <= 1 || (knights.is_empty() && one_color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{PERFT_POSITIONS, board, mirror_fen, static_eval};

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
        let (b, w) = (static_eval(&board(better)), static_eval(&board(worse)));
        assert!(b > w, "expected {better} ({b:?}) > {worse} ({w:?})");

        let (better, worse) = (mirror_fen(better), mirror_fen(worse));
        let (b, w) = (static_eval(&board(&better)), static_eval(&board(&worse)));
        assert!(b > w, "expected {better} ({b:?}) > {worse} ({w:?})");
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
}
