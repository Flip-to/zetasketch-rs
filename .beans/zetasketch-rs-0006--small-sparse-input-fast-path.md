---
# zetasketch-rs-0006
title: 'Fast path for small sparse inputs'
status: in-progress
type: task
created_at: 2026-10-29T18:00:00Z
updated_at: 2026-10-29T18:00:00Z
---

Cut the per-call cost of folding very small sparse sketches (a handful of values) into an
`Accumulator`, the common shape in event-grain data: a compact proto parse and a scratch-free
path into a dense accumulator.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | Look for processing improvements that need neither cubes nor a custom data format; try a fast path for tiny sparse inputs | chat requests "anything that could boost a lot?", "Yes, try it" | 2026-10-29 |
| Built | Agent: Claude (session eba8300c-b0a6-4596-a5ff-034f5941aaf0), approved by Yuri Olive | `StateView::parse_compact` (one-byte tags, values and lengths; any other layout returns `None` and the general parser decides) and `Accumulator::add_small_sparse` (stack decode, no scratch, state unchanged on error) | branch `perf/tiny-sparse-inputs` | 2026-10-29 |
| Proven | Agent: Claude (session eba8300c-b0a6-4596-a5ff-034f5941aaf0), approved by Yuri Olive | L2: `parse_compact_matches_general` (valid and mangled inputs: a `Some` equals the general parse); existing `tests/accumulator.rs` equality with the reference fold; L1: `examples/single_value_fold.rs` | local `cargo test --release`, `cargo clippy --all-targets -- -D warnings`, 2026-10-29; CI pending | 2026-10-29 |
| Reviewed | Reviewer: Yuri Olive | | pending: merged after green CI under the standing instruction | |
| Shipped | CI | | pending: merge | |
| Verified | CI | | pending: green CI on main | |
