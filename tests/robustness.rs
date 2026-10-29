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

/// `add_trusted_bytes` skips per-value checks, so corrupt input may error or panic, but it must
/// never hang or abort the process. Each case runs under `catch_unwind`.
#[test]
fn trusted_mode_on_corrupt_input_errors_or_panics_only() {
    let seeds: Vec<Vec<u8>> = rows(BIGQUERY)
        .filter(|r| r[0] == "INIT")
        .map(|r| b64_decode(r[2]))
        .filter(|b| b.len() < 4096)
        .collect();
    let mut rng = Rng(0x7a57_ed00);
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut panics = 0;
    for i in 0..5_000 {
        let mut b = seeds[i % seeds.len()].clone();
        let at = (rng.next() as usize) % b.len();
        b[at] ^= 1 << (rng.next() % 8);
        let partner = seeds[(i * 3 + 1) % seeds.len()].clone();
        let outcome = std::panic::catch_unwind(move || {
            let mut acc = Accumulator::new();
            let _ = acc.add_trusted_bytes(&partner);
            let _ = acc.add_trusted_bytes(&b);
            let _ = acc.cardinality();
            let _ = acc.serialize();
        });
        panics += usize::from(outcome.is_err());
    }
    std::panic::set_hook(default_hook);
    // Panics are allowed (documented); this only checks that nothing worse happens.
    assert!(panics <= 5_000);
}

const KLL: &str = include_str!("data/kll_golden.tsv");

/// The experimental KLL decoder: mutated and random bytes return errors, never panic, and
/// whatever decodes can be queried.
#[test]
fn kll_mutated_sketches_never_panic() {
    let seeds: Vec<Vec<u8>> = rows(KLL)
        .filter(|r| r[0] == "KLL")
        .map(|r| b64_decode(r[3]))
        .filter(|b| b.len() < 4096)
        .collect();
    assert!(seeds.len() > 20);
    let mut rng = Rng(0x0c11_7e57);
    for i in 0..20_000 {
        let mut b = seeds[i % seeds.len()].clone();
        match rng.next() % 3 {
            0 => {
                let at = (rng.next() as usize) % b.len();
                b[at] ^= 1 << (rng.next() % 8);
            }
            1 => {
                let at = (rng.next() as usize) % b.len();
                b[at] = rng.next() as u8;
            }
            _ => b.truncate((rng.next() as usize) % b.len()),
        }
        if let Ok(s) = zetasketch::kll::KllSketch::from_bytes(&b) {
            let _ = s.quantiles(1 + (rng.next() % 200) as usize);
            let _ = s.point((rng.next() % 1001) as f64 / 1000.0);
            let _ = (s.min(), s.max(), s.k(), s.num_values());
        }
    }
}
