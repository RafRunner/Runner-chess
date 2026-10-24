use crate::{
    score::Score,
    search::{NodeInfo, Searcher},
};

/// Reverse futility pruning only trusts the static eval this close to the horizon
const RFP_MAX_DEPTH: u32 = 7;
/// What the opponent can still win back, per ply left
const RFP_MARGIN: i16 = 80;

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
