//! Pre-decoded entry points against `add_bytes`: 2-5 value sketches (`add_sparse_values`) and one
//! value per call or per run (`add_single_values`), into one dense or 2,000 sparse accumulators.
//! Synthetic values only. Best of 5, ns per sketch.
//!
//!     cargo run --release --example decoded_fold
use std::time::Instant;
use zetasketch::{Accumulator, HyperLogLogPlusPlus, SparseHeader};

fn main() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let n = 2_000_000;
    // bytes: 2-5 value sketches
    let bytes: Vec<Vec<u8>> = (0..n)
        .map(|_| {
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            for _ in 0..2 + next() % 4 {
                s.add_long((next() % 1_000_000_000) as i64).unwrap();
            }
            s.serialize()
        })
        .collect();
    // decoded: same counts, synthetic ascending plain sparse values (sp = 20, low 5 bits nonzero)
    let h = SparseHeader::new(8, 15, 20).unwrap(); // LONG
    let decoded: Vec<Vec<u32>> = (0..n)
        .map(|_| {
            let k = 2 + next() % 4;
            let mut v: Vec<u32> = (0..k).map(|_| ((next() % (1 << 20)) as u32) | 1).collect();
            v.sort_unstable();
            v.dedup();
            v
        })
        .collect();
    let singles: Vec<u32> = (0..n).map(|_| ((next() % (1 << 20)) as u32) | 1).collect();
    for (label, groups) in [("dense, 1 acc", 1usize), ("sparse, 2000 accs", 2000)] {
        let time = |f: &mut dyn FnMut(&mut [Accumulator])| {
            let mut best = f64::MAX;
            for _ in 0..5 {
                let mut accs: Vec<Accumulator> = (0..groups).map(|_| Accumulator::new()).collect();
                let t = Instant::now();
                f(&mut accs);
                best = best.min(t.elapsed().as_secs_f64() * 1e9 / n as f64);
                std::hint::black_box(&accs);
            }
            best
        };
        let a = time(&mut |accs| {
            for (i, b) in bytes.iter().enumerate() {
                accs[i % accs.len()].add_bytes(b).unwrap();
            }
        });
        let b = time(&mut |accs| {
            for (i, v) in decoded.iter().enumerate() {
                accs[i % accs.len()].add_sparse_values(&h, v).unwrap();
            }
        });
        let c = time(&mut |accs| {
            for (i, v) in singles.iter().enumerate() {
                accs[i % accs.len()]
                    .add_single_values(&h, std::slice::from_ref(v))
                    .unwrap();
            }
        });
        let d = time(&mut |accs| {
            let g = accs.len();
            let run = n / g;
            for (i, ch) in singles.chunks(run).enumerate() {
                accs[i % g].add_single_values(&h, ch).unwrap();
            }
        });
        println!("{label:<18} 2-5 values: add_bytes {a:5.1} ns, add_sparse_values {b:5.1} ns | one value: per-row add_single_values {c:5.1} ns, one run per acc {d:5.1} ns");
    }
}
