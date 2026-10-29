# Changelog

All notable changes to this crate. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[SemVer](https://semver.org/) (pre-1.0: a minor bump may break the API).

## [Unreleased]

## [0.2.3] - 2026-10-29

### Performance

- Small sparse sketches (a few values, under 128 bytes) fold about 2x faster. A compact proto
  parse handles the layout writers produce (one-byte tags, values and lengths) and returns to the
  general parser for anything else, so every error and every unusual input is unchanged. An
  `Accumulator` that is already dense also folds a sparse input of at most 16 values without
  touching the scratch buffers. On `examples/single_value_fold.rs` (synthetic single-value
  sketches into a dense accumulator): about 79 to 41 ns per `add_bytes`, 75 to 33 ns per
  `add_trusted_bytes`; into a sparse accumulator, 79 to 51 ns.

## [0.2.2] - 2026-10-29

### Performance

- Folds of mid-size sparse sketches (hundreds to thousands of values) that keep the union
  sparse near the sparse -> dense threshold: in-place insertion is now limited to 16 values
  (inserting hundreds cost ~50 us per sketch from moving memory), and deferred values plus the
  current input are merged into the union in one pass after a run-aware sort. On
  `examples/midsize_probe.rs`, inputs into a sparse accumulator go from 54 to 36 us.

### Added

- CI differential oracle against Java ZetaSketch (`oracle/Oracle.java`,
  `examples/oracle_driver.rs`): random INT64 and STRING scenarios, adds interleaved with
  merges, byte-identical output required.
- **Experimental** `kll` module: decodes BigQuery `KLL_QUANTILES` sketches (INT64, FLOAT64)
  and extracts quantiles and points. 1,629 of 1,638 BigQuery outputs match on 126 synthetic
  fixtures (`tests/kll_golden.rs`, `scripts/gen_kll_fixtures.py`); the 9 known differences are
  all at k = 10 with n >= 10,000. No merge (BigQuery's KLL is randomized). The API may change.
- `Accumulator::merge(&other)`: combine accumulators that folded different chunks, for example on
  different threads. Exactly `add_bytes(&other.serialize())` without the round trip; equal to
  the reference merge of the partial results (how BigQuery combines partial aggregates).
  `examples/parallel_fold.rs`: 8,000 mixed sketches, 69.6 ms on 1 thread to 7.4 ms on 32
  threads (9.4x), same estimate.
- `docs/guides/real_data_validation.md` and `examples/validate_groups.rs`: check `Accumulator`
  against BigQuery `HLL_COUNT.MERGE` / `MERGE_PARTIAL` on your own exported groups, privately.
  CI runs it on a synthetic self-check and on the committed BigQuery merge fixtures.

## [0.2.1] - 2026-10-29

### Added

- `Accumulator::add_trusted_bytes`: folds bytes that were validated earlier, skipping the
  per-value range scans. Same result on valid input. On corrupt input it may error, give a wrong
  result or panic; never undefined behavior. About 37% faster on dense p = 15 inputs (1.20 to
  0.76 us); no measurable change on tiny sparse inputs.

### Performance

- Same-precision sparse merges are deferred while the sparse -> dense threshold provably can't
  be reached, then merged once. `cardinality`/`estimate` count distinct sparse indices without
  sorting. The proof is in `docs/decisions/20261029_deferred_sparse_merges.md`. On a grouped
  request (`examples/group_by.rs`: 1,000 groups x 17 metrics x 30 small sketches) this goes from
  655 to about 210 ms (3.1x); sparse-heavy fold bench about 0.2–0.4 us per sketch, from 1.7.
- Faster parsing and validation: 1-byte fast path in the proto varint reader (parse and header
  checks 144 to 52 ns for a 95-byte sketch), and range checks of the validated sparse decoder
  done once at the end of the stream instead of per value (30 values 230 to 60 ns). A valid
  stream is non-decreasing and plain values sort below rho-encoded ones, so the checks are
  monotone. A stream whose deltas wrap past 2^32 is now rejected (it was accepted as an unsorted
  stream). Local Windows: grouped request about 192 ms. On the Linux CI runner (AMD EPYC 7763)
  the end-to-end effect is small: tiny-sketch fold 420 to 409 ns, grouped request 229 to 227 ms.
- Deferred sparse values are capped at 2,048 per accumulator (8 KiB): memory stays bounded when a
  request holds thousands of accumulators. Flushing earlier is exact.
- Accumulator scratch buffers are lent from a per-thread pool, so idle accumulators hold no
  scratch memory. `size_of::<Accumulator>()` went from 296 to 192 bytes.

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

- `bench.yml`: fold benchmarks and microbenchmarks on a Linux x86-64 runner, posted to the
  job summary (informational, never fails).

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
