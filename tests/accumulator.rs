//! `Accumulator` must equal the reference fold (`from_bytes` + `merge_bytes`) byte for byte:
//! on the golden merges, on randomized folds around the sparse -> dense crossover, across
//! precision changes (fallback path), and after `reset`.

mod common;

use common::{b64_decode, b64_encode, rows};
use zetasketch::{Accumulator, HyperLogLogPlusPlus, ValueKind};

const CORPUS: &str = include_str!("data/golden_corpus.tsv");
const BIGQUERY: &str = include_str!("data/bigquery_golden.tsv");

/// Reference fold: the first input decoded, the rest merged in order.
fn reference_fold(inputs: &[Vec<u8>]) -> (Vec<u8>, i64, i64) {
    // Empty inputs (NULL sketches) are skipped, as merge_bytes and HLL_COUNT.MERGE do.
    let mut it = inputs.iter().filter(|b| !b.is_empty());
    let mut acc = HyperLogLogPlusPlus::from_bytes(it.next().unwrap()).unwrap();
    for b in it {
        acc.merge_bytes(b).unwrap();
    }
    // Estimate before serialize: serialize() compacts a buffered sparse sketch, which can move
    // the estimate from linear counting to the dense estimator (reference behavior).
    let card = acc.cardinality();
    let est = acc.estimate();
    assert_eq!(card, est, "reference cardinality() != estimate()");
    (acc.serialize(), est, acc.num_values())
}

fn fast_fold(acc: &mut Accumulator, inputs: &[Vec<u8>]) -> (Vec<u8>, i64, i64) {
    acc.reset();
    for b in inputs {
        acc.add_bytes(b).unwrap();
    }
    let card = acc.cardinality();
    let est = acc.estimate();
    assert_eq!(card, est, "accumulator cardinality() != estimate()");
    let result = (acc.serialize().unwrap(), est, acc.num_values());

    // Trusted mode must give the same result on valid input.
    let mut trusted = Accumulator::new();
    for b in inputs {
        trusted.add_trusted_bytes(b).unwrap();
    }
    let trusted_est = trusted.estimate();
    assert_eq!(
        (
            trusted.serialize().unwrap(),
            trusted_est,
            trusted.num_values()
        ),
        result,
        "add_trusted_bytes differs from add_bytes"
    );
    result
}

#[test]
fn golden_merges() {
    let mut acc = Accumulator::new();
    let mut n = 0;
    for r in rows(CORPUS).filter(|r| r[0] == "MERGE") {
        let inputs = vec![b64_decode(r[2]), b64_decode(r[3])];
        let (bytes, est, _) = fast_fold(&mut acc, &inputs);
        assert_eq!(b64_encode(&bytes), r[4], "{}: bytes", r[1]);
        assert_eq!(est, r[5].parse::<i64>().unwrap(), "{}: estimate", r[1]);
        n += 1;
    }
    for r in rows(BIGQUERY).filter(|r| r[0] == "MERGE") {
        let inputs: Vec<Vec<u8>> = r[2..r.len() - 2].iter().map(|s| b64_decode(s)).collect();
        let (bytes, est, _) = fast_fold(&mut acc, &inputs);
        assert_eq!(b64_encode(&bytes), r[r.len() - 2], "{}: bytes", r[1]);
        assert_eq!(
            est,
            r[r.len() - 1].parse::<i64>().unwrap(),
            "{}: estimate",
            r[1]
        );
        n += 1;
    }
    assert!(n >= 40, "only {n} merges");
}

#[test]
fn empty_and_single() {
    let mut acc = Accumulator::new();
    assert!(acc.is_empty());
    assert_eq!(acc.estimate(), 0);
    assert!(acc.serialize().is_none());
    acc.add_bytes(&[]).unwrap();
    assert!(acc.is_empty());

    for r in rows(BIGQUERY).filter(|r| r[0] == "INIT") {
        acc.reset();
        acc.add_bytes(&b64_decode(r[2])).unwrap();
        assert_eq!(b64_encode(&acc.serialize().unwrap()), r[2], "{}", r[1]);
        assert_eq!(acc.estimate(), r[3].parse::<i64>().unwrap(), "{}", r[1]);
    }
}

/// xorshift64*: deterministic, dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// Log-uniform size in `0..max`, so small, crossover and dense sizes all show up.
    fn size(&mut self, max: u64) -> u64 {
        let bits = 64 - max.leading_zeros() as u64;
        let b = self.below(bits + 1);
        self.below(1 << b).min(max)
    }
}

fn random_sketch(rng: &mut Rng, p: i32, max_size: u64) -> Vec<u8> {
    let mut s = HyperLogLogPlusPlus::for_longs(p).unwrap();
    let n = rng.size(max_size);
    let start = rng.below(50_000) as i64;
    for v in start..start + n as i64 {
        s.add_long(v).unwrap();
    }
    s.serialize()
}

#[test]
fn randomized_same_precision() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut acc = Accumulator::new();
    for round in 0..300 {
        let p = [10, 12, 15][round % 3];
        let k = 1 + rng.below(12) as usize;
        let inputs: Vec<Vec<u8>> = (0..k).map(|_| random_sketch(&mut rng, p, 12_000)).collect();
        assert_eq!(
            fast_fold(&mut acc, &inputs),
            reference_fold(&inputs),
            "round {round} p={p} k={k}"
        );
    }
}

/// `cardinality(&self)` equals `estimate(&mut self)` at every step of interleaved adds, including
/// while values sit unflushed in the sparse buffer.
#[test]
fn cardinality_matches_estimate_during_adds() {
    let mut rng = Rng(0x0dd_ba11);
    for p in [10, 15] {
        let mut s = HyperLogLogPlusPlus::for_longs(p).unwrap();
        for step in 0..3000 {
            s.add_long(rng.below(20_000) as i64).unwrap();
            if step % 37 == 0 {
                let card = s.cardinality();
                assert_eq!(card, s.estimate(), "p={p} step={step}");
            }
        }
    }
}

#[test]
fn many_tiny_sketches_fold_exactly() {
    // Exercises the in-place insertion path: thousands of 1..12-value sketches, so most merges
    // insert a few values into a large sparse union, and the fold crosses into dense.
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let mut acc = Accumulator::new();
    for p in [10, 12, 15] {
        let inputs: Vec<Vec<u8>> = (0..1500)
            .map(|_| {
                let mut s = HyperLogLogPlusPlus::for_longs(p).unwrap();
                let start = rng.below(200_000) as i64;
                for v in start..start + 1 + rng.below(12) as i64 {
                    s.add_long(v).unwrap();
                }
                s.serialize()
            })
            .collect();
        for k in [10, 100, 400, 1500] {
            assert_eq!(
                fast_fold(&mut acc, &inputs[..k]),
                reference_fold(&inputs[..k]),
                "p={p} k={k}"
            );
        }
    }
}

#[test]
fn randomized_mixed_precision_falls_back_exactly() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let mut acc = Accumulator::new();
    for round in 0..150 {
        let k = 2 + rng.below(6) as usize;
        let inputs: Vec<Vec<u8>> = (0..k)
            .map(|_| {
                let p = [10, 11, 13, 15][rng.below(4) as usize];
                random_sketch(&mut rng, p, 8_000)
            })
            .collect();
        assert_eq!(
            fast_fold(&mut acc, &inputs),
            reference_fold(&inputs),
            "round {round} k={k}"
        );
    }
}

#[test]
fn incompatible_type_is_rejected_without_change() {
    let mut longs = HyperLogLogPlusPlus::for_longs(15).unwrap();
    longs.add_long(1).unwrap();
    let mut strings = HyperLogLogPlusPlus::for_strings(15).unwrap();
    strings.add_str("a").unwrap();
    let mut acc = Accumulator::new();
    acc.add_bytes(&longs.serialize()).unwrap();
    let before = acc.serialize();
    assert!(acc.add_bytes(&strings.serialize()).is_err());
    assert_eq!(acc.serialize(), before);
}

/// Builds `count` in-memory INT64 sketches; `flush` decides whether each is serialized first
/// (which compacts its add-buffer) or left with unflushed adds.
fn in_memory_sketches(rng: &mut Rng, count: usize, flush: bool) -> Vec<HyperLogLogPlusPlus> {
    (0..count)
        .map(|_| {
            let p = [10, 12, 15][rng.below(3) as usize];
            let mut s = HyperLogLogPlusPlus::for_longs(p).unwrap();
            let start = rng.below(40_000) as i64;
            for v in start..start + rng.size(6_000) as i64 {
                s.add_long(v).unwrap();
            }
            if flush {
                let _ = s.serialize();
            }
            s
        })
        .collect()
}

/// `add_sketch` equals the reference fold (`merge_ref`) on in-memory sketches, with and without
/// unflushed add-buffers, across precisions (fast path, buffered fallback, mixed-precision
/// fallback).
#[test]
fn add_sketch_matches_reference_merge() {
    let mut rng = Rng(0xadd5_ce7c);
    let mut acc = Accumulator::new();
    for round in 0..120 {
        let flush = round % 2 == 0;
        let k = 1 + rng.below(8) as usize;
        let sketches = in_memory_sketches(&mut rng, k, flush);

        let mut reference = sketches[0].clone();
        for s in &sketches[1..] {
            reference.merge_ref(s).unwrap();
        }
        acc.reset();
        for s in &sketches {
            acc.add_sketch(s).unwrap();
        }
        let card = acc.cardinality();
        assert_eq!(card, reference.cardinality(), "round {round} flush={flush}");
        assert_eq!(
            acc.estimate(),
            reference.estimate(),
            "round {round} flush={flush}"
        );
        assert_eq!(acc.num_values(), reference.num_values(), "round {round}");
        assert_eq!(
            acc.serialize().unwrap(),
            reference.serialize(),
            "round {round} flush={flush}"
        );
    }
}

#[test]
fn merge_ref_leaves_other_unchanged_and_equals_merge() {
    let mut rng = Rng(0x0e1f);
    for _ in 0..50 {
        let s = in_memory_sketches(&mut rng, 2, false);
        let other_before = s[1].clone().serialize();
        let mut by_ref = s[0].clone();
        by_ref.merge_ref(&s[1]).unwrap();
        let mut by_value = s[0].clone();
        by_value.merge(s[1].clone()).unwrap();
        assert_eq!(by_ref.serialize(), by_value.serialize());
        assert_eq!(s[1].clone().serialize(), other_before);
    }
}

#[test]
fn value_kind_standard_error_and_precisions() {
    let mut longs = HyperLogLogPlusPlus::for_longs(15).unwrap();
    assert_eq!(longs.value_kind(), Some(ValueKind::Long));
    assert!((longs.standard_error() - 1.04 / 181.019_335_983_756_2).abs() < 1e-12);
    assert_eq!(
        HyperLogLogPlusPlus::for_strings(12).unwrap().value_kind(),
        Some(ValueKind::BytesOrString)
    );
    assert_eq!(
        HyperLogLogPlusPlus::for_integers(10).unwrap().value_kind(),
        Some(ValueKind::Integer)
    );

    let mut acc = Accumulator::new();
    assert_eq!(acc.precisions(), None);
    longs.add_long(1).unwrap();
    acc.add_bytes(&longs.serialize()).unwrap();
    assert_eq!(acc.precisions(), Some((15, 20)));
}

/// Mixed-precision folds that reach every new path: a dense accumulator meeting lower precisions
/// (registers lowered) and higher ones (input downgraded), sparse accumulators changing precision
/// (reference merge, then resume on the fast path), precision chains, and dense, sparse and
/// tiny inputs in random order. Bytes, estimate, cardinality and count must equal the reference.
#[test]
fn randomized_mixed_precision_dense_paths() {
    let mut rng = Rng(0x00c0_ffee_d00d_f00d);
    let mut acc = Accumulator::new();
    for round in 0..400 {
        let k = 2 + rng.below(12) as usize;
        let inputs: Vec<Vec<u8>> = (0..k)
            .map(|_| {
                let p = [10, 11, 12, 13, 14, 15, 16][rng.below(7) as usize];
                let mut s = HyperLogLogPlusPlus::for_longs(p).unwrap();
                let n = match rng.below(3) {
                    0 => rng.below(20),
                    1 => rng.size(4_000),
                    _ => 5_000 + rng.below(40_000),
                };
                let start = rng.below(100_000) as i64;
                for v in start..start + n as i64 {
                    s.add_long(v).unwrap();
                }
                s.serialize()
            })
            .collect();
        assert_eq!(
            fast_fold(&mut acc, &inputs),
            reference_fold(&inputs),
            "round {round} k={k}"
        );
    }
}

/// Reads while merges are deferred (`cardinality`, `serialize`, `to_sketch`, `num_values`) must
/// equal the reference after every single input, not only at the end. That covers deferral
/// through the sparse -> dense crossover and back-to-back reads.
#[test]
fn reads_during_deferred_merges_match_reference_at_every_step() {
    let mut rng = Rng(0xdefe_77ed);
    for p in [10, 12, 15] {
        let inputs: Vec<Vec<u8>> = (0..400)
            .map(|_| {
                let mut s = HyperLogLogPlusPlus::for_longs(p).unwrap();
                let start = rng.below(1_000_000) as i64;
                for v in start..start + 1 + rng.below(40) as i64 {
                    s.add_long(v).unwrap();
                }
                s.serialize()
            })
            .collect();
        let mut reference = HyperLogLogPlusPlus::from_bytes(&inputs[0]).unwrap();
        let mut acc = Accumulator::new();
        acc.add_bytes(&inputs[0]).unwrap();
        for (i, b) in inputs.iter().enumerate().skip(1) {
            reference.merge_bytes(b).unwrap();
            acc.add_bytes(b).unwrap();
            assert_eq!(acc.cardinality(), reference.cardinality(), "p={p} step={i}");
            assert_eq!(acc.num_values(), reference.num_values(), "p={p} step={i}");
            if i % 7 == 0 {
                assert_eq!(
                    acc.serialize().unwrap(),
                    reference.clone().serialize(),
                    "p={p} step={i}"
                );
                let mut snap = acc.to_sketch().unwrap();
                assert_eq!(
                    snap.serialize(),
                    reference.clone().serialize(),
                    "p={p} step={i}"
                );
            }
        }
        assert_eq!(acc.estimate(), reference.estimate(), "p={p} final");
    }
}

/// Long same-precision folds of mid-size sparse sketches cross the pending cap many times before
/// the sparse -> dense threshold. Every step must still match the reference.
#[test]
fn pending_cap_flushes_stay_exact() {
    let mut rng = Rng(0x00ca_9f15);
    let inputs: Vec<Vec<u8>> = (0..300)
        .map(|_| {
            let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
            let start = rng.below(10_000_000) as i64;
            for v in start..start + 20 + rng.below(30) as i64 {
                s.add_long(v).unwrap();
            }
            s.serialize()
        })
        .collect();
    let mut reference = HyperLogLogPlusPlus::from_bytes(&inputs[0]).unwrap();
    let mut acc = Accumulator::new();
    acc.add_bytes(&inputs[0]).unwrap();
    for (i, b) in inputs.iter().enumerate().skip(1) {
        reference.merge_bytes(b).unwrap();
        acc.add_bytes(b).unwrap();
        assert_eq!(acc.cardinality(), reference.cardinality(), "step {i}");
    }
    assert_eq!(acc.serialize().unwrap(), reference.serialize());
}
