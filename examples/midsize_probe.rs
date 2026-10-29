//! Probe: mid-size sparse sketches (up to 3,000 values) folded 1..40 per group, timed by the
//! accumulator's state before each input (sparse vs dense), using `precisions`/serialize length.
use std::time::Instant;
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

fn main() {
    let mut seed = 0x5e1f_c4ec_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let groups: Vec<Vec<Vec<u8>>> = (0..200)
        .map(|_| {
            (0..=(next() % 40))
                .map(|_| {
                    let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
                    let start = (next() % 1_000_000) as i64;
                    for v in start..start + (next() % 3_000) as i64 {
                        s.add_long(v).unwrap();
                    }
                    s.serialize()
                })
                .collect()
        })
        .collect();
    let mut acc = Accumulator::new();
    let (mut t_sparse, mut n_sparse, mut t_dense, mut n_dense) = (0f64, 0usize, 0f64, 0usize);
    for g in &groups {
        acc.reset();
        for b in g {
            // Dense iff the serialized state carries 2^15 registers (cheap enough for a probe).
            let dense = acc.to_sketch().is_some_and(|s| !s.is_sparse());
            let t = Instant::now();
            acc.add_bytes(b).unwrap();
            let e = t.elapsed().as_secs_f64();
            if dense {
                t_dense += e;
                n_dense += 1;
            } else {
                t_sparse += e;
                n_sparse += 1;
            }
        }
    }
    println!(
        "into sparse accumulator: {n_sparse} inputs, {:.0} ns each | into dense: {n_dense} inputs, {:.0} ns each",
        t_sparse * 1e9 / n_sparse.max(1) as f64,
        t_dense * 1e9 / n_dense.max(1) as f64
    );
}
