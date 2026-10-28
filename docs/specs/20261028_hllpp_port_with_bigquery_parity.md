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

**Fast fold.** `Accumulator` (`src/accumulator.rs`) folds serialized sketches with the same
result as the reference fold:

- parsing borrows the input, with no copy;
- the dense registers and the sparse buffers are reused;
- sparse input is decoded and validated in one pass, so a rejected input leaves the accumulator
  unchanged.

The fast path covers inputs that share the first input's precisions and have sparse mode
enabled. Anything else hands over to the reference implementation for the rest of the fold, so
the result stays exact.

Deliberate differences from Java, each only where Java throws or corrupts:

1. Java's `NormalRepresentation` caches its encoding at construction.
After a precision-lowering merge of a sparse sketch into a dense one, the returned object keeps
the old precision. Later operations would then index out of bounds or read wrongly. Here the
encoding is always derived from the current state, so the code never reads stale precision. On
every reachable input that Java handles without error, the output is the same.
2. When a sparse sketch is downgraded, buffered values are converted from their real encoding.
   Java misreads them; see `docs/tech_debt/sparse_downgrade_buffer_encoding.md`. BigQuery
   agrees with the fixed behavior: three lowering-chain fixtures pass.
3. Corrupt input is rejected at decode instead of failing later. This covers sparse values
   outside `2^sp` or `2^p`, `rhoW` too large for a register, and register bytes above `65 - p`.
   `tests/robustness.rs` runs 40,000 mutated or random inputs through every entry point with
   no panic.

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
4. Hot-path accumulator for many-sketch merges (done: `Accumulator`, bean `zetasketch-rs-0002`).

## Verification

`cargo test --release` on 2026-10-28 with rustc 1.91.0:

- `tests/golden_corpus.rs`: 3/3 pass (84 SKETCH, 5 MERGE, 98 LONGS rows, byte-exact).
- `tests/bigquery_golden.rs`: 3/3 pass (66 INIT decode/estimate/bytes, 66 rebuilt from values,
  13 MERGE with `MERGE_PARTIAL` bytes and `MERGE` estimate).
- `tests/bigquery_golden.rs` after adding the lowering-chain fixtures (2026-10-28): 69 INIT
  rows, 16 MERGE rows.
- Fixture coverage expansion (2026-10-28): 166 INIT rows and 35 MERGE rows, all pass on the
  first run with no code change. New cases:
  - every precision 10 to 24;
  - string and bytes lengths 0 to 199, covering every `Fingerprint2011` length branch;
  - unicode, raw bytes, negative and extreme INT64, NULL values;
  - crossover sweeps at p = 10 and 12;
  - 12 precision pairs, a nested `MERGE_PARTIAL` tree, a 100-way fold and NULL sketch inputs.
- `tests/accumulator.rs`: 5/5 pass. Byte, estimate and count equality with the reference fold
  on 21 golden merges, 300 randomized same-precision folds and 150 mixed-precision folds.
- `tests/robustness.rs`: 2/2 pass (40,000 corrupt inputs, no panic, debug and release).
- `cargo bench --bench fold`: the reference takes 22.0 us per sketch, the accumulator 10.4 us
  (2.1x).
- Doctests in `src/hll.rs` and `src/accumulator.rs` pass.
