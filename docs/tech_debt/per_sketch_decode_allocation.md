---
status: open
area: merge
impact: medium
effort: m
bean: zetasketch-rs-0001
---

# Every merge decodes and allocates the other sketch

## What

`merge_bytes` builds a full `HyperLogLogPlusPlus` per input: it copies the data and sparse
vectors and decodes the whole sparse stream into a `Vec<u32>`. Sparse+sparse merges re-encode
the union each time.

## Why it exists

The first goal was exact parity, so the code mirrors the Java object model.

## Consequence

Folding thousands of stored sketches into one result allocates and re-encodes per sketch. That
cost has not been measured yet. What would measure it: a criterion bench that folds N stored
p = 15 sketches.

## Fix

Add an accumulator API that keeps the faithful path as the oracle:

- zero-copy parse;
- a reusable dense register array (32 KiB at p = 15);
- a sorted-vector sparse fast path while the union stays small;
- register max, with SIMD if a bench shows it pays.

Differential tests and fuzzing against `HyperLogLogPlusPlus` must show the same estimate and
bytes.

## Workaround meanwhile

Use `merge` on already-decoded sketches and reuse one accumulator per request.
