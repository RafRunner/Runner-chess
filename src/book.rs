//! An opening book in the Polyglot format (http://hgm.nubati.net/book_format.html): a
//! file of 16-byte entries, sorted by the key of their position, each a move and its
//! weight. The keys are the Polyglot ones, which [`ZobristHash`] already gives.
//!
//! ```text
//! key: u64 | move: u16 | weight: u16 | learn: u32   (all big-endian)
//! ```
//!
//! The engine's own book is built from the lines of `books/aggressive.txt`, see
//! [`Book::build`].

use std::{collections::HashMap, fmt, fs, io, path::Path};

use crate::{
    board::Board,
    chess_move::{AbstractMove, Move, MoveKind, PromotionPiece},
    piece::{Color, Piece, PieceKind},
    rng::Rng,
    square::Square,
};

const ENTRY_SIZE: usize = 16;

/// The lines of the engine's own book
pub const BUILTIN_LINES: &str = include_str!("../books/aggressive.txt");

const USAGE: &str = "usage: runner-chess book <lines> <out.bin>";

/// A line of a book's source that can't be read, from 1
#[derive(Debug, PartialEq, Eq)]
pub struct BuildError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

#[derive(Debug)]
pub enum BookError {
    Io(io::Error),
    /// The size isn't a whole number of entries
    Truncated(usize),
    /// The entries aren't sorted by key, so they couldn't be found
    Unsorted,
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Truncated(len) => {
                write!(
                    f,
                    "{len} bytes isn't a whole number of {ENTRY_SIZE}-byte entries"
                )
            }
            Self::Unsorted => write!(f, "the entries aren't sorted by key"),
        }
    }
}

impl From<io::Error> for BookError {
    fn from(e: io::Error) -> Self {
        BookError::Io(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    key: u64,
    mv: u16,
    weight: u16,
}

#[derive(Debug, Clone, Default)]
pub struct Book {
    /// Sorted by key, as in the file
    entries: Vec<Entry>,
}

impl Book {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BookError> {
        Self::from_bytes(&fs::read(path)?)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BookError> {
        if !bytes.len().is_multiple_of(ENTRY_SIZE) {
            return Err(BookError::Truncated(bytes.len()));
        }

        let entries: Vec<Entry> = bytes
            .as_chunks::<ENTRY_SIZE>()
            .0
            .iter()
            .map(|chunk| Entry {
                key: u64::from_be_bytes(chunk[0..8].try_into().unwrap()),
                mv: u16::from_be_bytes(chunk[8..10].try_into().unwrap()),
                weight: u16::from_be_bytes(chunk[10..12].try_into().unwrap()),
            })
            .collect();
        if !entries.is_sorted_by_key(|entry| entry.key) {
            return Err(BookError::Unsorted);
        }

        Ok(Self { entries })
    }

    /// The book of `lines`, each `<side> <weight> <moves in UCI>` from the start
    /// position, with `#` starting a comment. The moves of `side` (`w` or `b`) go in the
    /// book, and a move in a position gets the weight of every line that plays it there.
    pub fn build(lines: &str) -> Result<Self, BuildError> {
        let mut weights: HashMap<(u64, u16), u32> = HashMap::new();

        for (i, line) in lines.lines().enumerate() {
            let error = |message: String| BuildError {
                line: i + 1,
                message,
            };
            let line = line.split('#').next().unwrap_or_default();
            let mut tokens = line.split_whitespace();
            let Some(side) = tokens.next() else {
                continue;
            };
            let side = match side {
                "w" => Color::White,
                "b" => Color::Black,
                _ => return Err(error(format!("the side is w or b, not {side}"))),
            };
            let weight = tokens
                .next()
                .and_then(|weight| weight.parse::<u16>().ok())
                .filter(|&weight| weight > 0)
                .ok_or_else(|| error("the weight is a number from 1 to 65535".to_string()))?;

            let mut board = Board::startpos();
            for uci in tokens {
                let illegal = || error(format!("{uci} is illegal in {}", board.to_fen()));
                let mv = AbstractMove::from_uci(uci).map_err(|_| illegal())?;
                let (mv, next) = board.resolve_move(mv).map_err(|_| illegal())?;
                if board.side_to_move() == side {
                    let total = weights.entry((board.hash().raw(), encode(mv))).or_default();
                    *total += u32::from(weight);
                }
                board = next;
            }
        }

        let mut entries: Vec<Entry> = weights
            .into_iter()
            .map(|((key, mv), weight)| Entry {
                key,
                mv,
                weight: weight.min(u16::MAX.into()) as u16,
            })
            .collect();
        // the heaviest move first, as Polyglot sorts them
        entries.sort_by_key(|entry| (entry.key, std::cmp::Reverse(entry.weight), entry.mv));
        Ok(Self { entries })
    }

    /// The engine's own book, from [`BUILTIN_LINES`]
    pub fn builtin() -> Self {
        Self::build(BUILTIN_LINES).expect("the lines of the engine's book are tested")
    }

    /// The book as a Polyglot file, which [`Book::from_bytes`] reads back
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.entries.len() * ENTRY_SIZE);
        for entry in &self.entries {
            bytes.extend(entry.key.to_be_bytes());
            bytes.extend(entry.mv.to_be_bytes());
            bytes.extend(entry.weight.to_be_bytes());
            bytes.extend(0u32.to_be_bytes());
        }
        bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The legal moves the book has for `board`, with their weights, in the order of the
    /// file. Moves of weight 0 are there to be known, not played, so they're left out,
    /// and so are the illegal ones a collision of keys could bring.
    pub fn moves(&self, board: &Board) -> Vec<(Move, u16)> {
        let key = board.hash().raw();
        let start = self.entries.partition_point(|entry| entry.key < key);

        self.entries[start..]
            .iter()
            .take_while(|entry| entry.key == key)
            .filter(|entry| entry.weight > 0)
            .filter_map(|entry| {
                let (mv, _) = board.resolve_move(decode(entry.mv, board)?).ok()?;
                Some((mv, entry.weight))
            })
            .collect()
    }

    /// One of the moves for `board`, each as likely as its weight
    pub(crate) fn pick(&self, board: &Board, rng: &mut Rng) -> Option<Move> {
        let moves = self.moves(board);
        let total: u64 = moves.iter().map(|&(_, weight)| u64::from(weight)).sum();
        if total == 0 {
            return None;
        }

        let mut ticket = rng.next_u64() % total;
        for (mv, weight) in moves {
            match ticket.checked_sub(u64::from(weight)) {
                Some(rest) => ticket = rest,
                None => return Some(mv),
            }
        }
        unreachable!("the ticket is below the sum of the weights")
    }
}

/// A Polyglot move: the destination in bits 0-5, the origin in 6-11, as `8 * rank +
/// file`, and the promotion in 12-14 (0 none, 1 knight, 2 bishop, 3 rook, 4 queen).
/// Castling is the king taking its own rook, which becomes the king's real destination.
fn decode(raw: u16, board: &Board) -> Option<AbstractMove> {
    let from = Square::new(((raw >> 6) & 0b11_1111) as u8);
    let to = Square::new((raw & 0b11_1111) as u8);
    let promotion_piece = match (raw >> 12) & 0b111 {
        0 => None,
        piece @ 1..=4 => Some(PromotionPiece::ALL[usize::from(piece) - 1]),
        _ => return None,
    };

    let side = board.side_to_move();
    let king_takes_rook = board.piece_at(from) == Some(Piece::new(side, PieceKind::King))
        && board.piece_at(to) == Some(Piece::new(side, PieceKind::Rook));
    let to = match (from, to) {
        (Square::E1, Square::H1) if king_takes_rook => Square::G1,
        (Square::E1, Square::A1) if king_takes_rook => Square::C1,
        (Square::E8, Square::H8) if king_takes_rook => Square::G8,
        (Square::E8, Square::A8) if king_takes_rook => Square::C8,
        _ => to,
    };

    Some(AbstractMove::new(from, to, promotion_piece))
}

/// The Polyglot encoding of `mv`, as [`decode`] reads it
fn encode(mv: Move) -> u16 {
    let to = match (mv.kind(), mv.to()) {
        (MoveKind::Castle, _) => mv.get_rook_squares().0,
        (_, to) => to,
    };
    let promotion = match mv.kind() {
        MoveKind::Promotion(piece) | MoveKind::PromotionCapture(piece) => piece as u16 + 1,
        _ => 0,
    };
    (promotion << 12) | ((mv.from().index() as u16) << 6) | to.index() as u16
}

/// `runner-chess book <lines> <out.bin>`: writes the book of the lines, as
/// [`Book::build`] reads them, as a Polyglot file for other programs
pub fn run(args: &[String]) -> Result<(), String> {
    let [lines, out] = args else {
        return Err(USAGE.to_string());
    };
    let source = fs::read_to_string(lines).map_err(|e| format!("{lines}: {e}"))?;
    let book = Book::build(&source).map_err(|e| format!("{lines}: {e}"))?;
    fs::write(out, book.to_bytes()).map_err(|e| format!("{out}: {e}"))?;
    println!("{} entries written to {out}", book.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{CASTLING_BLACK, CASTLING_WHITE, board, play};

    /// A book file of `entries`: the move of each, in UCI, played from its board
    fn book_bytes(entries: &[(&Board, &str, u16)]) -> Vec<u8> {
        let mut entries: Vec<(u64, u16, u16)> = entries
            .iter()
            .map(|&(board, uci, weight)| {
                let mv = AbstractMove::from_uci(uci).unwrap();
                let (mv, _) = board.resolve_move(mv).unwrap();
                (board.hash().raw(), encode(mv), weight)
            })
            .collect();
        // the sort is stable, so the moves of a position keep their order
        entries.sort_by_key(|&(key, _, _)| key);

        let mut bytes = Vec::new();
        for (key, mv, weight) in entries {
            bytes.extend(key.to_be_bytes());
            bytes.extend(mv.to_be_bytes());
            bytes.extend(weight.to_be_bytes());
            bytes.extend(0u32.to_be_bytes());
        }
        bytes
    }

    fn book(entries: &[(&Board, &str, u16)]) -> Book {
        Book::from_bytes(&book_bytes(entries)).unwrap()
    }

    fn ucis(moves: &[(Move, u16)]) -> Vec<(String, u16)> {
        moves.iter().map(|&(mv, w)| (mv.to_uci(), w)).collect()
    }

    /// Written by hand from the format page, so that `encode` can't agree with
    /// `decode` on a wrong encoding
    #[test]
    fn moves_are_read_as_polyglot_writes_them() {
        let start = Board::startpos();
        let entry = |mv: u16| {
            let mut bytes = start.hash().raw().to_be_bytes().to_vec();
            bytes.extend(mv.to_be_bytes());
            bytes.extend([0, 1, 0, 0, 0, 0]);
            bytes
        };
        // e2e4: from 12, to 28
        let book = Book::from_bytes(&entry((12 << 6) | 28)).unwrap();
        assert_eq!(ucis(&book.moves(&start)), [("e2e4".to_string(), 1)]);

        // the king on e1 (4) takes its rook on h1 (7)
        let castling = board(CASTLING_WHITE);
        let mut bytes = castling.hash().raw().to_be_bytes().to_vec();
        bytes.extend(((4u16 << 6) | 7).to_be_bytes());
        bytes.extend([0, 1, 0, 0, 0, 0]);
        let book = Book::from_bytes(&bytes).unwrap();
        assert_eq!(ucis(&book.moves(&castling)), [("e1g1".to_string(), 1)]);
    }

    #[test]
    fn the_moves_of_a_position_come_with_their_weights() {
        let start = Board::startpos();
        let after_e4 = play(Board::STARTPOS, "e2e4");
        let book = book(&[
            (&start, "e2e4", 30),
            (&after_e4, "c7c5", 7),
            (&start, "d2d4", 10),
            // known but never played
            (&start, "g1f3", 0),
        ]);

        assert_eq!(book.len(), 4);
        assert_eq!(
            ucis(&book.moves(&start)),
            [("e2e4".to_string(), 30), ("d2d4".to_string(), 10)]
        );
        assert_eq!(ucis(&book.moves(&after_e4)), [("c7c5".to_string(), 7)]);
        // the same position by another order of moves
        let transposed = play(Board::STARTPOS, "g1f3 g8f6 b1c3");
        let book = self::book(&[(&play(Board::STARTPOS, "b1c3 g8f6 g1f3"), "e7e6", 1)]);
        assert_eq!(ucis(&book.moves(&transposed)), [("e7e6".to_string(), 1)]);
    }

    #[test]
    fn a_position_out_of_the_book_has_no_moves() {
        let book = book(&[(&Board::startpos(), "e2e4", 1)]);
        let out = play(Board::STARTPOS, "a2a3");
        assert!(book.moves(&out).is_empty());
        assert_eq!(book.pick(&out, &mut Rng(1)), None);
        assert_eq!(Book::default().pick(&Board::startpos(), &mut Rng(1)), None);
    }

    #[test]
    fn castling_is_the_king_taking_its_rook() {
        for (fen, uci, rook) in [
            (CASTLING_WHITE, "e1g1", Square::H1),
            (CASTLING_WHITE, "e1c1", Square::A1),
            (CASTLING_BLACK, "e8g8", Square::H8),
            (CASTLING_BLACK, "e8c8", Square::A8),
        ] {
            let board = board(fen);
            let book = book(&[(&board, uci, 1)]);
            let (mv, _) = book.moves(&board)[0];
            assert_eq!(mv.kind(), MoveKind::Castle, "{uci}");
            assert_eq!(mv.to_uci(), uci);
            assert_eq!(encode(mv) & 0b11_1111, rook.index() as u16, "{uci}");
        }
    }

    #[test]
    fn promotions_keep_their_piece() {
        let board = board("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1");
        let ucis_in = ["b7b8q", "b7b8n", "b7b8r", "b7b8b"];
        let entries: Vec<_> = ucis_in.iter().map(|&uci| (&board, uci, 1)).collect();
        let book = book(&entries);
        let read: Vec<String> = book
            .moves(&board)
            .iter()
            .map(|(mv, _)| mv.to_uci())
            .collect();
        assert_eq!(read, ucis_in);
    }

    /// A move that doesn't fit the position, as a collision of keys could bring
    #[test]
    fn illegal_moves_are_left_out() {
        let start = Board::startpos();
        let mut bytes = book_bytes(&[(&start, "e2e4", 1)]);
        let mut illegal = bytes.clone();
        // e2e5
        illegal[8..10].copy_from_slice(&((12u16 << 6) | 36).to_be_bytes());
        // a promotion code that doesn't exist
        let mut unknown = bytes.clone();
        unknown[8..10].copy_from_slice(&((5u16 << 12) | (12 << 6) | 28).to_be_bytes());
        bytes.extend(illegal);
        bytes.extend(unknown);

        let book = Book::from_bytes(&bytes).unwrap();
        assert_eq!(ucis(&book.moves(&start)), [("e2e4".to_string(), 1)]);
    }

    #[test]
    fn picks_follow_the_weights() {
        let start = Board::startpos();
        let book = book(&[
            (&start, "e2e4", 3),
            (&start, "d2d4", 1),
            (&start, "c2c4", 0),
        ]);
        let mut rng = Rng(1);

        let picks: Vec<String> = (0..4000)
            .map(|_| book.pick(&start, &mut rng).unwrap().to_uci())
            .collect();
        let e4 = picks.iter().filter(|&uci| uci == "e2e4").count();
        let d4 = picks.iter().filter(|&uci| uci == "d2d4").count();
        assert_eq!(e4 + d4, picks.len());
        assert!((2850..=3150).contains(&e4), "{e4} of 4000");
    }

    #[test]
    fn broken_files_are_refused() {
        let start = Board::startpos();
        let after_e4 = play(Board::STARTPOS, "e2e4");
        let bytes = book_bytes(&[(&start, "e2e4", 1), (&after_e4, "e7e5", 1)]);

        assert!(matches!(
            Book::from_bytes(&bytes[..20]),
            Err(BookError::Truncated(20))
        ));

        let mut swapped = bytes[16..].to_vec();
        swapped.extend(&bytes[..16]);
        assert!(matches!(
            Book::from_bytes(&swapped),
            Err(BookError::Unsorted)
        ));

        assert!(matches!(
            Book::open("/nonexistent/book.bin"),
            Err(BookError::Io(_))
        ));
        assert!(Book::from_bytes(&[]).unwrap().is_empty());
    }

    #[test]
    fn the_engines_book_builds() {
        let book = Book::builtin();
        assert!(book.len() > 300, "{}", book.len());
        // a Polyglot file of it reads back the same
        let read = Book::from_bytes(&book.to_bytes()).unwrap();
        assert_eq!(read.entries, book.entries);
        // only 1.e4 with white
        assert_eq!(ucis(&book.moves(&Board::startpos())).len(), 1);
    }

    #[test]
    fn a_built_book_has_the_moves_of_its_side_with_their_weights_added() {
        let book = Book::build(
            "# a comment\n\
             w 3 e2e4 e7e5 g1f3   # Nf3\n\
             \n\
             w 2 e2e4 e7e5 f2f4\n\
             b 5 e2e4 c7c5\n",
        )
        .unwrap();

        let start = Board::startpos();
        let after_e4 = play(Board::STARTPOS, "e2e4");
        let after_e5 = play(Board::STARTPOS, "e2e4 e7e5");
        assert_eq!(ucis(&book.moves(&start)), [("e2e4".to_string(), 5)]);
        // only the black line puts black's moves in the book
        assert_eq!(ucis(&book.moves(&after_e4)), [("c7c5".to_string(), 5)]);
        // the heaviest first
        assert_eq!(
            ucis(&book.moves(&after_e5)),
            [("g1f3".to_string(), 3), ("f2f4".to_string(), 2)]
        );
    }

    #[test]
    fn built_books_store_castling_as_polyglot_does() {
        let book = Book::build("w 1 e2e4 e7e5 g1f3 b8c6 f1c4 f8c5 e1g1").unwrap();
        let bytes = book.to_bytes();
        let castling = bytes
            .as_chunks::<ENTRY_SIZE>()
            .0
            .iter()
            .map(|entry| u16::from_be_bytes([entry[8], entry[9]]))
            .find(|&mv| mv >> 6 == 4)
            .unwrap();
        // e1 takes h1
        assert_eq!(castling & 0b11_1111, 7);
    }

    #[test]
    fn broken_lines_are_refused_with_their_number() {
        for (lines, line) in [
            ("w 1 e2e4\nx 1 e2e4", 2),
            ("w 0 e2e4", 1),
            ("w e2e4", 1),
            ("\n\nb 1 e2e4 e7e5 e5e4", 3),
            ("w 1 e2e9", 1),
        ] {
            assert_eq!(Book::build(lines).unwrap_err().line, line, "{lines}");
        }
    }
}
