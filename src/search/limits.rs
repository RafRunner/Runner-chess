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
    /// From this depth on, the soft limit is multiplied by `(SHARE_OFFSET - share) *
    /// SHARE_SCALE`, with the share of the nodes that the best move took: most of them,
    /// an easy move that gets less time; few, a hard one that gets more. The first
    /// iterations are too noisy for it
    const SHARE_DEPTH: u32 = 8;
    const SHARE_OFFSET: f64 = 1.5;
    const SHARE_SCALE: f64 = 1.35;

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

    /// No new iteration after `depth`, at `now`, with `nodes` searched and `best_share`
    /// of them taken by the best move
    pub(super) fn soft_reached(
        &self,
        depth: u32,
        nodes: u64,
        best_share: f64,
        now: Instant,
    ) -> bool {
        let time = self.soft.is_some_and(|soft| {
            if depth < Self::SHARE_DEPTH {
                return now >= soft;
            }
            let scale = (Self::SHARE_OFFSET - best_share) * Self::SHARE_SCALE;
            now - self.start >= (soft - self.start).mul_f64(scale)
        });

        time || self.soft_nodes.is_some_and(|soft| nodes >= soft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        search::tests::{Searched, search},
        test_utils::{MATED, STALEMATED, board},
    };

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

    #[test]
    fn a_sure_best_move_stops_earlier_and_an_unsure_one_later() {
        // the soft limit is a quarter of the movetime: 1 s
        let (budget, start) = budget(
            Limits {
                movetime: Some(ms(4_000)),
                ..Limits::default()
            },
            Color::White,
        );
        let at = |millis| start + ms(millis);

        // the first iterations only look at the clock
        assert!(!budget.soft_reached(7, 0, 1.0, at(990)));
        assert!(budget.soft_reached(7, 0, 0.0, at(1_000)));

        // 90% of the nodes stops at 0.81 of it, 10% at 1.89
        assert!(!budget.soft_reached(8, 0, 0.9, at(800)));
        assert!(budget.soft_reached(8, 0, 0.9, at(820)));
        assert!(!budget.soft_reached(8, 0, 0.1, at(1_880)));
        assert!(budget.soft_reached(8, 0, 0.1, at(1_900)));
    }

    #[test]
    fn soft_nodes_ignore_the_share_of_the_best_move() {
        let budget = Budget::depth(None).with_soft_nodes(1_000);
        let now = Instant::now();
        for (depth, share) in [(3, 0.5), (10, 0.0), (10, 1.0)] {
            assert!(
                !budget.soft_reached(depth, 999, share, now),
                "{depth} {share}"
            );
            assert!(
                budget.soft_reached(depth, 1_000, share, now),
                "{depth} {share}"
            );
        }
    }

    #[test]
    fn the_clock_works_however_the_nodes_split_among_the_moves() {
        let limits = Limits {
            time: [Some(ms(10_000)); 2],
            ..Limits::default()
        };
        for (fen, has_moves) in [
            // Qxg4 takes nearly every node, as the other moves lose
            (
                "rnb1kbnr/pppp1ppp/8/4p3/4P1q1/8/PPPPQPPP/RNB1KBNR w KQkq - 0 1",
                true,
            ),
            // every move completes the fifty moves: draws, found before any search
            ("k7/8/8/8/8/8/8/1K5R w - - 99 150", true),
            // no move at all, so no best move to take a share
            (MATED, false),
            (STALEMATED, false),
        ] {
            let position = board(fen);
            let us = position.side_to_move();
            let budget = Budget::new(&limits, us, Instant::now());
            let Searched { best, out, .. } = search(&position, &[], budget);
            assert_eq!(best.is_some(), has_moves, "{fen}\n{out}");
        }
    }
}
