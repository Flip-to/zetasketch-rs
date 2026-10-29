//! Small tests for paths the coverage report (cargo-llvm-cov, CI run 40792010183) showed as never
//! executed: the accumulator's fallback accessors, untyped sketches, error messages, and the
//! p = 5 estimator constant.

use zetasketch::{Accumulator, Error, HyperLogLogPlusPlus, ValueKind};

fn sketch(p: i32, values: std::ops::Range<i64>) -> Vec<u8> {
    let mut s = HyperLogLogPlusPlus::for_longs(p).unwrap();
    for v in values {
        s.add_long(v).unwrap();
    }
    s.serialize()
}

/// Bytes of an empty sketch with no value type: decodes as "accepts any kind".
fn untyped_empty(p: i32, sp: i32) -> Vec<u8> {
    // AggregatorStateProto { type: 112, num_values: 0, encoding_version: 2,
    //   [hll] { precision_or_num_buckets: p, sparse_precision_or_num_buckets: sp } }
    let hll = [0x18, p as u8, 0x20, sp as u8];
    let mut out = vec![
        0x08,
        0x70,
        0x10,
        0x00,
        0x18,
        0x02,
        0x82,
        0x07,
        hll.len() as u8,
    ];
    out.extend_from_slice(&hll);
    out
}

#[test]
fn untyped_sketch_accepts_any_kind_until_first_add() {
    let mut s = HyperLogLogPlusPlus::from_bytes(&untyped_empty(12, 17)).unwrap();
    assert_eq!(s.value_kind(), None);
    s.add_str("x").unwrap();
    assert_eq!(s.value_kind(), Some(ValueKind::BytesOrString));
    assert!(s.add_long(1).is_err());
}

#[test]
fn accumulator_accessors_in_fallback_mode() {
    // Sparse mode disabled (sp = 0) is never on the fast path, so the first input puts the
    // accumulator straight into the reference fallback.
    let mut dense_only = HyperLogLogPlusPlus::new(12, 0, ValueKind::Long).unwrap();
    for v in 0..100 {
        dense_only.add_long(v).unwrap();
    }
    let bytes = dense_only.serialize();
    let mut acc = Accumulator::new();
    acc.add_bytes(&bytes).unwrap();
    assert!(!acc.is_empty());
    assert_eq!(acc.precisions(), Some((12, 0)));
    let snapshot = acc.to_sketch().unwrap();
    assert_eq!(snapshot.normal_precision(), 12);
    assert_eq!(acc.cardinality(), dense_only.cardinality());

    acc.reset();
    assert!(acc.is_empty());
    assert_eq!(acc.precisions(), None);
    assert!(acc.to_sketch().is_none());
    assert_eq!(acc.cardinality(), 0);
}

#[test]
fn estimate_at_precision_5_uses_its_alpha_constant() {
    // p = 5 has its own alpha (0.697121585) and a dense estimate above the linear-counting
    // threshold once every register is set.
    let mut s = HyperLogLogPlusPlus::new(5, 0, ValueKind::Long).unwrap();
    for v in 0..2_000 {
        s.add_long(v).unwrap();
    }
    let e = s.estimate();
    assert!((1_500..2_600).contains(&e), "estimate {e}");
    assert_eq!(e, s.cardinality());
    // Round trip keeps it.
    let mut back = HyperLogLogPlusPlus::from_bytes(&s.serialize()).unwrap();
    assert_eq!(back.estimate(), e);
}

#[test]
fn error_messages_name_the_problem() {
    let proto = HyperLogLogPlusPlus::from_bytes(&[0x0a]).unwrap_err();
    assert!(matches!(proto, Error::Proto(_)));
    assert!(proto.to_string().starts_with("malformed proto"));

    let mut longs = HyperLogLogPlusPlus::from_bytes(&sketch(12, 0..10)).unwrap();
    let mut strings = HyperLogLogPlusPlus::for_strings(12).unwrap();
    strings.add_str("a").unwrap();
    let ty = longs.merge(strings).unwrap_err();
    assert!(matches!(ty, Error::IncompatibleType(_)));
    assert!(ty.to_string().starts_with("incompatible value type"));
}
