# /// script
# requires-python = ">=3.10"
# dependencies = ["google-cloud-bigquery>=3.20"]
# ///
# DECISION(synthetic_fixtures_only)
"""Generate tests/data/bigquery_golden.tsv from BigQuery HLL_COUNT over synthetic inputs.

Every input is generated inside the query (GENERATE_ARRAY, literals); no table is read, so the
fixtures contain no real data. Re-run to refresh after a BigQuery change:

    uv run scripts/gen_bigquery_fixtures.py --project <billing-project>

Rows (tab separated, base64 sketches):

    INIT   <family>_p<p>_n<n>  <b64 sketch>                      <HLL_COUNT.EXTRACT>
    MERGE  <name>  <b64 input>... <b64 MERGE_PARTIAL> <HLL_COUNT.MERGE>

An INIT row is HLL_COUNT.INIT(<family value of x>, p) over x in [0, n). `tests/bigquery_golden.rs`
rebuilds every row from the same values (FAMILIES below must stay in sync with
`family_value` there). In a MERGE row an empty input field is a NULL sketch.
"""

from __future__ import annotations

import argparse
from pathlib import Path

from google.cloud import bigquery

OUT = Path(__file__).resolve().parent.parent / "tests" / "data" / "bigquery_golden.tsv"

INT64_EDGES = [
    0, 1, -1, 2, -2, 127, 128, -128, 255, 256, 65535, 65536,
    2147483647, 2147483648, -2147483648, -2147483649, 4294967295, 4294967296,
    9223372036854775807, -9223372036854775808,
]

# family -> SQL expression of x. Each one exercises a different hash or type path.
FAMILIES = {
    "int64": "x",
    "int64neg": "-x * 7919 - 1",
    "int64edge": "[" + ", ".join(str(v) for v in INT64_EDGES) + "][OFFSET(x)]",
    "int64nulls": "IF(MOD(x, 3) = 0, NULL, x)",
    "string": "CONCAT('v', CAST(x AS STRING))",
    # Lengths 0..n-1 cover every Fingerprint2011 branch (<=32, 33-64, >64 bytes) and the empty string.
    "stringlen": "REPEAT('a', x)",
    "string41": "CONCAT('m', LPAD(CAST(x AS STRING), 40, '0'))",
    "string100": "CONCAT(REPEAT('L', 100), CAST(x AS STRING))",
    "unicode": "CONCAT('ü字\U0001f642', CAST(x AS STRING))",
    "bytes": "CAST(CONCAT('b', CAST(x AS STRING)) AS BYTES)",
    "bytesraw": "CONCAT(b'\\x00\\xff', CAST(CAST(x AS STRING) AS BYTES), b'\\x00')",
    "byteslen": "CAST(REPEAT('z', x) AS BYTES)",
}

CARDINALITIES = [1, 2, 10, 100, 1000, 3000, 5000, 6000, 20000, 100000, 1000000]

# (family, precision, cardinalities)
INIT_CASES: list[tuple[str, int, list[int]]] = [
    *[(f, p, CARDINALITIES) for f in ("int64", "string", "bytes") for p in (10, 15)],
    # Every precision BigQuery accepts, sparse; dense at 16.
    *[("int64", p, [100, 3000]) for p in range(11, 25) if p != 15],
    ("int64", 16, [100000]),
    # Fine sweeps across the sparse -> dense crossover (0.75 * 2^p sparse bytes).
    ("int64", 10, list(range(150, 451, 10))),
    ("int64", 12, list(range(900, 1501, 25))),
    # Value and type edge cases.
    ("int64neg", 15, [100, 10000]),
    ("int64edge", 15, [len(INT64_EDGES)]),
    ("int64edge", 10, [len(INT64_EDGES)]),
    ("int64nulls", 15, [100, 30000]),
    ("stringlen", 15, [200]),
    ("stringlen", 10, [200]),
    ("string41", 15, [100, 10000]),
    ("string100", 15, [100, 10000]),
    ("unicode", 15, [100, 10000]),
    ("bytesraw", 15, [100, 10000]),
    ("byteslen", 15, [200]),
]

# (name, family, [(precision, lo, hi)]) -> sketches over x in [lo, hi); lo == hi is a NULL sketch.
MERGE_CASES: list[tuple[str, str, list[tuple[int, int, int]]]] = [
    ("int_sparse_sparse_overlap_p15", "int64", [(15, 0, 100), (15, 50, 150)]),
    ("int_sparse_sparse_to_dense_p15", "int64", [(15, 0, 3000), (15, 3000, 6000)]),
    ("int_sparse_dense_p15", "int64", [(15, 0, 500), (15, 1000, 40000)]),
    ("int_dense_sparse_p15", "int64", [(15, 1000, 40000), (15, 0, 500)]),
    ("int_dense_dense_p15", "int64", [(15, 0, 50000), (15, 25000, 90000)]),
    ("int_lowering_sparse_p15_p10", "int64", [(15, 0, 200), (10, 100, 300)]),
    ("int_lowering_dense_p15_p10", "int64", [(15, 0, 50000), (10, 0, 20000)]),
    ("int_lowering_sparse_dense_p10_p15", "int64", [(10, 0, 100), (15, 0, 60000)]),
    ("int_ten_way_disjoint_p15", "int64", [(15, i * 700, (i + 1) * 700) for i in range(10)]),
    ("int_duplicates_p15", "int64", [(15, 0, 1000), (15, 0, 1000), (15, 0, 1000)]),
    ("int_lowering_chain_p11_p13_p10", "int64", [(11, 0, 300), (13, 200, 700), (10, 600, 900)]),
    ("int_lowering_chain_p12_p15_p10_dense", "int64", [(12, 0, 400), (15, 0, 30000), (10, 100, 500)]),
    ("int_lowering_chain_p13_p14_p11_p10", "int64", [(13, 0, 50), (14, 25, 400), (11, 300, 800), (10, 0, 90)]),
    ("string_sparse_sparse_overlap_p15", "string", [(15, 0, 100), (15, 50, 150)]),
    ("string_sparse_sparse_to_dense_p15", "string", [(15, 0, 3000), (15, 3000, 6000)]),
    ("string_sparse_dense_p15", "string", [(15, 0, 500), (15, 1000, 40000)]),
    ("string_dense_dense_p15", "string", [(15, 0, 50000), (15, 25000, 90000)]),
    ("bytes_sparse_dense_p15", "bytes", [(15, 0, 500), (15, 1000, 40000)]),
    ("unicode_sparse_sparse_p15", "unicode", [(15, 0, 300), (15, 200, 600)]),
    ("int_hundred_way_p15", "int64", [(15, i * 50, (i + 1) * 50) for i in range(100)]),
    ("int_crossover_by_merge_p12", "int64", [(12, i * 150, (i + 1) * 150) for i in range(10)]),
    ("int_null_inputs_p15", "int64", [(15, 0, 0), (15, 0, 100), (15, 5, 5), (15, 50, 300), (15, 9, 9)]),
    *[
        (f"int_pair_p{a}_p{b}_{kind}", "int64", [(a, 0, n), (b, n // 2, n + n // 2)])
        for a, b in [(10, 12), (12, 10), (14, 16), (16, 14), (10, 16), (16, 10)]
        for kind, n in (("sparse", 120), ("dense", 30000))
    ],
]

INIT_SQL = """
SELECT n, TO_BASE64(s) b64, HLL_COUNT.EXTRACT(s) est
FROM (
  SELECT n, HLL_COUNT.INIT({value}, {p}) s
  FROM UNNEST(@ns) n, UNNEST(GENERATE_ARRAY(0, n - 1)) x
  GROUP BY n
)
ORDER BY n
"""

# Two-level merge tree: 20 INIT sketches of 1,000 values, merged in groups of 5 with
# MERGE_PARTIAL; the 4 partials are the inputs, the merge of all of them the expectation.
NESTED_SQL = """
WITH v AS (SELECT x, DIV(x, 1000) g FROM UNNEST(GENERATE_ARRAY(0, 19999)) x),
leaves AS (SELECT g, HLL_COUNT.INIT(x, 15) s FROM v GROUP BY g),
partials AS (SELECT DIV(g, 5) h, HLL_COUNT.MERGE_PARTIAL(s) s FROM leaves GROUP BY h)
SELECT
  (SELECT ARRAY_AGG(TO_BASE64(s) ORDER BY h) FROM partials) inputs,
  (SELECT TO_BASE64(HLL_COUNT.MERGE_PARTIAL(s)) FROM partials) merged,
  (SELECT HLL_COUNT.MERGE(s) FROM partials) est
"""


def run(client: bigquery.Client, sql: str, params: list | None = None) -> list[bigquery.Row]:
    cfg = bigquery.QueryJobConfig(query_parameters=params or [])
    return list(client.query(sql, job_config=cfg).result())


def merge_sql(family: str, parts: list[tuple[int, int, int]]) -> str:
    value = FAMILIES[family]
    inits = " UNION ALL ".join(
        f"SELECT {i} i, (SELECT HLL_COUNT.INIT({value}, {p}) "
        f"FROM UNNEST(GENERATE_ARRAY({lo}, {hi - 1})) x) s"
        for i, (p, lo, hi) in enumerate(parts)
    )
    return f"""
WITH parts AS ({inits})
SELECT
  (SELECT ARRAY_AGG(IFNULL(TO_BASE64(s), '') ORDER BY i) FROM parts) inputs,
  (SELECT TO_BASE64(HLL_COUNT.MERGE_PARTIAL(s)) FROM parts) merged,
  (SELECT HLL_COUNT.MERGE(s) FROM parts) est
"""


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--project", required=True, help="billing project for the query jobs")
    args = ap.parse_args()
    client = bigquery.Client(project=args.project)

    lines = [
        "# Generated by scripts/gen_bigquery_fixtures.py from BigQuery HLL_COUNT over synthetic",
        "# inputs (GENERATE_ARRAY, literals). Do not edit by hand.",
    ]
    seen: set[str] = set()
    for family, p, ns in INIT_CASES:
        params = [bigquery.ArrayQueryParameter("ns", "INT64", ns)]
        for row in run(client, INIT_SQL.format(value=FAMILIES[family], p=p), params):
            name = f"{family}_p{p}_n{row.n}"
            if name not in seen:
                seen.add(name)
                lines.append(f"INIT\t{name}\t{row.b64}\t{row.est}")
    for name, family, parts in MERGE_CASES:
        row = run(client, merge_sql(family, parts))[0]
        lines.append("\t".join(["MERGE", name, *row.inputs, row.merged, str(row.est)]))
    row = run(client, NESTED_SQL)[0]
    lines.append("\t".join(["MERGE", "int_nested_tree_p15", *row.inputs, row.merged, str(row.est)]))

    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(lines) - 2} rows to {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
