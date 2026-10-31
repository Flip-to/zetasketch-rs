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
| Tighter deferral bound for merges: a merged source lengthens the union by at most its own encoded length (not 5 bytes per value), so the values of several sources wait for one sort and one merge pass; cap 16,384 values | 44% less per merge at 0.7 KB partials, 30% at 2.4 KB, 8% slower at 7.4 KB (partials that end up dense); needs the exact union length of the source, which `flushed_union_bytes` returns | PR #41, `merge_probe`; cap 2,048 gave nothing, 8,192 helped 0.7 KB but not 2.4 KB |
| Fix for merging an oversized, never-merged sparse source (found in review of the direct merge) | correctness | PR #36 |

## Tried and dropped (do not retry as is)

| Idea | Why it failed | Evidence |
| --- | --- | --- |
| One-pass `merge_many` of all sources of a group (pairwise merges of the sorted unions, decided from the encoded length and the collapsed count) | No faster than 8 sequential merges: three pairwise passes read about as many elements as the sequential ones | `20261030_merge_cost_and_k_way_merge.md` |
| Branchless varint decoder (earlier) | The stream is predictable; no gain | earlier in this effort; not recorded elsewhere, measure again before relying on it |
| Exact lazy deferral with a settle step (B/D slack rule, replay) | No gain on the Parquet path; kept on branch `perf/sparse-phase` (bean 0009) for after an engine layout that stores sketches decoded | branch `perf/sparse-phase` |
| Galloping merge of a short run into a long union (binary search, block copy, adjust the encoded length by the gaps at the junctions) | Exact (differential test, mutation caught), but no measurable gain: 4.9 against 5.0 us per merge at an 11,000-value union, within run-to-run noise (4.5 to 5.4 us). Most of a merge of a short partial is outside the pass, and the long-union regime only exists just below the dense threshold | `20261030_merge_cost_and_k_way_merge.md` (PR #39) |
| Loser-tree k-way merge of 4 to 16 sorted runs (dedupe and encoded length included) in place of sequential pairwise merges | Slower except at small runs: 7.9 against 9.9 ns per value for 8 runs of 300, but 17.5 against 10.1 for 8 of 1,000, 18.4 against 10.0 for 8 of 3,000, 22.7 against 19.3 for 16 of 1,000, 13.2 against 5.2 for 4 of 3,000. A data-dependent compare chain per output value loses to the branch-free two-way loop even at 3 to 4 elements read per value | throwaway benchmark, 2026-10-30 |
| Sorting all sources' values together (concatenate, one sort, dedupe) in place of a flush and merge per source | In isolation 1.3 to 2x faster (8 to 11 ns against 12 to 20 ns per value for 8 sources), but the library already defers and sorts the values of merged sources together; what stopped it was the deferral bound and the 2,048-value cap, fixed in PR #41 | throwaway benchmark; `merge_probe` |
| Header cache for larger sketches: up to 16 values and 64 bytes (64 slots) instead of 8 and 32 (16 slots) | 1 to 12 value sketches 70.2 to 68.8 ns (2%, `add_bytes`) and 62.4 to 60.5 (trusted); but the common cases got slower, 15 to 20%: single value 24.4 to 29.5, one to three values 32.2 to 36.1 (the cached header is copied out of its slot, and it is twice the size). Event-grain data is mostly one to three values | `examples/single_value_fold.rs` (new 1-12 value rows), min of 5 interleaved runs |
| Radix sort (3 passes of 11 bits) of the deferred values instead of the standard sort | 4.5 ns per value against 5.8 (stable) and 4.9 (unstable) at 2,048 values, and 9.8 against 4.3 and 3.5 at 256; the gain is at most 1.3x at the largest batch and a loss below it. Not worth the code | measurement below |
| `sort_unstable` instead of the stable `sort` for the deferred values (equal `u32` values are indistinguishable, so the output is identical) | Faster in isolation (about 15% per value) but the sort only runs when deferred values are flushed, rare in a fold. In `examples/single_value_fold.rs` the trusted rows moved 17.1 to 17.3 and 25.8 to 25.0 ns per sketch (noise level), while the validating rows moved 29.4 to 21.2 and 37.5 to 29.2 ns, which a sort swap cannot explain (those runs never reach the sort), so that part is code layout, not the change. Not shipped, no claim made | measurement, three interleaved runs each way |

## Not tried yet

- Making the cached header cheaper to read (borrow it instead of copying it out of the slot), which
  might let a larger cache pay for itself; the gain for 4 to 12 value sketches is bounded by the
  2% measured above, so probably not worth it.
- A native pre-decoded layout in the engine, after which the unmerged `perf/sparse-phase` branch
  pays off (see above).
