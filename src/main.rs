use std::{
    env,
    io::{self, BufRead},
    process::ExitCode,
};

use runner_chess::{
    datagen,
    uci::{Control, Stdout, Uci},
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    // `runner-chess datagen <dir> [options]` plays games for training data and exits
    if args.first().map(String::as_str) == Some("datagen") {
        return match datagen::run(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    // no lock on stdout here: the search thread writes to it too
    let mut uci = Uci::new(Stdout);

    // `runner-chess bench [search [depth]]` runs a benchmark and exits, for scripts
    // and hyperfine
    if args.first().map(String::as_str) == Some("bench") {
        return match uci.handle(&args.join(" ")) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e:?}");
                ExitCode::FAILURE
            }
        };
    }

    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };

        match uci.handle(&line) {
            Ok(Control::Quit) => return ExitCode::SUCCESS,
            Ok(Control::Continue) => (),
            Err(e) => eprintln!("{e:?}"),
        }
    }

    // the input closed without a `quit`, so no `stop` can come anymore either: a search
    // still running ends now, with its `bestmove`, as Stockfish does
    if let Err(e) = uci.handle("quit") {
        eprintln!("{e:?}");
    }
    ExitCode::SUCCESS
}
