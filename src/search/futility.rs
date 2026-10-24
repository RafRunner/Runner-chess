use crate::{
    board::Board,
    chess_move::Move,
    score::Score,
    search::{NodeInfo, Searcher},
};

/// Reverse futility pruning only trusts the static eval this close to the horizon
const RFP_MAX_DEPTH: u32 = 7;
/// What the opponent can still win back, per ply left
const RFP_MARGIN: i16 = 80;

/// Quiet moves are only skipped this close to the horizon
const LMP_MAX_DEPTH: u32 = 8;
const FP_MAX_DEPTH: u32 = 6;
/// What a quiet move can still be worth with one ply left, and with each ply more
const FP_BASE: i16 = 100;
const FP_MARGIN: i16 = 100;

impl Searcher<'_> {
    /// The static eval is so far above beta that the few plies left can't bring it
    /// back down: the node fails high without searching a move. Never on the principal
    /// variation, whose score has to be exact
    pub(super) fn reverse_futility_pruning(
        node: NodeInfo<'_>,
        static_eval: Score,
    ) -> Option<Score> {
        let margin = RFP_MARGIN * node.depth as i16;

        (!node.is_pv()
            && node.depth <= RFP_MAX_DEPTH
            && !node.beta.is_mate()
            && static_eval - margin >= node.beta)
            .then_some(static_eval)
    }
}

/// A quiet move, that doesn't give check, of a node that already has a move that
/// doesn't lose to a mate, that isn't worth searching:
/// - late move pruning: so many quiet moves came before it, in a good order, that it's
///   very unlikely to be the one that raises alpha
/// - futility pruning: the static eval is so far below alpha that a quiet move can't
///   make up for it in the plies left
///
/// `static_eval` is `None` in check, where every evasion gets searched.
pub(super) fn prunes_quiet(
    node: NodeInfo<'_>,
    static_eval: Option<Score>,
    best: Score,
    quiets_tried: usize,
    mv: Move,
    next: &Board,
) -> bool {
    let Some(static_eval) = static_eval else {
        return false;
    };
    if node.ply == 0 || best <= -Score::MAX_EVAL || !mv.is_quiet() {
        return false;
    }
    if next.is_in_check(next.side_to_move()) {
        return false;
    }

    let late = node.depth <= LMP_MAX_DEPTH && quiets_tried >= lmp_threshold(node.depth);
    let futile = node.depth <= FP_MAX_DEPTH
        && static_eval + FP_BASE + FP_MARGIN * node.depth as i16 <= node.alpha;

    late || futile
}

/// How many quiet moves a node `depth` plies deep tries before skipping the rest
fn lmp_threshold(depth: u32) -> usize {
    3 + (depth * depth) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{board, find_move};

    const QUIET_FEN: &str = "4k3/pppp4/8/8/8/8/PPPP4/4K1N1 w - - 0 1";

    /// What `prunes_quiet` says about Nf3 from `QUIET_FEN`, in a node `depth` plies deep
    /// at ply 3, with alpha at `alpha` and a static eval of `eval`
    fn prunes(depth: u32, eval: Option<i32>, alpha: i32, best: Score, quiets: usize) -> bool {
        let board = board(QUIET_FEN);
        let mv = find_move(&board, "g1f3");
        let next = board.make_move(mv);
        let node = NodeInfo::new(&board, depth, 3, Score::new(alpha), Score::new(alpha + 1));
        prunes_quiet(node, eval.map(Score::new), best, quiets, mv, &next)
    }

    #[test]
    fn late_quiet_moves_get_skipped() {
        let best = Score::new(0);
        assert!(!prunes(2, Some(0), 0, best, lmp_threshold(2) - 1));
        assert!(prunes(2, Some(0), 0, best, lmp_threshold(2)));
        // too far from the horizon
        assert!(!prunes(LMP_MAX_DEPTH + 1, Some(0), 0, best, 1000));
    }

    #[test]
    fn futile_quiet_moves_get_skipped() {
        let best = Score::new(-500);
        // one ply left: 200 below alpha is enough
        assert!(prunes(1, Some(-200), 0, best, 0));
        assert!(!prunes(1, Some(-199), 0, best, 0));
        // each ply more needs another 100
        assert!(!prunes(2, Some(-200), 0, best, 0));
        assert!(prunes(2, Some(-300), 0, best, 0));
    }

    #[test]
    fn nothing_gets_skipped_without_a_safe_move_or_in_check() {
        // the only moves so far get mated, or none was searched
        assert!(!prunes(1, Some(-1000), 0, Score::mated_in(5), 100));
        assert!(!prunes(1, Some(-1000), 0, -Score::INF, 100));
        // in check there's no static eval
        assert!(!prunes(1, None, 0, Score::new(0), 100));
    }

    #[test]
    fn checks_and_captures_never_get_skipped() {
        let board = board("4k3/8/8/8/8/8/3p4/R3K3 w - - 0 1");
        let node = NodeInfo::new(&board, 1, 3, Score::new(0), Score::new(1));
        let eval = Some(Score::new(-1000));
        for uci in ["a1a8", "e1d2"] {
            let mv = find_move(&board, uci);
            let next = board.make_move(mv);
            assert!(
                !prunes_quiet(node, eval, Score::new(0), 100, mv, &next),
                "{uci}"
            );
        }
    }
}
