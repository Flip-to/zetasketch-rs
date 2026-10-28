---
status: open
area: merge
impact: low
effort: m
bean: zetasketch-rs-0003
---

# Mixed-precision folds fall back to the reference implementation

## What

`Accumulator` switches to `HyperLogLogPlusPlus` (copies, a per-input decode into a `Vec`) for the
rest of a fold as soon as an input's precision differs from the first input's.

## Why it exists

Precision lowering in the reference is order-dependent: the buffered values, the normalization
threshold at each precision, and the downgrade chain all interact. The fallback keeps those folds
exact without porting that logic a second time.

## Consequence

Mixed-precision folds run at reference speed, about 22 us per sketch on the fold bench rather than
about 10. BigQuery's `HLL_COUNT.INIT` defaults to p = 15, so all-default data never takes this
path.

## Fix

A downgrading fast path: when a lower-precision input arrives, convert the accumulator's
registers or sparse values once (same rules as `normal_maybe_downgrade` and `sparse_downgrade`),
then continue on the fast path. Differential tests already exist
(`randomized_mixed_precision_falls_back_exactly`).

## Workaround meanwhile

Build all sketches at one precision.
