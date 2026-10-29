---
status: accepted
kind: developer
who: "Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive"
ticket: None
spec: docs/specs/20261028_hllpp_port_with_bigquery_parity.md
prs: []
---

# `Accumulator` defers same-precision sparse merges while the threshold provably can't be reached

## Context

Grouped report workloads fold thousands of small sketches into thousands of accumulators (one
per group and metric). Merging each input into the growing sorted union cost O(union) per
input, so O(rows^2 x values) per group. On `examples/group_by.rs` (1,000 groups x 17 metrics x
30 sketches of 1 to 60 values, p = 15, 2026-10-29) one "request" took 655 ms.

## Decision

Same-precision sparse inputs are appended to an unsorted `pending` list, without merging, while

    sparse_bytes + 5 * (pending + incoming) < 0.75 * 2^p

Otherwise pending values are merged once (sort plus one pass) before the exact per-input merge
continues. `cardinality`/`estimate` count distinct sparse indices with a per-thread bitset (no
sort). `serialize`/`to_sketch` merge on the fly. `densify` and fallback conversion flush first.

## Why

It is exact: the same bytes, estimate and count as the reference fold, at every step.

- The reference checks the normalization threshold after every merge.
- Adding one value grows the encoded union by at most 5 bytes, one varint. Inserting into a gap
  adds at most one varint. Replacing a rho-encoded value with a larger one in the same group
  changes a gap by less than 64, so at most 1 byte.
- So while the bound holds, no intermediate merge of the reference could reach the threshold,
  and the reference stays sparse throughout.
- A sparse union with max-rho dedupe is commutative and associative, so merging the deferred
  values later, in any order, gives the identical union.
- Dedupe collapses exactly the values that share a sparse index (plain values are their index;
  rho-encoded values decode to an index with zero low bits, so they never collide with plain
  ones). So the union's length equals the number of distinct indices, which `cardinality`
  counts without building the union.

Tests: every accumulator differential test, plus a test that compares `cardinality`,
`serialize`, `to_sketch` and `num_values` with the reference after every single input while
merges are deferred. A `debug_assert` checks the bound on every flush.

Result on `examples/group_by.rs`: 655 to about 210 ms per request (3.1x). Fold bench,
sparse-heavy workload: 1.7 us to about 0.2–0.4 us per sketch.

## Alternatives

- Insert into the sorted union per input: O(n) per input from moving memory. That was the
  previous fast path for small inputs.
- Java-style unsorted buffer with flushes at `m/4` values: its threshold timing is not exact
  against per-merge checks without the 5-byte bound argument, and it still sorts at every flush.

## Impact

If the sparse value encoding ever allowed a value to add more than 5 bytes, the bound would
have to change. The bound is a named constant in `merge_sparse` with this reasoning next to it.

## Who

Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive ("do the
ones that don't need personal data", 2026-10-29).
