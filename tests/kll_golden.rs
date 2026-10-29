//! BigQuery `KLL_QUANTILES` parity for the experimental `kll` module. Fixtures come from
//! `scripts/gen_kll_fixtures.py` over synthetic inputs; `EXTRACT_*` and `EXTRACT_POINT_*` were
//! computed by BigQuery from the stored bytes.

mod common;

use common::{b64_decode, rows};
use zetasketch::kll::{KllSketch, KllValue};

const FIXTURES: &str = include_str!("data/kll_golden.tsv");

/// Known differences, all at k = 10 (the smallest precision) with n >= 10,000 (heavy
/// compaction, item weights up to 2048): BigQuery picks the neighbouring stored item. The
/// effective target is sometimes slightly above and sometimes slightly below `j/q x total
/// weight`, so it is not a constant offset; see
/// docs/findings/20261029_kll_quantiles_feasibility_spike.md. 9 of 1,638 checked outputs.
/// Each entry is (fixture name, what differs). Any other mismatch fails the test.
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[
    ("seq_k10_n10000", "q=100 at [81]"),
    ("seq_k10_n10000", "point phi=0.999"),
    ("seq_k10_n100000", "q=100 at [89]"),
    ("seq_k10_n100000", "point phi=0.999"),
    ("dup_k10_n10000", "q=100 at [58]"),
    ("dup_k10_n100000", "q=100 at [62]"),
    ("big_k10_n10000", "q=100 at [67]"),
    ("big_k10_n100000", "q=100 at [87]"),
    ("f64tiny_k10_n100000", "q=100 at [99]"),
];

fn parse_value(ty: &str, s: &str) -> KllValue {
    if ty == "INT64" {
        KllValue::Int64(s.parse().unwrap())
    } else {
        KllValue::Float64(s.parse().unwrap())
    }
}

#[test]
fn extract_and_point_match_bigquery() {
    let mut mismatches: Vec<(String, String)> = Vec::new();
    let mut checks = 0;
    for r in rows(FIXTURES).filter(|r| r[0] == "KLL") {
        let (name, ty) = (r[1], r[2]);
        let sketch =
            KllSketch::from_bytes(&b64_decode(r[3])).unwrap_or_else(|e| panic!("{name}: {e}"));
        for (col, q) in [(4, 2), (5, 4), (6, 10), (7, 100)] {
            let want: Vec<KllValue> = r[col].split(',').map(|v| parse_value(ty, v)).collect();
            let got = sketch.quantiles(q).unwrap();
            checks += 1;
            if got != want {
                let at: Vec<usize> = (0..want.len()).filter(|&i| got[i] != want[i]).collect();
                mismatches.push((name.to_string(), format!("q={q} at {at:?}")));
            }
        }
        for pair in r[8].split(',') {
            let (phi, want) = pair.split_once('=').unwrap();
            let got = sketch.point(phi.parse().unwrap()).unwrap();
            checks += 1;
            if got != parse_value(ty, want) {
                mismatches.push((
                    name.to_string(),
                    format!("point phi={phi}: got {got:?}, want {want}"),
                ));
            }
        }
    }
    for m in &mismatches {
        println!("MISMATCH {} {}", m.0, m.1);
    }
    println!(
        "{} of {checks} BigQuery outputs match",
        checks - mismatches.len()
    );
    let unexpected: Vec<_> = mismatches
        .iter()
        .filter(|(n, what)| {
            !KNOWN_DIFFERENCES
                .iter()
                .any(|(kn, kw)| kn == n && what.starts_with(kw))
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "unexpected mismatches: {unexpected:?}"
    );
}

#[test]
fn rejects_hll_bytes_and_garbage() {
    let hll = zetasketch::HyperLogLogPlusPlus::for_longs(15)
        .unwrap()
        .serialize();
    assert!(KllSketch::from_bytes(&hll).is_err());
    assert!(KllSketch::from_bytes(&[0x08]).is_err());
    assert!(KllSketch::from_bytes(&[]).is_err());
}
