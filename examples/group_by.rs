//! Microbench shaped like a grouped report request: many small concurrent folds.
//!
//! 1,000 groups x 17 metrics, each folding 30 small serialized sketches (1..60 values at p=15),
//! then one cardinality per (group, metric). Times one request with `add_bytes` and with
//! `add_trusted_bytes`.
use std::time::Instant;
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

const GROUPS: usize = 1_000;
const METRICS: usize = 17;
const ROWS: usize = 30;

fn main() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    // A pool of distinct small sketches; requests index into it.
    let pool: Vec<Vec<u8>> = (0..4_096)
        .map(|_| {
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            let start = (next() % 10_000_000) as i64;
            for v in start..start + 1 + (next() % 60) as i64 {
                s.add_long(v).unwrap();
            }
            s.serialize()
        })
        .collect();
    let picks: Vec<usize> = (0..GROUPS * METRICS * ROWS)
        .map(|_| (next() % 4_096) as usize)
        .collect();

    println!(
        "size_of::<Accumulator>() = {} bytes",
        std::mem::size_of::<Accumulator>()
    );
    for trusted in [false, true] {
        let mut best = f64::MAX;
        let mut total = 0i64;
        for _ in 0..5 {
            let t = Instant::now();
            let mut accs: Vec<Accumulator> =
                (0..GROUPS * METRICS).map(|_| Accumulator::new()).collect();
            for (i, &p) in picks.iter().enumerate() {
                if trusted {
                    accs[i / ROWS].add_trusted_bytes(&pool[p]).unwrap();
                } else {
                    accs[i / ROWS].add_bytes(&pool[p]).unwrap();
                }
            }
            total = accs.iter().map(Accumulator::cardinality).sum();
            best = best.min(t.elapsed().as_secs_f64() * 1e3);
            std::hint::black_box(&accs);
        }
        let mode = if trusted {
            "add_trusted_bytes"
        } else {
            "add_bytes"
        };
        println!(
            "group-by request ({mode}): {best:.1} ms ({:.0} ns per sketch fold), total cardinality {total}",
            best * 1e6 / (GROUPS * METRICS * ROWS) as f64
        );
    }
}
