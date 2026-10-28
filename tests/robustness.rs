//! Corrupt input must produce an `Err`, never a panic, on every public entry point. Mutates the
//! golden sketches (bit flips, truncation, splices) and runs decode, merge, estimate, serialize
//! and the accumulator on whatever decodes. This is the in-CI complement to `fuzz/`.

mod common;

use common::{b64_decode, rows};
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

const BIGQUERY: &str = include_str!("data/bigquery_golden.tsv");

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

fn exercise(bytes: &[u8], partner: &[u8]) {
    if let Ok(mut s) = HyperLogLogPlusPlus::from_bytes(bytes) {
        let _ = s.estimate();
        let _ = s.serialize();
        let _ = s.add_long(7);
        let _ = s.merge_bytes(partner);
        let _ = s.estimate();
        let _ = s.serialize();
    }
    if let Ok(mut s) = HyperLogLogPlusPlus::from_bytes(partner) {
        let _ = s.merge_bytes(bytes);
        let _ = s.estimate();
        let _ = s.serialize();
    }
    let mut acc = Accumulator::new();
    let _ = acc.add_bytes(partner);
    let _ = acc.add_bytes(bytes);
    let _ = acc.estimate();
    let _ = acc.serialize();
    let mut acc = Accumulator::new();
    let _ = acc.add_bytes(bytes);
    let _ = acc.add_bytes(partner);
    let _ = acc.estimate();
    let _ = acc.serialize();
}

#[test]
fn mutated_sketches_never_panic() {
    // Small sketches only: the dense p=15 ones are 32 KiB each and add nothing new.
    let seeds: Vec<Vec<u8>> = rows(BIGQUERY)
        .filter(|r| r[0] == "INIT")
        .map(|r| b64_decode(r[2]))
        .filter(|b| b.len() < 4096)
        .collect();
    assert!(seeds.len() > 10);
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for i in 0..20_000 {
        let base = &seeds[i % seeds.len()];
        let partner = &seeds[(i * 7 + 3) % seeds.len()];
        let mut b = base.clone();
        match rng.next() % 4 {
            0 => {
                let at = (rng.next() as usize) % b.len();
                b[at] ^= 1 << (rng.next() % 8);
            }
            1 => {
                let at = (rng.next() as usize) % b.len();
                b[at] = rng.next() as u8;
            }
            2 => b.truncate((rng.next() as usize) % b.len()),
            _ => {
                let at = (rng.next() as usize) % b.len();
                let other = &seeds[(rng.next() as usize) % seeds.len()];
                let from = (rng.next() as usize) % other.len();
                b.splice(at.., other[from..].iter().copied());
            }
        }
        exercise(&b, partner);
    }
}

#[test]
fn random_bytes_never_panic() {
    let partner = b64_decode(rows(BIGQUERY).find(|r| r[0] == "INIT").unwrap()[2]);
    let mut rng = Rng(42);
    for _ in 0..20_000 {
        let len = (rng.next() % 64) as usize;
        let b: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        exercise(&b, &partner);
    }
}
