use std::io::{self, BufRead};

use runner_chess::uci::{Control, Uci};

fn main() {
    let mut uci = Uci::new();
    let mut stdout = io::stdout().lock();

    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };

        match uci.handle(&line, &mut stdout) {
            Ok(Control::Quit) => break,
            Ok(Control::Continue) => (),
            Err(e) => eprintln!("{e:?}"),
        }
    }
}
