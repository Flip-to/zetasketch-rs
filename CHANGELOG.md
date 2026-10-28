# Changelog

All notable changes to this crate. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[SemVer](https://semver.org/) (pre-1.0: a minor bump may break the API).

## [0.2.0] - unreleased

### Changed (breaking)

- `Error` and `ValueKind` are `#[non_exhaustive]`, so future variants won't break `match`es.
  Add a `_ =>` arm.

### Added

- `HyperLogLogPlusPlus::cardinality(&self)` and `Accumulator::cardinality(&self)`: the current
  estimate through a shared reference, for cached and concurrently read results. The value equals
  `estimate()`.

### Performance

- `Accumulator` folds of many small sparse sketches insert the new values in place instead of
  rewriting the union: 18.2 to 1.37 us per sketch on the sparse-heavy bench (31.6x faster than
  the reference fold, which before this was 2.6x).

- Bias correction no longer allocates (one heap allocation saved per dense estimate).
- `Accumulator` sparse + sparse merges run in one fused pass (merge, dedupe, byte count) instead
  of three.
- `Accumulator::serialize` writes straight from its buffers, with no 32 KiB register copy and no
  re-validation.

### Internal

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
