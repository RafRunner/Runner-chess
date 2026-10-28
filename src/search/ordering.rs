use crate::{
    board::Board,
    chess_move::{Move, MoveKind, PromotionPiece},
    piece::{Color, PieceKind},
    score::Score,
    search::NodeInfo,
};

pub struct MoveOrdering {
    killers: [[Option<Move>; 2]; Score::MAX_PLY as usize],
    // history is 32 KB. We store it in a box to avoid copying all values on move
    history: Box<[[[i32; 64]; 64]; 2]>,
    // helper vector to speed up ordering
    keys: Vec<u64>,
}

impl MoveOrdering {
    const NOISY_BASE: i32 = 2_000_000;
    const KILLER_BASE: i32 = 1_000_000;
    /// Captures that lose material in the exchange come after every quiet move
    const BAD_CAPTURE_BASE: i32 = -1_000_000;
    /// Rook and bishop promotions come after every quiet move: a queen does all they do,
    /// and the knight's moves are the only ones it lacks
    const UNDERPROMOTION_BASE: i32 = -2_000_000;
    /// Only for the order of captures and promotions
    const VALUES: [i32; 6] = [100, 320, 330, 500, 900, 0];
    pub(super) const MAX_BONUS: i32 = 16_384;

    pub fn new() -> Self {
        Self {
            killers: [[None; 2]; Score::MAX_PLY as usize],
            history: Box::new([[[0; 64]; 64]; 2]),
            keys: Vec::with_capacity(256),
        }
    }

    pub fn new_search(&mut self) {
        self.killers = [[None; 2]; Score::MAX_PLY as usize];

        for color in self.history.iter_mut() {
            for to in color.iter_mut() {
                for bonus in to.iter_mut() {
                    *bonus /= 2;
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.killers = [[None; 2]; Score::MAX_PLY as usize];
        *self.history = [[[0; 64]; 64]; 2];
    }

    pub(super) fn on_beta_cutoff(&mut self, node: NodeInfo, mv: Move, quiets_tried: &[Move]) {
        if !mv.is_quiet() {
            return;
        }
        let old = self.killers[node.ply as usize][0].replace(mv);
        if old != Some(mv) {
            self.killers[node.ply as usize][1] = old;
        }

        let bonus = (300 * node.depth as i32 - 250).min(Self::MAX_BONUS);

        self.update_history(node.board.side_to_move(), mv, bonus);

        for &mv in quiets_tried {
            self.update_history(node.board.side_to_move(), mv, -bonus);
        }
    }

    pub(super) fn is_killer(&self, mv: Move, ply: u32) -> bool {
        self.killers[ply as usize][0] == Some(mv) || self.killers[ply as usize][1] == Some(mv)
    }

    pub(super) fn history_bonus(&self, mv: Move, color: Color) -> i32 {
        self.history[color][mv.from()][mv.to()]
    }

    pub(super) fn legal_ordered<'board>(
        &mut self,
        board: &'board Board,
        ply: u32,
        first: Option<Move>,
    ) -> impl Iterator<Item = (Move, Board)> + 'board {
        let moves = self.sorted(board.generate_moves(), board, ply, first);
        Self::made(moves.into_iter(), board)
    }

    /// The captures that don't lose material in the exchange (see [`Board::see`]): the
    /// others won't improve on standing pat. Checked only as they come, since a cutoff
    /// often leaves the later ones unsearched
    pub(super) fn winning_captures_ordered<'board>(
        &mut self,
        board: &'board Board,
    ) -> impl Iterator<Item = (Move, Board)> + 'board {
        let moves = self.sorted(board.generate_captures(), board, 0, None);
        Self::made(moves.into_iter().filter(|&mv| board.see(mv, 0)), board)
    }

    /// Possibly strong moves come first
    fn sorted(
        &mut self,
        mut moves: Vec<Move>,
        board: &Board,
        ply: u32,
        first: Option<Move>,
    ) -> Vec<Move> {
        self.keys.clear();

        // sorting u64 is a lot faster and avoids allocations and swaps in a (Move, Board) vec
        for (i, &mv) in moves.iter().enumerate() {
            let key = self.move_sort_key(board, ply, mv, first);
            self.keys.push(pack(key, i, mv));
        }
        self.keys.sort_unstable();
        for (mv, &packed) in moves.iter_mut().zip(self.keys.iter()) {
            *mv = unpack(packed);
        }

        moves
    }

    /// The legal ones of `moves`, made on `board`
    fn made<'board>(
        moves: impl Iterator<Item = Move> + 'board,
        board: &'board Board,
    ) -> impl Iterator<Item = (Move, Board)> + 'board {
        moves.filter_map(move |mv| board.make_legal_move(mv).map(|next| (mv, next)))
    }

    fn update_history(&mut self, color: Color, mv: Move, bonus: i32) {
        let history = &mut self.history[color];
        let h = history[mv.from()][mv.to()];
        history[mv.from()][mv.to()] += bonus - h * bonus.abs() / Self::MAX_BONUS;
    }

    fn move_sort_key(&self, board: &Board, ply: u32, mv: Move, first: Option<Move>) -> i32 {
        if first == Some(mv) {
            return i32::MAX;
        }
        if !mv.is_quiet() {
            return Self::noisy_key(board, mv, true);
        }

        match self.killers[ply as usize]
            .iter()
            .position(|&killer| killer == Some(mv))
        {
            Some(idx) => Self::KILLER_BASE + (1 - idx as i32),
            None => self.history_bonus(mv, board.side_to_move()),
        }
    }

    /// Captures and promotions by the material they win, then by the cheaper piece
    /// moving: a promotion wins the new piece, less the pawn. With `see`, a capture that
    /// loses material in the exchange (see [`Board::see`]) goes after the quiet moves
    fn noisy_key(board: &Board, mv: Move, see: bool) -> i32 {
        let attacker = board
            .piece_at(mv.from())
            .map_or(0, |p| p.kind().index() as i32);

        let mut gain = 0;
        if mv.is_capture() {
            // en passant takes a pawn that isn't on the target square
            let victim = board
                .piece_at(mv.to())
                .map_or(PieceKind::Pawn, |p| p.kind());
            gain += Self::VALUES[victim];
        }

        let base = match mv.kind() {
            MoveKind::Promotion(piece) | MoveKind::PromotionCapture(piece) => {
                gain += Self::VALUES[piece.to_piece_kind()] - Self::VALUES[PieceKind::Pawn];
                match piece {
                    PromotionPiece::Rook | PromotionPiece::Bishop => Self::UNDERPROMOTION_BASE,
                    PromotionPiece::Queen | PromotionPiece::Knight => Self::NOISY_BASE,
                }
            }
            _ if see && !board.see(mv, 0) => Self::BAD_CAPTURE_BASE,
            _ => Self::NOISY_BASE,
        };

        // the 8 keeps any difference in gain above the attacker's 0 to 5
        base + 8 * gain + (5 - attacker)
    }
}

impl Default for MoveOrdering {
    fn default() -> Self {
        Self::new()
    }
}

/// A move with its key, so that sorting puts the highest keys first, and among equal
/// ones the move that came first
fn pack(key: i32, idx: usize, mv: Move) -> u64 {
    // i32 -> u32 keeping the order, inverted for the highest first
    let inverted = !((key as u32) ^ 0x8000_0000);
    (u64::from(inverted) << 32) | ((idx as u64) << 16) | u64::from(mv.raw())
}

fn unpack(packed: u64) -> Move {
    Move::from_raw(packed as u16) // the low 16 bits are the move
}

fn is_underpromotion(mv: Move) -> bool {
    matches!(
        mv.kind(),
        MoveKind::Promotion(PromotionPiece::Rook | PromotionPiece::Bishop)
            | MoveKind::PromotionCapture(PromotionPiece::Rook | PromotionPiece::Bishop)
    )
}

/// The moves of a node in the order of [`MoveOrdering::legal_ordered`], but sorted a
/// stage at a time, since the first ones often cut: the move of the table, the noisy
/// moves that don't lose material, the quiet ones, the captures that do, and the rook and
/// bishop promotions. A capture's exchange only gets evaluated when its turn comes. The
/// quiet moves get their keys as the node starts: with the history that the moves before
/// them leave, the tree grew a third. The moves aren't checked for legality
pub(super) struct MovePicker {
    stage: Stage,
    /// Only if it's one of the moves here
    tt_move: Option<Move>,
    /// Packed with their keys by [`pack`]
    noisy: Vec<u64>,
    next_noisy: usize,
    quiets: Vec<u64>,
    next_quiet: usize,
    /// In the order they came
    losing_captures: Vec<Move>,
    next_losing: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    TtMove,
    SortNoisy,
    GoodNoisy,
    SortQuiets,
    Quiets,
    LosingCaptures,
    Underpromotions,
    Done,
}

impl MovePicker {
    pub(super) fn new(
        board: &Board,
        tt_move: Option<Move>,
        ordering: &MoveOrdering,
        ply: u32,
    ) -> Self {
        let noisy = board.generate_noisy();
        let quiets = board.generate_quiets();
        let tt_move = tt_move.filter(|mv| noisy.contains(mv) || quiets.contains(mv));
        let packed = |moves: Vec<Move>, key: &dyn Fn(Move) -> i32| -> Vec<u64> {
            let indexed = moves.into_iter().enumerate();
            indexed.map(|(i, mv)| pack(key(mv), i, mv)).collect()
        };

        Self {
            stage: Stage::TtMove,
            tt_move,
            noisy: packed(noisy, &|mv| MoveOrdering::noisy_key(board, mv, false)),
            next_noisy: 0,
            quiets: packed(quiets, &|mv| {
                ordering.move_sort_key(board, ply, mv, tt_move)
            }),
            next_quiet: 0,
            losing_captures: Vec::new(),
            next_losing: 0,
        }
    }

    pub(super) fn next(&mut self, board: &Board) -> Option<Move> {
        let tt_move = self.tt_move;
        loop {
            match self.stage {
                Stage::TtMove => {
                    self.stage = Stage::SortNoisy;
                    if tt_move.is_some() {
                        return tt_move;
                    }
                }
                Stage::SortNoisy => {
                    self.noisy.sort_unstable();
                    self.stage = Stage::GoodNoisy;
                }
                Stage::GoodNoisy => {
                    // the underpromotions, sorted last, wait for their own stage
                    while let Some(&packed) = self.noisy.get(self.next_noisy)
                        && !is_underpromotion(unpack(packed))
                    {
                        self.next_noisy += 1;
                        let mv = unpack(packed);
                        if Some(mv) == tt_move {
                            continue;
                        }
                        if !mv.is_promotion() && !board.see(mv, 0) {
                            self.losing_captures.push(mv);
                            continue;
                        }
                        return Some(mv);
                    }
                    self.stage = Stage::SortQuiets;
                }
                Stage::SortQuiets => {
                    self.quiets.sort_unstable();
                    self.stage = Stage::Quiets;
                }
                Stage::Quiets => {
                    while let Some(&packed) = self.quiets.get(self.next_quiet) {
                        self.next_quiet += 1;
                        if Some(unpack(packed)) != tt_move {
                            return Some(unpack(packed));
                        }
                    }
                    self.stage = Stage::LosingCaptures;
                }
                Stage::LosingCaptures => {
                    if let Some(&mv) = self.losing_captures.get(self.next_losing) {
                        self.next_losing += 1;
                        return Some(mv);
                    }
                    self.stage = Stage::Underpromotions;
                }
                Stage::Underpromotions => {
                    while let Some(&packed) = self.noisy.get(self.next_noisy) {
                        self.next_noisy += 1;
                        if Some(unpack(packed)) != tt_move {
                            return Some(unpack(packed));
                        }
                    }
                    self.stage = Stage::Done;
                }
                Stage::Done => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;
    use crate::{
        search::{Budget, Searcher, TranspositionTable},
        test_utils::{KIWIPETE, PERFT_POSITIONS, board, both_colors, find_move, legal_uci, sorted},
    };

    /// The legal moves of `fen` in the order the search tries them at `ply`, with `first`
    /// (the move from the TT) put before the others.
    fn ordered_with(
        ordering: &mut MoveOrdering,
        fen: &str,
        ply: u32,
        first: Option<Move>,
    ) -> Vec<Move> {
        ordering
            .legal_ordered(&board(fen), ply, first)
            .map(|(mv, _)| mv)
            .collect()
    }

    /// The order before the search taught anything to the ordering.
    fn ordered(fen: &str) -> Vec<Move> {
        ordered_after(fen, None)
    }

    /// The same, with `first` put before the others.
    fn ordered_after(fen: &str, first: Option<Move>) -> Vec<Move> {
        ordered_with(&mut MoveOrdering::new(), fen, 0, first)
    }

    fn ordered_uci(fen: &str) -> Vec<String> {
        ordered(fen).into_iter().map(Move::to_uci).collect()
    }

    fn index_of(moves: &[String], uci: &str) -> usize {
        moves
            .iter()
            .position(|mv| mv == uci)
            .unwrap_or_else(|| panic!("{uci} not in {moves:?}"))
    }

    /// The quiet moves of `moves`, in the order they come.
    fn quiets(moves: &[Move]) -> Vec<String> {
        moves
            .iter()
            .filter(|mv| mv.is_quiet())
            .map(|mv| mv.to_uci())
            .collect()
    }

    /// The quiet moves of `fen` before the search taught anything to the ordering.
    fn fresh_quiets(fen: &str) -> Vec<String> {
        quiets(&ordered(fen))
    }

    /// `uci` caused a beta cutoff in `fen`, after the quiet moves `tried` failed to.
    fn cut(
        ordering: &mut MoveOrdering,
        fen: &str,
        uci: &str,
        ply: u32,
        depth: u32,
        tried: &[&str],
    ) {
        let position = board(fen);
        let tried: Vec<Move> = tried.iter().map(|t| find_move(&position, t)).collect();
        ordering.on_beta_cutoff(
            NodeInfo::full_window(&position, depth, ply),
            find_move(&position, uci),
            &tried,
        );
    }

    /// Captures and promotions come before the quiet moves, and captures that lose
    /// material, then rook and bishop promotions, after them.
    fn assert_noisy_first(board: &Board, moves: &[Move]) {
        let band = |mv: &Move| match mv.kind() {
            MoveKind::Promotion(PromotionPiece::Rook | PromotionPiece::Bishop)
            | MoveKind::PromotionCapture(PromotionPiece::Rook | PromotionPiece::Bishop) => 3,
            _ if mv.is_quiet() => 1,
            _ if !mv.is_promotion() && !board.see(*mv, 0) => 2,
            _ => 0,
        };
        assert!(moves.iter().map(band).is_sorted(), "{moves:?}");
    }

    // the picker

    /// Every move `MovePicker` gives, with `ordering` the same throughout
    fn picked(
        ordering: &MoveOrdering,
        board: &Board,
        ply: u32,
        tt_move: Option<Move>,
    ) -> Vec<Move> {
        let mut picker = MovePicker::new(board, tt_move, ordering, ply);
        std::iter::from_fn(|| picker.next(board)).collect()
    }
    #[test]
    fn the_picker_gives_the_order_of_a_full_sort() {
        // the search leaves killers and history for the ordering to use
        let mut ordering = MoveOrdering::new();
        let fens: Vec<String> = PERFT_POSITIONS.into_iter().flat_map(both_colors).collect();
        for fen in &fens {
            let mut tt = TranspositionTable::new(1);
            Searcher::new(Budget::depth(Some(6)), &mut tt, &mut ordering)
                .search_root(&board(fen), &[], &mut io::sink())
                .unwrap();
        }

        // a move of another position, which the picker has to leave out
        let elsewhere = find_move(&board(KIWIPETE), "e2a6");
        for fen in &fens {
            let position = board(fen);
            let moves = position.generate_moves();
            let tt_moves = moves
                .iter()
                .copied()
                .map(Some)
                .chain([None, Some(elsewhere)]);
            for (tt_move, ply) in tt_moves.zip((0..4).cycle()) {
                let expected = ordering.sorted(position.generate_moves(), &position, ply, tt_move);
                assert_eq!(
                    picked(&ordering, &position, ply, tt_move),
                    expected,
                    "{fen}, {tt_move:?} from the table, at ply {ply}"
                );
            }
        }
    }

    // the order of a fresh ordering

    #[test]
    fn ordering_keeps_every_legal_move() {
        for fen in PERFT_POSITIONS {
            assert_eq!(sorted(ordered_uci(fen)), legal_uci(fen), "{fen}");
        }
    }

    #[test]
    fn the_tt_move_comes_first_and_only_once() {
        // the last move gets promoted, and the others keep their order
        for fen in PERFT_POSITIONS {
            let moves = ordered(fen);
            let last = *moves.last().unwrap();
            let after = ordered_after(fen, Some(last));

            assert_eq!(after[0], last, "{fen}");
            assert_eq!(after[1..], moves[..moves.len() - 1], "{fen}");
        }
    }

    #[test]
    fn captures_come_before_quiet_moves() {
        for fen in PERFT_POSITIONS {
            assert_noisy_first(&board(fen), &ordered(fen));
        }
    }

    #[test]
    fn most_valuable_victim_first() {
        // Qxd7 takes a rook, exf5 only a knight, even though the pawn is the cheaper attacker
        let moves = ordered_uci("6k1/3r4/8/5n2/4P3/8/8/3Q2K1 w - - 0 1");
        assert_eq!(moves[0], "d1d7", "{moves:?}");
        assert!(
            index_of(&moves, "d1d7") < index_of(&moves, "e4f5"),
            "{moves:?}"
        );
    }

    #[test]
    fn least_valuable_attacker_first() {
        // the same knight, taken by a pawn or by the queen
        let moves = ordered_uci("6k1/8/8/5n2/4P3/7Q/8/6K1 w - - 0 1");
        assert!(
            index_of(&moves, "e4f5") < index_of(&moves, "h3f5"),
            "{moves:?}"
        );
    }

    #[test]
    fn a_capture_that_loses_material_comes_after_the_quiet_moves() {
        // Qxd5 exd5: the queen for a pawn
        let moves = ordered_uci("4k3/8/4p3/3p4/8/8/8/3QK3 w - - 0 1");
        assert_eq!(moves.last().map(String::as_str), Some("d1d5"), "{moves:?}");
    }

    #[test]
    fn en_passant_is_ordered_as_a_capture() {
        // the target square is empty, but the victim is a pawn
        let moves = ordered_uci("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
        assert_eq!(moves[0], "e5d6", "{moves:?}");
    }

    #[test]
    fn a_promotion_counts_as_taking_the_new_piece_less_the_pawn() {
        // a queen less a pawn: less than taking a queen, more than a rook
        let moves = ordered_uci("7k/1P6/8/3r1q2/4P3/8/8/6K1 w - - 0 1");
        assert_eq!(moves[..3], ["e4f5", "b7b8q", "e4d5"], "{moves:?}");

        // a knight less a pawn: less than taking a knight, more than a pawn
        let moves = ordered_uci("7k/1P6/8/3n1p2/4P3/8/8/6K1 w - - 0 1");
        assert_eq!(moves[..4], ["b7b8q", "e4d5", "b7b8n", "e4f5"], "{moves:?}");
    }

    #[test]
    fn rook_and_bishop_promotions_come_last() {
        let moves = ordered_uci("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1");
        assert_eq!(moves[..2], ["b7b8q", "b7b8n"], "{moves:?}");
        assert_eq!(moves[moves.len() - 2..], ["b7b8r", "b7b8b"], "{moves:?}");
    }

    #[test]
    fn quiescence_gets_only_the_winning_captures_in_the_same_order() {
        for fen in PERFT_POSITIONS {
            let board = board(fen);
            let captures: Vec<Move> = MoveOrdering::new()
                .winning_captures_ordered(&board)
                .map(|(mv, _)| mv)
                .collect();
            let expected: Vec<Move> = ordered(fen)
                .into_iter()
                .filter(|&mv| mv.is_capture() && board.see(mv, 0))
                .collect();

            assert_eq!(captures, expected, "{fen}");
        }
    }

    // killers

    #[test]
    fn a_killer_is_the_first_quiet_move_of_its_ply() {
        // the last one, so it can only get to the front by being a killer
        let killer = fresh_quiets(KIWIPETE).pop().unwrap();
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);

        let moves = ordered_with(&mut ordering, KIWIPETE, 3, None);
        assert_noisy_first(&board(KIWIPETE), &moves);
        assert_eq!(quiets(&moves)[0], killer, "{moves:?}");
    }

    #[test]
    fn the_tt_move_comes_before_the_killers() {
        let mut fresh = fresh_quiets(KIWIPETE);
        let (killer, tt) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let tt_move = find_move(&board(KIWIPETE), &tt);
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);

        let moves = ordered_with(&mut ordering, KIWIPETE, 3, Some(tt_move));
        assert_eq!(moves[0], tt_move, "{moves:?}");
        assert_noisy_first(&board(KIWIPETE), &moves[1..]);
        assert_eq!(quiets(&moves[1..])[0], killer, "{moves:?}");
    }

    #[test]
    fn killers_belong_to_their_ply() {
        // `strong` cut deeper, so only being a killer puts `killer` before it
        let mut fresh = fresh_quiets(KIWIPETE);
        let (killer, strong) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);
        cut(&mut ordering, KIWIPETE, &strong, 9, 10, &[]);

        let at_3 = quiets(&ordered_with(&mut ordering, KIWIPETE, 3, None));
        assert_eq!(at_3[..2], [killer.as_str(), strong.as_str()], "{at_3:?}");

        let at_4 = quiets(&ordered_with(&mut ordering, KIWIPETE, 4, None));
        assert_eq!(at_4[..2], [strong.as_str(), killer.as_str()], "{at_4:?}");
    }

    #[test]
    fn each_ply_keeps_the_two_newest_killers() {
        // `strong` cut deeper than the others, so it beats any of them that isn't a killer
        let mut fresh = fresh_quiets(KIWIPETE);
        let strong = fresh.pop().unwrap();
        let (a, b, c) = (
            fresh.pop().unwrap(),
            fresh.pop().unwrap(),
            fresh.pop().unwrap(),
        );
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &strong, 9, 10, &[]);
        for mv in [&a, &b, &c] {
            cut(&mut ordering, KIWIPETE, mv, 2, 1, &[]);
        }

        let moves = quiets(&ordered_with(&mut ordering, KIWIPETE, 2, None));
        assert_eq!(
            moves[..4],
            [c.as_str(), b.as_str(), strong.as_str(), a.as_str()],
            "{moves:?}"
        );
    }

    #[test]
    fn a_repeated_killer_does_not_push_out_the_other() {
        let mut fresh = fresh_quiets(KIWIPETE);
        let strong = fresh.pop().unwrap();
        let (a, b) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &strong, 9, 10, &[]);
        for mv in [&a, &b, &b] {
            cut(&mut ordering, KIWIPETE, mv, 2, 1, &[]);
        }

        let moves = quiets(&ordered_with(&mut ordering, KIWIPETE, 2, None));
        assert_eq!(
            moves[..3],
            [b.as_str(), a.as_str(), strong.as_str()],
            "{moves:?}"
        );
    }

    #[test]
    fn killers_reach_the_deepest_ply() {
        // the search orders moves up to MAX_PLY - 1, and evaluates beyond it
        let ply = Score::MAX_PLY - 1;
        let killer = fresh_quiets(KIWIPETE).pop().unwrap();
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, ply, 1, &[]);

        let moves = quiets(&ordered_with(&mut ordering, KIWIPETE, ply, None));
        assert_eq!(moves[0], killer, "{moves:?}");
    }

    #[test]
    fn captures_and_promotions_teach_nothing() {
        // they get ordered by what they take and what they become
        let promotion = "4k3/1P6/8/8/8/8/8/4K3 w - - 0 1";
        for (fen, noisy) in [(KIWIPETE, "e5f7"), (promotion, "b7b8n")] {
            let mut ordering = MoveOrdering::new();
            let before = ordered_with(&mut ordering, fen, 3, None);
            cut(&mut ordering, fen, noisy, 3, 10, &[]);

            for ply in [0, 3] {
                assert_eq!(
                    ordered_with(&mut ordering, fen, ply, None),
                    before,
                    "{fen} {noisy}"
                );
            }
        }
    }

    #[test]
    fn a_noisy_cutoff_keeps_the_killers() {
        // `strong` cut deeper, so `killer` only stays ahead of it while it is a killer
        let mut fresh = fresh_quiets(KIWIPETE);
        let (killer, strong) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &strong, 9, 10, &[]);
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);
        for noisy in ["e5f7", "e5d7"] {
            cut(&mut ordering, KIWIPETE, noisy, 3, 1, &[]);
        }

        let moves = quiets(&ordered_with(&mut ordering, KIWIPETE, 3, None));
        assert_eq!(moves[..2], [killer.as_str(), strong.as_str()], "{moves:?}");
    }

    // history

    #[test]
    fn history_brings_a_cutting_quiet_move_forward_at_every_ply() {
        let late = fresh_quiets(KIWIPETE).pop().unwrap();
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &late, 9, 4, &[]);

        // plies where it isn't a killer
        for ply in [0, 1, 8, 10] {
            let moves = ordered_with(&mut ordering, KIWIPETE, ply, None);
            assert_noisy_first(&board(KIWIPETE), &moves);
            assert_eq!(quiets(&moves)[0], late, "ply {ply}: {moves:?}");
        }
    }

    #[test]
    fn deeper_cutoffs_weigh_more() {
        // `shallow` comes first in a fresh order, so equal bonuses would keep it ahead
        let fresh = fresh_quiets(KIWIPETE);
        let (shallow, deep) = (&fresh[0], &fresh[1]);
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, shallow, 9, 2, &[]);
        cut(&mut ordering, KIWIPETE, deep, 9, 8, &[]);

        let moves = quiets(&ordered_with(&mut ordering, KIWIPETE, 0, None));
        assert!(
            index_of(&moves, deep) < index_of(&moves, shallow),
            "{moves:?}"
        );
    }

    #[test]
    fn quiets_tried_before_a_cutoff_go_last() {
        let fresh = fresh_quiets(KIWIPETE);
        let (tried, cutter) = (&fresh[0], fresh.last().unwrap());
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, cutter, 9, 4, &[tried.as_str()]);

        let moves = quiets(&ordered_with(&mut ordering, KIWIPETE, 0, None));
        assert_eq!(moves.first(), Some(cutter), "{moves:?}");
        assert_eq!(moves.last(), Some(tried), "{moves:?}");
    }

    #[test]
    fn history_stays_below_captures_and_killers() {
        // no amount of cutoffs takes a move out of the quiet band, either way
        let mut fresh = fresh_quiets(KIWIPETE);
        let (killer, pushed) = (fresh.pop().unwrap(), fresh.pop().unwrap());
        let pulled = fresh.remove(0);
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, KIWIPETE, &killer, 3, 1, &[]);

        let kiwipete = board(KIWIPETE);
        let (pushed_move, pulled_move) =
            (find_move(&kiwipete, &pushed), find_move(&kiwipete, &pulled));
        for _ in 0..100_000 {
            ordering.on_beta_cutoff(
                NodeInfo::full_window(&kiwipete, 9, 20),
                pushed_move,
                &[pulled_move],
            );
        }

        let at_0 = ordered_with(&mut ordering, KIWIPETE, 0, None);
        assert_noisy_first(&kiwipete, &at_0);
        let quiets_at_0 = quiets(&at_0);
        assert_eq!(quiets_at_0.first(), Some(&pushed), "{quiets_at_0:?}");
        assert_eq!(quiets_at_0.last(), Some(&pulled), "{quiets_at_0:?}");

        let at_3 = ordered_with(&mut ordering, KIWIPETE, 3, None);
        assert_noisy_first(&kiwipete, &at_3);
        assert_eq!(
            quiets(&at_3)[..2],
            [killer.as_str(), pushed.as_str()],
            "{at_3:?}"
        );
    }

    #[test]
    fn history_is_kept_apart_for_each_color() {
        // the same pieces on the same squares, so every move exists for both colors
        let white = "4k3/8/8/8/8/8/8/R3K3 w - - 0 1";
        let black = "4K3/8/8/8/8/8/8/r3k3 b - - 0 1";
        let before = ordered(black);
        let late = quiets(&before).pop().unwrap();
        let mut ordering = MoveOrdering::new();
        cut(&mut ordering, white, &late, 9, 10, &[]);

        assert_eq!(
            quiets(&ordered_with(&mut ordering, white, 0, None))[0],
            late
        );
        assert_eq!(ordered_with(&mut ordering, black, 0, None), before);
    }
}
