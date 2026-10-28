# `zetasketch-rs` — AI Agent Instructions (`AGENTS.md`)

All coding agents (Claude Code, Antigravity, Cursor, Copilot, Codex) working in this repository
must follow the rules in:

1. **[`CLAUDE.md`](CLAUDE.md)**: structure, commands, and the byte-parity and synthetic-fixture invariants.
2. **[`docs/conventions/docs_and_specs.md`](docs/conventions/docs_and_specs.md)**: layout and frontmatter for `docs/specs/`, `docs/decisions/`, `docs/tech_debt/` and `.beans/`.
3. **[`.claude/rules/evidence.md`](.claude/rules/evidence.md)**: every claim carries its evidence or is phrased as a question.
4. **[`.claude/rules/a2v-track.md`](.claude/rules/a2v-track.md)**: every bean carries an `## A2V track`.
5. **[`.claude/rules/beans-tracking.md`](.claude/rules/beans-tracking.md)**: bean lifecycle mirrors PR state.
6. **[`.claude/rules/docs-and-specs.md`](.claude/rules/docs-and-specs.md)**: spec, `DECISION(<slug>)` and `TECH_DEBT(<slug>)` markers.

## Required Checks Before Committing or Opening a PR

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --release
uv run python scripts/check_docs_layout.py --write-index
uv run ruff check scripts tests/python && uv run ruff format --check scripts tests/python
uv run pytest
```
