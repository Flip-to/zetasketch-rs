# Validating zetasketch on your own data

This repository's fixtures are synthetic by design (`DECISION(synthetic_fixtures_only)`): they
prove parity with Java ZetaSketch and BigQuery over a wide grid of sizes, precisions, value
types and merge shapes. Real data has size and sparsity distributions a grid only approximates,
so before relying on this crate in production, run two checks **in your own private
environment**. Nothing below ever needs to be committed here.

## 1. Parity on real sketches

Goal: for real groupings, `Accumulator` must give exactly BigQuery's `HLL_COUNT.MERGE` estimate
and `HLL_COUNT.MERGE_PARTIAL` bytes.

### Export from BigQuery

Replace `my_table`, `group_key` (the dimensions you group by in practice, concatenated if
needed) and `sketch` (a column filled by `HLL_COUNT.INIT`). Sample groups, not rows, so each
group is complete; adjust the sampling rate to get a few thousand groups.

```sql
-- rows.tsv: one stored sketch per line, group_key <TAB> base64 sketch
WITH sampled AS (
  SELECT DISTINCT group_key FROM my_table
  WHERE MOD(ABS(FARM_FINGERPRINT(CAST(group_key AS STRING))), 1000) = 0
)
SELECT CAST(t.group_key AS STRING) AS group_key, TO_BASE64(t.sketch) AS sketch
FROM my_table t JOIN sampled USING (group_key)
WHERE t.sketch IS NOT NULL;

-- expected.tsv: BigQuery's result per group
WITH sampled AS (... same as above ...)
SELECT CAST(t.group_key AS STRING) AS group_key,
       TO_BASE64(HLL_COUNT.MERGE_PARTIAL(t.sketch)) AS merged,
       HLL_COUNT.MERGE(t.sketch) AS estimate
FROM my_table t JOIN sampled USING (group_key)
GROUP BY 1;
```

Export both results as headerless tab-separated files (for example with
`bq query --format=csv` and converting commas, or the client library).

### Run the checker

```bash
cargo run --release --example validate_groups -- rows.tsv expected.tsv
cargo run --release --example validate_groups -- rows.tsv expected.tsv --trusted
```

It folds each group's sketches with `Accumulator`, prints every mismatch, and exits non-zero if
there is any. The `--trusted` run checks `add_trusted_bytes` on the same data. Expected result:
`N groups match, 0 mismatches`. Merge order does not matter: HLL++ merges are order-independent
for the final state BigQuery returns.

If anything mismatches, please open an issue with the **sketch bytes only** if your data
policy allows sharing them, or with the group's row count, precisions and whether the inputs
were sparse or dense.

## 2. Real-workload performance

Measure on the target hardware with your sketch mix rather than relying on this repository's
numbers (one development machine plus a shared CI runner, see `bench.yml`):

- `validate_groups` prints folded sketches per second for your real groups.
- `examples/group_by.rs` is a template for request-shaped workloads (many groups x metrics);
  replace its synthetic pool with your sketches to see per-request cost.
- Compare `add_bytes` with `add_trusted_bytes` (only for bytes your own pipeline validated) and
  `cardinality` against `estimate` for your read pattern.

Record the numbers with the hardware, the sketch size distribution (sparse vs dense, values per
sketch) and the crate version, so later regressions are visible.
