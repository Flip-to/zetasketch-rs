---
# zetasketch-rs-0001
title: 'HyperLogLog++ port with Java and BigQuery byte parity, repo scaffold and governance'
status: completed
type: task
created_at: 2026-10-28T18:00:00Z
updated_at: 2026-10-28T19:00:00Z
---

Port Java ZetaSketch HLL++ to a zero-dependency Rust crate. Prove byte parity against the
zetasketch-cpp golden corpus (Java reference) and against BigQuery `HLL_COUNT` fixtures built
from synthetic inputs. Scaffold the repo with the docs, decisions, tech debt, bean and CI
conventions used in `Flip-to/dbt-virtual`.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | Rust ZetaSketch implementation for a low-latency sketch-serving engine; public open-source repo under Flip-to, no private data | chat request | 2026-10-28 |
| Built | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | Function-by-function port (`docs/decisions/20261028_faithful_port_of_java_reference.md`), synthetic fixtures (`docs/decisions/20261028_synthetic_fixtures_only.md`) | initial commit | 2026-10-28 |
| Proven | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | L2: byte-exact parity with Java corpus (187 rows) and BigQuery (66 INIT + 66 rebuilt + 13 MERGE) | local `cargo test --release`, 2026-10-28, rustc 1.91.0; CI pending: repo not pushed | 2026-10-28 |
| Reviewed | Developer: Yuri Olive | Reviewed in session, approved publishing | chat approval "Commit and publish" | 2026-10-28 |
| Shipped | Developer: Yuri Olive | Public repo created, `main` pushed | https://github.com/Flip-to/zetasketch-rs | 2026-10-28 |
| Verified | CI | All jobs green (Linux stable + 1.75, Windows, macOS, lint, docs guard) | GitHub Actions run 40776085005 | 2026-10-28 |

## Todo

- [x] Port proto, encodings, sparse/normal representations, estimator, bias tables, Fingerprint2011
- [x] Golden corpus suite (Java reference via zetasketch-cpp)
- [x] BigQuery fixture generator + suite (synthetic only)
- [x] Docs: spec, decisions, tech debt; CLAUDE.md, AGENTS.md; CI
- [x] Create public repo `Flip-to/zetasketch-rs`, push, CI green
- [ ] Confirm crates.io name, publish v0.1.0 (moved to a later bean)
- [x] Accumulator fast path + fuzzing (moved to `zetasketch-rs-0002`)
