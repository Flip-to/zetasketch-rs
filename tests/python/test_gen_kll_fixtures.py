"""Offline checks for the KLL fixture generator (no BigQuery access needed)."""

from __future__ import annotations

import re
from pathlib import Path

import scripts.gen_kll_fixtures as gen

REPO = Path(__file__).resolve().parents[2]
FIXTURES = (REPO / "tests" / "data" / "kll_golden.tsv").read_text(encoding="utf-8")


def test_families_are_synthetic_expressions_of_x() -> None:
    for typ, expr in gen.FAMILIES.values():
        assert typ in {"INT64", "FLOAT64"}
        # Only arithmetic over the generated x: no column or table references.
        assert re.fullmatch(r"[\sx0-9.e+*\-/()A-Z_,]+", expr), expr
        assert "FROM" not in expr.upper()


def test_int64_families_do_not_overflow() -> None:
    n = max(gen.NS)
    for typ, expr in gen.FAMILIES.values():
        if typ == "INT64":
            # Evaluate the expression at the largest x in Python integers.
            value = eval(expr.replace("MOD(x, 17)", "(x % 17)"), {"x": n})
            assert -(2**63) <= value < 2**63, expr


def test_committed_fixtures_cover_every_case() -> None:
    names = {line.split("\t")[1] for line in FIXTURES.splitlines() if line.startswith("KLL\t")}
    for fam in gen.FAMILIES:
        for k in gen.KS:
            for n in gen.NS:
                assert f"{fam}_k{k}_n{n}" in names
