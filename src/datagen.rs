//! `runner-chess datagen`: games of the engine against itself, written as training data
//! for bullet (see `docs/nnue.md`). Every line is a quiet position of a game, with the
//! score of its search and the result of the game, both from White's point of view:
//!
//! ```text
//! <FEN> | <centipawns> | <1.0, 0.5 or 0.0>
//! ```

use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    board::Board,
    eval::is_dead_position,
    piece::Color,
    score::Score,
    search::{Budget, MoveOrdering, Searcher, TranspositionTable},
    zobrist::ZobristHash,
};

const USAGE: &str = "usage: runner-chess datagen <dir> [--threads <n>] [--positions <n>] \
                     [--nodes <n>] [--seed <n>]";

/// Random moves at the start of every game, one more half of the time, so that either
/// side can be the one to move first after them
const RANDOM_PLIES: usize = 8;
/// An opening already won for one side gets thrown away
const MAX_OPENING_SCORE: i32 = 400;
/// A side this far ahead for `WIN_PLIES` plies in a row wins
const WIN_SCORE: i32 = 2_000;
const WIN_PLIES: u32 = 4;
/// After `DRAW_AFTER` moves, a score this close to 0 for `DRAW_PLIES` plies in a row
/// is a draw
const DRAW_SCORE: i32 = 10;
const DRAW_PLIES: u32 = 8;
const DRAW_AFTER: u16 = 40;
/// A draw, only so that no game goes on forever
const MAX_GAME_PLIES: usize = 1_000;
/// The hard node limit of every search, as a multiple of the soft one: only a safety
/// net, since the soft one lets the iteration finish
const HARD_NODES_FACTOR: u64 = 10;
/// The table of each thread, cleared before every game
const HASH_MB: usize = 16;
const REPORT_EVERY: Duration = Duration::from_secs(10);

struct Config {
    dir: PathBuf,
    threads: usize,
    /// Between all the threads, which stop starting games once they get there
    positions: u64,
    /// The soft node limit of the search of every move
    nodes: u64,
    seed: u64,
}

impl Config {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut args = args.iter();
        let dir = args
            .next()
            .filter(|dir| !dir.starts_with("--"))
            .ok_or(USAGE)?;

        let mut config = Self {
            dir: PathBuf::from(dir),
            threads: thread::available_parallelism().map_or(1, |n| n.get()),
            positions: 1_000_000,
            nodes: 5_000,
            seed: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |time| time.as_nanos() as u64),
        };
        while let Some(option) = args.next() {
            let mut number = || {
                args.next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| format!("{option} expects a number\n{USAGE}"))
            };
            match option.as_str() {
                "--threads" => config.threads = number()?.max(1) as usize,
                "--positions" => config.positions = number()?,
                "--nodes" => config.nodes = number()?.max(1),
                "--seed" => config.seed = number()?,
                _ => return Err(format!("unknown option {option}\n{USAGE}")),
            }
        }
        Ok(config)
    }
}

/// Plays games on every thread until they wrote `--positions` positions between them,
/// into one file per thread in `<dir>`, named after the version, the seed and the
/// thread. A game gets written once it ends, so a run stopped in the middle keeps every
/// game it finished. The searches are limited by nodes only, so the same seed and
/// number of threads play the same games again.
pub fn run(args: &[String]) -> io::Result<()> {
    let config = Config::parse(args).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    fs::create_dir_all(&config.dir)?;
    let prefix = format!("{}-{}", env!("CARGO_PKG_VERSION"), config.seed);
    // never over the files of another run
    let files = (0..config.threads)
        .map(|thread| File::create_new(config.dir.join(format!("{prefix}-{thread}.txt"))))
        .collect::<io::Result<Vec<_>>>()?;

    println!(
        "datagen: {} threads, {} nodes per move, {} positions, seed {}, into {}",
        config.threads,
        config.nodes,
        config.positions,
        config.seed,
        config.dir.join(format!("{prefix}-*.txt")).display(),
    );

    let progress = Progress {
        start: Instant::now(),
        games: AtomicU64::new(0),
        positions: AtomicU64::new(0),
    };
    let mut seeds = Rng(config.seed);

    thread::scope(|scope| {
        let workers: Vec<_> = files
            .into_iter()
            .map(|file| {
                let rng = Rng(seeds.next_u64());
                let (config, progress) = (&config, &progress);
                scope.spawn(move || play_games(file, rng, config, progress))
            })
            .collect();

        let mut last_report = Instant::now();
        while workers.iter().any(|worker| !worker.is_finished()) {
            thread::sleep(Duration::from_millis(100));
            if last_report.elapsed() >= REPORT_EVERY {
                progress.report();
                last_report = Instant::now();
            }
        }
        progress.report();

        workers.into_iter().try_for_each(|worker| {
            worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    })
}

/// What all the threads did so far
struct Progress {
    start: Instant,
    games: AtomicU64,
    positions: AtomicU64,
}

impl Progress {
    fn report(&self) {
        let seconds = self.start.elapsed().as_secs_f64();
        let positions = self.positions.load(Ordering::Relaxed);
        println!(
            "{} games, {positions} positions, {:.0} positions/s, {seconds:.0} s",
            self.games.load(Ordering::Relaxed),
            positions as f64 / seconds,
        );
    }
}

/// The games of one thread, each written to `file` once it ends
fn play_games(file: File, mut rng: Rng, config: &Config, progress: &Progress) -> io::Result<()> {
    let mut out = BufWriter::new(file);
    let mut tt = TranspositionTable::new(HASH_MB);
    let mut ordering = MoveOrdering::new();

    while progress.positions.load(Ordering::Relaxed) < config.positions {
        let Some(game) = play_game(&mut rng, &mut tt, &mut ordering, config.nodes) else {
            continue;
        };
        for (fen, score) in &game.positions {
            writeln!(out, "{fen} | {score} | {}", game.result.as_bullet())?;
        }
        out.flush()?;

        progress.games.fetch_add(1, Ordering::Relaxed);
        progress
            .positions
            .fetch_add(game.positions.len() as u64, Ordering::Relaxed);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    WhiteWins,
    Draw,
    BlackWins,
}

impl Outcome {
    fn win_for(color: Color) -> Self {
        match color {
            Color::White => Self::WhiteWins,
            Color::Black => Self::BlackWins,
        }
    }

    /// As bullet reads it, from White's point of view
    fn as_bullet(self) -> &'static str {
        match self {
            Self::WhiteWins => "1.0",
            Self::Draw => "0.5",
            Self::BlackWins => "0.0",
        }
    }
}

struct Game {
    /// The FEN of each position kept, with its score in centipawns for White
    positions: Vec<(String, i16)>,
    result: Outcome,
}

/// A game from a random opening, or `None` when the opening came out already won for
/// one side. Kept: the positions after the opening that the search found quiet, not in
/// check, with a quiet best move and without a mate in sight.
fn play_game(
    rng: &mut Rng,
    tt: &mut TranspositionTable,
    ordering: &mut MoveOrdering,
    nodes: u64,
) -> Option<Game> {
    let (mut board, mut history) = random_opening(rng)?;
    tt.clear();
    ordering.clear();

    let mut positions = Vec::new();
    let opening_plies = history.len();
    // plies in a row with the score of each adjudication
    let (mut white_ahead, mut black_ahead, mut balanced) = (0, 0, 0);

    let result = loop {
        if let Some(result) = game_over(&board, &history) {
            break result;
        }
        if history.len() >= MAX_GAME_PLIES {
            break Outcome::Draw;
        }

        let budget = Budget::depth(Some(Score::MAX_PLY))
            .with_soft_nodes(nodes)
            .with_nodes(nodes * HARD_NODES_FACTOR);
        let mut searcher = Searcher::new(budget, tt, ordering);
        let best = searcher
            .search_root(&board, &history, &mut io::sink())
            .expect("the sink takes everything");
        let (Some(mv), Some(score)) = (best, searcher.score()) else {
            unreachable!("the game goes on, and the first iteration always finishes");
        };
        let white = white_score(&board, score);

        if history.len() == opening_plies && beyond(white, MAX_OPENING_SCORE) {
            return None;
        }
        if !board.is_in_check(board.side_to_move())
            && mv.is_quiet()
            && let Some(centipawns) = white.centipawns()
        {
            positions.push((board.to_fen(), centipawns));
        }

        white_ahead = if white >= Score::new(WIN_SCORE) {
            white_ahead + 1
        } else {
            0
        };
        black_ahead = if white <= -Score::new(WIN_SCORE) {
            black_ahead + 1
        } else {
            0
        };
        balanced = if board.fullmove_counter() > DRAW_AFTER && !beyond(white, DRAW_SCORE) {
            balanced + 1
        } else {
            0
        };
        if white_ahead >= WIN_PLIES {
            break Outcome::WhiteWins;
        }
        if black_ahead >= WIN_PLIES {
            break Outcome::BlackWins;
        }
        if balanced >= DRAW_PLIES {
            break Outcome::Draw;
        }

        history.push(board.hash());
        board = board.make_move(mv);
    };

    Some(Game { positions, result })
}

/// A few random legal moves from the starting position, with the hashes of the
/// positions before the last one, or `None` when they ended the game
fn random_opening(rng: &mut Rng) -> Option<(Board, Vec<ZobristHash>)> {
    let mut board = Board::startpos();
    let mut history = Vec::new();

    for _ in 0..RANDOM_PLIES + rng.below(2) {
        let mut successors: Vec<Board> = board.legal_successors().map(|(_, next)| next).collect();
        if successors.is_empty() {
            return None;
        }
        history.push(board.hash());
        board = successors.swap_remove(rng.below(successors.len()));
    }

    Some((board, history))
}

/// The result, when the game is over by the rules. `history` has the hashes of the
/// positions before `board`, oldest first.
fn game_over(board: &Board, history: &[ZobristHash]) -> Option<Outcome> {
    let us = board.side_to_move();
    if board.legal_successors().next().is_none() {
        return Some(if board.is_in_check(us) {
            Outcome::win_for(us.oposite())
        } else {
            Outcome::Draw
        });
    }

    // nothing before the last capture or pawn move can come back
    let repetitions = history
        .iter()
        .rev()
        .take(board.halfmove_clock() as usize)
        .filter(|&&hash| hash == board.hash())
        .count();

    (board.halfmove_clock() >= 100 || repetitions >= 2 || is_dead_position(board))
        .then_some(Outcome::Draw)
}

/// `score`, for the side to move in `board`, from White's point of view
fn white_score(board: &Board, score: Score) -> Score {
    match board.side_to_move() {
        Color::White => score,
        Color::Black => -score,
    }
}

/// More than `centipawns` away from 0, either way
fn beyond(score: Score, centipawns: i32) -> bool {
    score > Score::new(centipawns) || score < -Score::new(centipawns)
}

/// splitmix64: small, fast, and good enough to pick moves and seeds
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n`
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::Path};

    use super::*;
    use crate::test_utils::{board, find_move};

    // the end of a game

    #[test]
    fn mate_and_stalemate_end_the_game() {
        // Black to move in both
        let mated = board("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1");
        assert_eq!(game_over(&mated, &[]), Some(Outcome::WhiteWins));
        let stalemate = board("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
        assert_eq!(game_over(&stalemate, &[]), Some(Outcome::Draw));
    }

    #[test]
    fn the_draws_by_the_rules_end_the_game() {
        let fifty_moves = board("4k3/8/8/8/8/8/8/R3K3 w - - 100 80");
        assert_eq!(game_over(&fifty_moves, &[]), Some(Outcome::Draw));
        let dead = board("k7/8/8/8/3K4/8/5N2/8 w - - 0 1");
        assert_eq!(game_over(&dead, &[]), Some(Outcome::Draw));
        assert_eq!(game_over(&Board::startpos(), &[]), None);
    }

    #[test]
    fn the_third_repetition_ends_the_game() {
        let mut position = Board::startpos();
        let mut history = Vec::new();
        for round in 1..=2 {
            for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] {
                history.push(position.hash());
                position = position.make_move(find_move(&position, uci));
            }
            // back at the start: the second time, then the third
            let expected = (round == 2).then_some(Outcome::Draw);
            assert_eq!(game_over(&position, &history), expected, "{round}");
        }
    }

    // the positions of a game

    #[test]
    fn scores_are_from_whites_point_of_view() {
        let score = Score::new(35);
        assert_eq!(white_score(&Board::startpos(), score), score);
        let black_to_move = board("4k3/8/8/8/8/8/8/R3K3 b - - 0 1");
        assert_eq!(white_score(&black_to_move, score), -score);
    }

    #[test]
    fn a_game_keeps_only_quiet_positions_after_the_opening() {
        let mut rng = Rng(1);
        let (mut tt, mut ordering) = (TranspositionTable::new(1), MoveOrdering::new());
        let game = (0..10)
            .find_map(|_| play_game(&mut rng, &mut tt, &mut ordering, 200))
            .expect("not every opening is lost");

        assert!(!game.positions.is_empty());
        for (fen, _) in &game.positions {
            let position = board(fen);
            assert!(!position.is_in_check(position.side_to_move()), "{fen}");
            assert!(
                position.fullmove_counter() > (RANDOM_PLIES / 2) as u16,
                "{fen}"
            );
        }
    }

    // a whole run

    /// The lines bullet would read from every file of `dir`, after checking them
    fn read_lines(dir: &Path) -> Vec<String> {
        let mut lines = Vec::new();
        for file in fs::read_dir(dir).unwrap() {
            for line in fs::read_to_string(file.unwrap().path()).unwrap().lines() {
                let [fen, score, result] = line.split('|').collect::<Vec<_>>()[..] else {
                    panic!("not 3 fields: {line}");
                };
                Board::from_fen(fen.trim()).unwrap();
                score.trim().parse::<i16>().unwrap();
                assert!(["1.0", "0.5", "0.0"].contains(&result.trim()), "{line}");
                lines.push(line.to_string());
            }
        }
        lines
    }

    #[test]
    fn a_run_writes_one_file_per_thread_in_the_bullet_format() {
        let dir = env::temp_dir().join(format!("runner-chess-datagen-{}", std::process::id()));
        let args: Vec<String> = [
            dir.to_str().unwrap(),
            "--threads",
            "2",
            "--positions",
            "50",
            "--nodes",
            "100",
            "--seed",
            "7",
        ]
        .map(String::from)
        .to_vec();

        run(&args).unwrap();
        let version = env!("CARGO_PKG_VERSION");
        for thread in 0..2 {
            assert!(dir.join(format!("{version}-7-{thread}.txt")).exists());
        }
        assert!(read_lines(&dir).len() >= 50);

        // the same seed again would write over the files
        let again = run(&args).unwrap_err();
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn options_are_checked() {
        let parse =
            |args: &[&str]| Config::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>());
        assert!(parse(&[]).is_err());
        assert!(parse(&["--threads", "2"]).is_err());
        assert!(parse(&["data", "--threads"]).is_err());
        assert!(parse(&["data", "--depth", "5"]).is_err());

        let config = parse(&["data", "--threads", "3", "--nodes", "800", "--seed", "9"]).unwrap();
        assert_eq!(
            (config.threads, config.nodes, config.seed, config.positions),
            (3, 800, 9, 1_000_000)
        );
    }
}
