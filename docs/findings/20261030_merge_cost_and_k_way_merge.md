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
argument for no measured gain.

## Where the time goes (instrumented, then fixed)

Timers around the steps of `merge` (temporary, not committed), 8 partials of one sketch each merged
into a fresh accumulator, ns per merge:

| Values per partial | Total | Merge-and-dedupe pass | Elements read per merge | ns per element | Sorting deferred values |
|---|---|---|---|---|---|
| 300 | 2,909 | 849 | 297 | 2.9 | 1,533 |
| 1,000 | 14,401 | 7,599 | 1,307 | 5.8 | 2,853 |
| 3,000 | 48,720 | 46,236 | 9,879 | 4.7 | none |

Nothing else shows up (cloning and merging the source's deferred values is 24 ns, densifying never
runs). The pass reads the whole growing union for every source, so per incoming value it reads
(union + incoming) / incoming elements: about 1.5 at 300 values and 3.3 at 3,000, at 3 to 6 ns an
element. That explains the rise from 7.8 to 16.2 ns per value, and why a pairwise one-pass merge did
not help: three pairwise passes read about as many elements as eight sequential merges. The lever is
the cost per element of the pass itself (4.7 ns is slow for a merge), or a loser-tree merge that
reads every value once.

The pass now merges the plain values (almost all of them: a rho-encoded value needs an index with
`sp - p` zero low bits) with a branch-free loop that drops repeats and counts the encoded length,
and keeps the old general loop for the values at or above the rho flag. Per merge of 8 synthetic
partials: 31.7 to 22.0 us (2.4 KB) and 104.5 to 59.4 us (7.4 KB); 5.5 us (0.7 KB) and the dense case
unchanged.

Not tried: a loser-tree k-way merge (reads each value once, about 1 element per incoming value
against 3.3), and keeping the sources' deferred values unsorted and sorting all of them together.
