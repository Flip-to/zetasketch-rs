---
status: paid
area: merge
impact: low
effort: m
paid_by: Flip-to/zetasketch-rs#PR
---

# Mixed-precision folds fell back to the reference implementation

## What

`Accumulator` switched to `HyperLogLogPlusPlus` (copies, a per-input decode into a `Vec`) for the
rest of a fold as soon as an input's precision differed from the first input's.

## Why it exists

Precision lowering in the reference is order-dependent: the buffered values, the normalization
threshold at each precision, and the downgrade chain all interact. The fallback kept those folds
exact without porting that logic a second time.

## Consequence

On `cargo bench --bench fold` (2026-10-29), the mixed-precision workload (1 sketch in 20 at
p = 12, the rest at p = 15) ran at reference speed: 40.9 us per sketch, 1.0x.

## Fix

Paid. Java's dense merge across precisions is deterministic: `maybeDowngrade`, then a max with
downgrading, and no buffer. Only the sparse merge across precisions buffers values one by one.
So:

- A dense accumulator takes inputs of any precision on the fast path. It lowers its registers
  once when a lower-precision input arrives, and downgrades higher-precision inputs on the fly.
- A sparse accumulator meeting a different precision runs that one merge on the reference
  implementation, then resumes on the fast path as soon as the reference state has no unflushed
  buffer (`HyperLogLogPlusPlus::resumable_state`).
- Register downgrading uses a closed form per block of `2^d` registers
  (`encoding::downgrade_registers_max_into`). Offset 0 maps to `r + d`, and a non-zero register at
  offset `o > 0` maps to `d - floor(log2 o)`. So a block needs only a first-non-zero scan, done 8
  bytes per `u64` word. It is checked against Java's per-register form for every precision pair
  from 4 to 16 at five densities.

Result on the same bench: 17.2 us per sketch, 3.1x faster than the reference fold. Exactness:
`randomized_mixed_precision_dense_paths` folds 400 random mixes of p = 10 to 16 (tiny, sparse and
dense inputs) and compares bytes, estimate, cardinality and count. The earlier mixed-precision
and `add_sketch` tests also pass.

## Workaround meanwhile

n/a: paid.
