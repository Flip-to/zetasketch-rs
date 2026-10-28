//! Any bytes: decode, estimate, serialize, merge into a valid sketch. Must never panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

fuzz_target!(|data: &[u8]| {
    if let Ok(mut s) = HyperLogLogPlusPlus::from_bytes(data) {
        let _ = s.estimate();
        let bytes = s.serialize();
        // Re-serializing what we decoded must round-trip.
        let mut again = HyperLogLogPlusPlus::from_bytes(&bytes).expect("own output decodes");
        assert_eq!(again.serialize(), bytes);
        let _ = s.add_long(1);
    }
    let mut base = HyperLogLogPlusPlus::for_longs(15).unwrap();
    base.add_long(42).unwrap();
    let _ = base.merge_bytes(data);
    let mut acc = Accumulator::new();
    let _ = acc.add_bytes(&base.serialize());
    let _ = acc.add_bytes(data);
    let _ = acc.estimate();
    let _ = acc.serialize();
});
