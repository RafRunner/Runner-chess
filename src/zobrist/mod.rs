use crate::{board::Board, castling::CastlingRights, piece::Color, zobrist::random64::RANDOM64};

mod random64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZobristHash(u64);

impl ZobristHash {
    const CASTLE_OFFSET: usize = 768;
    const EP_OFFSET: usize = 772;
    const TO_MOVE_OFFSET: usize = 780;

    pub fn hash(board: &Board) -> Self {
        let us = board.side_to_move();
        let them = us.oposite();

        let mut piece_hash = 0;

        for sq in board.all_pieces_bb() {
            let piece = board.piece_at(sq).expect("There should be a piece here");
            let kind_of_piece =
                piece.kind().index() * 2 + if piece.color() == Color::White { 1 } else { 0 };

            piece_hash ^= RANDOM64[sq.index() + kind_of_piece * 64];
        }

        let mut castle_hash = 0;
        let castling = board.castling();

        if castling.has(CastlingRights::WHITE_SHORT) {
            castle_hash ^= RANDOM64[Self::CASTLE_OFFSET]
        }
        if castling.has(CastlingRights::WHITE_LONG) {
            castle_hash ^= RANDOM64[Self::CASTLE_OFFSET + 1]
        }
        if castling.has(CastlingRights::BLACK_SHORT) {
            castle_hash ^= RANDOM64[Self::CASTLE_OFFSET + 2]
        }
        if castling.has(CastlingRights::BLACK_LONG) {
            castle_hash ^= RANDOM64[Self::CASTLE_OFFSET + 3]
        }

        let mut ep_hash = 0;

        if let Some(sq) = board.en_passant() {
            let file = sq.file();
            ep_hash = RANDOM64[Self::EP_OFFSET + file as usize];
        }

        let turn_hash = if us == Color::White {
            RANDOM64[Self::TO_MOVE_OFFSET]
        } else {
            0
        };

        Self(piece_hash ^ castle_hash ^ ep_hash ^ turn_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{square::Square, test_utils::board};

    // The offsets below are written as the book format page gives them, instead of
    // reusing the constants, so a wrong constant can't hide behind its own test.

    /// The test positions from http://hgm.nubati.net/book_format.html
    #[rustfmt::skip]
    const POLYGLOT_KEYS: [(&str, &str, &str); 9] = [
        ("startpos", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", "463b96181691fc9c"),
        ("e2e4", "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1", "823c9b50fd114196"),
        ("e2e4 d7d5", "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2", "0756b94461c50fb0"),
        ("e2e4 d7d5 e4e5", "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2", "662fafb965db29d4"),
        ("e2e4 d7d5 e4e5 f7f5", "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3", "22a48b5a8e47ff78"),
        ("e2e4 d7d5 e4e5 f7f5 e1e2", "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPPKPPP/RNBQ1BNR b kq - 0 3", "652a607ca3f242c1"),
        ("e2e4 d7d5 e4e5 f7f5 e1e2 e8f7", "rnbq1bnr/ppp1pkpp/8/3pPp2/8/8/PPPPKPPP/RNBQ1BNR w - - 0 4", "00fdd303c946bdd9"),
        ("a2a4 b7b5 h2h4 b5b4 c2c4", "rnbqkbnr/p1pppppp/8/8/PpP4P/8/1P1PPPP1/RNBQKBNR b KQkq c3 0 3", "3c8123ea7b067637"),
        ("a2a4 b7b5 h2h4 b5b4 c2c4 b4c3 a1a3", "rnbqkbnr/p1pppppp/8/8/P6P/R1p5/1P1PPPP1/1NBQKBNR b Kkq - 0 4", "5c3f9b829b279560"),
    ];

    /// The pieces in the order of their keys, so the position of a piece here is its
    /// `kind_of_piece`: black pawn 0, white pawn 1, black knight 2, ..., white king 11.
    const POLYGLOT_PIECES: &str = "pPnNbBrRqQkK";

    fn hash(fen: &str) -> u64 {
        ZobristHash::hash(&board(fen)).0
    }

    /// The kings on e1 and e8, plus `piece` on `at`, white to move.
    fn kings_and(piece: char, at: Square) -> String {
        let mut placement = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let sq = Square::from_file_and_rank(file, rank);
                let found = match sq {
                    _ if sq == at => piece,
                    Square::E1 => 'K',
                    Square::E8 => 'k',
                    _ => {
                        empty += 1;
                        continue;
                    }
                };
                if empty > 0 {
                    placement += &empty.to_string();
                    empty = 0;
                }
                placement.push(found);
            }
            if empty > 0 {
                placement += &empty.to_string();
            }
            if rank > 0 {
                placement.push('/');
            }
        }
        format!("{placement} w - - 0 1")
    }

    #[test]
    fn polyglot_reference_keys() {
        for (moves, fen, key) in POLYGLOT_KEYS {
            assert_eq!(format!("{:016x}", hash(fen)), key, "after {moves}");
        }
    }

    #[test]
    fn each_piece_on_each_square_has_its_own_key() {
        let kings = hash("4k3/8/8/8/8/8/8/4K3 w - - 0 1");

        // the kings are already on the board, the other pieces go everywhere they can
        for piece in POLYGLOT_PIECES.chars().filter(|p| !"kK".contains(*p)) {
            let kind = POLYGLOT_PIECES.find(piece).unwrap();
            for rank in 0..8 {
                for file in 0..8 {
                    let sq = Square::from_file_and_rank(file, rank);
                    // the kings' squares, pawns on the last ranks, checks to black
                    let Ok(board) = Board::from_fen(&kings_and(piece, sq)) else {
                        continue;
                    };
                    let key = RANDOM64[64 * kind + 8 * rank as usize + file as usize];
                    assert_eq!(ZobristHash::hash(&board).0 ^ kings, key, "{piece} on {sq}");
                }
            }
        }
    }

    #[test]
    fn each_castling_right_has_its_own_key() {
        let fen = |rights: &str| format!("r3k2r/8/8/8/8/8/8/R3K2R w {rights} - 0 1");
        let all = hash(&fen("KQkq"));

        for (right, others, offset) in [
            ('K', "Qkq", 768),
            ('Q', "Kkq", 769),
            ('k', "KQq", 770),
            ('q', "KQk", 771),
        ] {
            assert_eq!(all ^ hash(&fen(others)), RANDOM64[offset], "{right}");
        }
    }

    #[test]
    fn white_to_move_has_its_own_key() {
        let white = hash("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
        let black = hash("4k3/8/8/8/8/8/8/4K3 b - - 0 1");
        assert_eq!(white ^ black, RANDOM64[780]);
    }

    /// The en passant key is the one of the file of the pushed pawn, and only if a pawn
    /// of the side to move is beside it.
    #[test]
    fn en_passant_counts_only_when_a_pawn_can_take() {
        let without_ep = |fen: &str| {
            let mut fields: Vec<&str> = fen.split_whitespace().collect();
            fields[3] = "-";
            fields.join(" ")
        };
        let ep_key = |fen: &str| hash(fen) ^ hash(&without_ep(fen));

        let not_hashed = [
            // 1.e4: no black pawn beside e4
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
            // 1.e4 d5: the pawn on e4 can take on d5, but it isn't beside it
            "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2",
            // the pawn beside d4 is white, like the one that was pushed
            "4k3/8/8/8/3PP3/8/8/4K3 b - d3 0 1",
            // a black pawn diagonal to e3, but behind it instead of beside e4
            "4k3/8/8/8/4P3/8/3p4/K7 b - e3 0 1",
        ];
        for fen in not_hashed {
            assert_eq!(ep_key(fen), 0, "{fen}");
        }

        let hashed = [
            // 1.e4 d5 2.e5 f5: the pawn on e5 can take on f6
            (
                "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
                772 + 5,
            ),
            // on the edges of the board
            ("4k3/8/8/pP6/8/8/8/4K3 w - a6 0 1", 772),
            ("4k3/8/8/8/6pP/8/8/4K3 b - h3 0 1", 772 + 7),
            // Polyglot doesn't check if the capture is legal: here it would leave the
            // black king in check from the rook
            ("8/8/8/8/R2Pp2k/8/8/4K3 b - d3 0 1", 772 + 3),
        ];
        for (fen, offset) in hashed {
            assert_eq!(ep_key(fen), RANDOM64[offset], "{fen}");
        }
    }
}
