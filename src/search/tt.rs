use crate::{chess_move::Move, score::Score, zobrist::ZobristHash};

/// What a search found out about the value of a position, given the window it was
/// searched with. Alpha-beta rarely needs the exact value: knowing that it falls
/// outside the window is often enough, and only that much gets searched.
///
/// The names say what the score is for the real value, not how the search ended, so
/// they cross: failing low gives an upper bound, failing high a lower one.
///
/// | search ended     | real value | can stand for a later search when |
/// |------------------|------------|-----------------------------------|
/// | inside the window| = score    | always                            |
/// | fail high (>= β) | >= score   | score >= its beta                 |
/// | fail low (<= α)  | <= score   | score <= its alpha                |
///
/// Only the window of the later search matters, not the one the entry came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    /// The score fell inside the window: it is the real value
    Exact,
    /// A move reached beta and cut the search: the real value is at least the score,
    /// maybe more, since the remaining moves were never searched
    Lower,
    /// No move beat alpha: the real value is at most the score
    Upper,
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
    entries: Vec<Option<Entry>>, // any length: the index scales the hash to it
}

impl TranspositionTable {
    /// As many entries as fit in `megabytes`, and at least the 1 MB of the `Hash` option
    pub fn new(megabytes: usize) -> Self {
        let bytes = megabytes.max(1) * 1024 * 1024;
        Self::with_entries(bytes / size_of::<Option<Entry>>())
    }

    fn with_entries(entries: usize) -> Self {
        Self {
            entries: vec![None; entries],
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

    /// The hash as a fraction of 2^64, times the length: always inside the table, and
    /// from the high bits of the hash
    fn index(&self, hash: ZobristHash) -> usize {
        ((hash.raw() as u128 * self.entries.len() as u128) >> 64) as usize
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
    fn the_table_takes_every_megabyte_it_gets() {
        // as many entries as fit, whatever the size
        let entry = size_of::<Option<Entry>>();
        for megabytes in [1, 3, 100] {
            let bytes = TranspositionTable::new(megabytes).entries.len() * entry;
            assert!(bytes <= megabytes << 20, "{megabytes}");
            assert!(bytes + entry > megabytes << 20, "{megabytes}");
        }
        // no less than the `Hash` option allows
        assert_eq!(
            TranspositionTable::new(0).entries.len(),
            TranspositionTable::new(1).entries.len()
        );
    }

    #[test]
    fn a_table_of_any_size_keeps_what_it_gets() {
        // 3 MB isn't a power of two of entries, and the index still lands inside
        let mut tt = TranspositionTable::new(3);
        for fen in PERFT_POSITIONS {
            tt.store(hash(fen), 1, Score::DRAW, Bound::Exact, None);
            assert!(tt.probe(hash(fen)).is_some(), "{fen}");
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
        let mut tt = TranspositionTable::with_entries(1);
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
