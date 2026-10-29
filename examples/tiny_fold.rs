//! Microbench: one reused accumulator folding 30 tiny sketches (1..60 values), repeated.
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
    let groups: Vec<Vec<Vec<u8>>> = (0..2_000)
        .map(|_| {
            (0..30)
                .map(|_| {
                    let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
                    let start = (next() % 10_000_000) as i64;
                    for v in start..start + 1 + (next() % 60) as i64 {
                        s.add_long(v).unwrap();
                    }
                    s.serialize()
                })
                .collect()
        })
        .collect();
    let values: usize = groups.iter().flatten().map(Vec::len).sum();
    let mut acc = Accumulator::new();
    let mut best = f64::MAX;
    for _ in 0..10 {
        let t = Instant::now();
        for g in &groups {
            acc.reset();
            for b in g {
                acc.add_bytes(b).unwrap();
            }
            std::hint::black_box(acc.cardinality());
        }
        best = best.min(t.elapsed().as_nanos() as f64 / (groups.len() * 30) as f64);
    }
    println!(
        "tiny fold (reused accumulator): {best:.0} ns per sketch; {:.1} bytes per sketch",
        values as f64 / (groups.len() * 30) as f64
    );
}
