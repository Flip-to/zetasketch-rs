# KLL_QUANTILES feasibility spike: decode and EXTRACT reproducible, bytes are not

Roadmap item 3 (`docs/specs/20261028_roadmap_future_improvements.md`). Every observation below
comes from BigQuery queries over synthetic inputs (`GENERATE_ARRAY`), location EU, 2026-10-29.

## Answer

| Question | Result | Evidence |
|---|---|---|
| Can the sketch bytes be decoded? | **Yes.** The same envelope as HLL++, plus a KLL message with levelled, delta-varint compactors | Decoded below |
| Is `KLL_QUANTILES.INIT_*` deterministic? | **No.** The same values give different bytes in different query executions, and a reversed input order changes the bytes too | Fingerprints below |
| Is `MERGE_PARTIAL` deterministic? | **No.** It differs between executions and between merge orders | Fingerprints below |
| Can `EXTRACT_*` be reproduced exactly from given bytes? | **Very likely** (one sample so far) | Decile rule below |

**Consequence:** byte parity with BigQuery KLL is impossible. BigQuery seeds the random compaction
per execution. That is a property of BigQuery, not a gap we can close.

The achievable, testable targets are:

1. exact `EXTRACT_*` / `EXTRACT_POINT_*` on BigQuery-built sketches;
2. merges that are correct KLL (the right levels and weights, and rank error inside KLL's bound),
   checked statistically, never byte for byte.

## Evidence

### Non-determinism

The same query text was run twice. The second run was BigQuery's result cache (identical text),
so it proves nothing about determinism. A third run with changed query text forced a recompute:

| Run | `INIT_INT64(x, 100)` over 1..5000 | `MERGE_PARTIAL` of 10 parts of 5,000 |
|---|---|---|
| 1 | `5613902131970102396` | `-759819338104105681` |
| 2 (cached) | `5613902131970102396` | `-759819338104105681` |
| 3 (recomputed) | `5433856301696102595` | `-6808296185414764637` |
| 3, parts merged in reverse order | n/a | `-952492236108539234` |
| 3, input values in reverse order (5000..1) | `1279285663288723770` | n/a |

The numbers are `FARM_FINGERPRINT` of the sketch bytes.

### Format

`KLL_QUANTILES.INIT_INT64(x, 20)` over 1..300, decoded:

```
AggregatorStateProto
  1 type          = 113        (KLL; HLL++ is 112)
  2 num_values    = 300
  4 value_type    = 4          (INT64; HLL_COUNT uses 8 = UINT64 for INT64 input)
  113 (KLL state, length-delimited):
    1 = 54          unknown. 54 at k=20, 280 at k=100, 2954 at k=1000: not k, not item count (93)
    2 = 20          precision argument k (default 1000)
    3 = bytes 01    min, varint (1)
    4 = bytes ac02  max, varint (300)
    5 = compactor, level 0: field 2 = delta varints ->   5 items (296..300), weight 1
    5 = compactor, level 1: field 2 = delta varints ->  26 items (244..294), weight 2
    5 = compactor, level 2: field 2 = delta varints ->  62 items (2..242),   weight 4
```

The total item weight is 5 + 2·26 + 4·62 = 305. The true count is 300, so the weights are KLL's
approximation. Not yet checked: how negative INT64s are encoded in min/max and the deltas
(zigzag?), the FLOAT64 layout, and what field 1 is.

### EXTRACT rule (one sample)

BigQuery's `EXTRACT_INT64(sketch, 10)` on that sketch returned
`[1, 30, 61, 89, 119, 151, 179, 210, 238, 270, 300]`. The rule below reproduces it exactly:

> Endpoints are the stored min and max. The j-th inner quantile is the smallest stored item whose
> cumulative weight (items sorted by value) is at least j / q x the total item weight.

Ranking against the true count (300) instead of the total item weight (305) gives a different
answer for 7 of the 9 inner deciles. The strict `>` variant differs at one decile.

## Follow-up: 84 sketches, 1,092 BigQuery outputs (2026-10-29)

`scripts/research/kll_probe.py` built `INIT_INT64` / `INIT_FLOAT64` sketches. The grid:

- families: sequential, negative (`-x*7919-1`), heavy duplicates (`MOD(x,17)-8`), FLOAT64
  (`x*0.5-100.25`);
- k in {10, 100, 1000};
- n in {1, 2, 10, 100, 1,000, 10,000, 100,000}.

For each it kept `EXTRACT_*` for q in {2, 4, 10, 100} and `EXTRACT_POINT_*` for 9 values of phi
from 0 to 1.

**Encodings confirmed:**

- INT64 min, max and the first delta of a compactor are protobuf `int64` (two's-complement
  varint, 10 bytes when negative, not zigzag). Later deltas are positive varints in compactor
  field 2.
- FLOAT64 sketches have value_type = 10, min and max as raw little-endian doubles, and items as
  packed doubles in compactor field 1.
- Compactors are positional: an empty compactor still occupies its level, so weights are 2^index.
- Field 1 is 26, 54, 280 and 2954 for k = 10, 20, 100 and 1000, close to 3k. Probably a derived
  capacity; EXTRACT doesn't need it.

**EXTRACT rule:** 335 of 336 quantile vectors and 753 of 756 point lookups match exactly (1,088
of 1,092), using the rule above (target j/q x W or phi x W, first item with cumulative weight at
least the target, stored min and max at the ends). The 4 misses were re-run against the exact
stored bytes (`FROM_BASE64`) and BigQuery returned the same values, so they are real
differences, not artifacts of re-running a randomized `INIT`.

All 4 misses are at k = 10 with n of at least 10,000 (heavy compaction): three extreme points
(phi = 0.001 and 0.999) and one q = 100 vector. Variants tried, none better:

| Variant | Score |
|---|---|
| strict `>` | 854/1092 |
| midpoint (cumulative minus w/2) | 675/1092 |
| target from the true count n | 968/1092 |
| floor of the target | 226/336 quantile vectors, 630/756 points |
| ceiling of the target | same as exact, 753/756 points |

The needed target shifts down for small phi and up for large phi. That suggests BigQuery treats
the tails differently (stored min and max as weighted anchors? interpolation?). Unresolved; a
KLL implementation must settle it before claiming exact `EXTRACT`.

Fixture generation for an implementation must compute `EXTRACT_*` from the stored bytes
(`FROM_BASE64`), not from the same CTE. BigQuery does not promise to evaluate a CTE once, and
`INIT` is randomized.

## Implementation result (2026-10-29)

The experimental `kll` module implements the decode and the rule above. A new fixture set
(`scripts/gen_kll_fixtures.py`, 126 sketches) computes EXTRACT from the stored bytes in a
separate query. On it, 1,629 of 1,638 outputs match.

The 9 differences are all at k = 10 with n of at least 10,000, and include some inner
percentiles, not only the tails (heavy-duplicate data at the 58th and 62nd percentile). The
needed effective target is sometimes slightly above `j/q x W` and sometimes slightly below, so it
is not a constant offset, a different total weight, the true count n, or an inclusive/exclusive
switch. Unresolved; listed exactly in `tests/kll_golden.rs` so new differences fail the test.

## Recommendation

KLL is feasible as **decode + exact EXTRACT + statistically correct merge**. Before building it,
in this order:

1. Fixtures: `INIT_INT64` and `INIT_FLOAT64` over synthetic inputs, covering several k (10, 100,
   1000 default), n from 1 to 10^6, negatives, duplicates and extremes, each with its
   `EXTRACT_*(q)` for several q and `EXTRACT_POINT_*` for several phi. Store the bytes with the
   expected outputs; they are stable once stored, even though `INIT` itself is not.
2. Confirm the EXTRACT rule (including `EXTRACT_POINT` and FLOAT64) on all fixtures, and work out
   field 1 and the negative-number encoding.
3. Implement decode and extract; `EXTRACT` parity is the hard gate. Merges are tested with rank
   error bounds against exact quantiles of the known synthetic inputs.

Note: BigQuery documents KLL functions as a Preview in its own blog. Whether they are GA now has
not been checked (`docs.cloud.google.com/bigquery/docs/reference/standard-sql/kll_functions`).
