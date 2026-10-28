//! Builds sketches from fuzzer-chosen precisions and value ranges, then checks that
//! `Accumulator` and the reference fold agree on bytes, estimate and value count.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

#[derive(Debug, Arbitrary)]
struct Part {
    precision: u8,
    start: u32,
    len: u16,
}

fuzz_target!(|parts: Vec<Part>| {
    let inputs: Vec<Vec<u8>> = parts
        .iter()
        .take(8)
        .map(|p| {
            let precision = 10 + i32::from(p.precision % 6);
            let mut s = HyperLogLogPlusPlus::for_longs(precision).unwrap();
            for v in i64::from(p.start)..i64::from(p.start) + i64::from(p.len % 8000) {
                s.add_long(v).unwrap();
            }
            s.serialize()
        })
        .collect();
    if inputs.is_empty() {
        return;
    }
    let mut reference = HyperLogLogPlusPlus::from_bytes(&inputs[0]).unwrap();
    for b in &inputs[1..] {
        reference.merge_bytes(b).unwrap();
    }
    let mut acc = Accumulator::new();
    for b in &inputs {
        acc.add_bytes(b).unwrap();
    }
    assert_eq!(acc.estimate(), reference.estimate());
    assert_eq!(acc.num_values(), reference.num_values());
    assert_eq!(acc.serialize().unwrap(), reference.serialize());
});
