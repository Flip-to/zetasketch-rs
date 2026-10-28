//! `Accumulator` must equal the reference fold (`from_bytes` + `merge_bytes`) byte for byte:
//! on the golden merges, on randomized folds around the sparse -> dense crossover, across
//! precision changes (fallback path), and after `reset`.

mod common;

use common::{b64_decode, b64_encode, rows};
use zetasketch::{Accumulator, HyperLogLogPlusPlus};

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
    let est = acc.estimate();
    (acc.serialize(), est, acc.num_values())
}

fn fast_fold(acc: &mut Accumulator, inputs: &[Vec<u8>]) -> (Vec<u8>, i64, i64) {
    acc.reset();
    for b in inputs {
        acc.add_bytes(b).unwrap();
    }
    let est = acc.estimate();
    (acc.serialize().unwrap(), est, acc.num_values())
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
