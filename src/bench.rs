//! A fixed perft workload for comparing builds: the node counts check that the
//! engine is still correct, the time measures how fast it is.

use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use crate::board::Board;

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

    writeln!(out)?;
    writeln!(out, "Total time (ms) : {}", total_time.as_millis())?;
    writeln!(out, "Nodes searched  : {total_nodes}")?;
    writeln!(out, "Nodes/second    : {}", nps(total_nodes, total_time))?;
    Ok(())
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
}
