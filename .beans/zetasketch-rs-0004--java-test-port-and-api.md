---
# zetasketch-rs-0004
title: 'Port Java ZetaSketch unit tests; API conveniences (add_sketch, merge_ref, accessors)'
status: in-progress
type: task
created_at: 2026-10-29T00:30:00Z
updated_at: 2026-10-29T01:30:00Z
---

Roadmap items 1 and 2 (`docs/specs/20261028_roadmap_future_improvements.md`): port Java
ZetaSketch's `javatests/` and add the API conveniences.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | Keep improving: the next roadmap items | chat request "lets keep improving" | 2026-10-29 |
| Built | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | Tests ported by two subagents in parallel (public API, internal modules); API added in the main session | branch `feat/java-test-port-and-api` | 2026-10-29 |
| Proven | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | L2: 74 + 97 ported Java tests pass, 0 ignored; `add_sketch` equals the reference merge on 120 randomized folds | local `cargo test --release` and debug `cargo test`, 2026-10-29; CI pending | 2026-10-29 |
| Reviewed | Reviewer: Yuri Olive | | pending: merged after green CI under the standing instruction "feel free to merge the PRs after ci is ok" | |
| Shipped | CI | | pending: merge | |
| Verified | CI | | pending: green CI on main | |

## Todo

- [x] Port HyperLogLogPlusPlusTest, HyperLogLogPlusPlusMergeTest, HashTest (74 tests)
- [x] Port State, Encoding{Normal,Sparse}, Data, representation, DifferenceEncoder/Decoder, MergedIntIterator tests (97 tests)
- [x] `Accumulator::add_sketch`, `merge_ref`, `value_kind`, `standard_error`, `Accumulator::precisions`
- [ ] `Aggregator` trait (deferred until a second sketch type exists)
