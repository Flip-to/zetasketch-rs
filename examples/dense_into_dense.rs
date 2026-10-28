//! Microbench: cost per dense p=15 input folded into a dense accumulator.
use std::time::Instant;
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

fn main() {
    let dense: Vec<Vec<u8>> = (0..300)
        .map(|i| {
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            for v in 0..30_000 {
                s.add_long(i * 10_000_019 + v).unwrap();
            }
            s.serialize()
        })
        .collect();
    let mut acc = Accumulator::new();
    let mut best = f64::MAX;
    for _ in 0..20 {
        acc.reset();
        let t = Instant::now();
        for s in &dense {
            acc.add_bytes(s).unwrap();
        }
        best = best.min(t.elapsed().as_nanos() as f64 / dense.len() as f64);
        std::hint::black_box(acc.cardinality());
    }
    println!(
        "dense -> dense: {best:.0} ns/input ({:.2} ns/register, best of 20)",
        best / 32768.0
    );
}
