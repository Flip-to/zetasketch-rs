//! Folding one large group on several threads with `Accumulator::merge`, compared with one thread.
//! 8,000 synthetic p=15 sketches (70% sparse, 30% dense), split into one chunk per thread.
use std::time::Instant;
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

fn main() {
    let mut seed = 0x9a7a_11e1u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let inputs: Vec<Vec<u8>> = (0..8_000)
        .map(|i| {
            let n = if i % 10 < 7 {
                1 + next() % 2_000
            } else {
                5_000 + next() % 45_000
            };
            let start = (next() % 50_000_000) as i64;
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            for v in start..start + n as i64 {
                s.add_long(v).unwrap();
            }
            s.serialize()
        })
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
    let fold = |c: &[Vec<u8>]| {
        let mut a = Accumulator::new();
        for b in c {
            a.add_bytes(b).unwrap();
        }
        a
    };
    let mut best_single = f64::MAX;
    let mut best_parallel = f64::MAX;
    let mut est = (0, 0);
    for _ in 0..5 {
        let t = Instant::now();
        let a = fold(&inputs);
        best_single = best_single.min(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let parts: Vec<Accumulator> = std::thread::scope(|s| {
            let hs: Vec<_> = inputs
                .chunks(inputs.len().div_ceil(threads))
                .map(|c| s.spawn(move || fold(c)))
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let mut acc = parts[0].clone();
        for p in &parts[1..] {
            acc.merge(p).unwrap();
        }
        best_parallel = best_parallel.min(t.elapsed().as_secs_f64() * 1e3);
        est = (a.cardinality(), acc.cardinality());
    }
    println!(
        "parallel fold: 1 thread {best_single:.1} ms, {threads} threads {best_parallel:.1} ms ({:.1}x); estimates {} / {}",
        best_single / best_parallel,
        est.0,
        est.1
    );
}
