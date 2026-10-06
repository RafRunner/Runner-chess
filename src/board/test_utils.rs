//! Helpers shared by the tests of the board module and its submodules.

use super::Board;
use crate::chess_move::Move;

// the perft reference positions from https://www.chessprogramming.org/Perft_Results
pub const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
pub const POSITION_3: &str = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";
pub const POSITION_4: &str = "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1";
pub const POSITION_5: &str = "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8";

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
