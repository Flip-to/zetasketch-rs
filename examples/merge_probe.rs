//! Cost of `Accumulator::merge` of sparse and dense partials into one accumulator: the combine step
//! of a parallel fold (one partial per thread, merged per group).
//!
//!     cargo run --release --example merge_probe
//!
//! Synthetic data only. For each size, 8 partials are folded from random single- and multi-value
//! sketches drawn from a shared pool of values (so the partials overlap), then merged into a fresh
//! accumulator; the best of five rounds is printed as time per merge and per stored value.
use std::time::Instant;

use zetasketch::{Accumulator, HyperLogLogPlusPlus};

fn main() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    println!(
        "{:>10} {:>9} {:>12} {:>12} {:>12} {:>10}",
        "inputs", "state", "bytes/part", "us / merge", "ns / byte", "estimate"
    );
    for (inputs, pool) in [
        (100u64, 3_000u64),
        (400, 8_000),
        (1_500, 20_000),
        (6_000, 200_000),
    ] {
        // Sketches of 1 to 7 values drawn from the pool.
        let sketches: Vec<Vec<u8>> = (0..inputs * 8)
            .map(|_| {
                let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
                for _ in 0..=next() % 6 {
                    s.add_long((next() % pool) as i64).unwrap();
                }
                s.serialize()
            })
            .collect();
        let (mut best, mut values, mut est, mut dense) = (f64::MAX, 0usize, 0i64, 0usize);
        for _ in 0..5 {
            let mut partials: Vec<Accumulator> = Vec::new();
            for part in sketches.chunks(inputs as usize) {
                let mut a = Accumulator::new();
                for b in part {
                    a.add_trusted_bytes(b).unwrap();
                }
                partials.push(a);
            }
            values = partials
                .iter()
                .map(|p| p.serialize().map_or(0, |b| b.len()))
                .sum::<usize>()
                / partials.len();
            dense = partials
                .iter()
                .filter(|p| p.serialize().is_some_and(|b| b.len() > 20_000))
                .count();
            let t = Instant::now();
            let mut acc = Accumulator::new();
            for p in &partials {
                acc.merge(p).unwrap();
            }
            best = best.min(t.elapsed().as_secs_f64() * 1e6);
            est = acc.estimate();
        }
        println!(
            "{inputs:>10} {:>9} {values:>12} {:>12.1} {:>12.1} {est:>10}",
            if dense > 4 { "dense" } else { "sparse" },
            best / 8.0,
            best * 1e3 / (8.0 * values as f64),
        );
    }
}
