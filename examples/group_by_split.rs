//! Splits the group-by microbench into fold time and estimate time.
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
    let picks: Vec<usize> = (0..17_000 * 30)
        .map(|_| (next() % 4_096) as usize)
        .collect();
    let (mut fold, mut card, mut est) = (f64::MAX, f64::MAX, f64::MAX);
    for _ in 0..5 {
        let t = Instant::now();
        let mut accs: Vec<Accumulator> = (0..17_000).map(|_| Accumulator::new()).collect();
        for (i, &p) in picks.iter().enumerate() {
            accs[i / 30].add_bytes(&pool[p]).unwrap();
        }
        fold = fold.min(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let c: i64 = accs.iter().map(Accumulator::cardinality).sum();
        card = card.min(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let e: i64 = accs.iter_mut().map(Accumulator::estimate).sum();
        est = est.min(t.elapsed().as_secs_f64() * 1e3);
        assert_eq!(c, e);
    }
    println!("fold {fold:.1} ms | cardinality(&self) {card:.1} ms | estimate(&mut) {est:.1} ms");
}
