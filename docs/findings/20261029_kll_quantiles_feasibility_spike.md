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
