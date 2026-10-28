---
status: draft
ticket: None
bean: None
prs: []
---

# Roadmap: future improvements

## Problem

`zetasketch` 0.1.0 covers BigQuery's native HLL++ (`HLL_COUNT.*`) with byte parity. Users want
everything BigQuery's sketch features cover, plus the confidence of Java ZetaSketch's own test
suite. This spec lists what is still missing, grouped by effort and risk. Every claim below was
checked on 2026-10-28; sources are inline.

What BigQuery supports, checked with queries in the EU location on 2026-10-28:

| Family | Functions | Format | Status |
|---|---|---|---|
| HLL++ (native) | `HLL_COUNT.INIT/MERGE/MERGE_PARTIAL/EXTRACT` | ZetaSketch `AggregatorStateProto`, type 112 | Done (0.1.0) |
| KLL quantiles (native) | `KLL_QUANTILES.INIT_INT64/INIT_FLOAT64`, `MERGE_*`, `EXTRACT_*`, `EXTRACT_POINT_*` | Same proto envelope, type 113 (`KLL_QUANTILES.INIT_INT64` output starts `08 71`). No `INIT_UINT64`/`INIT_DOUBLE` (BigQuery returns "Function not found") | Missing |
| Apache DataSketches (`bqutil.datasketches_<region>` UDFs) | Theta, Tuple, CPC, HLL, KLL, REQ, frequent items, t-digest | Apache DataSketches binary (for example, compact theta preamble `01 04 03`) | Out of scope |
| Per-query approximations | `APPROX_QUANTILES`, `APPROX_TOP_COUNT/SUM`, `APPROX_COUNT_DISTINCT` | No stored sketch | Out of scope |

References:

- [KLL quantile functions](https://docs.cloud.google.com/bigquery/docs/reference/standard-sql/kll_functions)
- [Sketches in BigQuery](https://docs.cloud.google.com/bigquery/docs/sketches)
- [BigQuery supports Apache DataSketches](https://cloud.google.com/blog/products/data-analytics/bigquery-supports-apache-datasketches-for-approximate-analytics/)

## Decision

Planned work, in priority order. Each item becomes its own bean and PR.

### 1. Port Java ZetaSketch's own unit tests (done, bean `zetasketch-rs-0004`)

`google/zetasketch` `javatests/` (HEAD `d098c49`, 2026-04-17) has 16 files and about 4,500 lines;
none are ported yet. The golden-vector suites prove the normal paths byte for byte, but not
Java's error behavior or internal edge cases. Port:

- `HyperLogLogPlusPlusTest` (1,455 lines): rejected inputs, type narrowing after the first add,
  null and empty merges, `noSparseMode`, precision boundaries 4 and 24, `numValues` after mixed
  merges;
- `StateTest` (519 lines): which fields are written, defaults, unknown fields;
- `SparseRepresentationTest`, `NormalRepresentationTest`, `EncodingSparseTest`,
  `EncodingNormalTest`, `DataTest`, `HashTest`, `DifferenceEncoderTest`,
  `DifferenceDecoderTest`, `MergedIntIteratorTest`.

Skip `ByteSliceTest` and `GrowingByteSliceTest`: they test Java buffer classes this crate does not
have. Skip the Truth `testing/` subjects: they are Java assertion helpers.

Each Java assertion that this crate deliberately breaks becomes a named test that documents the
difference: the buffered-downgrade fix, and the stricter bounds checks at decode.

### 2. API conveniences (done, bean `zetasketch-rs-0004`, except the `Aggregator` trait)

| Java | Today | Add |
|---|---|---|
| `merge(other)` leaves `other` unchanged | `merge(other)` consumes it | `merge_ref(&other)` |
| `forProto` / `serializeToProto` expose the proto | bytes only | Read-only accessors: value type, encoding version, whether data / sparse data is present (no proto dependency) |
| `Aggregator<V, R, A>` interface | none | An `Aggregator` trait, only once a second sketch type exists |
| `UniqueStatsProto.expected_error` | none | `expected_error()` ≈ 1.04 / sqrt(2^p) |

### 3. Native KLL quantiles (`KLL_QUANTILES`), spike first (spike done)

Spike result: `docs/findings/20261029_kll_quantiles_feasibility_spike.md`. BigQuery KLL bytes
are **not deterministic**, because `INIT` and `MERGE_PARTIAL` differ between executions, so byte
parity is impossible. The sketch format decodes cleanly, and `EXTRACT` appears exactly
reproducible from the stored bytes. The plan narrows to decode + exact `EXTRACT` + statistically
correct merges.


KLL answers a different question from HLL++:

| | HLL++ (`HLL_COUNT`) | KLL (`KLL_QUANTILES`) |
|---|---|---|
| Question | How many distinct values? | Which value sits at rank X (median, p95, quartiles)? |
| Input | Any hashable value; duplicates do not count | Ordered numbers (INT64, FLOAT64); duplicates count |
| State | Per register, the max leading-zero count of hashed values; values are discarded | Samples of real values in levels ("compactors"); an item at level h weighs 2^h |
| Shrinking | Fixed size: 2^p registers, with sparse mode while small | A full level is sorted and half of it moves up a level at double weight. Which half is chosen at random |
| Merge | Register-wise max: exact, deterministic, order-independent | Concatenate levels and compact again: merge order and the random seed change the bytes |
| Output | One cardinality | Any quantile from one sketch |
| Error | Relative error ≈ 1.04 / sqrt(m), about 0.6% at p = 15 | Rank error ≈ 1/k: a "median" may be the 49.5th to 50.5th percentile |
| Size | Fixed by precision | Grows like O(k log(n/k)) |

Why it is harder than HLL++:

1. **No open reference.** Google's KLL behind `KLL_QUANTILES` is not in the ZetaSketch repo, so the
   type-113 proto has to be decoded from BigQuery output alone (synthetic fixtures only,
   `DECISION(synthetic_fixtures_only)`).
2. **Randomness.** Byte-exact `MERGE_PARTIAL` would require reproducing BigQuery's random number
   generator and seeding, which may not be observable from outside. `EXTRACT` on sketches BigQuery
   built, with no merging, is a realistic exact target. After merges, the honest target is "within
   KLL's rank-error bound".
3. **Different test oracle.** Tests check "within epsilon of the true rank", not "same number",
   except where determinism is proven.

Plan: a time-boxed spike that (a) decodes the proto fields, (b) matches `EXTRACT_*` and
`EXTRACT_POINT_*` exactly on single sketches, and (c) measures whether merges reproduce
`MERGE_PARTIAL` bytes. Its result decides the scope before any further KLL work.

### 4. Faster sparse inputs in `Accumulator` (sparse-heavy folds done: 31.6x; mixed precision done: 3.1x)

`cargo bench --bench fold` (2026-10-28): sparse inputs cost about 10 ns per value, mostly varint
decoding and scattered register writes. Candidates: branch-light varint decoding, and fusing
validate and apply for dense targets. Only worth doing if a consumer benchmark shows sparse
inputs dominate.

### 5. Upstream the Java downgrade bug

File an issue on `google/zetasketch` with a failing JUnit test: on a p11, p13, p10 merge chain,
`SparseRepresentation.downgrade` re-adds buffered values without converting them. See
`docs/tech_debt/sparse_downgrade_buffer_encoding.md`. The repo is synced from Google's internal
code with Copybara and external PRs are rarely merged, so lead with the issue. It is unrelated to
open PR #18, which fixes the read position of the merge source.

### Out of scope

Apache DataSketches (Theta, Tuple, CPC, ...) is a separate format from ZetaSketch, and it already
has an official Rust port, [`apache/datasketches-rust`](https://github.com/apache/datasketches-rust).
It cannot read `HLL_COUNT` sketches, and `HLL_COUNT` sketches cannot be turned into theta
sketches.

`APPROX_QUANTILES`, `APPROX_TOP_COUNT/SUM` and `APPROX_COUNT_DISTINCT` compute over rows at query
time and store no sketch, so there is nothing to port. A consumer that serves them computes them
itself (for example with KLL or exactly) and cannot expect to match BigQuery's unspecified
internal results.

## Alternatives

- **Skip KLL entirely:** possible if no consumer needs stored quantile sketches. The spike is cheap
  and settles it.
- **Byte parity as the only KLL target:** likely unreachable after merges because of randomness.
  The spike measures this before anyone commits to it.

## Rollout

1. Port the Java unit tests and add the API conveniences (items 1 and 2), one PR.
2. KLL spike (item 3), reported as a `docs/findings/` file before any KLL API is published.
3. Items 4 and 5 when a consumer or a maintainer response justifies them.

## Verification

- Item 1: all ported Java tests pass, or are named and documented as deliberate differences.
  Result (2026-10-28): 171 ported, all pass, 0 ignored, 2 `_differs_from_java` (setup only).
- Item 2: doctests and unit tests for each new accessor.
- Item 3: the spike's finding shows exact `EXTRACT` on synthetic single sketches, and a measured
  answer on whether merges are byte-reproducible.
- Item 4: `cargo bench --bench fold` improves on sparse inputs with `tests/accumulator.rs` still green.
