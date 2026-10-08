//! Helpers shared by the tests of the board module and its submodules.

use super::Board;
use crate::chess_move::Move;

pub use crate::bench::{KIWIPETE, POSITION_3, POSITION_4, POSITION_5};

pub const PERFT_POSITIONS: [&str; 5] = [
    Board::STARTPOS,
    KIWIPETE,
    POSITION_3,
    POSITION_4,
    POSITION_5,
];

/// A board the test expects to be valid.
pub fn board(fen: &str) -> Board {
    Board::from_fen(fen).unwrap_or_else(|err| panic!("{fen}: {err:?}"))
}

pub fn moves_of(board: &Board) -> Vec<Move> {
    let mut moves = Vec::new();
    board.generate_moves(&mut moves);
    moves
}

/// The generated move written as `uci`, with the kind the generator gave it.
pub fn find_move(board: &Board, uci: &str) -> Move {
    moves_of(board)
        .into_iter()
        .find(|mv| mv.to_uci() == uci)
        .unwrap_or_else(|| panic!("{uci} was not generated in\n{board}"))
}
