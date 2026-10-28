---
status: implemented
ticket: None
bean: zetasketch-rs-0001
prs: []
---

# HyperLogLog++ port with Java and BigQuery byte parity

## Problem

A Rust service that serves distinct counts from BigQuery `HLL_COUNT.INIT` sketches must merge
them and estimate exactly as BigQuery does. Two Rust or C++ options existed when this was
written:

- `zetasketch-rs` (Codeberg): a port of Java 0.1.0, with one conformance test.
- `zetasketch-cpp`: says its sparse/dense logic was corrected function by function against Java.
  That suggests defects remain in the earlier Rust port on the merge paths.

Neither was checked against BigQuery itself. BigQuery runs Google's internal implementation.

## Decision

Write a new zero-dependency crate `zetasketch`. It is a function-by-function port of Java
ZetaSketch (`DECISION(faithful_port_of_java_reference)`):

- `proto.rs`: hand-written reader and writer for the two messages. Fields are omitted exactly
  where Java `State.writeTo` omits them.
- `encoding.rs`: `Encoding.Normal` / `Encoding.Sparse`, the difference/varint stream,
  `MergedIntIterator` and `dedupe`.
- `hll.rs`: one struct holding the state plus a `Normal | Sparse(buffer)` mode. It replaces the
  Java `Representation` classes. Each Java method that returns a new representation becomes a
  mode change, in the same order: buffer flush thresholds (`m * 0.25` values), normalization at
  `m * 0.75` sparse bytes, and downgrade on merge.
- `estimate.rs` + generated `bias_data.rs`: `Data.java`, including a stable kNN sort and Java
  `Math.round`.
- `hash.rs`: Guava `Fingerprint2011`, so sketches built in Rust match BigQuery.

One deliberate difference. Java's `NormalRepresentation` caches its encoding at construction.
After a precision-lowering merge of a sparse sketch into a dense one, the returned object keeps
the old precision. Later operations would then index out of bounds or read wrongly. Here the
encoding is always derived from the current state, so the code never reads stale precision. On
every reachable input that Java handles without error, the output is the same.

Proof is two golden suites. Both use synthetic data only (`DECISION(synthetic_fixtures_only)`).

## Alternatives

- **Depend on zetasketch-rs (Codeberg):** it has the merge-path concerns above and no BigQuery
  evidence.
- **Link zetasketch-cpp through `cxx`:** it needs a C++23/Bazel toolchain in every consumer's
  build, plus FFI. It remains the documented fallback if parity ever regressed.
- **Write a minimal decoder and merger without the add path:** the add path is cheap to port. It
  lets the BigQuery suite rebuild sketches from values, which tests the hash and the sparse
  buffering too.

## Rollout

1. Port, then golden corpus green (done, commit in bean `zetasketch-rs-0001`).
2. Generate BigQuery fixtures, then green (done).
3. Publish `Flip-to/zetasketch-rs` as a public repo, CI green, tag `v0.1.0`, publish to crates.io
   (crate name to be confirmed: is `zetasketch` free?).
4. Hot-path accumulator API for many-sketch merges (`docs/tech_debt/per_sketch_decode_allocation.md`).

## Verification

`cargo test --release` on 2026-10-28 with rustc 1.91.0:

- `tests/golden_corpus.rs`: 3/3 pass (84 SKETCH, 5 MERGE, 98 LONGS rows, byte-exact).
- `tests/bigquery_golden.rs`: 3/3 pass (66 INIT decode/estimate/bytes, 66 rebuilt from values,
  13 MERGE with `MERGE_PARTIAL` bytes and `MERGE` estimate).
- Doctest in `src/hll.rs` passes.
