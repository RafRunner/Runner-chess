use crate::{
    score::Score,
    search::{NodeInfo, Searcher},
};

impl Searcher<'_> {
    /// The side to move is so far ahead that even passing the turn keeps it above beta:
    /// a shallower search confirms it. `static_eval` is `None` in check, where passing is
    /// illegal. Not on the principal variation, whose score has to be exact
    pub(super) fn null_move_pruning(
        &mut self,
        node: NodeInfo<'_>,
        static_eval: Option<Score>,
    ) -> Option<Score> {
        let board = node.board;
        let eval = static_eval?;

        if node.ply > 0
            && !node.is_pv()
            && node.depth >= 3
            && self.null_ply != Some(node.ply - 1)
            && board.has_non_pawn_material(board.side_to_move()) // zugzwang protection
            && eval >= node.beta
        {
            // the further above beta, the less the search needs to confirm it
            let surplus = eval
                .centipawns()
                .zip(node.beta.centipawns())
                .map_or(0, |(eval, beta)| (i32::from(eval) - i32::from(beta)) / 200);
            let reduce = 3 + node.depth / 3 + surplus.min(3) as u32;
            let null = board.make_null_move();

            let previous = self.null_ply.replace(node.ply);
            let score = self.search_child(
                node.board,
                node.scout_child(&null, reduce, -node.beta),
                None,
            );
            self.null_ply = previous;

            if score >= node.beta {
                return Some(if score.is_mate() { node.beta } else { score });
            }
        }

        None
    }
}
