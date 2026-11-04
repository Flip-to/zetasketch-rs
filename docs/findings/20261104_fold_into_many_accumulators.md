# Folding into thousands of live accumulators: where the nanoseconds go

Dated 2026-11-04. One Windows desktop with other jobs running at the same time, so absolute numbers
drift by up to 1.7x between runs; release build of 0.2.5; synthetic data only
(`examples/many_accumulators.rs`). Ratios inside one run matter, absolute numbers do not.

## Question

A day-grain request folds one sketch per metric per row into one accumulator per (day, metric): 365
days x 17 metrics = 6,205 live accumulators per thread, rows sorted by entity then day (69
entities), `add_bytes` per non-NULL sketch. A user of the crate measured a per-sketch CPU cost in
such a fold well above the 17 to 30 ns of `examples/single_value_fold.rs`. Is the gap the library?

## Method

`many_accumulators`: 69 x 365 x 17 sketches generated from three synthetic mixes (the real length
distribution of stored sketches was not available), packed into one buffer like a decoded column,
folded in the engine's order into 6,205 accumulators (one per day and metric), into 17 (one per
metric) and into 1; `add_bytes` and `add_trusted_bytes`; best of five passes per cell, then the
minimum of three runs. ns per non-NULL sketch.

A first version kept each sketch in its own heap allocation and measured 100 to 250 ns for
one-value sketches: the pointer chase (a cache miss per sketch) was the harness, not the library.
Packing the sketches into one buffer removed it.

## Results

| Mix | 6,205 accumulators (`add_bytes` / trusted) | 17 accumulators | 1 accumulator |
| --- | --- | --- | --- |
| tiny: 40% NULL, 55% one value, 5% two or three | 48.7 / 44.1 | 75.9 / 72.8 | 41.4 / 36.3 |
| mixed: 30% NULL, 40% one, 20% two to eight, 8% nine to sixty, 2% up to 600 | 167.7 / 188.7 | 186.1 / 180.0 | 121.4 / 117.0 |
| heavy: 10% NULL, 20% one, 30% two to eight, 30% nine to sixty, 10% up to 600 | 955.6 / 986.7 | 347.3 / 366.0 | 290.0 / 302.9 |

In the same session `single_value_fold` gave 47.5 ns for a one-value `add_bytes` into a dense
accumulator (37.3 trusted); on a quiet machine the same example gave 24 and 20.

## Reading

- One-value sketches cost 36 to 50 ns here, once the machine state is accounted for (the same
  example ran about 2x slower than on a quiet machine). The library is not far off the quiet-machine
  numbers; the gap to the 17 to 30 ns example is mostly machine state plus the mix of larger
  sketches, not a defect.
- Validation (`add_bytes` against `add_trusted_bytes`) is 4 to 8 ns for one-value sketches and
  within noise for larger ones.
- Cost follows the number of values in the sketches: the heavy mix averages about 45 values per
  sketch, so 290 ns is about 6 ns per value (varint decode and register update) even into a single
  accumulator.
- Spreading the same sketches over 6,205 accumulators costs up to 3.3x for the heavy mix (955
  against 290): none of them reaches the dense threshold (69 sketches each), so they stay sparse, and
  a sparse accumulator with many values pays for sorting and merging its deferred values, 2,048 at a
  time, which a dense one does not. For the tiny mix spreading was not slower (48.7 against 41.4):
  the accumulators stay small.
- Not measured: the real length distribution. With it, the same harness answers the question
  directly; the engine side can produce a histogram of sketch lengths without any data.

## Real length distribution (follow-up)

A length histogram of the stored sketches of one real month of the user's data (counts of byte
lengths only; not reproduced here) showed that nearly all non-NULL sketches are one value (about 22
bytes), with a small tail of larger ones and most cells NULL. That is the tiny mix: one-value
sketches at 36 to 50 ns here. For the request that motivated this, the library's share of the fold
is then a minor part of the CPU measured for it; the rest is in the code around the fold (key
hashing, per-cell null checks and value access, filtering), not in the sketch arithmetic. No
library change follows from this.

## Not tried

- The decoded sparse entry points (`add_sparse_batch`): they skip the protobuf parse and the
  per-sketch validation. The library supports them; the fold measured by the user does not use
  them.
