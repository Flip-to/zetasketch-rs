from __future__ import annotations

from pathlib import Path

import pytest

import scripts.gen_bias_data as gen

FAKE_DATA_JAVA = """
public final class Data {
  private static final double[][] meanData = {
    // precision 4
    {
      11, 11.717, 12.5, // trailing comment
      13,
    },
    {
      20, 21.25,
    }
  };

  private static final double[][] biasData = {
    {
      10, 9.717, -1e-3, 7,
    },
    {
      1.5, -2,
    }
  };
}
"""


def test_rows_parses_nested_arrays_and_strips_comments() -> None:
    assert gen.rows(FAKE_DATA_JAVA, "meanData") == [["11", "11.717", "12.5", "13"], ["20", "21.25"]]
    assert gen.rows(FAKE_DATA_JAVA, "biasData") == [["10", "9.717", "-1e-3", "7"], ["1.5", "-2"]]


@pytest.mark.parametrize(
    ("java", "rust"),
    [("11", "11.0"), ("-2", "-2.0"), ("11.717", "11.717"), ("-1e-3", "-1e-3"), ("2E5", "2E5")],
)
def test_literal_makes_every_value_an_f64(java: str, rust: str) -> None:
    assert gen.literal(java) == rust


def test_main_writes_rust_tables(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    java = tmp_path / "Data.java"
    java.write_text(FAKE_DATA_JAVA, encoding="utf-8")
    out = tmp_path / "bias_data.rs"
    monkeypatch.setattr(gen, "OUT", out)
    assert gen.main(["gen_bias_data.py", str(java)]) == 0
    text = out.read_text(encoding="utf-8")
    assert "pub(crate) static MEAN_DATA: [&[f64]; 2] = [" in text
    assert "pub(crate) static BIAS_DATA: [&[f64]; 2] = [" in text
    assert "11.0, 11.717, 12.5, 13.0," in text
    assert "// precision 5" in text


def test_committed_tables_have_one_row_per_precision_4_to_18() -> None:
    text = (Path(__file__).resolve().parents[2] / "src" / "bias_data.rs").read_text(
        encoding="utf-8"
    )
    assert "MEAN_DATA: [&[f64]; 15]" in text
    assert "BIAS_DATA: [&[f64]; 15]" in text
