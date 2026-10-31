//! Time of each merge in a sequence of 8 sparse partials of about 7 KB each into one accumulator
//! (the union crosses the dense threshold at the second or third merge), and of the estimate.
//!
//!     cargo run --release --example merge_steps
//!
//! Synthetic data only; best of 15 rounds per step, in microseconds.
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
    for (inputs, pool) in [(1_500u64, 20_000u64), (3_000, 200_000), (6_000, 200_000)] {
        let sketches: Vec<Vec<u8>> = (0..inputs * 8)
            .map(|_| {
                let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
                for _ in 0..=next() % 6 {
                    s.add_long((next() % pool) as i64).unwrap();
                }
                s.serialize()
            })
            .collect();
        let partials: Vec<Accumulator> = sketches
            .chunks(inputs as usize)
            .map(|part| {
                let mut a = Accumulator::new();
                for b in part {
                    a.add_trusted_bytes(b).unwrap();
                }
                a
            })
            .collect();
        // The same partials with their deferred values already merged into the sorted union (an
        // estimate does that), to separate the cost of flushing a source from the cost of folding it.
        let settled: Vec<Accumulator> = partials
            .iter()
            .map(|p| {
                let mut p = p.clone();
                std::hint::black_box(p.estimate());
                p
            })
            .collect();
        let bytes = partials[0].serialize().map_or(0, |b| b.len());
        let (mut best, mut best_est, mut best_ser) = ([f64::MAX; 8], f64::MAX, f64::MAX);
        for _ in 0..15 {
            let mut acc = Accumulator::new();
            for (i, p) in partials.iter().enumerate() {
                let t = Instant::now();
                acc.merge(p).unwrap();
                best[i] = best[i].min(t.elapsed().as_secs_f64() * 1e6);
            }
            let t = Instant::now();
            std::hint::black_box(acc.estimate());
            best_est = best_est.min(t.elapsed().as_secs_f64() * 1e6);
            let t = Instant::now();
            std::hint::black_box(acc.serialize());
            best_ser = best_ser.min(t.elapsed().as_secs_f64() * 1e6);
        }
        let mut settled_best = [f64::MAX; 8];
        for _ in 0..15 {
            let mut acc = Accumulator::new();
            for (i, p) in settled.iter().enumerate() {
                let t = Instant::now();
                acc.merge(p).unwrap();
                settled_best[i] = settled_best[i].min(t.elapsed().as_secs_f64() * 1e6);
            }
        }
        let settled_steps: Vec<String> = settled_best.iter().map(|v| format!("{v:.1}")).collect();
        let steps: Vec<String> = best.iter().map(|v| format!("{v:.1}")).collect();
        println!(
            "{inputs} inputs per partial ({bytes} bytes): merges {} us; estimate {best_est:.1} us; serialize {best_ser:.1} us",
            steps.join(" ")
        );
        println!(
            "    sources settled first:   merges {} us",
            settled_steps.join(" ")
        );
    }
}
