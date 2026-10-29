//! Builds sketches from fuzzer-chosen precisions and value ranges, then checks that
//! `Accumulator` (via `add_bytes`, and via `add_sketch` on in-memory sketches that may hold
//! unflushed adds) and the reference fold agree on bytes, estimate, cardinality and value count.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

#[derive(Debug, Arbitrary)]
struct Part {
    precision: u8,
    start: u32,
    /// Log-scale size: 2^(size_bits % 15) values at most, so most parts are small and fast while
    /// some still cross the sparse -> dense threshold.
    size_bits: u8,
    size_frac: u16,
    /// Serialize (flush) the in-memory sketch before handing it to `add_sketch`?
    flush: bool,
}

fn build(p: &Part) -> HyperLogLogPlusPlus {
    let precision = 10 + i32::from(p.precision % 7);
    let max = 1u64 << (p.size_bits % 15);
    let n = u64::from(p.size_frac) % (max + 1);
    let mut s = HyperLogLogPlusPlus::for_longs(precision).unwrap();
    for v in u64::from(p.start)..u64::from(p.start) + n {
        s.add_long(v as i64).unwrap();
    }
    s
}

fuzz_target!(|parts: Vec<Part>| {
    let parts = &parts[..parts.len().min(10)];
    if parts.is_empty() {
        return;
    }
    let mut sketches: Vec<HyperLogLogPlusPlus> = parts.iter().map(build).collect();

    // In-memory path first: sketches whose part says `flush` are serialized (compacted), the
    // others keep unflushed adds.
    let mut acc = Accumulator::new();
    for (s, p) in sketches.iter_mut().zip(parts) {
        if p.flush {
            let _ = s.serialize();
        }
    }
    // `add_sketch` sees the (possibly) flushed sketches; rebuild the reference on the same ones.
    let mut reference_mem = sketches[0].clone();
    for s in &sketches[1..] {
        reference_mem.merge_ref(s).unwrap();
    }
    for s in &sketches {
        acc.add_sketch(s).unwrap();
    }
    assert_eq!(acc.cardinality(), reference_mem.cardinality());
    assert_eq!(acc.estimate(), reference_mem.estimate());
    assert_eq!(acc.num_values(), reference_mem.num_values());
    assert_eq!(acc.serialize().unwrap(), reference_mem.serialize());

    // Serialized path.
    let inputs: Vec<Vec<u8>> = sketches.iter_mut().map(HyperLogLogPlusPlus::serialize).collect();
    let mut reference = HyperLogLogPlusPlus::from_bytes(&inputs[0]).unwrap();
    for b in &inputs[1..] {
        reference.merge_bytes(b).unwrap();
    }
    acc.reset();
    for b in &inputs {
        acc.add_bytes(b).unwrap();
    }
    assert_eq!(acc.cardinality(), reference.cardinality());
    assert_eq!(acc.estimate(), reference.estimate());
    assert_eq!(acc.num_values(), reference.num_values());
    assert_eq!(acc.serialize().unwrap(), reference.serialize());
});
