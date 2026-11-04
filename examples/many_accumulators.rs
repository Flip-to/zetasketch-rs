//! Cost of folding sketches into many live accumulators, in the order a day-grain request does:
//! for each entity, for each day, one sketch per metric into the accumulator of that (day, metric).
//!
//!     cargo run --release --example many_accumulators
//!
//! Synthetic data only. 69 entities x 365 days x 17 metrics sketches, with a given mix of NULL,
//! one-value, few-value and many-value sketches; each scenario is folded into 6,205 accumulators
//! (one per day and metric), into 17 (one per metric) and into 1, with `add_bytes` and
//! `add_trusted_bytes`, and the best of five passes is printed as ns per non-NULL sketch.
use std::time::Instant;

use zetasketch::{Accumulator, HyperLogLogPlusPlus};

const ENTITIES: usize = 69;
const DAYS: usize = 365;
const METRICS: usize = 17;

/// Cumulative percentages: NULL, 1 value, 2 to 8, 9 to 60, 61 to 600.
const MIXES: [(&str, [u32; 5]); 3] = [
    ("tiny (40% null, 55% one value)", [40, 95, 100, 100, 100]),
    ("mixed (30% null, 40% one, 2% big)", [30, 70, 90, 98, 100]),
    ("heavy (10% null, 10% big)", [10, 30, 60, 90, 100]),
];

fn main() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    println!(
        "{:<36} {:>9} {:>10} {:>10} {:>10} {:>10}",
        "mix", "sketches", "6205 acc", "17 acc", "1 acc", "dense@6205"
    );
    for (name, cum) in MIXES {
        // sketches[(entity * DAYS + day) * METRICS + metric], None for NULL.
        // Packed into one buffer like a decoded column; `offsets[i]..offsets[i + 1]`, empty for NULL.
        let mut bytes: Vec<u8> = Vec::new();
        let mut offsets: Vec<usize> = vec![0];
        for e in 0..ENTITIES {
            for _ in 0..DAYS * METRICS {
                let r = (next() % 100) as u32;
                let kind = cum.iter().position(|&c| r < c).unwrap();
                let n = match kind {
                    0 => {
                        offsets.push(bytes.len());
                        continue;
                    }
                    1 => 1,
                    2 => 2 + next() % 7,
                    3 => 9 + next() % 52,
                    _ => 61 + next() % 540,
                };
                let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
                for _ in 0..n {
                    s.add_long((((e as u64) << 40) | (next() % 4_000_000)) as i64)
                        .unwrap();
                }
                bytes.extend_from_slice(&s.serialize());
                offsets.push(bytes.len());
            }
        }
        let folds = offsets.windows(2).filter(|w| w[1] > w[0]).count();
        let run = |groups: usize, trusted: bool| -> (f64, usize) {
            let mut best = f64::MAX;
            let mut dense = 0;
            for _ in 0..5 {
                let mut accs: Vec<Accumulator> = (0..groups).map(|_| Accumulator::new()).collect();
                let t = Instant::now();
                let mut i = 0;
                for _e in 0..ENTITIES {
                    for day in 0..DAYS {
                        for m in 0..METRICS {
                            let b = &bytes[offsets[i]..offsets[i + 1]];
                            if !b.is_empty() {
                                let g = match groups {
                                    6205 => day * METRICS + m,
                                    17 => m,
                                    _ => 0,
                                };
                                if trusted {
                                    accs[g].add_trusted_bytes(b).unwrap();
                                } else {
                                    accs[g].add_bytes(b).unwrap();
                                }
                            }
                            i += 1;
                        }
                    }
                }
                best = best.min(t.elapsed().as_secs_f64() * 1e9 / folds as f64);
                dense = accs
                    .iter()
                    .filter(|a| a.serialize().is_some_and(|s| s.len() > 30_000))
                    .count();
            }
            (best, dense)
        };
        let (a6205, d) = run(6205, false);
        let (a17, _) = run(17, false);
        let (a1, _) = run(1, false);
        let (t6205, _) = run(6205, true);
        let (t17, _) = run(17, true);
        let (t1, _) = run(1, true);
        println!(
            "{name:<36} {folds:>9} {a6205:>6.1}/{t6205:<4.1} {a17:>6.1}/{t17:<4.1} {a1:>6.1}/{t1:<4.1} {d:>10}"
        );
    }
    println!("(ns per non-NULL sketch: add_bytes / add_trusted_bytes)");
}
