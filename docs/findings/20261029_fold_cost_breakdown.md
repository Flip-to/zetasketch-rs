# Where `Accumulator` fold time goes, and a rejected branchless varint decoder

Measured 2026-10-29 on one Windows machine (rustc 1.91.0, release), with the microbenchmarks in
`examples/`. Absolute numbers depend on the machine; the ratios are what matter.

## Breakdown

| Input folded into a dense p = 15 accumulator | Cost | Example |
|---|---|---|
| Dense p = 15 sketch (32 KiB registers) | 1.4 to 3.1 us per input (about 0.04 ns per register) | `dense_into_dense`, `mixed_breakdown` |
| Sparse p = 15 sketch | 3.0 to 3.9 ns per stored value | `sparse_into_dense`, `mixed_breakdown` |

The `fold` bench's mixed workload has 1,656 sparse inputs averaging 3,149 values each and 344
dense inputs. Its about 10 us per sketch is therefore almost all sparse values (1,656 x 3,149 x
3 ns, versus 344 x 3 us). The cost per stored value is the number to improve, not a fixed cost
per input.

## Rejected: branchless word-at-a-time varint decoding

Idea: decode each sparse-stream varint without branching on its length. Load 8 bytes as a `u64`,
find the terminating byte with `trailing_zeros`, gather the 7-bit groups with shifts and masks.

Result: `sparse_into_dense` went from 3.86 to 5.11 ns per value, about 32% slower, so it was
reverted. Real sparse streams at p = 15 / sp = 20 have mostly 2-byte gaps in a predictable
pattern, so the byte loop's branches are well predicted. The extra `u64` load and gathering
cost more than the mispredictions they remove. A differential test showed the decoder was
correct, so the rejection is purely on speed.

## Mid-size sparse folds near the dense threshold (2026-10-29)

`examples/midsize_probe.rs`: groups of 1 to 40 sketches of up to 3,000 values at p = 15. While
the accumulator is still sparse but close to the threshold, an input can't be deferred, because
its 5-bytes-per-value bound reaches the threshold. Two costs dominated:

- in-place insertion of hundreds of values, each shifting up to ~30 KiB (about 50 us per sketch);
  now limited to inputs of 16 values or fewer;
- flushing the deferred values (a full sort) and then merging the input in a second pass; now
  one run-aware sort of pending plus input, then one merge pass.

Result: 54 to 36 us per input into a sparse accumulator (dense-phase inputs about 10 us, about
3 ns per value). What remains is one O(union) merge per input in this narrow regime. An
LSM-style second sorted run with incremental byte accounting could cut it further; not built,
given the complexity.

## Remaining ideas (not tried)

- Fusing validation and register writes for dense targets saves one pass over the decoded
  values. But it gives up "on error the accumulator is unchanged" unless the registers are
  copied first, so it's probably not worth it.
- A software prefetch of the target register line: the writes are scattered over 32 KiB, which
  mostly fits in L1 already, so the expected gain is small.
