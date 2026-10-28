//! Fold throughput: reference `merge_bytes` vs `Accumulator`, on synthetic p=15 sketches.
//!
//!     cargo bench --bench fold
//!
//! Workload: 2,000 serialized INT64 sketches, 70% sparse (1..2,000 values) and 30% dense
//! (5,000..50,000 values), folded into one result 20 times. Std-only timing, no criterion.

use std::hint::black_box;
use std::time::Instant;

use zetasketch::{Accumulator, HyperLogLogPlusPlus};

fn workload() -> Vec<Vec<u8>> {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    (0..2_000)
        .map(|i| {
            let n = if i % 10 < 7 {
                1 + next() % 2_000
            } else {
                5_000 + next() % 45_000
            };
            let start = (next() % 1_000_000) as i64;
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            for v in start..start + n as i64 {
                s.add_long(v).unwrap();
            }
            s.serialize()
        })
        .collect()
}

fn time(
    label: &str,
    sketches: &[Vec<u8>],
    rounds: u32,
    mut fold: impl FnMut(&[Vec<u8>]) -> i64,
) -> f64 {
    black_box(fold(sketches));
    let start = Instant::now();
    let mut est = 0;
    for _ in 0..rounds {
        est = black_box(fold(sketches));
    }
    let per_sketch = start.elapsed().as_nanos() as f64 / f64::from(rounds) / sketches.len() as f64;
    println!("{label:<12} {per_sketch:>9.0} ns/sketch  (estimate {est})");
    per_sketch
}

fn main() {
    let sketches = workload();
    let bytes: usize = sketches.iter().map(Vec::len).sum();
    println!(
        "{} sketches, {:.1} MiB serialized",
        sketches.len(),
        bytes as f64 / 1_048_576.0
    );

    let reference = time("reference", &sketches, 20, |s| {
        let mut acc = HyperLogLogPlusPlus::from_bytes(&s[0]).unwrap();
        for b in &s[1..] {
            acc.merge_bytes(b).unwrap();
        }
        acc.estimate()
    });
    let mut acc = Accumulator::new();
    let fast = time("accumulator", &sketches, 20, |s| {
        acc.reset();
        for b in s {
            acc.add_bytes(b).unwrap();
        }
        acc.estimate()
    });
    println!("speedup      {:>9.1}x", reference / fast);
}
