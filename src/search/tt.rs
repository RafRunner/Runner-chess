use crate::{chess_move::Move, score::Score, zobrist::ZobristHash};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    Exact, // the score fell inside the window: it is the real value
    Lower, // a beta cutoff: the real value is >= score
    Upper, // no move beat alpha: the real value is <= score
}

impl Bound {
    pub fn new(score: Score, alpha: Score, beta: Score) -> Self {
        if score >= beta {
            Self::Lower
        } else if score > alpha {
            Self::Exact
        } else {
            Self::Upper
        }
    }
}

#[derive(Clone, Copy)]
pub struct Entry {
    key: ZobristHash, // the whole hash, to tell apart positions that share a slot
    pub best_move: Option<Move>,
    pub score: Score, // mates counted from this position, not the root (Score::to_tt)
    pub depth: u8,
    pub bound: Bound,
}

impl Entry {
    pub fn new(
        hash: ZobristHash,
        depth: u32,
        score: Score,
        bound: Bound,
        best_move: Option<Move>,
    ) -> Self {
        Self {
            key: hash,
            best_move,
            score,
            depth: depth.min(u8::MAX.into()) as u8,
            bound,
        }
    }
}

pub struct TranspositionTable {
    entries: Vec<Option<Entry>>, // a power of 2 long, so the index is the low bits of the hash
}

impl TranspositionTable {
    pub fn new(megabytes: usize) -> Self {
        let bytes = megabytes * 1024 * 1024;
        let capacity = (bytes / size_of::<Option<Entry>>()).max(1);

        let capacity = 1 << capacity.ilog2();

        Self {
            entries: vec![None; capacity],
        }
    }

    pub fn clear(&mut self) {
        self.entries.fill(None);
    }

    pub fn probe(&self, hash: ZobristHash) -> Option<Entry> {
        let index = self.index(hash);

        self.entries[index].filter(|entry| entry.key == hash)
    }

    pub fn store(
        &mut self,
        hash: ZobristHash,
        depth: u32,
        score: Score,
        bound: Bound,
        best_move: Option<Move>,
    ) {
        let index = self.index(hash);
        self.entries[index] = Some(Entry::new(hash, depth, score, bound, best_move));
    }

    fn index(&self, hash: ZobristHash) -> usize {
        hash.raw() as usize & (self.entries.len() - 1)
    }
    // later: new_search() (a generation, to age entries) and hashfull() for `info`
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        board::Board,
        test_utils::{KIWIPETE, PERFT_POSITIONS, board, find_move},
    };

    fn hash(fen: &str) -> ZobristHash {
        board(fen).hash()
    }

    #[test]
    fn an_entry_takes_16_bytes() {
        assert_eq!(size_of::<Option<Entry>>(), 16);
    }

    #[test]
    fn the_largest_power_of_two_that_fits() {
        for megabytes in [1, 3, 64] {
            let bytes =
                TranspositionTable::new(megabytes).entries.len() * size_of::<Option<Entry>>();
            assert!(bytes.is_power_of_two(), "{megabytes}");
            assert!(bytes <= megabytes << 20, "{megabytes}");
            assert!(2 * bytes > megabytes << 20, "{megabytes}");
        }
    }

    #[test]
    fn a_new_table_is_empty() {
        let tt = TranspositionTable::new(1);
        for fen in PERFT_POSITIONS {
            assert!(tt.probe(hash(fen)).is_none(), "{fen}");
        }
    }

    #[test]
    fn probe_finds_what_was_stored() {
        let board = board(Board::STARTPOS);
        let e4 = find_move(&board, "e2e4");
        let mut tt = TranspositionTable::new(1);
        tt.store(board.hash(), 5, Score::new(30), Bound::Lower, Some(e4));

        let entry = tt.probe(board.hash()).expect("just stored");
        assert_eq!(entry.depth, 5);
        assert_eq!(entry.score, Score::new(30));
        assert_eq!(entry.bound, Bound::Lower);
        assert_eq!(entry.best_move, Some(e4));
    }

    #[test]
    fn positions_sharing_a_slot_do_not_mix() {
        // a single entry, so every position falls on it and the last one stays
        let mut tt = TranspositionTable::new(0);
        let (startpos, kiwipete) = (hash(Board::STARTPOS), hash(KIWIPETE));

        tt.store(startpos, 3, Score::DRAW, Bound::Exact, None);
        assert!(tt.probe(kiwipete).is_none());

        tt.store(kiwipete, 1, Score::DRAW, Bound::Exact, None);
        assert!(tt.probe(startpos).is_none());
        assert!(tt.probe(kiwipete).is_some());
    }

    #[test]
    fn clear_empties_the_table() {
        let mut tt = TranspositionTable::new(1);
        let hash = hash(Board::STARTPOS);
        tt.store(hash, 1, Score::DRAW, Bound::Exact, None);

        tt.clear();
        assert!(tt.probe(hash).is_none());

        // and the table still works
        tt.store(hash, 1, Score::DRAW, Bound::Exact, None);
        assert!(tt.probe(hash).is_some());
    }

    #[test]
    fn depths_past_a_byte_saturate() {
        let mut tt = TranspositionTable::new(1);
        let hash = hash(Board::STARTPOS);
        tt.store(hash, 1000, Score::DRAW, Bound::Exact, None);
        assert_eq!(tt.probe(hash).unwrap().depth, u8::MAX);
    }

    #[test]
    fn the_bound_tells_where_the_score_fell() {
        let (alpha, beta) = (Score::new(-50), Score::new(50));
        assert_eq!(Bound::new(Score::new(80), alpha, beta), Bound::Lower);
        assert_eq!(Bound::new(beta, alpha, beta), Bound::Lower);
        assert_eq!(Bound::new(Score::new(10), alpha, beta), Bound::Exact);
        assert_eq!(Bound::new(alpha, alpha, beta), Bound::Upper);
        assert_eq!(Bound::new(Score::new(-80), alpha, beta), Bound::Upper);
    }
}
