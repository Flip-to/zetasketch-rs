//! Microbench: cost of one dense estimate (`cardinality`) at p = 15.
use std::time::Instant;
use zetasketch::HyperLogLogPlusPlus;

fn main() {
    let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
    for v in 0..1_000_000 {
        s.add_long(v).unwrap();
    }
    let _ = s.serialize();
    let mut best = f64::MAX;
    let mut est = 0;
    for _ in 0..20 {
        let t = Instant::now();
        for _ in 0..200 {
            est = std::hint::black_box(&s).cardinality();
        }
        best = best.min(t.elapsed().as_nanos() as f64 / 200.0);
    }
    println!("dense estimate p=15: {best:.0} ns (estimate {est})");
}
