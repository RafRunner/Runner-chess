use std::{
    env,
    io::{self, BufRead},
    process::ExitCode,
};

use runner_chess::uci::{Control, Uci};

fn main() -> ExitCode {
    let mut uci = Uci::new();
    let mut stdout = io::stdout().lock();

    // `runner-chess bench` runs the benchmark and exits, for scripts and hyperfine
    if env::args().nth(1).as_deref() == Some("bench") {
        return match uci.handle("bench", &mut stdout) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e:?}");
                ExitCode::FAILURE
            }
        };
    }

    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };

        match uci.handle(&line, &mut stdout) {
            Ok(Control::Quit) => break,
            Ok(Control::Continue) => (),
            Err(e) => eprintln!("{e:?}"),
        }
    }

    ExitCode::SUCCESS
}
