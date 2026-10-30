# Merging sparse partials: where the time goes, and a rejected one-pass `merge_many`

Dated 2026-10-30. One Windows machine, release builds; synthetic sketches only
(`examples/merge_probe.rs`). Ratios matter, not absolute numbers. The "before" numbers are `main`
at `f7a9b59` (run `merge_probe` there to reproduce them); the one-pass `merge_many` prototype was
not committed (it merged the sorted unions pairwise, as described below).

## Question

A parallel fold ends by merging the per-thread accumulators of each group, one `merge` per thread.
Two ideas to make that cheaper: stop the sparse source going through varint bytes (done in this
change), and merge all sources of a group in one pass (`merge_many`) instead of one pass over the
growing union per source.

## Method

`merge_probe`: 8 partials, each folded from random sketches of 1 to 7 values drawn from a shared
pool of values (so they overlap), merged into one fresh accumulator; best of five rounds, per merge.
For the one-pass idea a prototype merged the sorted unions of the target and the sources pairwise
into one (3 passes for 8 sources), decided from the result's encoded length `B` and the number of
collapsed values `D` (`B >= T`: dense; `B + D < T`: sparse; otherwise merge one at a time, which
the prototype implemented and a differential test checked against the loop of `merge`, including
the undecided case).

## Results

Sequential `merge`, before and after this change (partials folded from many small sketches, so they
hold deferred values):

| Partial size | Before (us) | After (us) | Change |
|---|---|---|---|
| 0.7 KB sparse | 8.0 | 6.1 | -24 % |
| 2.4 KB sparse | 38.9 | 28.2 | -27 % |
| 7.4 KB sparse | 134.5 | 94.0 | -30 % |
| 22 KB (around the threshold) | 206.5 | 137.6 | -33 % |

One-pass prototype against the sequential `merge` after this change (same partials):

| Partial size | Sequential (us) | One pass (us) |
|---|---|---|
| 0.7 KB | 5.3 | 6.3 |
| 2.4 KB | 29.8 | 29.5 |
| 7.4 KB | 99.0 | 99.4 |
| 22 KB | 140.3 | 134.6 |

With partials of one sketch each (no deferred values), 300, 1,000 and 3,000 values per partial:
sequential 2.3, 12.8 and 48.5 us per partial against 2.3, 11.7 and 48.4 us for the one pass (7.8,
12.8 and 16.2 ns per value).

## Conclusion

The one-pass merge did not beat merging one at a time at any size tried (-9 % to +19 %), with or
without deferred values, so it was not shipped: it would add a public method and an exactness
argument for no measured gain. Sorting the deferred values of each source (done once per source
either way) and the per-value work of the union pass are a large part of a merge, not the number of
passes over the growing union. The per-value cost grows with partial size (7.8 to 16.2 ns), which
this experiment does not explain; a profile of a merge at 3,000 values is the next step if merges
matter again.

Not tried: a loser-tree k-way merge instead of pairwise passes (the pairwise version touches each
value about log2(k) times; a loser tree about once), and keeping the sources' deferred values
unsorted and sorting all of them together.
