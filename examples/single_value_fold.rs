//! Microbench: single-value (and few-value) sparse sketches folded into one accumulator, the
//! shape of most non-null sketches in a day-grain event root (about 94% are 1 value). Reports
//! ns per `add_bytes` / `add_trusted_bytes` once the accumulator is dense, and while it is sparse.
use std::time::Instant;
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

fn sketches(n: usize, max_values: u64, seed: &mut u64) -> Vec<Vec<u8>> {
    let mut next = || {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    };
    (0..n)
        .map(|_| {
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            let start = (next() % 1_000_000_000) as i64;
            for v in start..start + 1 + (next() % max_values) as i64 {
                s.add_long(v).unwrap();
            }
            s.serialize()
        })
        .collect()
}

/// Inputs packed into one buffer, like a columnar binary array.
struct Packed {
    bytes: Vec<u8>,
    offsets: Vec<usize>,
}

impl Packed {
    fn new(items: &[Vec<u8>]) -> Self {
        let mut bytes = Vec::new();
        let mut offsets = vec![0];
        for i in items {
            bytes.extend_from_slice(i);
            offsets.push(bytes.len());
        }
        Packed { bytes, offsets }
    }

    fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    fn get(&self, i: usize) -> &[u8] {
        &self.bytes[self.offsets[i]..self.offsets[i + 1]]
    }
}

fn time(name: &str, warm: &[Vec<u8>], inputs: &Packed, trusted: bool) {
    let mut best = f64::MAX;
    for _ in 0..7 {
        let mut acc = Accumulator::new();
        for b in warm {
            acc.add_bytes(b).unwrap();
        }
        let t = Instant::now();
        for i in 0..inputs.len() {
            let b = inputs.get(i);
            if trusted {
                acc.add_trusted_bytes(b).unwrap();
            } else {
                acc.add_bytes(b).unwrap();
            }
        }
        best = best.min(t.elapsed().as_nanos() as f64 / inputs.len() as f64);
        std::hint::black_box(acc.cardinality());
    }
    println!("{name:<42} {best:6.1} ns per sketch");
}

/// ns per sketch for fresh accumulators that each take `n` singles (the sparse phase, up to the
/// dense threshold).
fn sparse_phase(name: &str, singles: &[Vec<u8>], n: usize) {
    let reps = (200_000 / n).max(5);
    let mut best = f64::MAX;
    for _ in 0..5 {
        let mut acc = Accumulator::new();
        let t = Instant::now();
        for _ in 0..reps {
            acc.reset();
            for b in &singles[..n] {
                acc.add_bytes(b).unwrap();
            }
        }
        best = best.min(t.elapsed().as_nanos() as f64 / (reps * n) as f64);
        std::hint::black_box(acc.cardinality());
    }
    println!("{name:<42} {best:6.1} ns per sketch");
}

fn main() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let warm = sketches(20_000, 1, &mut seed); // enough singles to turn the accumulator dense
    let singles_vec = sketches(1_000_000, 1, &mut seed);
    let few_vec = sketches(1_000_000, 3, &mut seed);
    let (singles, few) = (Packed::new(&singles_vec), Packed::new(&few_vec));
    println!(
        "single sketch: {} bytes; few: {:.1} bytes on average",
        singles_vec[0].len(),
        few.bytes.len() as f64 / few.len() as f64
    );
    time("single value, dense accumulator", &warm, &singles, false);
    time("single value, dense, trusted", &warm, &singles, true);
    time("1-3 values, dense accumulator", &warm, &few, false);
    time("1-3 values, dense, trusted", &warm, &few, true);
    // Sparse phase: fresh accumulators that take 300, 2,000 and 8,000 singles.
    for n in [300, 2_000, 8_000] {
        sparse_phase(
            &format!("single value, sparse accumulator ({n})"),
            &singles_vec,
            n,
        );
    }
}
