//! A grouped request at a fine grouping level: many small groups, folded on 8 "threads" (row
//! chunks folded into partials one after another), then combined with `merge` or `absorb`, then
//! estimated. Synthetic data only.
//!
//!     cargo run --release --example fine_level_combine
//!
//! Rows: 400,000, each with 17 sketch columns, most cells NULL, non-NULL sketches mostly of one
//! value (92%, the rest two or three). Each row belongs to one of `groups` groups with a skewed
//! distribution (a few big groups, a long tail). Best of 5 rounds per phase, in milliseconds.
use std::time::Instant;

use zetasketch::{Accumulator, HyperLogLogPlusPlus};

const ROWS: usize = 400_000;
const METRICS: usize = 17;
const PARTS: usize = 8;

fn main() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    // Sketches packed in one buffer, column by column, like a decoded column.
    let mut data = Vec::new();
    let mut cells: Vec<Vec<(u32, u32, u32)>> = vec![Vec::new(); METRICS]; // (row, start, end)
    for (m, col) in cells.iter_mut().enumerate() {
        let filled = if m == 0 { 950 } else { 150 };
        let pool = 200_000 / (m as u64 + 1);
        for r in 0..ROWS {
            if next() % 1000 >= filled {
                continue;
            }
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            let n = if next() % 100 < 92 { 1 } else { 2 + next() % 2 };
            for _ in 0..n {
                s.add_long((next() % pool) as i64).unwrap();
            }
            let start = data.len() as u32;
            data.extend_from_slice(&s.serialize());
            col.push((r as u32, start, data.len() as u32));
        }
    }
    let sketches: usize = cells.iter().map(Vec::len).sum();
    println!("{ROWS} rows, {sketches} non-NULL sketches");

    for groups in [100usize, 1_000, 5_000, 20_000] {
        let group_of: Vec<usize> = (0..ROWS)
            .map(|_| {
                let (a, b) = (next() % groups as u64, next() % groups as u64);
                a.min(b) as usize
            })
            .collect();
        let fold = || -> Vec<Vec<Accumulator>> {
            let chunk = ROWS.div_ceil(PARTS);
            (0..PARTS)
                .map(|part| {
                    let mut accs: Vec<Accumulator> =
                        (0..groups * METRICS).map(|_| Accumulator::new()).collect();
                    let rows = part * chunk..((part + 1) * chunk).min(ROWS);
                    for (m, col) in cells.iter().enumerate() {
                        for &(r, s, e) in col {
                            if rows.contains(&(r as usize)) {
                                accs[group_of[r as usize] * METRICS + m]
                                    .add_bytes(&data[s as usize..e as usize])
                                    .unwrap();
                            }
                        }
                    }
                    accs
                })
                .collect()
        };
        let mut t_fold = f64::MAX;
        let (mut t_merge, mut t_absorb, mut t_est, mut t_est2) =
            (f64::MAX, f64::MAX, f64::MAX, f64::MAX);
        let mut checks = (0i64, 0i64);
        for _ in 0..5 {
            let t = Instant::now();
            let mut parts = fold();
            t_fold = t_fold.min(t.elapsed().as_secs_f64() * 1e3);
            let first = parts.remove(0);
            let rest = parts;
            // merge (dropping the partials is timed too: a combine consumes them)
            let mut all = first.clone();
            let copies = rest.clone();
            let t = Instant::now();
            for part in copies {
                for (into, from) in all.iter_mut().zip(&part) {
                    into.merge(from).unwrap();
                }
            }
            t_merge = t_merge.min(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            checks.0 = all.iter_mut().map(Accumulator::estimate).sum();
            t_est = t_est.min(t.elapsed().as_secs_f64() * 1e3);
            // absorb
            let mut all = first;
            let t = Instant::now();
            for part in rest {
                for (into, from) in all.iter_mut().zip(part) {
                    into.absorb(from).unwrap();
                }
            }
            t_absorb = t_absorb.min(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            checks.1 = all.iter_mut().map(Accumulator::estimate).sum();
            t_est2 = t_est2.min(t.elapsed().as_secs_f64() * 1e3);
        }
        assert_eq!(checks.0, checks.1);
        println!(
            "{groups:>6} groups: fold {t_fold:>6.1} ms; merge {t_merge:>6.1} + estimate {t_est:>5.1} ms; absorb {t_absorb:>6.1} + estimate {t_est2:>5.1} ms ({:.2}x)",
            (t_merge + t_est) / (t_absorb + t_est2)
        );
    }
}
