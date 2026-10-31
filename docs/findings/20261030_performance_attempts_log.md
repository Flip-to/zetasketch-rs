# Performance attempts log: what was tried, what shipped, what was dropped

Dated 2026-10-30. One Windows machine, release builds; synthetic data only. This is the index of
every performance idea tried on the fold and merge paths, so none is tried twice. Details of each
live in the linked finding, bean or PR. Ratios matter, not absolute numbers. Add a row for every new
attempt, including the ones that fail.

## Shipped

| Idea | Result | Where |
| --- | --- | --- |
| Merge a sparse accumulator into a started one without encoding its values to varint bytes and decoding them again | 24 to 33% less per merge | PR #36, bean 0010 |
| Merge-and-dedupe pass: plain values (almost all) with a branch-free loop that also counts the encoded length; general loop only for rho-encoded values | 30 to 43% faster on sparse partials of 2.4 and 7.4 KB; 0.7 KB and dense unchanged | PR #38, bean 0012 |
| Header cache: small sparse sketches (1 to 8 values, at most 32 bytes) whose bytes before the stream equal a header folded before skip the parse; both `add_trusted_bytes` and `add_bytes` (with the value range check) | 1.5 to 1.9x on small-sketch folds | PR #37, bean 0011 |
| Fix for merging an oversized, never-merged sparse source (found in review of the direct merge) | correctness | PR #36 |

## Tried and dropped (do not retry as is)

| Idea | Why it failed | Evidence |
| --- | --- | --- |
| One-pass `merge_many` of all sources of a group (pairwise merges of the sorted unions, decided from the encoded length and the collapsed count) | No faster than 8 sequential merges: three pairwise passes read about as many elements as the sequential ones | `20261030_merge_cost_and_k_way_merge.md` |
| Branchless varint decoder (earlier) | The stream is predictable; no gain | earlier in this effort; not recorded elsewhere, measure again before relying on it |
| Exact lazy deferral with a settle step (B/D slack rule, replay) | No gain on the Parquet path; kept on branch `perf/sparse-phase` (bean 0009) for after an engine layout that stores sketches decoded | branch `perf/sparse-phase` |
| Galloping merge of a short run into a long union (binary search, block copy, adjust the encoded length by the gaps at the junctions) | Exact (differential test, mutation caught), but no measurable gain: 4.9 against 5.0 us per merge at an 11,000-value union, within run-to-run noise (4.5 to 5.4 us). Most of a merge of a short partial is outside the pass, and the long-union regime only exists just below the dense threshold | `20261030_merge_cost_and_k_way_merge.md` (PR #39) |
| Radix sort (3 passes of 11 bits) of the deferred values instead of the standard sort | 4.5 ns per value against 5.8 (stable) and 4.9 (unstable) at 2,048 values, and 9.8 against 4.3 and 3.5 at 256; the gain is at most 1.3x at the largest batch and a loss below it. Not worth the code | measurement below |

## Measured, not yet decided

- Deferred values are sorted with the stable `sort`; `sort_unstable` is about 15% faster per value
  (3.5 against 4.3 ns at 256 values, 4.9 against 5.8 at 2,048, runs of 1, 3 or 20 values alike, so
  the stable sort gains nothing from the runs). Equal `u32` values cannot be told apart, so the result
  is identical. A small change, to be measured in the fold before it is shipped.

## Not tried yet

- Loser-tree k-way merge to combine partials: reads each value once, about 1 element per incoming
  value against 3.3 at 3,000 values with sequential merges. The pairwise `merge_many` above does not
  count: it failed for reading as many elements, which a loser tree does not.
- Keeping the sources' deferred values unsorted and sorting all of them together.
- Header cache for sketches larger than 8 values or 32 bytes (event-grain data is mostly 1 to 3
  values, so expect little).

## Method of the sort measurement

A throwaway program (not committed): a batch of `n` values below 2^25 built as concatenated
sorted runs of the given length, sorted by each method, best of 200 runs, ns per value. Windows
machine, release build, 2026-10-30. To reproduce, write the radix as three counting passes of 11
bits with a scratch buffer and compare with `sort` and `sort_unstable`.
