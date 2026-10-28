use std::ops::{self, Add};

/// A score in centipawns, from the point of view of the side to move. Beyond
/// `MAX_EVAL` it is a forced mate instead, and says how far away it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Score(i16);

impl Score {
    pub const MATE: Self = Self(32_000);
    pub const DRAW: Self = Self(0);
    /// Beyond every real score, and still safe to negate.
    pub const INF: Self = Self(i16::MAX - 1);
    /// The deepest a search can go, so mate scores never reach the evaluations.
    pub const MAX_PLY: u32 = 248;
    /// The best evaluation that isn't a mate.
    pub const MAX_EVAL: Self = Self(Self::MATE.0 - Self::MAX_PLY as i16 - 1);

    /// An evaluation, kept short of the mate scores.
    pub fn new(centipawns: i32) -> Self {
        let max = Self::MAX_EVAL.0 as i32;
        Self(centipawns.clamp(-max, max) as i16)
    }

    pub fn mate_in(ply: u32) -> Self {
        Self(Self::MATE.0 - ply as i16)
    }

    pub fn mated_in(ply: u32) -> Self {
        -Self::mate_in(ply)
    }

    pub fn is_mate(self) -> bool {
        self.0.abs() > Self::MAX_EVAL.0
    }

    /// The evaluation in centipawns, or `None` for a mate.
    pub fn centipawns(self) -> Option<i16> {
        (!self.is_mate()).then_some(self.0)
    }

    /// How many plies away the mate is, whoever gives it.
    pub fn mate_plies(self) -> Option<u32> {
        self.is_mate().then(|| (Self::MATE.0 - self.0.abs()) as u32)
    }

    /// When storing on the tt, we remove the ply from the score to indicate
    /// the mate score from the position at ply, not the root
    pub fn to_tt(self, ply: u32) -> Self {
        if self.is_mate() {
            if self > Self::DRAW {
                Self(self.0 + ply as i16)
            } else {
                Self(self.0 - ply as i16)
            }
        } else {
            self
        }
    }

    /// Opposite of to_tt
    pub fn from_tt(self, ply: u32) -> Self {
        if self.is_mate() {
            if self > Self::DRAW {
                Self(self.0 - ply as i16)
            } else {
                Self(self.0 + ply as i16)
            }
        } else {
            self
        }
    }

    /// As UCI wants it: `cp <centipawns>`, or `mate <moves>`, negative when the side
    /// to move is the one getting mated.
    pub fn to_uci(self) -> String {
        let Some(plies) = self.mate_plies() else {
            return format!("cp {}", self.0);
        };

        let moves = (plies as i32 + 1) / 2;
        format!("mate {}", if self > Self::DRAW { moves } else { -moves })
    }
}

impl ops::Neg for Score {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl Add<i16> for Score {
    type Output = Self;

    /// Never past infinity, either way, so the result can still be negated
    fn add(self, rhs: i16) -> Self::Output {
        Self(self.0.saturating_add(rhs).clamp(-Self::INF.0, Self::INF.0))
    }
}

impl ops::Sub<i16> for Score {
    type Output = Self;

    fn sub(self, rhs: i16) -> Self::Output {
        self.add(-rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluations_stay_short_of_mate() {
        assert!(Score::new(35) > Score::DRAW);
        assert_eq!(Score::new(1_000_000), Score::MAX_EVAL);
        assert_eq!(Score::new(-1_000_000), -Score::MAX_EVAL);
        assert!(!Score::new(1_000_000).is_mate());
    }

    #[test]
    fn mates_are_ordered_by_distance() {
        // giving mate sooner is better, getting mated later is better
        assert!(Score::mate_in(1) > Score::mate_in(3));
        assert!(Score::mated_in(3) > Score::mated_in(1));
        assert!(Score::mate_in(Score::MAX_PLY) > Score::MAX_EVAL);
        assert!(Score::mated_in(Score::MAX_PLY) < -Score::MAX_EVAL);
    }

    #[test]
    fn mated_in_is_the_other_side_of_mate_in() {
        for ply in 0..10 {
            assert_eq!(Score::mated_in(ply), -Score::mate_in(ply), "{ply}");
        }
    }

    #[test]
    fn infinity_is_beyond_every_mate() {
        assert!(Score::INF > Score::mate_in(0));
        assert!(-Score::INF < Score::mated_in(0));
    }

    #[test]
    fn a_margin_moves_the_score() {
        assert_eq!(Score::new(100) + 50, Score::new(150));
        assert_eq!(Score::new(100) - 150, Score::new(-50));
        // mates get nothing special: one more is a ply closer
        assert_eq!(Score::mate_in(3) + 1, Score::mate_in(2));
    }

    #[test]
    fn a_margin_never_goes_past_infinity() {
        assert_eq!(Score::INF + 1, Score::INF);
        assert_eq!(Score::INF + i16::MAX, Score::INF);
        assert_eq!(-Score::INF + -1, -Score::INF);
        assert_eq!(-Score::INF + i16::MIN, -Score::INF);
        // so whatever comes out can still be negated
        assert_eq!(-(Score::INF + i16::MAX), -Score::INF);
        assert_eq!(-(-Score::INF + i16::MIN), Score::INF);
    }

    #[test]
    fn mate_plies() {
        assert_eq!(Score::mate_in(5).mate_plies(), Some(5));
        assert_eq!(Score::mated_in(5).mate_plies(), Some(5));
        assert_eq!(Score::new(100).mate_plies(), None);
        assert_eq!(Score::MAX_EVAL.mate_plies(), None);
        assert!(Score::mate_in(Score::MAX_PLY).is_mate());
    }

    #[test]
    fn centipawns() {
        assert_eq!(Score::new(-35).centipawns(), Some(-35));
        assert_eq!(Score::MAX_EVAL.centipawns(), Some(Score::MAX_EVAL.0));
        assert_eq!(Score::mate_in(5).centipawns(), None);
        assert_eq!(Score::mated_in(5).centipawns(), None);
    }

    #[test]
    fn the_tt_counts_mates_from_the_node() {
        // a mate 5 plies from the root, seen 2 plies in, is 3 plies away from that node
        assert_eq!(Score::mate_in(5).to_tt(2), Score::mate_in(3));
        assert_eq!(Score::mated_in(5).to_tt(2), Score::mated_in(3));
        // and reached again 4 plies in, 7 plies from the root
        assert_eq!(Score::mate_in(5).to_tt(2).from_tt(4), Score::mate_in(7));
        assert_eq!(Score::mated_in(5).to_tt(2).from_tt(4), Score::mated_in(7));
        // the rest doesn't depend on where it was found
        assert_eq!(Score::new(150).to_tt(2), Score::new(150));
        assert_eq!(Score::MAX_EVAL.to_tt(2), Score::MAX_EVAL);
    }

    #[test]
    fn from_tt_undoes_to_tt() {
        for score in [
            Score::mate_in(7),
            Score::mated_in(7),
            Score::new(-300),
            Score::DRAW,
            -Score::MAX_EVAL,
        ] {
            for ply in 0..=7 {
                assert_eq!(score.to_tt(ply).from_tt(ply), score, "{score:?} {ply}");
            }
        }
    }

    #[test]
    fn uci_format() {
        assert_eq!(Score::new(35).to_uci(), "cp 35");
        assert_eq!(Score::new(-120).to_uci(), "cp -120");
        assert_eq!(Score::DRAW.to_uci(), "cp 0");
        assert_eq!(Score::mate_in(1).to_uci(), "mate 1");
        assert_eq!(Score::mate_in(3).to_uci(), "mate 2");
        assert_eq!(Score::mated_in(2).to_uci(), "mate -1");
        assert_eq!(Score::mated_in(4).to_uci(), "mate -2");
    }
}
