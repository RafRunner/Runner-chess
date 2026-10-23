//! `cargo r -r --bin validate -- <checkpoint dir> <data> [wdl]`: the training loss of
//! `raw.bin` on other data, since bullet has no validation yet, and the loss against the
//! result and the score alone.

use std::{fs, path::Path, thread};

use bulletformat::{BulletFormat, ChessBoard};

const HIDDEN_SIZE: usize = 128;
const SCALE: f32 = 400.0;
const WEIGHTS: usize = 768 * HIDDEN_SIZE + HIDDEN_SIZE + 2 * HIDDEN_SIZE + 1;

struct Network {
    l0w: Vec<f32>, // the column of each input, one after the other
    l0b: Vec<f32>,
    l1w: Vec<f32>, // the side to move's half, then the other's
    l1b: f32,
}

impl Network {
    fn load(path: &Path) -> Self {
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(bytes.len(), WEIGHTS * 4, "not a 768x128 network");
        let values: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();

        let (l0w, rest) = values.split_at(768 * HIDDEN_SIZE);
        let (l0b, rest) = rest.split_at(HIDDEN_SIZE);
        let (l1w, rest) = rest.split_at(2 * HIDDEN_SIZE);
        Self {
            l0w: l0w.to_vec(),
            l0b: l0b.to_vec(),
            l1w: l1w.to_vec(),
            l1b: rest[0],
        }
    }

    /// Before the sigmoid, with the inputs of bullet's `Chess768`
    fn output(&self, board: ChessBoard) -> f32 {
        let mut stm: [f32; HIDDEN_SIZE] = self.l0b[..].try_into().unwrap();
        let mut ntm = stm;
        for (piece, square) in board {
            let c = usize::from(piece & 8 > 0);
            let pc = 64 * usize::from(piece & 7);
            let sq = usize::from(square);
            for (acc, feature) in [
                (&mut stm, [0, 384][c] + pc + sq),
                (&mut ntm, [384, 0][c] + pc + (sq ^ 56)),
            ] {
                let column = &self.l0w[feature * HIDDEN_SIZE..][..HIDDEN_SIZE];
                for (a, w) in acc.iter_mut().zip(column) {
                    *a += w;
                }
            }
        }

        let screlu = |x: f32| x.clamp(0.0, 1.0).powi(2);
        let (us, them) = self.l1w.split_at(HIDDEN_SIZE);
        stm.iter().zip(us).map(|(&x, w)| screlu(x) * w).sum::<f32>()
            + ntm
                .iter()
                .zip(them)
                .map(|(&x, w)| screlu(x) * w)
                .sum::<f32>()
            + self.l1b
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (checkpoint, data) = match &args[..] {
        [checkpoint, data, ..] => (Path::new(checkpoint), data),
        _ => {
            eprintln!("usage: cargo r -r --bin validate -- <checkpoint dir> <data> [wdl]");
            std::process::exit(1);
        }
    };
    let wdl: f32 = args.get(2).map_or(0.75, |w| w.parse().expect("wdl"));

    let network = Network::load(&checkpoint.join("raw.bin"));
    let bytes = fs::read(data).unwrap_or_else(|e| panic!("{data}: {e}"));
    let boards: Vec<ChessBoard> = bytes
        .chunks_exact(size_of::<ChessBoard>())
        // any 32 bytes are a valid ChessBoard: plain integers, no padding
        .map(|chunk| unsafe { std::ptr::read_unaligned(chunk.as_ptr().cast::<ChessBoard>()) })
        .collect();

    let threads = thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = boards.len().div_ceil(threads);
    let sums = thread::scope(|scope| {
        let workers: Vec<_> = boards
            .chunks(chunk)
            .map(|share| {
                let network = &network;
                scope.spawn(move || {
                    let mut sums = [0f64; 3];
                    for &board in share {
                        let predicted = sigmoid(network.output(board));
                        // the scale goes in as a factor: sigmoid(score * k)
                        let targets = [
                            board.blended_result(wdl, 1.0 / SCALE),
                            board.result(),
                            board.blended_result(0.0, 1.0 / SCALE),
                        ];
                        for (sum, target) in sums.iter_mut().zip(targets) {
                            *sum += f64::from((predicted - target).powi(2));
                        }
                    }
                    sums
                })
            })
            .collect();
        workers.into_iter().fold([0f64; 3], |mut total, worker| {
            for (t, s) in total.iter_mut().zip(worker.join().unwrap()) {
                *t += s;
            }
            total
        })
    });

    let n = boards.len() as f64;
    println!(
        "{}: {} positions, loss {:.6} (wdl {wdl}), {:.6} against the result, {:.6} against the score",
        checkpoint.display(),
        boards.len(),
        sums[0] / n,
        sums[1] / n,
        sums[2] / n,
    );
}
