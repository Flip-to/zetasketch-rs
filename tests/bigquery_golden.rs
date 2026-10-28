//! Parity with BigQuery `HLL_COUNT`, which runs Google's internal implementation rather than the
//! open-source Java one. Fixtures come from `scripts/gen_bigquery_fixtures.py` over synthetic
//! inputs only.
//!
//! - `INIT name b64 est`: decode, `estimate == HLL_COUNT.EXTRACT`, re-serialize byte for byte.
//! - `MERGE name in... merged est`: fold the inputs, `serialize == HLL_COUNT.MERGE_PARTIAL` and
//!   `estimate == HLL_COUNT.MERGE`.

mod common;

use common::{b64_decode, b64_encode, rows};
use zetasketch::HyperLogLogPlusPlus;

const FIXTURES: &str = include_str!("data/bigquery_golden.tsv");

#[test]
fn init_rows() {
    let mut n = 0;
    for r in rows(FIXTURES).filter(|r| r[0] == "INIT") {
        let (name, b64) = (r[1], r[2]);
        let mut s = HyperLogLogPlusPlus::from_bytes(&b64_decode(b64))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            s.estimate(),
            r[3].parse::<i64>().unwrap(),
            "{name}: estimate"
        );
        assert_eq!(b64_encode(&s.serialize()), b64, "{name}: bytes");
        n += 1;
    }
    assert!(n > 0, "no INIT rows");
}

/// Rebuilds each INIT sketch from the same synthetic values the generator used
/// (`x`, `'v' || x`, `b'b' || x` for `x` in `0..n`), so the hash is checked against BigQuery too.
#[test]
fn init_rows_rebuilt_from_values() {
    for r in rows(FIXTURES).filter(|r| r[0] == "INIT") {
        let name = r[1];
        let mut parts = name.split('_');
        let kind = parts.next().unwrap();
        let p: i32 = parts.next().unwrap()[1..].parse().unwrap();
        let n: i64 = parts.next().unwrap()[1..].parse().unwrap();
        let mut s = match kind {
            "int64" => HyperLogLogPlusPlus::for_longs(p),
            _ => HyperLogLogPlusPlus::for_strings(p),
        }
        .unwrap();
        for x in 0..n {
            match kind {
                "int64" => s.add_long(x),
                "string" => s.add_str(&format!("v{x}")),
                _ => s.add_bytes(format!("b{x}").as_bytes()),
            }
            .unwrap();
        }
        assert_eq!(b64_encode(&s.serialize()), r[2], "{name}: bytes");
    }
}

#[test]
fn merge_rows() {
    let mut n = 0;
    for r in rows(FIXTURES).filter(|r| r[0] == "MERGE") {
        let name = r[1];
        let inputs = &r[2..r.len() - 2];
        let mut acc = HyperLogLogPlusPlus::from_bytes(&b64_decode(inputs[0])).unwrap();
        for b64 in &inputs[1..] {
            acc.merge_bytes(&b64_decode(b64))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        assert_eq!(
            acc.estimate(),
            r[r.len() - 1].parse::<i64>().unwrap(),
            "{name}: estimate"
        );
        assert_eq!(
            b64_encode(&acc.serialize()),
            r[r.len() - 2],
            "{name}: bytes"
        );
        n += 1;
    }
    assert!(n > 0, "no MERGE rows");
}
