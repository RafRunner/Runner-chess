use crate::{board::Board, score::Score, search::Searcher};

impl Searcher<'_> {
    /// Draws by the rules, checked before quiescence so that a repetition right at
    /// the horizon still counts. The root has to return a move anyway.
    pub(super) fn check_draw(&self, board: &Board, ply: u32) -> Option<Score> {
        if ply == 0 {
            return None;
        }

        if self.is_repetition(board) {
            return Some(Score::DRAW);
        }
        if board.halfmove_clock() >= 100 {
            // unless the move that completed the fifty moves gave mate
            let mated = board.is_in_check(board.side_to_move())
                && board.legal_successors().next().is_none();

            return Some(if mated {
                Score::mated_in(ply)
            } else {
                Score::DRAW
            });
        }

        None
    }

    /// Where the current position appeared before, the most recent first: every other
    /// ply, back to the last capture or pawn move, since nothing older can come back
    fn earlier_occurrences(&self, board: &Board) -> impl Iterator<Item = usize> + '_ {
        debug_assert_eq!(self.hashes.last(), Some(&board.hash()));

        let current = self.hashes.len() - 1;
        let reach = (board.halfmove_clock() as usize).min(current);

        (4..=reach)
            .step_by(2)
            .map(move |back| current - back)
            .filter(move |&index| self.hashes[index] == self.hashes[current])
    }

    /// A draw by repetition, as the search counts it: once is enough inside the search,
    /// but the game needs the position twice before, which makes the third time
    fn is_repetition(&self, board: &Board) -> bool {
        let mut earlier = self.earlier_occurrences(board);
        match earlier.next() {
            Some(index) if index > self.root_index => true,
            // the most recent was already in the game, so any other one is too
            Some(_) => earlier.next().is_some(),
            None => false,
        }
    }

    /// The position already appeared in the game or in the line being searched, so its
    /// score may depend on the way to it, and an entry from another way can't be trusted
    pub(super) fn has_appeared(&self, board: &Board) -> bool {
        self.earlier_occurrences(board).next().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        search::{
            Budget,
            tests::{Searched, assert_best, score, search},
        },
        test_utils::{LOST_KING, board, both_colors, play_with_history, static_eval},
    };

    #[test]
    fn perpetual_check_saves_the_lost_side() {
        // White is lost, but Qe8+ Kh7 Qh5+ Kg8 Qe8+ repeats, which the iterations reach
        // by depth 5. A single search of depth 5 doesn't: without moves from the table,
        // every node of it gets a ply less
        for fen in both_colors("6k1/rr4p1/8/8/8/8/ppp1Q1PP/7K w - - 0 1") {
            assert!(static_eval(&board(&fen)) < Score::new(-100), "{fen}");

            let searched = search(&board(&fen), &[], Budget::depth(Some(5)));
            assert_eq!(searched.score, Some(Score::DRAW), "{fen}");
        }
    }

    #[test]
    fn fifty_move_rule() {
        // 99 plies without a capture or pawn move: only the pawn move doesn't draw
        for depth in 1..=3 {
            assert_best("6k1/8/8/8/8/7P/8/R5K1 w - - 99 80", depth, "h3h4");
        }
    }

    #[test]
    fn mate_beats_the_fifty_move_rule() {
        // Ra8# is the hundredth ply, and still mate
        for fen in both_colors("6k1/5ppp/8/8/8/8/8/R5K1 w - - 99 80") {
            assert_eq!(score(&fen, 2), Score::mate_in(1), "{fen}");
        }
    }

    #[test]
    fn trades_into_a_dead_draw_when_lost() {
        // a knight down, with Rxd8# coming, Black holds only by trading rooks: K+N vs K
        // can't be won. K+R+N vs K+R alone is nearly a draw for the network, so the mate
        // makes the other moves lose. It shows from depth 5 on
        let fen = "3r3k/8/6K1/8/8/4N3/8/3R4 b - - 0 1";
        for depth in 6..=7 {
            assert_best(fen, depth, "d8d1");
        }
        for fen in both_colors(fen) {
            assert_eq!(score(&fen, 6), Score::DRAW, "{fen}");
        }
    }

    /// Plays `moves` from `fen` as the game so far, then searches the position they reach
    fn search_after(fen: &str, moves: &str, depth: u32) -> Searched {
        let (board, history) = play_with_history(fen, moves);
        search(&board, &history, Budget::depth(Some(depth)))
    }

    #[test]
    fn repetitions_come_from_the_game_too() {
        // Kh8 brings back a position the game already had twice
        let moves = "g8h8 b1c1 h8g7 c1b1 g7h8 b1c1 h8g8 c1b1";

        // at depth 1 the repetition is right at the horizon, so it has to be checked
        // before quiescence
        for depth in 1..=3 {
            let Searched { best, out, .. } = search_after(LOST_KING, moves, depth);
            assert_eq!(best.as_deref(), Some("g8h8"), "{out}");
            assert!(out.contains(" score cp 0 "), "{out}");

            // the same position without the game behind it is just lost
            let out = search_after(LOST_KING, "", depth).out;
            assert!(!out.contains(" score cp 0 "), "{out}");
        }
    }

    #[test]
    fn a_single_repetition_of_the_game_is_not_a_draw_yet() {
        // the game had the position after Kh8 only once: playing it again makes it the
        // second time, and White doesn't have to repeat it a third
        for depth in 1..=3 {
            let out = search_after(LOST_KING, "g8h8 b1c1 h8g8 c1b1", depth).out;
            assert!(!out.contains(" score cp 0 "), "{out}");
        }
    }

    #[test]
    fn repetitions_inside_the_search_count_once_after_a_long_game() {
        // the perpetual of perpetual_check_saves_the_lost_side, after two quiet moves:
        // the halfmove clock reaches back into the game, but the repetition itself is
        // all inside the search, where once is enough
        let fen = "6k1/r5p1/1r6/8/8/8/ppp1Q1PP/6K1 w - - 0 1";
        let Searched { best, out, .. } = search_after(fen, "g1h1 b6b7", 5);
        assert_eq!(best.as_deref(), Some("e2e8"), "{out}");
        assert!(out.contains(" score cp 0 "), "{out}");
    }
}
