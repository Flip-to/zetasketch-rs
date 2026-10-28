"""Offline tests for the BigQuery fixture generator (no BigQuery access needed).

The generator's value families must stay in sync with `family_value` in
`tests/bigquery_golden.rs`, which rebuilds every INIT row from the same values.
"""

from __future__ import annotations

import re
from pathlib import Path

import scripts.gen_bigquery_fixtures as gen

REPO = Path(__file__).resolve().parents[2]
RUST_TEST = (REPO / "tests" / "bigquery_golden.rs").read_text(encoding="utf-8")
FIXTURES = (REPO / "tests" / "data" / "bigquery_golden.tsv").read_text(encoding="utf-8")


def rust_families() -> set[str]:
    body = RUST_TEST[RUST_TEST.index("fn family_value") :]
    body = body[: body.index("\n}\n")]
    return set(re.findall(r'"([a-z0-9]+)"(?:\s*\|\s*"[a-z0-9]+")*\s*(?:if [^=]+)?=>', body)) | set(
        re.findall(r'\|\s*"([a-z0-9]+)"', body)
    )


def test_every_sql_family_is_rebuilt_in_rust() -> None:
    assert set(gen.FAMILIES) == rust_families()


def test_int64_edges_match_rust() -> None:
    m = re.search(r"const INT64_EDGES: \[i64; (\d+)\] = \[(.*?)\];", RUST_TEST, re.DOTALL)
    assert m, "INT64_EDGES not found in tests/bigquery_golden.rs"
    named = {"i64::MAX": 2**63 - 1, "i64::MIN": -(2**63)}
    tokens = [t.strip() for t in m.group(2).split(",") if t.strip()]
    rust = [named[t] if t in named else int(t.replace("_", "")) for t in tokens]
    assert int(m.group(1)) == len(gen.INT64_EDGES)
    assert rust == gen.INT64_EDGES


def test_init_cases_use_known_families_and_bigquery_precisions() -> None:
    for family, p, ns in gen.INIT_CASES:
        assert family in gen.FAMILIES
        assert 10 <= p <= 24, "HLL_COUNT.INIT accepts precision 10..24"
        assert all(n >= 1 for n in ns)


def test_merge_cases_are_well_formed() -> None:
    names = [name for name, _, _ in gen.MERGE_CASES]
    assert len(names) == len(set(names))
    for name, family, parts in gen.MERGE_CASES:
        assert family in gen.FAMILIES, name
        assert len(parts) >= 2, name
        for p, lo, hi in parts:
            assert 10 <= p <= 24 and lo <= hi, name


def test_merge_sql_renders_one_init_per_part_and_keeps_null_inputs() -> None:
    sql = gen.merge_sql("int64", [(15, 0, 10), (12, 5, 5), (10, 3, 8)])
    assert sql.count("HLL_COUNT.INIT(x, ") == 3
    assert "GENERATE_ARRAY(5, 4)" in sql, "lo == hi must produce an empty range (a NULL sketch)"
    assert "IFNULL(TO_BASE64(s), '')" in sql


def test_generator_reads_no_tables() -> None:
    sql = " ".join(
        [
            gen.INIT_SQL,
            gen.NESTED_SQL,
            *(gen.merge_sql(f, [(15, 0, 2), (15, 1, 3)]) for f in gen.FAMILIES),
        ]
    )
    # Synthetic-only rule: no FROM over a table, only UNNEST / CTEs.
    for m in re.finditer(r"\bFROM\s+([^\s(]+)", sql, re.IGNORECASE):
        assert m.group(1).upper().startswith(("UNNEST", "PARTS", "PARTIALS", "LEAVES", "V")), (
            m.group(0)
        )


def test_committed_fixtures_cover_every_init_case() -> None:
    names = {line.split("\t")[1] for line in FIXTURES.splitlines() if line.startswith("INIT\t")}
    for family, p, ns in gen.INIT_CASES:
        for n in ns:
            assert f"{family}_p{p}_n{n}" in names
    merges = {line.split("\t")[1] for line in FIXTURES.splitlines() if line.startswith("MERGE\t")}
    assert {name for name, _, _ in gen.MERGE_CASES} | {"int_nested_tree_p15"} == merges
