use crate::{board::Board, score::Score};

/// Where a node is in the search and the window it gets searched with (see `negamax`)
#[derive(Debug, Clone, Copy)]
pub(super) struct NodeInfo<'board> {
    pub(super) board: &'board Board,
    pub(super) depth: u32,
    pub(super) ply: u32,
    pub(super) alpha: Score,
    pub(super) beta: Score,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Worse,  // didn't beat alpha
    Raised, // the new best, inside the window (alpha, beta)
    Cut,    // reached beta
}

impl<'board> NodeInfo<'board> {
    pub(super) fn new(
        board: &'board Board,
        depth: u32,
        ply: u32,
        alpha: Score,
        beta: Score,
    ) -> Self {
        Self {
            board,
            depth,
            ply,
            alpha,
            beta,
        }
    }

    #[cfg(test)]
    pub(super) fn full_window(board: &'board Board, depth: u32, ply: u32) -> Self {
        Self::new(board, depth, ply, -Score::INF, Score::INF)
    }

    /// The node after a move to `board`: a ply further, one less to go, and the window
    /// seen from the other side
    pub(super) fn child<'next>(&self, board: &'next Board) -> NodeInfo<'next> {
        NodeInfo {
            board,
            depth: self.depth.saturating_sub(1),
            ply: self.ply + 1,
            alpha: -self.beta,
            beta: -self.alpha,
        }
    }

    /// Creates a child with reduced depth and/or window, for tests like NMP
    pub(super) fn scout_child<'next>(
        &self,
        board: &'next Board,
        reduce: u32,
        threshold: Score,
    ) -> NodeInfo<'next> {
        NodeInfo {
            board,
            depth: self.depth.saturating_sub(reduce + 1),
            ply: self.ply + 1,
            alpha: threshold,
            beta: threshold + 1,
        }
    }

    /// A node whose exact score matters, on the principal variation: the others only
    /// have to tell whether they beat a null window (alpha, alpha + 1)
    pub(super) fn is_pv(&self) -> bool {
        self.beta > self.alpha + 1
    }

    /// Takes the score of a move into the node: the best so far, and the window
    pub(super) fn update(&mut self, best: &mut Score, score: Score) -> Outcome {
        if score > *best {
            *best = score;

            if *best > self.alpha {
                // We found something better, so we raise the floor. The window closes and we cut more
                self.alpha = *best;

                // the window closed: a beta cutoff
                if self.alpha >= self.beta {
                    // Opponent has something better guaranteed, therefore we can cut the analysis here
                    return Outcome::Cut;
                }

                return Outcome::Raised;
            }
        }

        Outcome::Worse
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_with_window(board: &Board, alpha: i32, beta: i32) -> NodeInfo<'_> {
        NodeInfo::new(board, 5, 2, Score::new(alpha), Score::new(beta))
    }

    #[test]
    fn a_child_sees_the_window_from_the_other_side() {
        let board = Board::startpos();
        let child = node_with_window(&board, -30, 50).child(&board);

        assert_eq!((child.depth, child.ply), (4, 3));
        assert_eq!((child.alpha, child.beta), (Score::new(-50), Score::new(30)));
    }

    #[test]
    fn a_scout_child_has_a_null_window_and_less_depth() {
        let board = Board::startpos();
        let parent = node_with_window(&board, -30, 50);
        let scout = parent.scout_child(&board, 2, Score::new(-50));

        assert_eq!((scout.depth, scout.ply), (2, 3));
        assert_eq!(
            (scout.alpha, scout.beta),
            (Score::new(-50), Score::new(-49))
        );
        // never below 0, which is a quiescence search
        assert_eq!(parent.scout_child(&board, 10, Score::DRAW).depth, 0);
    }

    #[test]
    fn update_tells_where_a_score_fell() {
        let board = Board::startpos();
        let mut node = node_with_window(&board, -30, 50);
        let mut best = -Score::INF;

        // below alpha: the best so far, but the window stays
        assert_eq!(node.update(&mut best, Score::new(-60)), Outcome::Worse);
        assert_eq!((best, node.alpha), (Score::new(-60), Score::new(-30)));
        // a worse one changes nothing
        assert_eq!(node.update(&mut best, Score::new(-80)), Outcome::Worse);
        assert_eq!(best, Score::new(-60));
        // inside the window: alpha goes up to it
        assert_eq!(node.update(&mut best, Score::new(10)), Outcome::Raised);
        assert_eq!((best, node.alpha), (Score::new(10), Score::new(10)));
        // reaching beta: a cut
        assert_eq!(node.update(&mut best, Score::new(50)), Outcome::Cut);
        assert_eq!(best, Score::new(50));
    }
}
