use std::time::{Duration, Instant};

use crate::{piece::Color, score::Score};

/// What a `go` command allows the search to spend.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Limits {
    pub depth: Option<u32>,
    pub movetime: Option<Duration>,
    /// wtime and btime, indexed by color
    pub time: [Option<Duration>; 2],
    /// winc and binc, indexed by color
    pub inc: [Duration; 2],
    pub movestogo: Option<u32>,
    pub nodes: Option<u64>,
    /// Search until `stop`, whatever the clock says
    pub infinite: bool,
}

const DEFAULT_DEPTH: u32 = 7;
const MOVE_OVERHEAD: Duration = Duration::from_millis(50);

/// What the search obeys, worked out from `Limits` when the search starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    pub start: Instant,
    pub depth: u32,            // `depth`, or as deep as the search can go
    pub soft: Option<Instant>, // no new iteration after this
    pub hard: Option<Instant>, // stop in the middle of one after this
    /// No new iteration after this many nodes
    pub soft_nodes: Option<u64>,
    /// Stop in the middle of an iteration after this many nodes
    pub nodes: Option<u64>,
}

impl Budget {
    const SOFT_FACTOR: u32 = 4;

    /// The budget of a `go` that arrived at `start`, with `us` to move.
    pub fn new(limits: &Limits, us: Color, start: Instant) -> Self {
        let mut budget = Self::depth(limits.depth);
        budget.start = start;
        budget.nodes = limits.nodes;

        if limits.infinite {
            budget.depth = limits.depth.unwrap_or(Score::MAX_PLY);
            return budget;
        }

        match (limits.movetime, limits.time[us]) {
            (Some(movetime), _) => {
                budget.soft = Some(start + movetime / Self::SOFT_FACTOR);
                budget.hard = Some(start + movetime.saturating_sub(MOVE_OVERHEAD));

                budget.depth = limits.depth.unwrap_or(Score::MAX_PLY);
            }
            (None, Some(time)) => {
                let moves = limits.movestogo.unwrap_or(25);
                let movetime = time / moves + limits.inc[us] * 3 / 4;

                budget.soft = Some(start + movetime / Self::SOFT_FACTOR);
                budget.hard =
                    Some((start + movetime * 3).min(start + time.saturating_sub(MOVE_OVERHEAD)));

                budget.depth = limits.depth.unwrap_or(Score::MAX_PLY);
            }
            (None, None) => (),
        }

        budget
    }

    pub fn depth(depth: Option<u32>) -> Self {
        Self {
            start: Instant::now(),
            depth: depth.unwrap_or(DEFAULT_DEPTH),
            soft: None,
            hard: None,
            soft_nodes: None,
            nodes: None,
        }
    }

    pub fn with_nodes(self, nodes: u64) -> Self {
        Self {
            nodes: Some(nodes),
            ..self
        }
    }

    pub fn with_soft_nodes(self, nodes: u64) -> Self {
        Self {
            soft_nodes: Some(nodes),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn budget(limits: Limits, us: Color) -> (Budget, Instant) {
        let start = Instant::now();
        (Budget::new(&limits, us, start), start)
    }

    #[test]
    fn a_depth_alone_has_no_deadlines() {
        let (budget, _) = budget(
            Limits {
                depth: Some(5),
                ..Limits::default()
            },
            Color::White,
        );
        assert_eq!((budget.depth, budget.soft, budget.hard), (5, None, None));
    }

    #[test]
    fn a_depth_still_limits_a_timed_search() {
        let timed = [
            Limits {
                movetime: Some(ms(1_000)),
                ..Limits::default()
            },
            Limits {
                time: [Some(ms(60_000)); 2],
                ..Limits::default()
            },
        ];
        for limits in timed {
            let with_depth = Limits {
                depth: Some(5),
                ..limits.clone()
            };
            assert_eq!(budget(with_depth, Color::White).0.depth, 5, "{limits:?}");
        }
    }

    #[test]
    fn movetime_ends_before_the_overhead() {
        let (budget, start) = budget(
            Limits {
                movetime: Some(ms(1_000)),
                ..Limits::default()
            },
            Color::White,
        );
        assert_eq!(budget.hard, Some(start + ms(1_000) - MOVE_OVERHEAD));
    }

    #[test]
    fn the_hard_limit_never_passes_the_clock() {
        // (time, increment, movestogo)
        let clocks = [
            (300_000, 2_000, None),
            (1_000, 0, None),
            (10_000, 5_000, None),
            (60_000, 0, Some(1)),
            (30, 0, None),
        ];
        for (time, inc, movestogo) in clocks {
            let limits = Limits {
                time: [Some(ms(time)); 2],
                inc: [ms(inc); 2],
                movestogo,
                ..Limits::default()
            };
            let (budget, start) = budget(limits, Color::White);
            let hard = budget.hard.unwrap();
            assert!(
                hard <= start + ms(time).saturating_sub(MOVE_OVERHEAD),
                "{time} {inc} {movestogo:?}"
            );
        }
    }

    #[test]
    fn an_infinite_search_ignores_the_clock() {
        let (budget, _) = budget(
            Limits {
                infinite: true,
                time: [Some(ms(1_000)); 2],
                movetime: Some(ms(1_000)),
                ..Limits::default()
            },
            Color::White,
        );
        assert_eq!(
            (budget.depth, budget.soft, budget.hard),
            (Score::MAX_PLY, None, None)
        );
    }

    #[test]
    fn uses_the_clock_of_the_side_to_move() {
        // White has plenty of time, Black almost none
        let limits = Limits {
            time: [Some(ms(600_000)), Some(ms(1_000))],
            ..Limits::default()
        };
        let (white, start) = budget(limits.clone(), Color::White);
        let (black, _) = budget(limits, Color::Black);
        assert!(white.hard.unwrap() > start + ms(1_000));
        assert!(black.hard.unwrap() <= black.start + ms(1_000));
    }
}
