//! Helpers shared by the tests of every module.

use crate::{
    bitboard::BitBoard,
    board::Board,
    chess_move::Move,
    eval::{self, Accumulators},
    score::Score,
    square::Square,
    zobrist::ZobristHash,
};

pub use crate::bench::{KIWIPETE, POSITION_3, POSITION_4, POSITION_5};

pub const PERFT_POSITIONS: [&str; 5] = [
    Board::STARTPOS,
    KIWIPETE,
    POSITION_3,
    POSITION_4,
    POSITION_5,
];

/// Kings and rooks on their squares, with every castling right
pub const CASTLING_WHITE: &str = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
pub const CASTLING_BLACK: &str = "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1";
/// Ra8 mates on the back rank
pub const MATE_IN_ONE: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
/// Black to move, checkmated
pub const MATED: &str = "7k/6Q1/6K1/8/8/8/8/8 b - - 0 1";
/// Black to move, stalemated
pub const STALEMATED: &str = "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1";
/// Black is lost, with the king shuffling around h8 against a queen
pub const LOST_KING: &str = "6k1/8/8/8/8/8/8/KQ6 b - - 0 1";

/// A board the test expects to be valid.
pub fn board(fen: &str) -> Board {
    Board::from_fen(fen).unwrap_or_else(|err| panic!("{fen}: {err:?}"))
}

/// The search's evaluation of `board`, from fresh accumulators
pub fn static_eval(board: &Board) -> Score {
    eval::evaluate(board, &Accumulators::new(board))
}

pub fn moves_of(board: &Board) -> Vec<Move> {
    board.generate_moves()
}

/// The generated move written as `uci`, with the kind the generator gave it.
pub fn find_move(board: &Board, uci: &str) -> Move {
    moves_of(board)
        .into_iter()
        .find(|mv| mv.to_uci() == uci)
        .unwrap_or_else(|| panic!("{uci} was not generated in\n{board}"))
}

/// The board after playing the space separated `moves` from `fen`.
pub fn play(fen: &str, moves: &str) -> Board {
    play_with_history(fen, moves).0
}

/// The same, with the game behind it: the hash of every position before each move.
pub fn play_with_history(fen: &str, moves: &str) -> (Board, Vec<ZobristHash>) {
    let mut board = board(fen);
    let mut history = Vec::new();
    for uci in moves.split_whitespace() {
        history.push(board.hash());
        board = board.make_move(find_move(&board, uci));
    }
    (board, history)
}

/// The legal moves of `fen` in UCI, sorted.
pub fn legal_uci(fen: &str) -> Vec<String> {
    sorted(
        board(fen)
            .legal_successors()
            .map(|(mv, _)| mv.to_uci())
            .collect(),
    )
}

/// Sorted, so failures print in a predictable order.
pub fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

/// The squares of `bb`, from a1 to h8.
pub fn squares(bb: BitBoard) -> Vec<Square> {
    bb.collect()
}

/// `fen` and its mirror, so every rule is checked for both colors.
pub fn both_colors(fen: &str) -> [String; 2] {
    [fen.to_string(), mirror_fen(fen)]
}

/// The same move seen from the other side of the board: e2e4 becomes e7e5.
pub fn mirror_uci(uci: &str) -> String {
    uci.chars()
        .map(|c| match c {
            '1'..='8' => (b'1' + b'8' - c as u8) as char,
            _ => c,
        })
        .collect()
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
