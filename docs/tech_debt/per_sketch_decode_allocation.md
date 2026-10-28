---
status: paid
area: merge
impact: medium
effort: m
paid_by: Flip-to/zetasketch-rs#PR
---

# Every merge decodes and allocates the other sketch

## What

`merge_bytes` builds a full `HyperLogLogPlusPlus` per input: it copies the data and sparse
vectors and decodes the whole sparse stream into a `Vec<u32>`. Sparse+sparse merges re-encode
the union each time.

## Why it exists

The first goal was exact parity, so the code mirrors the Java object model.

## Consequence

Measured with `cargo bench --bench fold` on 2026-10-28 (Windows, rustc 1.91.0, release). The
workload is 2,000 synthetic p = 15 sketches, 70% sparse and 30% dense. The reference fold takes
22.0 us per sketch.

## Fix

Paid by `Accumulator` (`src/accumulator.rs`):

- parsing borrows the input bytes, with no copy;
- the dense registers and the sparse buffers are reused across inputs and across `reset()`;
- sparse input is decoded and validated in one pass;
- the register max is a plain zip that the compiler vectorizes.

The same bench: 10.4 us per sketch (2.1x faster), with identical estimates. Dense inputs cost
about 1.8 us each.

Equality with the reference fold is enforced in `tests/accumulator.rs`:

- all golden and BigQuery merges;
- 300 randomized same-precision folds;
- 150 mixed-precision folds;

and in `fuzz/fuzz_targets/differential_fold.rs`.

## Workaround meanwhile

n/a: paid.
