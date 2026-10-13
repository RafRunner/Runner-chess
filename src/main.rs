use std::{
    env,
    io::{self, BufRead},
    process::ExitCode,
};

use runner_chess::uci::{Control, Uci};

fn main() -> ExitCode {
    let mut uci = Uci::new();
    let mut stdout = io::stdout().lock();

    // `runner-chess bench [search [depth]]` runs a benchmark and exits, for scripts
    // and hyperfine
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("bench") {
        return match uci.handle(&args.join(" "), &mut stdout) {
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
