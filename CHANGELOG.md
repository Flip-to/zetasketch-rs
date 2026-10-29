# Changelog

All notable changes to this crate. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[SemVer](https://semver.org/) (pre-1.0: a minor bump may break the API).

## [0.2.0] - 2026-10-29

### Changed (breaking)

- `Error` and `ValueKind` are `#[non_exhaustive]`, so future variants won't break `match`es.
  Add a `_ =>` arm.

### Added

- `HyperLogLogPlusPlus::cardinality(&self)` and `Accumulator::cardinality(&self)`: the current
  estimate through a shared reference, for cached and concurrently read results. The value equals
  `estimate()`.
- `Accumulator::add_sketch(&HyperLogLogPlusPlus)`: fold an in-memory sketch without serializing
  or copying it.
- `HyperLogLogPlusPlus::merge_ref(&other)`: merge without consuming `other`, like Java's `merge`.
- `HyperLogLogPlusPlus::value_kind()`, `standard_error()` (1.04 / sqrt(2^p)) and
  `Accumulator::precisions()`.

### Performance

- `Accumulator` folds of many small sparse sketches insert the new values in place instead of
  rewriting the union: 18.2 to 1.37 us per sketch on the sparse-heavy bench (31.6x faster than
  the reference fold, which before this was 2.6x).

- Bias correction no longer allocates (one heap allocation saved per dense estimate).
- `Accumulator` sparse + sparse merges run in one fused pass (merge, dedupe, byte count) instead
  of three.
- `Accumulator::serialize` writes straight from its buffers, with no 32 KiB register copy and no
  re-validation.

- Mixed-precision folds stay on the fast path: dense accumulators take any precision
  (registers lowered once, inputs downgraded with a per-block closed form). Sparse accumulators
  resume the fast path after one reference merge. Mixed-precision bench: 40.9 to 17.2 us per
  sketch (3.1x faster than the reference fold).

- Dense estimates (`estimate`, `cardinality`) sum a register histogram exactly instead of
  looping in f64: 19.9 to 8.0 us at p = 15. Results are bit-identical, with the proof in
  `docs/decisions/20261029_dense_estimate_exact_histogram.md`.

### Internal

- Ported Java ZetaSketch's own unit tests: 74 public-API tests in
  `tests/java_hyperloglogplusplus.rs` and 97 internal-module tests in `src/java_internal_tests.rs`.
  All Java expectations hold. Two tests are named `_differs_from_java` for setup reasons: Rust's
  sparse encoder only debug-asserts sorted input, and Rust cannot represent a normal sketch with
  no registers.

- Python tooling is a uv project (`pyproject.toml`, `uv.lock`) with pytest suites in
  `tests/python/`, including checks that the BigQuery generator's value families and edge values
  match the Rust rebuild test. Ruff lints and formats `scripts/` and `tests/python/`.

- CI checks `cargo doc` with warnings denied, `cargo package`, and that `src/bias_data.rs`
  regenerates byte for byte from Java ZetaSketch `Data.java` at the pinned commit `d098c49`.
- A nightly fuzz workflow (15 min per target) keeps its corpus between runs.

## [0.1.0] - 2026-10-28

First release:

- `HyperLogLogPlusPlus`: decode, merge, estimate, serialize and add, byte-compatible with Java
  ZetaSketch and BigQuery `HLL_COUNT`;
- `Accumulator` for fast folds;
- parity suites against the Java golden corpus and BigQuery fixtures built from synthetic inputs.
