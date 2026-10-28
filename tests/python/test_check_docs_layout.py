from __future__ import annotations

from pathlib import Path

from scripts.check_docs_layout import markers, problems, write_indexes

REPO = Path(__file__).resolve().parents[2]


def test_repo_docs_layout_is_clean() -> None:
    write_indexes(REPO)
    found = problems(REPO)
    assert found == [], f"Docs layout problems found: {found}"


def test_tech_debt_markers_are_found_in_rust_sources(tmp_path: Path) -> None:
    src = tmp_path / "src"
    src.mkdir()
    (src / "lib.rs").write_text("// TECH_DEBT(some_slug): reason\nfn f() {}\n", encoding="utf-8")
    found = markers(tmp_path)
    assert found == {"some_slug": ["src/lib.rs:1"]}


def test_marker_without_debt_file_is_a_problem(tmp_path: Path) -> None:
    (tmp_path / "docs" / "tech_debt").mkdir(parents=True)
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "lib.rs").write_text("// TECH_DEBT(missing_item)\n", encoding="utf-8")
    write_indexes(tmp_path)
    assert any("TECH_DEBT(missing_item) has no" in p for p in problems(tmp_path))


def test_hyphenated_doc_name_is_a_problem(tmp_path: Path) -> None:
    (tmp_path / "docs" / "guides").mkdir(parents=True)
    (tmp_path / "docs" / "guides" / "bad-name.md").write_text("# x\n", encoding="utf-8")
    write_indexes(tmp_path)
    assert any("bad-name.md" in p for p in problems(tmp_path))
