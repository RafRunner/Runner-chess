//! Fixed workloads for comparing builds. In the perft one the node counts check that
//! the move generator is still correct; in the search one the total node count is a
//! signature of the search. In both, the time measures how fast the engine is.

use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use crate::{
    board::Board,
    search::{Budget, MoveOrdering, Searcher, TranspositionTable},
};

// the perft reference positions from https://www.chessprogramming.org/Perft_Results
pub const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
pub const POSITION_3: &str = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";
pub const POSITION_4: &str = "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1";
pub const POSITION_5: &str = "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8";

pub struct BenchPosition {
    pub name: &'static str,
    pub fen: &'static str,
    pub depth: u32,
    /// the published perft result, so a faster but wrong build is caught
    pub nodes: u64,
}

/// About 38 million nodes, under a second in release on an Apple M4.
pub const BENCH: [BenchPosition; 5] = [
    BenchPosition {
        name: "startpos",
        fen: Board::STARTPOS,
        depth: 5,
        nodes: 4_865_609,
    },
    BenchPosition {
        name: "kiwipete",
        fen: KIWIPETE,
        depth: 4,
        nodes: 4_085_603,
    },
    BenchPosition {
        name: "position 3",
        fen: POSITION_3,
        depth: 6,
        nodes: 11_030_083,
    },
    BenchPosition {
        name: "position 4",
        fen: POSITION_4,
        depth: 5,
        nodes: 15_833_292,
    },
    BenchPosition {
        name: "position 5",
        fen: POSITION_5,
        depth: 4,
        nodes: 2_103_487,
    },
];

/// Positions for `run_search`. The total node count works as a signature: a change
/// meant only to make the search faster must leave it identical, and when it moves,
/// the search itself changed.
pub const SEARCH_BENCH: [(&str, &str); 6] = [
    ("startpos", Board::STARTPOS),
    ("kiwipete", KIWIPETE),
    (
        "middlegame 1",
        "r1bq1rk1/pp2bppp/2n1pn2/3p4/2PP4/2N1PN2/PP3PPP/R2QKB1R w KQ - 0 8",
    ),
    (
        "middlegame 2",
        "r2q1rk1/pp1nbppp/2p1pn2/3p4/2PP1B2/2N1PN2/PP1Q1PPP/R3KB1R w KQ - 0 9",
    ),
    // 24...e5 opens the sixth rank for the queen to take the bishop on g6
    (
        "tactic",
        "3r1rk1/ppp3p1/2q1p1Bp/2np4/5P2/2P3P1/PP2QP2/2KRR3 b - - 7 24",
    ),
    (
        "rook endgame",
        "4r1k1/1p3ppp/p1p5/3n4/3P4/P1N2P2/1P4PP/4R1K1 w - - 0 25",
    ),
];

/// Under 3 seconds in release on an Apple M4.
pub const SEARCH_BENCH_DEPTH: u32 = 6;

#[derive(Debug)]
pub enum BenchError {
    Io(io::Error),
    WrongNodeCount {
        name: &'static str,
        expected: u64,
        found: u64,
    },
}

impl From<io::Error> for BenchError {
    fn from(e: io::Error) -> Self {
        BenchError::Io(e)
    }
}

/// Runs perft on every position, printing one line each and then the totals.
pub fn run(positions: &[BenchPosition], out: &mut impl Write) -> Result<(), BenchError> {
    let mut total_nodes = 0;
    let mut total_time = Duration::ZERO;

    for position in positions {
        let board = Board::from_fen(position.fen).expect("bench positions are valid");

        let start = Instant::now();
        let nodes = board.perft(position.depth);
        let elapsed = start.elapsed();

        if nodes != position.nodes {
            return Err(BenchError::WrongNodeCount {
                name: position.name,
                expected: position.nodes,
                found: nodes,
            });
        }

        writeln!(
            out,
            "{:<10}  depth {}  {:>11} nodes  {:>6} ms  {:>11} nps",
            position.name,
            position.depth,
            nodes,
            elapsed.as_millis(),
            nps(nodes, elapsed)
        )?;
        total_nodes += nodes;
        total_time += elapsed;
    }

    write_totals(total_nodes, total_time, out)?;
    Ok(())
}

/// Searches every position to `depth`, printing one line each and then the totals.
pub fn run_search(positions: &[(&str, &str)], depth: u32, out: &mut impl Write) -> io::Result<()> {
    let mut total_nodes = 0;
    let mut total_time = Duration::ZERO;

    for &(name, fen) in positions {
        let board = Board::from_fen(fen).expect("bench positions are valid");
        let mut tt = TranspositionTable::new(64);
        let mut ordering = MoveOrdering::new();
        let mut searcher = Searcher::new(Budget::depth(Some(depth)), &mut tt, &mut ordering);

        let start = Instant::now();
        let best = searcher.search_root(&board, &[], &mut io::sink())?;
        let elapsed = start.elapsed();
        let nodes = searcher.nodes();

        writeln!(
            out,
            "{:<12}  depth {}  {:>11} nodes  {:>6} ms  {:>11} nps  bestmove {}",
            name,
            depth,
            nodes,
            elapsed.as_millis(),
            nps(nodes, elapsed),
            best.map_or("0000".to_string(), |mv| mv.to_uci()),
        )?;
        total_nodes += nodes;
        total_time += elapsed;
    }

    write_totals(total_nodes, total_time, out)
}

fn write_totals(nodes: u64, time: Duration, out: &mut impl Write) -> io::Result<()> {
    writeln!(out)?;
    writeln!(out, "Total time (ms) : {}", time.as_millis())?;
    writeln!(out, "Nodes searched  : {nodes}")?;
    writeln!(out, "Nodes/second    : {}", nps(nodes, time))
}

/// Nodes per second, safe for runs too short to measure.
pub fn nps(nodes: u64, elapsed: Duration) -> u64 {
    (nodes as f64 / elapsed.as_secs_f64().max(1e-9)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// the real workload is far too slow for debug builds
    const SMALL: [BenchPosition; 2] = [
        BenchPosition {
            name: "startpos",
            fen: Board::STARTPOS,
            depth: 2,
            nodes: 400,
        },
        BenchPosition {
            name: "kiwipete",
            fen: KIWIPETE,
            depth: 1,
            nodes: 48,
        },
    ];

    fn output_of(positions: &[BenchPosition]) -> Result<String, BenchError> {
        let mut out = Vec::new();
        run(positions, &mut out)?;
        Ok(String::from_utf8(out).unwrap())
    }

    #[test]
    fn prints_a_line_per_position_and_the_totals() {
        let out = output_of(&SMALL).unwrap();
        let lines: Vec<&str> = out.lines().collect();

        assert!(lines[0].starts_with("startpos    depth 2"), "{out}");
        assert!(lines[0].contains(" 400 nodes"), "{out}");
        assert!(lines[1].starts_with("kiwipete    depth 1"), "{out}");
        assert!(lines[1].contains(" 48 nodes"), "{out}");
        assert!(lines.contains(&"Nodes searched  : 448"), "{out}");
        assert!(
            lines.iter().any(|l| l.starts_with("Total time (ms) : ")),
            "{out}"
        );
        assert!(
            lines.iter().any(|l| l.starts_with("Nodes/second    : ")),
            "{out}"
        );
    }

    #[test]
    fn a_wrong_node_count_is_an_error() {
        let wrong = [BenchPosition {
            name: "startpos",
            fen: Board::STARTPOS,
            depth: 2,
            nodes: 401,
        }];
        assert!(matches!(
            output_of(&wrong),
            Err(BenchError::WrongNodeCount {
                name: "startpos",
                expected: 401,
                found: 400,
            })
        ));
    }

    #[test]
    fn nps_survives_a_zero_duration() {
        assert_eq!(nps(1000, Duration::from_millis(500)), 2000);
        assert!(nps(1000, Duration::ZERO) > 0);
    }

    #[test]
    #[ignore = "slow in debug builds; run with cargo test --release -- --ignored"]
    fn full_bench_matches_the_published_counts() {
        output_of(&BENCH).unwrap();
    }

    fn search_output(depth: u32) -> String {
        let mut out = Vec::new();
        run_search(&SEARCH_BENCH[..2], depth, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// The node count of each position line of a search bench.
    fn node_counts(out: &str) -> Vec<u64> {
        out.lines()
            .filter_map(|line| line.split_once(" nodes "))
            .map(|(before, _)| before.split_whitespace().last().unwrap().parse().unwrap())
            .collect()
    }

    #[test]
    fn search_bench_prints_a_line_per_position_and_the_totals() {
        let out = search_output(2);
        let lines: Vec<&str> = out.lines().collect();

        assert!(lines[0].starts_with("startpos      depth 2"), "{out}");
        assert!(lines[1].starts_with("kiwipete      depth 2"), "{out}");
        for line in &lines[..2] {
            assert!(line.contains(" bestmove "), "{out}");
        }

        let counts = node_counts(&out);
        assert_eq!(counts.len(), 2, "{out}");
        let total = format!("Nodes searched  : {}", counts.iter().sum::<u64>());
        assert!(lines.contains(&total.as_str()), "{out}");
    }

    #[test]
    fn search_bench_signature_is_deterministic() {
        // the same search must visit the same nodes, or the signature means nothing
        assert_eq!(
            node_counts(&search_output(2)),
            node_counts(&search_output(2))
        );
    }
}
