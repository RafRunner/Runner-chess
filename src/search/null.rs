use crate::{
    score::Score,
    search::{NodeInfo, Searcher},
};

impl Searcher<'_> {
    /// The side to move is so far ahead that even passing the turn
    /// keeps it above beta: a shallower search confirms it
    pub(super) fn null_move_pruning(&mut self, node: NodeInfo<'_>) -> Option<Score> {
        let board = node.board;

        if node.ply > 0
            && node.depth >= 3
            && self.null_ply != Some(node.ply - 1)
            && !board.is_in_check(board.side_to_move())
            && board.has_non_pawn_material(board.side_to_move()) // zugzwang protection
            && self.evaluate(node) >= node.beta
        {
            let reduce = 2 + node.depth / 4;
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
