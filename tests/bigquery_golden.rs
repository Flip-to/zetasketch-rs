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

/// Mirrors `INT64_EDGES` in `scripts/gen_bigquery_fixtures.py`.
const INT64_EDGES: [i64; 20] = [
    0,
    1,
    -1,
    2,
    -2,
    127,
    128,
    -128,
    255,
    256,
    65535,
    65536,
    2_147_483_647,
    2_147_483_648,
    -2_147_483_648,
    -2_147_483_649,
    4_294_967_295,
    4_294_967_296,
    i64::MAX,
    i64::MIN,
];

enum Value {
    Long(i64),
    Str(String),
    Bytes(Vec<u8>),
    Null,
}

/// The value the generator's `FAMILIES[family]` SQL expression yields for `x`.
fn family_value(family: &str, x: i64) -> Value {
    match family {
        "int64" => Value::Long(x),
        "int64neg" => Value::Long(-x * 7919 - 1),
        "int64edge" => Value::Long(INT64_EDGES[x as usize]),
        "int64nulls" if x % 3 == 0 => Value::Null,
        "int64nulls" => Value::Long(x),
        "string" => Value::Str(format!("v{x}")),
        "stringlen" => Value::Str("a".repeat(x as usize)),
        "string41" => Value::Str(format!("m{x:0>40}")),
        "string100" => Value::Str(format!("{}{x}", "L".repeat(100))),
        "unicode" => Value::Str(format!("\u{fc}\u{5b57}\u{1f642}{x}")),
        "bytes" => Value::Bytes(format!("b{x}").into_bytes()),
        "bytesraw" => {
            let mut b = vec![0x00, 0xff];
            b.extend_from_slice(x.to_string().as_bytes());
            b.push(0x00);
            Value::Bytes(b)
        }
        "byteslen" => Value::Bytes("z".repeat(x as usize).into_bytes()),
        other => panic!("unknown family {other}"),
    }
}

/// Rebuilds each INIT sketch from the same synthetic values the generator used, so the
/// Fingerprint2011 hash (every input length branch, negatives, unicode, NULLs) and the sparse
/// buffering are checked against BigQuery, not just decoding.
#[test]
fn init_rows_rebuilt_from_values() {
    let mut n_rows = 0;
    for r in rows(FIXTURES).filter(|r| r[0] == "INIT") {
        let name = r[1];
        let mut parts = name.rsplitn(3, '_');
        let n: i64 = parts.next().unwrap()[1..].parse().unwrap();
        let p: i32 = parts.next().unwrap()[1..].parse().unwrap();
        let family = parts.next().unwrap();
        let mut s = if family.starts_with("int64") {
            HyperLogLogPlusPlus::for_longs(p)
        } else {
            HyperLogLogPlusPlus::for_strings(p)
        }
        .unwrap();
        for x in 0..n {
            match family_value(family, x) {
                Value::Long(v) => s.add_long(v),
                Value::Str(v) => s.add_str(&v),
                Value::Bytes(v) => s.add_bytes(&v),
                Value::Null => Ok(()),
            }
            .unwrap();
        }
        assert_eq!(b64_encode(&s.serialize()), r[2], "{name}: bytes");
        n_rows += 1;
    }
    assert!(n_rows >= 160, "only {n_rows} INIT rows");
}

#[test]
fn merge_rows() {
    let mut n = 0;
    for r in rows(FIXTURES).filter(|r| r[0] == "MERGE") {
        let name = r[1];
        // An empty field is a NULL sketch, which HLL_COUNT.MERGE skips.
        let mut inputs = r[2..r.len() - 2].iter().filter(|s| !s.is_empty());
        let mut acc = HyperLogLogPlusPlus::from_bytes(&b64_decode(inputs.next().unwrap())).unwrap();
        for b64 in inputs {
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
