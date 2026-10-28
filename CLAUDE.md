# `zetasketch-rs` — Agent & Developer Guide

Byte-compatible Rust port of Google ZetaSketch HyperLogLog++ (the BigQuery `HLL_COUNT` sketch
format). It decodes, merges, estimates and re-serializes sketches, and builds them from values.

## Project Structure

```
Cargo.toml                    # crate `zetasketch`, zero dependencies, unsafe forbidden
src/
  lib.rs                      # public API re-exports
  hll.rs                      # HyperLogLogPlusPlus: sparse/normal representations, add, merge, estimate, serialize
  encoding.rs                 # Normal/Sparse encodings, rhoW, downgrade, dedupe, difference varint stream
  proto.rs                    # hand-rolled AggregatorStateProto + HyperLogLogPlusUniqueStateProto reader/writer
  estimate.rs                 # alpha, linear-counting thresholds, kNN bias correction, Java Math.round
  bias_data.rs                # GENERATED from Java Data.java by scripts/gen_bias_data.py
  hash.rs                     # Guava Fingerprint2011
  error.rs                    # Error / Result
tests/
  golden_corpus.rs            # Java reference parity (zetasketch-cpp golden corpus)
  bigquery_golden.rs          # BigQuery HLL_COUNT parity (synthetic fixtures)
  data/                       # golden_corpus.tsv, bigquery_golden.tsv
scripts/
  check_docs_layout.py        # CI guard for docs, decisions, tech debt, beans, A2V
  gen_bias_data.py            # regenerate src/bias_data.rs
  gen_bigquery_fixtures.py    # regenerate tests/data/bigquery_golden.tsv (synthetic inputs only)
```

Written docs live in `docs/` (`specs/`, `decisions/`, `tech_debt/`, `conventions/`,
`templates/`). Names are `snake_case`. Specs carry a `status` frontmatter. Each decision is one
file with `Context` / `Decision` / `Why` / `Alternatives` / `Impact` / `Who`, referenced in code
as `DECISION(<slug>)`. Each known debt is a `docs/tech_debt/<slug>.md` file plus a
`TECH_DEBT(<slug>)` comment in code. Standard and CI gate: `docs/conventions/docs_and_specs.md`,
`scripts/check_docs_layout.py`. Every claim carries its evidence or is phrased as a question:
`.claude/rules/evidence.md`. Every bean carries an A2V track: `.claude/rules/a2v-track.md`.

## Common Commands

```bash
cargo test --release                                  # golden corpus + BigQuery parity (release: 10^6-value rebuilds)
cargo clippy --all-targets -- -D warnings             # pedantic lints, see [lints] in Cargo.toml
cargo fmt --check
uv run scripts/check_docs_layout.py [--write-index]   # docs/decisions/tech_debt/beans guard
uv run scripts/gen_bigquery_fixtures.py --project <p> # refresh BigQuery fixtures
```

## Invariants

- **Byte parity is the contract.** Any change must keep `tests/golden_corpus.rs` and
  `tests/bigquery_golden.rs` green. When the reference does something odd, port it as-is and
  write it down (`docs/tech_debt/`). Do not "fix" it silently
  (`DECISION(faithful_port_of_java_reference)`).
- **Never merge serialized bytes with a bytewise max.** Merges work on decoded registers and
  sparse values.
- **Fixtures are synthetic only.** Never commit real or customer sketches, values, project
  names or table names (`DECISION(synthetic_fixtures_only)`). Generator queries read no tables.
- `src/bias_data.rs` is generated. Change `scripts/gen_bias_data.py`, never the output.
- Shifts that can reach the type width use `wrapping_shl`, because Java masks shift amounts.

## Agent Skills (`.claude/skills/`)

- `/record-decision`: add `docs/decisions/YYYYMMDD_<slug>.md` from `docs/templates/decision.md`.
- `/record-tech-debt`: add `docs/tech_debt/<slug>.md`, a bean, and a `TECH_DEBT(<slug>)` marker.
- `/a2v-track`: create and advance a bean's `## A2V track`.
