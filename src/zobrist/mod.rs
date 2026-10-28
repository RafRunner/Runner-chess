mod random64;

use random64::RANDOM64;

use crate::{
    board::Board,
    castling::CastlingRights,
    piece::{Color, Piece},
    square::Square,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZobristHash(u64);

impl ZobristHash {
    const CASTLE_OFFSET: usize = 768;
    const EP_OFFSET: usize = 772;
    const TO_MOVE_OFFSET: usize = 780;

    pub fn empty() -> Self {
        Self(0)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    pub fn hash(board: &Board) -> Self {
        let mut hash = Self(0);

        for sq in board.all_pieces_bb() {
            let piece = board.piece_at(sq).expect("There should be a piece here");
            hash = hash.toggle_piece_key(piece, sq);
        }

        hash = hash.toggle_castling_key(board.castling());

        if let Some(sq) = board.en_passant() {
            hash = hash.toggle_en_passant_key(sq);
        }

        if board.side_to_move() == Color::White {
            hash = hash.toggle_turn_key();
        }

        hash
    }

    #[must_use]
    pub fn toggle_piece_key(self, piece: Piece, sq: Square) -> Self {
        let raw = self.0;
        let kind_of_piece =
            piece.kind().index() * 2 + if piece.color() == Color::White { 1 } else { 0 };

        Self(raw ^ RANDOM64[sq.index() + kind_of_piece * 64])
    }

    #[must_use]
    pub fn toggle_castling_key(self, castling: CastlingRights) -> Self {
        let mut castle_hash = 0;

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

        Self(self.0 ^ castle_hash)
    }

    #[must_use]
    pub fn toggle_en_passant_key(self, ep_sq: Square) -> Self {
        let file = ep_sq.file();
        Self(self.0 ^ RANDOM64[Self::EP_OFFSET + file as usize])
    }

    #[must_use]
    pub fn toggle_turn_key(self) -> Self {
        Self(self.0 ^ RANDOM64[Self::TO_MOVE_OFFSET])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{CASTLING_WHITE, PERFT_POSITIONS, board, play};

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

    /// The same keys, but updated move by move by make_move instead of computed from
    /// the FEN. The last sequence ends with an en passant capture.
    #[test]
    fn incremental_hash_matches_polyglot_keys() {
        for (moves, _, key) in POLYGLOT_KEYS {
            let b = play(Board::STARTPOS, moves.trim_start_matches("startpos"));
            assert_eq!(format!("{:016x}", b.hash().0), key, "after {moves}");
        }
    }

    /// Playing the moves from the start and reading the FEN they reach give the same
    /// hash, the first updated by make_move and the second computed from scratch.
    #[test]
    fn played_moves_and_fen_have_the_same_hash() {
        for (moves, fen, _) in POLYGLOT_KEYS {
            let played = play(Board::STARTPOS, moves.trim_start_matches("startpos"));
            assert_eq!(played.hash(), board(fen).hash(), "after {moves}");
        }

        #[rustfmt::skip]
        let games = [
            // a promotion capturing the a8 rook, which also takes away black's long castle
            (
                "e2e4 d7d5 e4d5 c7c6 d5c6 g8f6 c6b7 e7e6 b7a8q",
                "Qnbqkb1r/p4ppp/4pn2/8/8/8/PPPP1PPP/RNBQKBNR b KQk - 0 5",
            ),
            // en passant, then both sides castle
            (
                "e2e4 g8f6 e4e5 d7d5 e5d6 e7d6 g1f3 f8e7 f1c4 e8g8 e1g1",
                "rnbq1rk1/ppp1bppp/3p1n2/8/2B5/5N2/PPPP1PPP/RNBQ1RK1 b - - 5 6",
            ),
        ];
        for (moves, fen) in games {
            let played = play(Board::STARTPOS, moves);
            // the FEN is written by hand, so check it's really where the moves lead
            assert_eq!(played.to_fen(), fen, "after {moves}");
            assert_eq!(played.hash(), board(fen).hash(), "after {moves}");
        }
    }

    #[test]
    fn clocks_do_not_change_the_hash() {
        for fen in PERFT_POSITIONS {
            let fields: Vec<&str> = fen.split_whitespace().take(4).collect();
            let other_clocks = format!("{} 37 80", fields.join(" "));
            assert_eq!(board(fen).hash(), board(&other_clocks).hash(), "{fen}");
        }

        // the knights go out and come back: the start position again, 4 halfmoves later
        let back = play(Board::STARTPOS, "g1f3 g8f6 f3g1 f6g8");
        assert_eq!(
            back.to_fen(),
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 4 3"
        );
        assert_eq!(back.hash(), board(Board::STARTPOS).hash());
    }

    #[test]
    fn transpositions_have_the_same_hash() {
        #[rustfmt::skip]
        let cases = [
            (Board::STARTPOS, "g1f3 g8f6 b1c3 b8c6", "b1c3 b8c6 g1f3 g8f6"),
            // taking en passant or normally ends with the same pawn on d6
            ("4k3/3p4/8/4P3/8/8/8/4K3 b - - 0 1", "d7d5 e5d6 e8d8", "d7d6 e5d6 e8d8"),
            // castling or walking the king and the rook to the same squares
            (CASTLING_WHITE, "e1g1 e8d8", "e1e2 a8b8 h1f1 b8a8 e2f2 e8e7 f2g1 e7d8"),
        ];
        for (fen, a, b) in cases {
            let (a_board, b_board) = (play(fen, a), play(fen, b));
            assert_eq!(
                a_board.to_fen().split(' ').take(4).collect::<Vec<_>>(),
                b_board.to_fen().split(' ').take(4).collect::<Vec<_>>(),
                "{a} / {b}: not the same position"
            );
            assert_eq!(a_board.hash(), b_board.hash(), "{a} / {b}");
        }
    }

    /// The kings come back to e1 and e8, but the castling rights don't.
    #[test]
    fn same_pieces_with_different_castling_have_different_hashes() {
        let walked = play(Board::STARTPOS, "e2e4 e7e5 e1e2 e8e7 e2e1 e7e8");
        let knights = play(Board::STARTPOS, "e2e4 e7e5 g1f3 g8f6 f3g1 f6g8");
        assert_eq!(
            walked.to_fen().split(' ').next(),
            knights.to_fen().split(' ').next()
        );
        assert_ne!(walked.castling(), knights.castling());
        assert_ne!(walked.hash(), knights.hash());
    }

    /// The same pieces, but only in one of them the pawn on e5 can take en passant.
    #[test]
    fn same_pieces_with_different_en_passant_have_different_hashes() {
        let fen = "4k3/3p4/8/4P3/8/8/8/4K3 b - - 0 1";
        let pushed = play(fen, "d7d5");
        let walked = play(
            fen,
            "e8f8 e1f1 f8e8 f1e1 d7d6 e1e2 e8e7 e2e1 e7e8 e1e2 d6d5 e2e1",
        );
        assert_eq!(
            pushed.to_fen().split(' ').next(),
            walked.to_fen().split(' ').next()
        );
        assert_eq!(pushed.en_passant(), Some(Square::D6));
        assert_eq!(walked.en_passant(), None);
        assert_ne!(pushed.hash(), walked.hash());
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
