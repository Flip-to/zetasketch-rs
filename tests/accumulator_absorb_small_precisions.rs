//! `Accumulator::absorb` against `merge` at small sparse precisions, where rho-encoded values are
//! common and the deferral bound is close to the threshold.
use zetasketch::{Accumulator, HyperLogLogPlusPlus, ValueKind};

fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut s = seed;
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    }
}

#[test]
fn absorb_fuzz_small_precisions() {
    let mut next = rng(0x1234_5678_9abc_def1);
    let mut compared = 0u64;
    for (p, sp) in [
        (10, 11),
        (10, 12),
        (10, 14),
        (11, 12),
        (12, 13),
        (15, 16),
        (10, 25),
    ] {
        for trial in 0..1500 {
            let parts = 2 + next() % 12;
            let mut merged = Accumulator::new();
            let mut absorbed = Accumulator::new();
            for step in 0..parts {
                let mut a = Accumulator::new();
                let sketches = next() % 30;
                let pool = [50u64, 3_000, 1_000_000][(next() % 3) as usize];
                for _ in 0..sketches {
                    let mut s = HyperLogLogPlusPlus::new(p, sp, ValueKind::Long).unwrap();
                    for _ in 0..=(next() % 5) {
                        s.add_long((next() % pool) as i64).unwrap();
                    }
                    a.add_bytes(&s.serialize()).unwrap();
                }
                if next() % 4 == 0 {
                    // Settle some partials first (an estimate flushes deferred values).
                    let _ = a.estimate();
                }
                let copy = a.clone();
                let r1 = merged.merge(&a);
                let r2 = absorbed.absorb(copy);
                assert_eq!(
                    r1.is_ok(),
                    r2.is_ok(),
                    "p {p} sp {sp} trial {trial} step {step}"
                );
                assert_eq!(
                    merged.serialize(),
                    absorbed.serialize(),
                    "p {p} sp {sp} trial {trial} step {step}"
                );
                assert_eq!(
                    merged.num_values(),
                    absorbed.num_values(),
                    "p {p} sp {sp} trial {trial} step {step}"
                );
                compared += 1;
            }
            assert_eq!(
                merged.estimate(),
                absorbed.estimate(),
                "p {p} sp {sp} trial {trial}"
            );
        }
    }
    assert!(compared > 40_000, "{compared}");
}
