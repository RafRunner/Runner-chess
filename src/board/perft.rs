use crate::chess_move::Move;

use super::Board;

impl Board {
    pub fn divide(&self, depth: u32) -> Vec<(Move, u64)> {
        self.legal_successors()
            .map(|(mv, next)| (mv, next.perft(depth - 1)))
            .collect()
    }

    pub fn perft(&self, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        self.legal_successors()
            .map(|(_, next)| next.perft(depth - 1))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        board::Board,
        test_utils::{
            board, mirror_fen, KIWIPETE, PERFT_POSITIONS, POSITION_3, POSITION_4, POSITION_5,
        },
    };

    /// published node counts for depths 1 to 4:
    /// https://www.chessprogramming.org/Perft_Results
    #[rustfmt::skip]
    const PERFT_RESULTS: [(&str, [u64; 4]); 5] = [
        (Board::STARTPOS, [20, 400, 8902, 197_281]),
        (KIWIPETE,        [48, 2039, 97_862, 4_085_603]),
        (POSITION_3,      [14, 191, 2812, 43_238]),
        (POSITION_4,      [6, 264, 9467, 422_333]),
        (POSITION_5,      [44, 1486, 62_379, 2_103_487]),
    ];

    fn assert_perft(fen: &str, depth: u32, expected: u64) {
        assert_eq!(board(fen).perft(depth), expected, "{fen} at depth {depth}");
    }

    #[test]
    fn perft_at_depth_zero_counts_the_position_itself() {
        assert_perft(Board::STARTPOS, 0, 1);
    }

    #[test]
    fn perft_shallow() {
        for (fen, counts) in PERFT_RESULTS {
            for depth in 1..=3 {
                assert_perft(fen, depth, counts[depth as usize - 1]);
            }
        }
    }

    /// swapping the colors and flipping the board can't change the count, so any
    /// rule that only works for one side (black's long castle, white's en passant...)
    /// shows up here
    #[test]
    fn mirrored_positions_have_the_same_perft() {
        for (fen, counts) in PERFT_RESULTS {
            let mirrored = mirror_fen(fen);
            for depth in 1..=2 {
                assert_perft(&mirrored, depth, counts[depth as usize - 1]);
            }
        }
    }

    #[test]
    #[ignore = "slow in debug builds; run with cargo test --release -- --ignored"]
    fn perft_deep() {
        for (fen, counts) in PERFT_RESULTS {
            assert_perft(fen, 4, counts[3]);
            assert_perft(&mirror_fen(fen), 4, counts[3]);
        }
    }

    /// small positions aimed at one rule each, from Martin Sedlak's perft suite
    #[test]
    #[ignore = "slow in debug builds; run with cargo test --release -- --ignored"]
    fn perft_edge_cases() {
        #[rustfmt::skip]
        let cases = [
            // en passant that would expose the king
            ("3k4/3p4/8/K1P4r/8/8/8/8 b - - 0 1",          6, 1_134_888),
            ("8/8/4k3/8/2p5/8/B2P2K1/8 w - - 0 1",         6, 1_015_133),
            // en passant that gives check
            ("8/8/1k6/2b5/2pP4/8/5K2/8 b - d3 0 1",        6, 1_440_467),
            // castling that gives check
            ("5k2/8/8/8/8/8/8/4K2R w K - 0 1",             6, 661_072),
            ("3k4/8/8/8/8/8/8/R3K3 w Q - 0 1",             6, 803_711),
            // castling rights lost and castling prevented
            ("r3k2r/1b4bq/8/8/8/8/7B/R3K2R w KQkq - 0 1",  4, 1_274_206),
            ("r3k2r/8/3Q4/8/8/5q2/8/R3K2R b KQkq - 0 1",   4, 1_720_476),
            // promotions: out of check, giving check, underpromotion giving check
            ("2K2r2/4P3/8/8/8/8/8/3k4 w - - 0 1",          6, 3_821_001),
            ("4k3/1P6/8/8/8/8/K7/8 w - - 0 1",             6, 217_342),
            ("8/P1k5/K7/8/8/8/8/8 w - - 0 1",              6, 92_683),
            // discovered check
            ("8/8/1P2K3/8/2n5/1q6/8/5k2 b - - 0 1",        5, 1_004_658),
            // stalemate and checkmate
            ("K1k5/8/P7/8/8/8/8/8 w - - 0 1",              6, 2217),
            ("8/k1P5/8/1K6/8/8/8/8 w - - 0 1",             7, 567_584),
            ("8/8/2k5/5q2/5n2/8/5K2/8 b - - 0 1",          4, 23_527),
        ];
        for (fen, depth, expected) in cases {
            assert_perft(fen, depth, expected);
            assert_perft(&mirror_fen(fen), depth, expected);
        }
    }

    /// the same breakdown `go perft 3` prints in Stockfish
    #[test]
    fn divide_startpos_matches_stockfish() {
        #[rustfmt::skip]
        let stockfish = [
            ("a2a3", 380), ("b2b3", 420), ("c2c3", 420), ("d2d3", 539),
            ("e2e3", 599), ("f2f3", 380), ("g2g3", 420), ("h2h3", 380),
            ("a2a4", 420), ("b2b4", 421), ("c2c4", 441), ("d2d4", 560),
            ("e2e4", 600), ("f2f4", 401), ("g2g4", 421), ("h2h4", 420),
            ("b1a3", 400), ("b1c3", 440), ("g1f3", 440), ("g1h3", 400),
        ];
        let mut expected: Vec<(String, u64)> = stockfish
            .iter()
            .map(|&(uci, nodes)| (uci.to_string(), nodes))
            .collect();
        expected.sort();

        let mut actual: Vec<(String, u64)> = board(Board::STARTPOS)
            .divide(3)
            .into_iter()
            .map(|(mv, nodes)| (mv.to_uci(), nodes))
            .collect();
        actual.sort();

        assert_eq!(actual, expected);
    }

    #[test]
    fn divide_adds_up_to_perft() {
        for fen in PERFT_POSITIONS {
            let b = board(fen);
            for depth in 1..=2 {
                let total: u64 = b.divide(depth).iter().map(|&(_, nodes)| nodes).sum();
                assert_eq!(total, b.perft(depth), "{fen} at depth {depth}");
            }
            // one ply down there's only the position after each move
            assert!(b.divide(1).iter().all(|&(_, nodes)| nodes == 1), "{fen}");
        }
    }
}
