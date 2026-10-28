---
status: paid
area: merge
impact: medium
effort: s
paid_by: Flip-to/zetasketch-rs#1
---

# Sparse downgrade re-added buffered values under the wrong encoding (Java bug, now fixed)

## What

Java `SparseRepresentation.downgrade(encoding)` converts the stored sparse stream to the lower
encoding. It then re-adds the unflushed buffer as if those values were already in the lower
encoding, but they are still in the old one. The first port mirrored this.

## Why it exists

`DECISION(faithful_port_of_java_reference)`: the first version copied reference quirks on
purpose.

## Consequence

It was first written off as unreachable for sketches decoded from bytes. That was wrong. A fold
of three sketches reaches it: low precision, then higher, then lower still (for example
p11, p13, p10). The second merge leaves downgraded values in the buffer, and the third merge
misreads them. The randomized mixed-precision test in `tests/accumulator.rs` found it as a panic
(index 1041 into 1024 registers) on 2026-10-28. Java would throw `ArrayIndexOutOfBoundsException`
at the same point, or write the wrong registers when the index happens to land in range.

## Fix

Re-add buffered values with `this.downgrade(v, enc)`, the same conversion the stored values get.
BigQuery agrees with the fixed behavior. Three new fixtures pass byte for byte against
`HLL_COUNT.MERGE_PARTIAL` and `HLL_COUNT.MERGE` (`tests/data/bigquery_golden.tsv`, generated
2026-10-28):

- `int_lowering_chain_p11_p13_p10`
- `int_lowering_chain_p12_p15_p10_dense`
- `int_lowering_chain_p13_p14_p11_p10`

This is a deliberate difference from Java, and the spec lists it.

## Workaround meanwhile

n/a: paid.
