//! Replicates the `fold` bench "mixed" workload and times sparse and dense inputs separately.
use std::time::{Duration, Instant};
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

fn main() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let sketches: Vec<(bool, usize, Vec<u8>)> = (0..2000)
        .map(|i| {
            let r = next();
            let n = if i % 10 < 7 {
                1 + r % 2_000
            } else {
                5_000 + r % 45_000
            };
            let start = (next() % 1_000_000) as i64;
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            for v in start..start + n as i64 {
                s.add_long(v).unwrap();
            }
            let sparse = s.is_sparse();
            (sparse, n as usize, s.serialize())
        })
        .collect();
    let n_sparse = sketches.iter().filter(|s| s.0).count();
    let vals: usize = sketches.iter().filter(|s| s.0).map(|s| s.1).sum();
    let mut acc = Accumulator::new();
    let (mut ts, mut td) = (Duration::ZERO, Duration::ZERO);
    for _ in 0..10 {
        acc.reset();
        for (sparse, _, b) in &sketches {
            let t = Instant::now();
            acc.add_bytes(b).unwrap();
            if *sparse {
                ts += t.elapsed();
            } else {
                td += t.elapsed();
            }
        }
    }
    println!(
        "sparse inputs: {n_sparse}, avg {:.0} values, {:.0} ns/input, {:.2} ns/value",
        vals as f64 / n_sparse as f64,
        ts.as_nanos() as f64 / 10.0 / n_sparse as f64,
        ts.as_nanos() as f64 / 10.0 / vals as f64
    );
    println!(
        "dense inputs:  {}, {:.0} ns/input",
        2000 - n_sparse,
        td.as_nanos() as f64 / 10.0 / (2000 - n_sparse) as f64
    );
}
