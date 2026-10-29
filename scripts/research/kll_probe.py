"""KLL_QUANTILES research probe (synthetic inputs only; no table reads).

    uv run --group bigquery python scripts/research/kll_probe.py <billing-project>

Writes kll_probe.json next to this file: for every (family, k, n) the INIT bytes and BigQuery's
EXTRACT_* (q = 2, 4, 10, 100) and EXTRACT_POINT_* outputs. See
docs/findings/20261029_kll_quantiles_feasibility_spike.md.
"""

import json
import sys
from pathlib import Path

FAMILIES = {
    "seq": ("INT64", "x"),
    "neg": ("INT64", "-x * 7919 - 1"),
    "dup": ("INT64", "MOD(x, 17) - 8"),
    "f64": ("FLOAT64", "CAST(x AS FLOAT64) * 0.5 - 100.25"),
}
CASES = []
for fam in FAMILIES:
    for k in (10, 100, 1000):
        for n in (1, 2, 10, 100, 1000, 10000, 100000):
            CASES.append((fam, k, n))

PHIS = [0.0, 0.001, 0.1, 0.25, 0.5, 0.75, 0.9, 0.999, 1.0]


def query(fam, k, n):
    typ, expr = FAMILIES[fam]
    suffix = "INT64" if typ == "INT64" else "FLOAT64"
    points = ",\n".join(
        f"KLL_QUANTILES.EXTRACT_POINT_{suffix}(s, {p}) AS pt{i}" for i, p in enumerate(PHIS)
    )
    return f"""
WITH sk AS (
  SELECT KLL_QUANTILES.INIT_{suffix}({expr}, {k}) AS s
  FROM UNNEST(GENERATE_ARRAY(1, {n})) AS x
)
SELECT TO_BASE64(s) AS b64,
  KLL_QUANTILES.EXTRACT_{suffix}(s, 2) AS q2,
  KLL_QUANTILES.EXTRACT_{suffix}(s, 4) AS q4,
  KLL_QUANTILES.EXTRACT_{suffix}(s, 10) AS q10,
  KLL_QUANTILES.EXTRACT_{suffix}(s, 100) AS q100,
  {points}
FROM sk
"""


def main():
    from google.cloud import bigquery

    client = bigquery.Client(project=sys.argv[1])
    out = []
    for fam, k, n in CASES:
        row = next(iter(client.query(query(fam, k, n)).result()))
        out.append(
            {
                "family": fam,
                "k": k,
                "n": n,
                "b64": row.b64,
                "q": {str(q): list(row[f"q{q}"]) for q in (2, 4, 10, 100)},
                "points": {str(p): row[f"pt{i}"] for i, p in enumerate(PHIS)},
            }
        )
        print(fam, k, n, len(row.b64), flush=True)
    Path(__file__).with_name("kll_probe.json").write_text(json.dumps(out))


if __name__ == "__main__":
    main()
