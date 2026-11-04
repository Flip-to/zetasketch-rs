---
# zetasketch-rs-0013
title: 'Accumulator::absorb: merge a partial without copying its values'
status: in-progress
type: task
created_at: 2026-11-04T22:00:00Z
updated_at: 2026-11-04T22:00:00Z
---

Combining per-thread partials of a request with many small groups calls `merge` once per group,
metric and thread. For a small sparse source most of that call is sorting and copying the source's
deferred values into a new union before appending it to the target's deferred values. `absorb`
takes the source by value: an empty target takes it over, a sparse source within the deferral bound
(its own bound of its encoded union as the growth, see `DECISION(deferred_sparse_merges)`) is
appended as it is, a dense target raises its registers directly. Anything else is `merge`.

Also tried in the same session: a smaller `Accumulator` (padding proxy, within noise, not built),
recorded in `docs/findings/20261030_performance_attempts_log.md`.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | "o que vc faria na biblioteca rs?" then go-ahead to prototype the consuming merge for fine-level combines | chat | 2026-11-04 |
| Built | Agent: Claude (session 16c9303b-0952-4cdc-8d4d-28abfd518593), approved by Yuri Olive | `Accumulator::absorb`; `examples/fine_level_combine.rs` | this PR | 2026-11-04 |
| Proven | Agent: Claude (session 16c9303b-0952-4cdc-8d4d-28abfd518593), approved by Yuri Olive | L2: `absorb_equals_merge_at_every_step` (cardinality, count, precisions, bytes after every step; tiny, sparse, dense, mixed precision; type error leaves target unchanged); removing the deferral bound check makes it fail; `fine_level_combine` 1.7 to 2.1x at 100 and 1,000 groups, 1.2 to 1.3x at 5,000, 1.05 to 1.1x at 20,000 | local `cargo test --release`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, 2026-11-04; CI pending | 2026-11-04 |
| Reviewed | Reviewer: Yuri Olive | | pending | |
| Shipped | Developer: Yuri Olive | | pending: merge, release only on the owner's approval | |
| Verified | Developer: Yuri Olive | | pending: combine timing in a consumer after release | |
