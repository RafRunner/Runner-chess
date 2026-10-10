//! Helpers shared by the tests of every module.

use crate::{board::Board, chess_move::Move};

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

/// The same position with the colors swapped and the board flipped vertically.
pub fn mirror_fen(fen: &str) -> String {
    let fields: Vec<&str> = fen.split_whitespace().collect();
    let swap_case = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_uppercase() {
                    c.to_ascii_lowercase()
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect()
    };

    let placement = fields[0]
        .split('/')
        .rev()
        .map(swap_case)
        .collect::<Vec<_>>()
        .join("/");
    let side = if fields[1] == "w" { "b" } else { "w" };
    let castling = match fields[2] {
        "-" => "-".to_string(),
        c => {
            let swapped = swap_case(c);
            let white: String = swapped.chars().filter(|c| c.is_ascii_uppercase()).collect();
            let black: String = swapped.chars().filter(|c| c.is_ascii_lowercase()).collect();
            white + &black
        }
    };
    let en_passant = match fields[3] {
        "-" => "-".to_string(),
        ep => {
            let (file, rank) = ep.split_at(1);
            format!("{file}{}", if rank == "3" { "6" } else { "3" })
        }
    };

    format!(
        "{placement} {side} {castling} {en_passant} {} {}",
        fields[4], fields[5]
    )
}
