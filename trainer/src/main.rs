//! Trains the networks of runner-chess with bullet, from this directory:
//!
//! ```text
//! cargo r -r -- <data> <net id> [superbatches] [wdl]
//! ```
//!
//! `<data>` is a file made by `scripts/prepare-data.sh`. The checkpoints go to
//! `checkpoints/<net id>-<superbatch>/`, where `quantised.bin` is the network the
//! engine embeds. Every superbatch is one pass over the data. At the end, the
//! evaluation of a few positions gets written to `checkpoints/<net id>-evals.txt`,
//! for the engine to check its inference against.

use std::{fs, path::Path};

use bullet::{
    game::{inputs::Chess768, outputs::MaterialCount},
    nn::optimiser::AdamW,
    trainer::{
        save::SavedFormat,
        schedule::{TrainingSchedule, TrainingSteps, lr, wdl},
        settings::LocalSettings,
    },
    value::{ValueTrainerBuilder, loader::DirectSequentialDataLoader},
};

// the engine's inference has to match these
const HIDDEN_SIZE: usize = 256;
/// The output layer of a position is the one of its bucket: (pieces - 2) / 4
const OUTPUT_BUCKETS: usize = 8;
const SCALE: i32 = 400;
const QA: i16 = 255;
const QB: i16 = 64;

const BATCH_SIZE: usize = 16_384;
/// A position in bullet's binary format
const POSITION_BYTES: u64 = 32;
const CHECKPOINTS: &str = "checkpoints";

/// For the engine to compare its evaluation with: both sides to move, material up
/// and down, a position with its colors swapped, which must evaluate the same, and
/// output buckets 7, 4, 1 and 0
const REFERENCE_FENS: [&str; 9] = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
    "rnbqkb1r/pppp1ppp/5n2/4p3/4P3/2N5/PPPP1PPP/R1BQKBNR b KQkq - 2 3",
    "4k3/8/8/8/8/8/PPP5/4K2R w K - 0 1",
    "4k2r/ppp5/8/8/8/8/8/4K3 b k - 0 1",
    "r4rk1/pp3ppp/8/8/8/8/PP3PPP/R4RK1 w - - 0 1",
    "8/8/8/4k3/8/8/2BN4/4K3 w - - 0 1",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (data, net_id) = match &args[..] {
        [data, net_id, ..] => (data.as_str(), net_id.as_str()),
        _ => {
            eprintln!("usage: cargo r -r -- <data> <net id> [superbatches] [wdl]");
            std::process::exit(1);
        }
    };
    let superbatches: usize = args.get(2).map_or(30, |n| n.parse().expect("superbatches"));
    let wdl_proportion: f32 = args.get(3).map_or(0.5, |w| w.parse().expect("wdl"));

    let positions = fs::metadata(data).expect("the data file").len() / POSITION_BYTES;
    let batches_per_superbatch = (positions as usize / BATCH_SIZE).max(1);

    let mut trainer = ValueTrainerBuilder::default()
        .dual_perspective()
        .optimiser(AdamW)
        .inputs(Chess768)
        .output_buckets(MaterialCount::<OUTPUT_BUCKETS>)
        .save_format(&[
            SavedFormat::id("l0w").round().quantise::<i16>(QA),
            SavedFormat::id("l0b").round().quantise::<i16>(QA),
            // a bucket's weights next to each other, as the engine reads them
            SavedFormat::id("l1w")
                .round()
                .quantise::<i16>(QB)
                .transpose(),
            SavedFormat::id("l1b").round().quantise::<i16>(QA * QB),
        ])
        // the target is wdl * result + (1 - wdl) * sigmoid(score / SCALE)
        .loss_fn(|output, target| output.sigmoid().squared_error(target))
        .build(|builder, stm_inputs, ntm_inputs, output_buckets| {
            let l0 = builder.new_affine("l0", 768, HIDDEN_SIZE);
            let l1 = builder.new_affine("l1", 2 * HIDDEN_SIZE, OUTPUT_BUCKETS);

            let stm_hidden = l0.forward(stm_inputs).screlu();
            let ntm_hidden = l0.forward(ntm_inputs).screlu();
            l1.forward(stm_hidden.concat(ntm_hidden))
                .select(output_buckets)
        });

    let initial_lr = 0.001;
    let schedule = TrainingSchedule {
        net_id: net_id.to_string(),
        eval_scale: SCALE as f32,
        steps: TrainingSteps {
            batch_size: BATCH_SIZE,
            batches_per_superbatch,
            start_superbatch: 1,
            end_superbatch: superbatches,
        },
        wdl_scheduler: wdl::ConstantWDL {
            value: wdl_proportion,
        },
        lr_scheduler: lr::CosineDecayLR {
            initial_lr,
            final_lr: initial_lr * 0.3f32.powi(5),
            final_superbatch: superbatches,
        },
        save_rate: 10,
    };

    let settings = LocalSettings {
        threads: 2,
        test_set: None,
        output_directory: CHECKPOINTS,
        batch_queue_size: 32,
    };

    let loader = DirectSequentialDataLoader::new(&[data]);
    trainer.run(&schedule, &settings, &loader);

    let evals: String = REFERENCE_FENS
        .iter()
        .map(|fen| {
            let centipawns = trainer.eval_raw_output(fen)[0] * SCALE as f32;
            format!("{fen} | {centipawns:.1}\n")
        })
        .collect();
    print!("{evals}");
    fs::write(
        Path::new(CHECKPOINTS).join(format!("{net_id}-evals.txt")),
        evals,
    )
    .expect("the evaluations");
}
