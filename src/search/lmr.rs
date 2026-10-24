use std::sync::LazyLock;

use crate::{
    board::Board,
    chess_move::Move,
    score::Score,
    search::{MoveOrdering, NodeInfo, Searcher},
};

/// Late move reductions only start at this depth
pub(super) const MIN_DEPTH: u32 = 3;
/// The first moves of a node always get the full depth
pub(super) const FULL_DEPTH_MOVES: usize = 5;

/// Move indices the table covers, later ones get calculated on the spot
const TABLE_MOVES: usize = 256;

impl Searcher<'_> {
    /// The score of `mv`, the move at `move_idx` of `node`, with a principal variation
    /// search: the first move is expected to be the best, so it gets the whole window,
    /// and the others only have to show that they don't beat it, on a null window
    /// (alpha, alpha + 1), which cuts much more. Late quiet moves are also expected to
    /// be bad, so they try that on less depth first. Whatever beats alpha gets searched
    /// again, on full depth and then on the whole window, for its real score.
    pub(super) fn search_move(
        &mut self,
        node: NodeInfo,
        mv: Move,
        move_idx: usize,
        next: &Board,
    ) -> Score {
        // a check gets searched a ply deeper: it often hides a tactic, and the evasions
        // are few
        let extension = u32::from(next.is_in_check(next.side_to_move()));
        let node = NodeInfo {
            depth: node.depth + extension,
            ..node
        };

        if move_idx == 0 {
            return self.search_child(node.board, node.child(next), Some(mv));
        }

        let reduce = if self.is_reducible(node, mv, move_idx, next) {
            let history = self.ordering.history_bonus(mv, node.board.side_to_move());
            // a ply more off the principal variation, whose moves are expected to fail
            // low anyway, and a ply less when the line is improving
            let extra = i32::from(!node.is_pv()) - i32::from(self.is_improving(node.ply));
            reduction(node.depth, move_idx, history, extra)
        } else {
            0
        };
        let null_window = |reduce| node.scout_child(next, reduce, -node.alpha - 1);

        let mut score = self.search_child(node.board, null_window(reduce), Some(mv));
        if score > node.alpha && reduce > 0 {
            score = self.search_child(node.board, null_window(0), Some(mv));
        }
        // in a null window node beta is alpha + 1, and this is already the real score
        if score > node.alpha && score < node.beta {
            score = self.search_child(node.board, node.child(next), Some(mv));
        }
        score
    }

    /// Most moves get the full depth, only late quiet ones get reduced
    fn is_reducible(&self, node: NodeInfo, mv: Move, move_idx: usize, next: &Board) -> bool {
        node.ply > 0
            && node.depth >= MIN_DEPTH
            && move_idx >= FULL_DEPTH_MOVES
            && mv.is_quiet()
            && !self.ordering.is_killer(mv, node.ply)
            && !node.board.is_in_check(node.board.side_to_move())
            && !next.is_in_check(next.side_to_move())
    }
}

static REDUCTIONS: LazyLock<[[u32; Score::MAX_PLY as usize]; TABLE_MOVES]> = LazyLock::new(|| {
    let mut reductions = [[0; Score::MAX_PLY as usize]; TABLE_MOVES];

    for (moves, by_depth) in reductions.iter_mut().enumerate() {
        for (depth, reduction) in by_depth.iter_mut().enumerate() {
            *reduction = calculate(depth as u32, moves);
        }
    }

    reductions
});

/// Each HISTORY_DIVISOR of history is one ply less (or more) of reduction, up to 2.
const HISTORY_DIVISOR: i32 = MoveOrdering::MAX_BONUS / 8;

/// How many plies less the move at `move_index` of a node `depth` plies deep gets searched,
/// with `extra` plies more (or less). The child keeps at least one ply, so it doesn't fall
/// straight into quiescence
pub(super) fn reduction(depth: u32, move_index: usize, history: i32, extra: i32) -> u32 {
    let base = REDUCTIONS
        .get(move_index)
        .and_then(|by_depth| by_depth.get(depth as usize))
        .copied()
        .unwrap_or_else(|| calculate(depth, move_index));

    // good history reduces less, bad history more
    let adjustment = (history / HISTORY_DIVISOR).clamp(-2, 2);

    base.saturating_add_signed(extra - adjustment)
        .min(depth.saturating_sub(2))
}

/// The reduction is BASE + ln(depth) * ln(move index) / DIVISOR, tuned by matches
const BASE: f64 = 0.99;
// close to π by chance, it is a tuned value like BASE
#[allow(clippy::approx_constant)]
const DIVISOR: f64 = 3.14;

fn calculate(depth: u32, moves: usize) -> u32 {
    (BASE + (depth as f64).ln() * (moves as f64).ln() / DIVISOR).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every (depth, move index) where the search reduces, up to past the table
    fn reduced() -> impl Iterator<Item = (u32, usize)> {
        (MIN_DEPTH..Score::MAX_PLY + 20)
            .flat_map(|depth| (FULL_DEPTH_MOVES..TABLE_MOVES + 20).map(move |idx| (depth, idx)))
    }

    #[test]
    fn the_first_late_move_already_loses_a_ply() {
        assert!(reduction(MIN_DEPTH, FULL_DEPTH_MOVES, 0, 0) >= 1);
    }

    #[test]
    fn a_reduced_move_still_gets_searched_a_ply_deep() {
        for (depth, idx) in reduced() {
            // the move itself is one ply, the reduction comes on top of it
            let child_depth = depth - 1 - reduction(depth, idx, 0, 0);
            assert!(child_depth >= 1, "depth {depth}, move {idx}: {child_depth}");
        }
    }

    #[test]
    fn later_moves_are_reduced_at_least_as_much() {
        for (depth, idx) in reduced() {
            assert!(
                reduction(depth, idx + 1, 0, 0) >= reduction(depth, idx, 0, 0),
                "depth {depth}, move {idx}"
            );
        }
    }

    #[test]
    fn deeper_nodes_reduce_at_least_as_much() {
        for (depth, idx) in reduced() {
            assert!(
                reduction(depth + 1, idx, 0, 0) >= reduction(depth, idx, 0, 0),
                "depth {depth}, move {idx}"
            );
        }
    }

    #[test]
    fn the_table_and_the_formula_agree() {
        // past the table on either side the formula takes over, without a jump
        for depth in MIN_DEPTH..Score::MAX_PLY {
            let last = TABLE_MOVES - 1;
            assert_eq!(
                reduction(depth, last, 0, 0),
                calculate(depth, last).min(depth - 2),
                "depth {depth}"
            );
        }
        for idx in FULL_DEPTH_MOVES..TABLE_MOVES {
            let past = Score::MAX_PLY;
            assert_eq!(
                reduction(past, idx, 0, 0),
                calculate(past, idx),
                "move {idx}"
            );
        }
    }

    /// History values from the worst to the best, past the ±2 plies on both sides
    const HISTORIES: [i32; 11] = [
        i32::MIN,
        -100 * HISTORY_DIVISOR,
        -2 * HISTORY_DIVISOR,
        -HISTORY_DIVISOR,
        -1,
        0,
        1,
        HISTORY_DIVISOR,
        2 * HISTORY_DIVISOR,
        100 * HISTORY_DIVISOR,
        i32::MAX,
    ];

    #[test]
    fn a_better_history_never_reduces_more() {
        for (depth, idx) in reduced() {
            for pair in HISTORIES.windows(2) {
                let (worse, better) = (pair[0], pair[1]);
                assert!(
                    reduction(depth, idx, better, 0) <= reduction(depth, idx, worse, 0),
                    "depth {depth}, move {idx}: history {better} vs {worse}"
                );
            }
        }
    }

    #[test]
    fn history_moves_the_reduction_by_up_to_two_plies() {
        // far enough from both limits: 3 plies with no history, out of 8 at most
        let (depth, idx) = (10, 20);
        assert_eq!(reduction(depth, idx, 0, 0), 3);

        let divisor = HISTORY_DIVISOR;
        for (history, expected) in [
            (divisor - 1, 3), // less than a divisor doesn't count yet
            (divisor, 2),
            (2 * divisor, 1),
            (100 * divisor, 1),
            (-(divisor - 1), 3),
            (-divisor, 4),
            (-2 * divisor, 5),
            (-100 * divisor, 5),
        ] {
            assert_eq!(
                reduction(depth, idx, history, 0),
                expected,
                "history {history}"
            );
        }
    }

    #[test]
    fn a_good_history_can_cancel_the_reduction() {
        // where the reduction is smallest, a good history searches the move at full depth
        assert_eq!(
            reduction(MIN_DEPTH, FULL_DEPTH_MOVES, 2 * HISTORY_DIVISOR, 0),
            0
        );
    }

    #[test]
    fn any_history_still_leaves_a_ply_to_search() {
        for (depth, idx) in reduced() {
            for history in HISTORIES {
                let child_depth = depth - 1 - reduction(depth, idx, history, 0);
                assert!(
                    child_depth >= 1,
                    "depth {depth}, move {idx}, history {history}: {child_depth}"
                );
            }
        }
    }

    #[test]
    fn extra_plies_still_leave_one_to_search() {
        for (depth, idx) in reduced() {
            for extra in -2..=2 {
                let child_depth = depth - 1 - reduction(depth, idx, 0, extra);
                assert!(child_depth >= 1, "depth {depth}, move {idx}, extra {extra}");
            }
            assert!(reduction(depth, idx, 0, 1) >= reduction(depth, idx, 0, 0));
            assert!(reduction(depth, idx, 0, -1) <= reduction(depth, idx, 0, 0));
        }
    }

    #[test]
    fn a_search_deeper_than_max_ply_does_not_panic() {
        // `go depth 1000` gets that far at the root's children
        assert!(reduction(1000, FULL_DEPTH_MOVES, 0, 0) > 0);
        assert!(reduction(1000, 10_000, 0, 0) > 0);
    }
}
