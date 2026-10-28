//! Parity with the reference Java ZetaSketch, through the golden corpus published by
//! zetasketch-cpp (`tests/golden_corpus.tsv`, Apache-2.0, generated from Java ZetaSketch built at
//! HEAD `d098c49`). Synthetic inputs only.
//!
//! - `SKETCH name b64 cardinality`: decode, estimate, re-serialize byte for byte.
//! - `MERGE name a b merged cardinality`: decode both, merge, estimate, serialize byte for byte.
//! - `LONGS name p sp count b64 cardinality`: add `0..count` as `i64`, serialize byte for byte.

mod common;

use common::{b64_decode, b64_encode, rows};
use zetasketch::{HyperLogLogPlusPlus, ValueKind};

const CORPUS: &str = include_str!("data/golden_corpus.tsv");

#[test]
fn sketch_rows_roundtrip_and_estimate() {
    let mut n = 0;
    for r in rows(CORPUS).filter(|r| r[0] == "SKETCH") {
        let (name, b64, card) = (r[1], r[2], r[3].parse::<i64>().unwrap());
        let mut s = HyperLogLogPlusPlus::from_bytes(&b64_decode(b64))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(s.estimate(), card, "{name}: estimate");
        assert_eq!(b64_encode(&s.serialize()), b64, "{name}: bytes");
        n += 1;
    }
    assert_eq!(n, 84);
}

#[test]
fn merge_rows() {
    let mut n = 0;
    for r in rows(CORPUS).filter(|r| r[0] == "MERGE") {
        let name = r[1];
        let mut a = HyperLogLogPlusPlus::from_bytes(&b64_decode(r[2])).unwrap();
        let b = HyperLogLogPlusPlus::from_bytes(&b64_decode(r[3])).unwrap();
        a.merge(b).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            a.estimate(),
            r[5].parse::<i64>().unwrap(),
            "{name}: estimate"
        );
        assert_eq!(b64_encode(&a.serialize()), r[4], "{name}: bytes");
        n += 1;
    }
    assert_eq!(n, 5);
}

#[test]
fn longs_rows_hash_and_build() {
    let mut n = 0;
    for r in rows(CORPUS).filter(|r| r[0] == "LONGS") {
        let name = r[1];
        let p: i32 = r[2].parse().unwrap();
        let sp: i32 = r[3].parse().unwrap();
        let count: i64 = r[4].parse().unwrap();
        let mut s = HyperLogLogPlusPlus::new(p, sp, ValueKind::Long).unwrap();
        for v in 0..count {
            s.add_long(v).unwrap();
        }
        assert_eq!(b64_encode(&s.serialize()), r[5], "{name}: bytes");
        assert_eq!(
            s.estimate(),
            r[6].parse::<i64>().unwrap(),
            "{name}: estimate"
        );
        n += 1;
    }
    assert_eq!(n, 98);
}
