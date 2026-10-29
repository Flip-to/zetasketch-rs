"""Convert tests/data/bigquery_golden.tsv MERGE rows into the two files validate_groups reads.

    python scripts/fixtures_to_group_tsv.py <out_dir>

Each MERGE row becomes one group: its inputs are the group's rows (NULL inputs skipped), and
BigQuery's MERGE_PARTIAL bytes and MERGE estimate are the expectation.
"""

import sys
from pathlib import Path

FIXTURES = Path(__file__).resolve().parent.parent / "tests" / "data" / "bigquery_golden.tsv"


def main() -> int:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    rows, expected = [], []
    for line in FIXTURES.read_text(encoding="utf-8").splitlines():
        f = line.split("\t")
        if f[0] != "MERGE":
            continue
        name, inputs, merged, est = f[1], f[2:-2], f[-2], f[-1]
        rows += [f"{name}\t{b}" for b in inputs if b]
        expected.append(f"{name}\t{merged}\t{est}")
    (out / "rows.tsv").write_text("\n".join(rows) + "\n", encoding="utf-8")
    (out / "expected.tsv").write_text("\n".join(expected) + "\n", encoding="utf-8")
    print(f"{len(expected)} groups, {len(rows)} rows")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
