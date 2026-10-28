//! Microbench: cost per stored value of folding sparse sketches into a dense accumulator.
use std::time::Instant;
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

fn main() {
    let mut big = HyperLogLogPlusPlus::for_longs(15).unwrap();
    for v in 0..100_000 {
        big.add_long(v).unwrap();
    }
    let dense = big.serialize();
    let sparse: Vec<Vec<u8>> = (0..1000)
        .map(|i| {
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            for v in 0..1500 {
                s.add_long(i * 1_000_003 + v).unwrap();
            }
            s.serialize()
        })
        .collect();
    let values = 1000.0 * 1500.0;
    let mut acc = Accumulator::new();
    let mut best = f64::MAX;
    for _ in 0..15 {
        acc.reset();
        acc.add_bytes(&dense).unwrap();
        let t = Instant::now();
        for s in &sparse {
            acc.add_bytes(s).unwrap();
        }
        best = best.min(t.elapsed().as_nanos() as f64 / values);
        std::hint::black_box(acc.cardinality());
    }
    println!("sparse -> dense: {best:.2} ns/value (best of 15)");
}
