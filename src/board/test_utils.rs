//! Helpers shared by the tests of the board module and its submodules.

use super::Board;
use crate::chess_move::Move;

/// The perft reference positions: startpos, kiwipete, position 3 and position 4.
pub const PERFT_POSITIONS: [&str; 4] = [
    Board::STARTPOS,
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
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
