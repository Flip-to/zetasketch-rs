---
status: open
area: merge
impact: low
effort: s
bean: zetasketch-rs-0003
---

# `Accumulator` sparse merges make three passes over the union

## What

`Accumulator::merge_sparse` makes three passes over the union of the stored and incoming sparse
values: merge the two sorted lists, dedupe, then count the encoded bytes to check the
normalization threshold.

## Why it exists

It keeps the code a close mirror of Java (`MergedIntIterator`, then `dedupe`, then
`updateRepresentation`), which made byte parity easy to review.

## Consequence

Sparse + sparse folds do about 3x the necessary passes until the union turns dense. Not measured
in isolation. In `cargo bench --bench fold` most inputs meet an already dense accumulator, so it
does not dominate there.

## Fix

One fused loop: merge, dedupe and count the varint bytes in a single pass. Guarded by
`tests/accumulator.rs` and the `differential_fold` fuzz target.

## Workaround meanwhile

None needed. Correctness is unaffected.
