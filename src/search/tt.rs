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
    /// The search that stored it, see [`TranspositionTable::new_search`]
    generation: u8,
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
            generation: 0,
        }
    }

    /// The static eval of the position, made closer to its real value by the score of
    /// the entry, a search of it, where the bound says which side it's on. A mate is
    /// left out: it is counted from where it was stored, and nothing to prune on
    pub fn refine(self, static_eval: Score) -> Score {
        if self.score.is_mate() {
            return static_eval;
        }
        let closer = match self.bound {
            Bound::Exact => true,
            Bound::Lower => self.score > static_eval,
            Bound::Upper => self.score < static_eval,
        };
        if closer { self.score } else { static_eval }
    }
}

/// Two entries share each slot of the table. The first keeps the deepest of the current
/// search, which saved the most work, and the second the latest of the rest, which is
/// likely to be needed again soon
type Bucket = [Option<Entry>; 2];

pub struct TranspositionTable {
    buckets: Vec<Bucket>, // any length: the index scales the hash to it
    /// Of the current search: entries of earlier ones give way to the new ones
    generation: u8,
}

impl TranspositionTable {
    /// As many entries as fit in `megabytes`, and at least the 1 MB of the `Hash` option
    pub fn new(megabytes: usize) -> Self {
        let bytes = megabytes.max(1) * 1024 * 1024;
        Self::with_buckets(bytes / size_of::<Bucket>())
    }

    fn with_buckets(buckets: usize) -> Self {
        let mut table = Vec::with_capacity(buckets);
        ask_for_huge_pages(&mut table);
        // only now written, so that the kernel picks the size of the pages on this touch
        table.resize(buckets, [None; 2]);
        Self {
            buckets: table,
            generation: 0,
        }
    }

    pub fn clear(&mut self) {
        self.buckets.fill([None; 2]);
        self.generation = 0;
    }

    /// The entries stored from now on belong to a new search, and the ones of the earlier
    /// searches, which may be of positions the game left behind, can be replaced by any
    pub fn new_search(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn probe(&self, hash: ZobristHash) -> Option<Entry> {
        self.buckets[self.index(hash)]
            .into_iter()
            .flatten()
            .find(|entry| entry.key == hash)
    }

    pub fn store(
        &mut self,
        hash: ZobristHash,
        depth: u32,
        score: Score,
        bound: Bound,
        best_move: Option<Move>,
    ) {
        let generation = self.generation;
        let index = self.index(hash);
        let bucket = &mut self.buckets[index];
        let mut new = Entry {
            generation,
            ..Entry::new(hash, depth, score, bound, best_move)
        };

        // the same position goes where it was, keeping its move if this search has none
        let same = bucket.iter().position(|e| e.is_some_and(|e| e.key == hash));
        let slot = same.unwrap_or_else(|| match bucket[0] {
            Some(deepest) if deepest.generation == generation && deepest.depth > new.depth => 1,
            _ => 0,
        });
        if let Some(old) = bucket[slot]
            && old.key == hash
        {
            new.best_move = new.best_move.or(old.best_move);
        }
        // the entry it pushes out of the first slot may still be the latest of the rest
        if same.is_none() && slot == 0 {
            bucket[1] = bucket[0];
        }
        bucket[slot] = Some(new);
    }

    /// Asks the memory for the bucket of `hash` without waiting for it, so that it's in
    /// the cache by the time the node probes it: the table is far bigger than the caches
    pub fn prefetch(&self, hash: ZobristHash) {
        let bucket: *const Bucket = &self.buckets[self.index(hash)];
        // SAFETY: a prefetch only hints the cache, it never faults nor writes
        #[cfg(target_arch = "aarch64")]
        unsafe {
            std::arch::asm!(
                "prfm pldl1keep, [{0}]",
                in(reg) bucket,
                options(nostack, readonly, preserves_flags)
            );
        }
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_MM_HINT_T0, _mm_prefetch};
            _mm_prefetch::<_MM_HINT_T0>(bucket.cast());
        }
    }

    /// The hash as a fraction of 2^64, times the length: always inside the table, and
    /// from the high bits of the hash
    fn index(&self, hash: ZobristHash) -> usize {
        ((hash.raw() as u128 * self.buckets.len() as u128) >> 64) as usize
    }
    // later: hashfull() for `info`
}

/// Asks Linux to back the reserved memory of `table` with pages of 2 MB instead of 4 KB.
/// Each probe lands on a random bucket, and with small pages nearly every one also misses
/// the TLB: the prefetches of the search took 11.8% of its time, and 5.6% with huge pages.
/// Only a hint: where the kernel has them off, the pages stay small
#[cfg(target_os = "linux")]
fn ask_for_huge_pages(table: &mut Vec<Bucket>) {
    const HUGE_PAGE: usize = 2 * 1024 * 1024;
    const MADV_HUGEPAGE: i32 = 14;
    unsafe extern "C" {
        fn madvise(addr: *mut std::ffi::c_void, len: usize, advice: i32) -> i32;
    }

    // the huge pages that fit whole in the reserved memory
    let start = table.as_mut_ptr() as usize;
    let end = start + table.capacity() * size_of::<Bucket>();
    let first = start.next_multiple_of(HUGE_PAGE);
    let len = end.saturating_sub(first) / HUGE_PAGE * HUGE_PAGE;
    if len > 0 {
        // SAFETY: the range lies inside memory the vector owns, and the advice changes
        // neither its contents nor its mapping. A failure only leaves the pages small
        unsafe {
            madvise(first as *mut std::ffi::c_void, len, MADV_HUGEPAGE);
        }
    }
}

/// Elsewhere there is no such hint without privileges: macOS on Apple silicon already has
/// pages of 16 KB, and Windows needs a right users don't have by default
#[cfg(not(target_os = "linux"))]
fn ask_for_huge_pages(_table: &mut Vec<Bucket>) {}

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
        assert_eq!(size_of::<Bucket>(), 32);
    }

    #[test]
    fn the_table_takes_every_megabyte_it_gets() {
        // as many entries as fit, whatever the size
        let bucket = size_of::<Bucket>();
        for megabytes in [1, 3, 100] {
            let bytes = TranspositionTable::new(megabytes).buckets.len() * bucket;
            assert!(bytes <= megabytes << 20, "{megabytes}");
            assert!(bytes + bucket > megabytes << 20, "{megabytes}");
        }
        // no less than the `Hash` option allows
        assert_eq!(
            TranspositionTable::new(0).buckets.len(),
            TranspositionTable::new(1).buckets.len()
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

    /// Hashes of positions that all fall on the single bucket of a table
    fn three_positions() -> [ZobristHash; 3] {
        [Board::STARTPOS, KIWIPETE, PERFT_POSITIONS[2]].map(hash)
    }

    #[test]
    fn positions_sharing_a_slot_do_not_mix() {
        let mut tt = TranspositionTable::with_buckets(1);
        let [startpos, kiwipete, _] = three_positions();

        tt.store(startpos, 3, Score::DRAW, Bound::Exact, None);
        assert!(tt.probe(kiwipete).is_none());

        tt.store(kiwipete, 1, Score::new(10), Bound::Exact, None);
        assert_eq!(tt.probe(startpos).unwrap().depth, 3);
        assert_eq!(tt.probe(kiwipete).unwrap().score, Score::new(10));
    }

    #[test]
    fn the_deepest_entry_of_the_search_stays() {
        let mut tt = TranspositionTable::with_buckets(1);
        let [deep, a, b] = three_positions();

        tt.store(deep, 8, Score::DRAW, Bound::Exact, None);
        tt.store(a, 2, Score::DRAW, Bound::Exact, None);
        tt.store(b, 3, Score::DRAW, Bound::Exact, None);
        // the shallow ones take turns in the other slot
        assert!(tt.probe(deep).is_some());
        assert!(tt.probe(a).is_none());
        assert!(tt.probe(b).is_some());

        // a deeper one takes the first slot, and the deep one moves to the second
        tt.store(a, 9, Score::DRAW, Bound::Exact, None);
        assert!(tt.probe(a).is_some());
        assert!(tt.probe(deep).is_some());
        assert!(tt.probe(b).is_none());
    }

    #[test]
    fn a_new_search_replaces_the_entries_of_the_old_ones() {
        let mut tt = TranspositionTable::with_buckets(1);
        let [deep, shallow, _] = three_positions();

        tt.store(deep, 8, Score::DRAW, Bound::Exact, None);
        tt.new_search();
        tt.store(shallow, 1, Score::DRAW, Bound::Exact, None);
        assert_eq!(tt.probe(shallow).unwrap().depth, 1);
        // still there, in the second slot, until something else comes
        assert!(tt.probe(deep).is_some());
    }

    #[test]
    fn the_same_position_keeps_its_slot_and_its_move() {
        let board = board(Board::STARTPOS);
        let e4 = find_move(&board, "e2e4");
        let mut tt = TranspositionTable::with_buckets(1);
        let [_, other, _] = three_positions();

        tt.store(board.hash(), 5, Score::new(30), Bound::Lower, Some(e4));
        tt.store(other, 2, Score::DRAW, Bound::Exact, None);
        // a fail low has no move, the one from before is still the best guess
        tt.store(board.hash(), 6, Score::new(10), Bound::Upper, None);

        let entry = tt.probe(board.hash()).unwrap();
        assert_eq!((entry.depth, entry.bound), (6, Bound::Upper));
        assert_eq!(entry.best_move, Some(e4));
        assert!(tt.probe(other).is_some());
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

    #[test]
    fn the_bound_says_when_the_score_refines_the_static_eval() {
        let refine = |bound, score: i32, eval: i32| {
            let entry = Entry::new(ZobristHash::empty(), 5, Score::new(score), bound, None);
            entry.refine(Score::new(eval))
        };

        assert_eq!(refine(Bound::Exact, 50, 10), Score::new(50));
        // at least 50: closer than an eval below it, not than one above
        assert_eq!(refine(Bound::Lower, 50, 10), Score::new(50));
        assert_eq!(refine(Bound::Lower, 50, 90), Score::new(90));
        // at most 50: the other way around
        assert_eq!(refine(Bound::Upper, 50, 10), Score::new(10));
        assert_eq!(refine(Bound::Upper, 50, 90), Score::new(50));

        let mate = Entry::new(
            ZobristHash::empty(),
            5,
            Score::mate_in(3),
            Bound::Exact,
            None,
        );
        assert_eq!(mate.refine(Score::new(10)), Score::new(10));
    }
}
