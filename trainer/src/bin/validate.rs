//! `cargo r -r --bin validate -- <checkpoint dir> <data> [wdl] [--king-buckets]
//! [--wing-outputs]`: the training loss of the network on other data, since bullet has
//! no validation yet, and the loss against the result and the score alone. Also how many
//! times bigger the network's scores are than those of the data, a factor the engine
//! divides its output by. The network is `quantised.bin`, the one the engine plays,
//! as `raw.bin` lacks the factoriser of the king buckets. The flags are the trainer's.
//! With `--fen <fen>` instead of the data, the network's score of that position, in
//! centipawns for the side to move, to check the engine's inference against.

use std::{fs, path::Path, thread};

use bulletformat::{BulletFormat, ChessBoard};
use trainer::{HIDDEN_SIZE, INPUT_BUCKETS, KING_BUCKETS, OUTPUT_BUCKETS, QA, QB, SCALE};

struct Network {
    /// The column of each input, one after the other
    l0w: Vec<f32>,
    l0b: Vec<f32>,
    /// For each bucket, the side to move's half and then the other's
    l1w: Vec<f32>,
    l1b: Vec<f32>,
    king_buckets: bool,
    wing_outputs: bool,
}

impl Network {
    fn load(path: &Path, king_buckets: bool, wing_outputs: bool) -> Self {
        let inputs = 768 * if king_buckets { INPUT_BUCKETS } else { 1 };
        let buckets = OUTPUT_BUCKETS * if wing_outputs { 2 } else { 1 };
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let values: Vec<f32> = bytes
            .chunks_exact(2)
            .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])))
            .collect();
        let sizes = [inputs * HIDDEN_SIZE, HIDDEN_SIZE, 2 * HIDDEN_SIZE * buckets, buckets];
        // bullet pads the file to a multiple of 64 bytes
        let size: usize = sizes.iter().sum();
        assert!(
            (size * 2).div_ceil(64) * 64 == bytes.len(),
            "not a network of {inputs} inputs and {buckets} buckets"
        );
        let mut rest = &values[..];
        let [l0w, l0b, l1w, l1b] = sizes.map(|n| {
            let (part, tail) = rest.split_at(n);
            rest = tail;
            part.to_vec()
        });
        let (qa, qb) = (f32::from(QA), f32::from(QB));
        let scale = |values: Vec<f32>, by: f32| values.into_iter().map(|v| v / by).collect();
        Self {
            l0w: scale(l0w, qa),
            l0b: scale(l0b, qa),
            l1w: scale(l1w, qb),
            l1b: scale(l1b, qa * qb),
            king_buckets,
            wing_outputs,
        }
    }

    /// Where a view's inputs start and how they get mirrored, as `ChessBucketsMirrored`,
    /// from its own king's square
    fn king_view(&self, ksq: u8) -> (usize, usize) {
        if !self.king_buckets {
            return (0, 0);
        }
        let (file, rank) = (usize::from(ksq % 8), usize::from(ksq / 8));
        let flip = if file > 3 { 7 } else { 0 };
        (768 * KING_BUCKETS[4 * rank + file.min(7 - file)], flip)
    }

    /// Before the sigmoid, with the inputs of bullet's `Chess768` or `ChessBucketsMirrored`
    fn output(&self, board: ChessBoard) -> f32 {
        let mut stm: [f32; HIDDEN_SIZE] = self.l0b[..].try_into().unwrap();
        let mut ntm = stm;
        let (stm_first, stm_flip) = self.king_view(board.our_ksq());
        let (ntm_first, ntm_flip) = self.king_view(board.opp_ksq());
        for (piece, square) in board {
            let c = usize::from(piece & 8 > 0);
            let pc = 64 * usize::from(piece & 7);
            let sq = usize::from(square);
            for (acc, feature) in [
                (&mut stm, stm_first + (([0, 384][c] + pc + sq) ^ stm_flip)),
                (&mut ntm, ntm_first + (([384, 0][c] + pc + (sq ^ 56)) ^ ntm_flip)),
            ] {
                let column = &self.l0w[feature * HIDDEN_SIZE..][..HIDDEN_SIZE];
                for (a, w) in acc.iter_mut().zip(column) {
                    *a += w;
                }
            }
        }

        let screlu = |x: f32| x.clamp(0.0, 1.0).powi(2);
        // bullet's MaterialCount, then the trainer's MaterialAndWings
        let material = (board.occ().count_ones() as usize - 2) / 32usize.div_ceil(OUTPUT_BUCKETS);
        let opposite = (board.our_ksq() % 8 > 3) != (board.opp_ksq() % 8 > 3);
        let bucket = material + OUTPUT_BUCKETS * usize::from(self.wing_outputs && opposite);
        let weights = &self.l1w[2 * HIDDEN_SIZE * bucket..][..2 * HIDDEN_SIZE];
        let us: f32 = (0..HIDDEN_SIZE).map(|i| screlu(stm[i]) * weights[i]).sum();
        let them: f32 = (0..HIDDEN_SIZE)
            .map(|i| screlu(ntm[i]) * weights[HIDDEN_SIZE + i])
            .sum();
        us + them + self.l1b[bucket]
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let king_buckets = args.iter().any(|a| a == "--king-buckets");
    let wing_outputs = args.iter().any(|a| a == "--wing-outputs");
    let fen = args.iter().position(|a| a == "--fen").map(|i| args[i + 1].clone());
    let args: Vec<&String> = args
        .iter()
        .filter(|a| !a.starts_with("--") && Some(*a) != fen.as_ref())
        .collect();
    if let (Some(fen), [checkpoint, ..]) = (&fen, &args[..]) {
        let network = Network::load(&Path::new(checkpoint).join("quantised.bin"), king_buckets, wing_outputs);
        let board: ChessBoard = format!("{fen} | 0 | 0.5").parse().expect("a FEN");
        println!("{:.1}", network.output(board) * SCALE as f32);
        return;
    }
    let (checkpoint, data) = match &args[..] {
        [checkpoint, data, ..] => (Path::new(checkpoint), data),
        _ => {
            eprintln!("usage: cargo r -r --bin validate -- <checkpoint dir> <data> [wdl]");
            std::process::exit(1);
        }
    };
    let wdl: f32 = args.get(2).map_or(0.5, |w| w.parse().expect("wdl"));

    let network = Network::load(&checkpoint.join("quantised.bin"), king_buckets, wing_outputs);
    let bytes = fs::read(data.as_str()).unwrap_or_else(|e| panic!("{data}: {e}"));
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
                    let mut sums = [0f64; 5];
                    for &board in share {
                        let output = network.output(board);
                        let predicted = sigmoid(output);
                        // the scale goes in as a factor: sigmoid(score * k)
                        let targets = [
                            board.blended_result(wdl, 1.0 / SCALE as f32),
                            board.result(),
                            board.blended_result(0.0, 1.0 / SCALE as f32),
                        ];
                        for (sum, target) in sums.iter_mut().zip(targets) {
                            *sum += f64::from((predicted - target).powi(2));
                        }
                        // for the least squares slope of the network's scores on the data's
                        let (score, net) = (f64::from(board.score()), f64::from(output * SCALE as f32));
                        sums[3] += score * net;
                        sums[4] += score * score;
                    }
                    sums
                })
            })
            .collect();
        workers.into_iter().fold([0f64; 5], |mut total, worker| {
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
    println!("scores {:.2} times those of the data", sums[3] / sums[4]);
}
