use std::{
    io::{self, Write},
    time::Instant,
};

use crate::{
    bench::{self, BenchError, BENCH},
    board::{movegen::IllegalMoveError, Board, FenParseError},
    chess_move::{AbstractMove, MoveParseError},
};

#[derive(Debug)]
pub enum UciError {
    EmptyInput,
    UnknownCommand(String),
    UnexpectedArgs(String),
    FenError(FenParseError),
    MoveParseError(String),
    IllegalMove(String),
    Bench(BenchError),
    Io(io::Error),
}

impl From<io::Error> for UciError {
    fn from(e: io::Error) -> Self {
        UciError::Io(e)
    }
}

impl From<BenchError> for UciError {
    fn from(e: BenchError) -> Self {
        UciError::Bench(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Continue,
    Quit,
}

pub struct Uci {
    board: Board,
    debug: bool,
}

impl Default for Uci {
    fn default() -> Self {
        Self::new()
    }
}

impl Uci {
    pub fn new() -> Self {
        Self {
            board: Board::startpos(),
            debug: false,
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn handle(&mut self, line: &str, out: &mut impl Write) -> Result<Control, UciError> {
        let mut tokens = line.split_whitespace();
        let command = tokens.next().ok_or(UciError::EmptyInput)?;

        match command {
            "uci" => {
                writeln!(out, "id name runner-chess")?;
                writeln!(out, "id author Rafael Nunes Santana")?;
                writeln!(out, "uciok")?;
            }
            "isready" => writeln!(out, "readyok")?,
            "debug" => match tokens.next() {
                Some("on") => self.debug = true,
                Some("off") => self.debug = false,
                Some(other) => {
                    return Err(UciError::UnexpectedArgs(format!(
                        "unexpected debug option: {other}"
                    )))
                }
                None => {
                    return Err(UciError::UnexpectedArgs(
                        "usage: debug [on | off]".to_string(),
                    ))
                }
            },
            "ucinewgame" => self.board = Board::startpos(),
            "position" => {
                let args: Vec<&str> = tokens.collect();
                self.board = parse_position(&args)?;
            }
            "go" => {
                let args: Vec<&str> = tokens.collect();
                match args.as_slice() {
                    ["perft", depth] => self.go_perft(depth, out)?,
                    ["perft", ..] => {
                        return Err(UciError::UnexpectedArgs(
                            "usage: go perft <depth>".to_string(),
                        ))
                    }
                    _ => match self.board.legal_successors().next() {
                        Some((mv, _)) => writeln!(out, "bestmove {mv}")?,
                        None => writeln!(out, "bestmove 0000")?,
                    },
                }
            }
            "d" => writeln!(out, "{}\n{}\n", self.board, self.board.to_fen())?,
            "bench" => bench::run(&BENCH, out)?,
            "quit" => return Ok(Control::Quit),
            _ => return Err(UciError::UnknownCommand(command.to_string())),
        }

        out.flush()?;
        Ok(Control::Continue)
    }

    /// `go perft <depth>`: the divide of the current position, in Stockfish's format
    fn go_perft(&self, depth: &str, out: &mut impl Write) -> Result<(), UciError> {
        let depth =
            depth
                .parse::<u32>()
                .ok()
                .filter(|&d| d > 0)
                .ok_or(UciError::UnexpectedArgs(
                    "go perft depth is not a positive number".to_string(),
                ))?;

        let start = Instant::now();
        let divide = self.board.divide(depth);
        let elapsed = start.elapsed();
        let total: u64 = divide.iter().map(|&(_, count)| count).sum();

        for (mv, count) in divide {
            writeln!(out, "{mv}: {count}")?;
        }
        if self.debug {
            writeln!(
                out,
                "info time {} nodes {total} nps {}",
                elapsed.as_millis(),
                bench::nps(total, elapsed)
            )?;
        }
        writeln!(out, "Nodes searched: {total}")?;
        Ok(())
    }
}

/// Parses the arguments of `position`:
/// `startpos [moves ...]` or `fen <6 fields> [moves ...]`.
fn parse_position(args: &[&str]) -> Result<Board, UciError> {
    let moves_idx = args.iter().position(|&t| t == "moves");
    let (setup, moves) = match moves_idx {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[][..]),
    };

    let board = match setup {
        ["startpos"] => Board::startpos(),
        ["fen", fen @ ..] if !fen.is_empty() => {
            Board::from_fen(&fen.join(" ")).map_err(UciError::FenError)?
        }
        _ => {
            return Err(UciError::UnexpectedArgs(format!(
                "invalid position args: {}",
                args.join(" ")
            )))
        }
    };

    play_moves(board, moves)
}

fn play_moves(mut board: Board, moves: &[&str]) -> Result<Board, UciError> {
    for &uci in moves {
        let mv = AbstractMove::from_uci(uci)
            .map_err(|MoveParseError| UciError::MoveParseError(uci.to_string()))?;
        let (_, next) = board
            .resolve_move(mv)
            .map_err(|IllegalMoveError| UciError::IllegalMove(uci.to_string()))?;
        board = next;
    }

    Ok(board)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(uci: &mut Uci, line: &str) -> Result<(Control, String), UciError> {
        let mut out = Vec::new();
        let control = uci.handle(line, &mut out)?;
        Ok((control, String::from_utf8(out).unwrap()))
    }

    fn fen_after(line: &str) -> String {
        let mut uci = Uci::new();
        run(&mut uci, line).unwrap();
        uci.board().to_fen()
    }

    #[test]
    fn handshake() {
        let mut uci = Uci::new();

        let (control, out) = run(&mut uci, "uci").unwrap();
        assert_eq!(control, Control::Continue);
        assert!(out.ends_with("uciok\n"));

        let (_, out) = run(&mut uci, "isready").unwrap();
        assert_eq!(out, "readyok\n");
    }

    #[test]
    fn quit() {
        let mut uci = Uci::new();
        assert_eq!(run(&mut uci, "quit").unwrap().0, Control::Quit);
    }

    #[test]
    fn empty_and_unknown_commands() {
        let mut uci = Uci::new();
        assert!(matches!(run(&mut uci, "   "), Err(UciError::EmptyInput)));
        assert!(matches!(
            run(&mut uci, "foo"),
            Err(UciError::UnknownCommand(_))
        ));
    }

    #[test]
    fn position_startpos() {
        assert_eq!(fen_after("position startpos"), Board::startpos().to_fen());
    }

    #[test]
    fn position_startpos_with_moves() {
        assert_eq!(
            fen_after("position startpos moves e2e4 e7e5 g1f3"),
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
        );
    }

    #[test]
    fn position_fen() {
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        assert_eq!(fen_after(&format!("position fen {fen}")), fen);
    }

    #[test]
    fn position_fen_with_moves() {
        assert_eq!(
            fen_after("position fen r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1 moves e1g1 e8c8"),
            "2kr3r/8/8/8/8/8/8/R4RK1 w - - 2 2"
        );
    }

    #[test]
    fn position_with_promotion() {
        assert_eq!(
            fen_after("position fen 8/P7/8/8/8/8/8/k6K w - - 0 1 moves a7a8n"),
            "N7/8/8/8/8/8/8/k6K b - - 0 1"
        );
    }

    #[test]
    fn position_persists_between_commands() {
        let mut uci = Uci::new();
        run(&mut uci, "position startpos moves e2e4").unwrap();
        run(&mut uci, "isready").unwrap();
        assert_eq!(
            uci.board().to_fen(),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"
        );
    }

    #[test]
    fn ucinewgame_resets_board() {
        let mut uci = Uci::new();
        run(&mut uci, "position startpos moves e2e4").unwrap();
        run(&mut uci, "ucinewgame").unwrap();
        assert_eq!(uci.board().to_fen(), Board::startpos().to_fen());
    }

    #[test]
    fn position_invalid_args() {
        let mut uci = Uci::new();
        for line in [
            "position",
            "position foo",
            "position fen",
            "position startpos e2e4",
        ] {
            assert!(
                matches!(run(&mut uci, line), Err(UciError::UnexpectedArgs(_))),
                "{line}"
            );
        }
        assert!(matches!(
            run(&mut uci, "position fen not/a/fen w - - 0 1"),
            Err(UciError::FenError(_))
        ));
    }

    #[test]
    fn position_bad_move_does_not_change_board() {
        let mut uci = Uci::new();
        run(&mut uci, "position startpos moves e2e4").unwrap();
        let before = uci.board().to_fen();

        assert!(matches!(
            run(&mut uci, "position startpos moves e2e4 e7e5 xx"),
            Err(UciError::MoveParseError(_))
        ));
        assert!(matches!(
            run(&mut uci, "position startpos moves e2e4 e2e4"),
            Err(UciError::IllegalMove(_))
        ));
        assert_eq!(uci.board().to_fen(), before);
    }

    #[test]
    fn go_outputs_legal_bestmove() {
        let mut uci = Uci::new();
        run(&mut uci, "position startpos").unwrap();
        let (_, out) = run(&mut uci, "go").unwrap();

        let mv = out.strip_prefix("bestmove ").unwrap().trim();
        let parsed = AbstractMove::from_uci(mv).unwrap();
        assert!(uci.board().resolve_move(parsed).is_ok());
    }

    #[test]
    fn go_without_legal_moves() {
        let mut uci = Uci::new();
        // Fool's mate: white is checkmated.
        run(&mut uci, "position startpos moves f2f3 e7e5 g2g4 d8h4").unwrap();
        let (_, out) = run(&mut uci, "go").unwrap();
        assert_eq!(out, "bestmove 0000\n");
    }

    #[test]
    fn go_with_search_args_still_answers() {
        let mut uci = Uci::new();
        let (_, out) = run(&mut uci, "go wtime 1000 btime 1000").unwrap();
        assert!(out.starts_with("bestmove "), "{out}");
    }

    fn perft_lines(uci: &mut Uci, depth: u32) -> Vec<String> {
        let (_, out) = run(uci, &format!("go perft {depth}")).unwrap();
        out.lines().map(str::to_string).collect()
    }

    #[test]
    fn go_perft_startpos() {
        let mut uci = Uci::new();
        let lines = perft_lines(&mut uci, 1);
        assert_eq!(lines.len(), 21);
        assert!(lines.contains(&"e2e4: 1".to_string()));
        assert_eq!(lines.last().unwrap(), "Nodes searched: 20");

        let lines = perft_lines(&mut uci, 3);
        assert!(lines.contains(&"e2e4: 600".to_string()));
        assert_eq!(lines.last().unwrap(), "Nodes searched: 8902");
    }

    #[test]
    fn go_perft_uses_current_position() {
        let mut uci = Uci::new();
        run(
            &mut uci,
            "position fen r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        assert_eq!(
            perft_lines(&mut uci, 2).last().unwrap(),
            "Nodes searched: 2039"
        );
    }

    #[test]
    fn go_perft_without_moves() {
        let mut uci = Uci::new();
        run(&mut uci, "position startpos moves f2f3 e7e5 g2g4 d8h4").unwrap();
        assert_eq!(perft_lines(&mut uci, 1), ["Nodes searched: 0"]);
    }

    #[test]
    fn go_perft_invalid_depth() {
        let mut uci = Uci::new();
        for line in [
            "go perft",
            "go perft 0",
            "go perft -1",
            "go perft abc",
            "go perft 2 3",
        ] {
            assert!(
                matches!(run(&mut uci, line), Err(UciError::UnexpectedArgs(_))),
                "{line}"
            );
        }
    }

    #[test]
    fn d_prints_board_and_fen() {
        let mut uci = Uci::new();
        run(&mut uci, "position startpos moves e2e4").unwrap();
        let (control, out) = run(&mut uci, "d").unwrap();

        assert_eq!(control, Control::Continue);
        assert!(out.contains(&uci.board().to_string()), "{out}");
        assert!(
            out.contains("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"),
            "{out}"
        );
    }

    #[test]
    fn debug_on_adds_timing_to_perft() {
        let mut uci = Uci::new();
        assert!(!perft_lines(&mut uci, 1)
            .iter()
            .any(|l| l.starts_with("info")));

        run(&mut uci, "debug on").unwrap();
        let lines = perft_lines(&mut uci, 1);
        let info = &lines[lines.len() - 2];
        assert!(info.starts_with("info time "), "{info}");
        assert!(info.contains(" nodes 20 nps "), "{info}");
        // still last, so scripts comparing against Stockfish keep working
        assert_eq!(lines.last().unwrap(), "Nodes searched: 20");

        run(&mut uci, "debug off").unwrap();
        assert!(!perft_lines(&mut uci, 1)
            .iter()
            .any(|l| l.starts_with("info")));
    }

    #[test]
    fn debug_invalid_args() {
        let mut uci = Uci::new();
        for line in ["debug", "debug maybe"] {
            assert!(
                matches!(run(&mut uci, line), Err(UciError::UnexpectedArgs(_))),
                "{line}"
            );
        }
    }

    #[test]
    #[ignore = "slow in debug builds; run with cargo test --release -- --ignored"]
    fn bench_command() {
        let mut uci = Uci::new();
        let (control, out) = run(&mut uci, "bench").unwrap();
        assert_eq!(control, Control::Continue);
        assert!(out.contains("Nodes searched  : 37918074"), "{out}");
    }
}
