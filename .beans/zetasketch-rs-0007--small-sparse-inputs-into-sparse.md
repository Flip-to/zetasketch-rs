---
# zetasketch-rs-0007
title: 'Small sparse inputs into a sparse accumulator without the scratch buffers'
status: in-progress
type: task
created_at: 2026-10-29T22:00:00Z
updated_at: 2026-10-29T22:00:00Z
---

Extend the small-input path of `Accumulator::add_bytes` / `add_trusted_bytes` (bean 0006) to
accumulators that are still sparse: append the decoded values to the deferred buffer, exactly the
deferral branch of `merge_sparse`, without lending the scratch buffers.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | Improve the algorithms and algos of this crate where results stay byte-exact | chat requests "we can further improve the rust zetasketch algorithms...", "Try building those" | 2026-10-29 |
| Built | Agent: Claude (session eba8300c-b0a6-4596-a5ff-034f5941aaf0), approved by Yuri Olive | `add_small_sparse` also handles a sparse accumulator when the 5-bytes-per-value bound stays under the threshold and the pending buffer under its cap; otherwise the general path | branch `perf/sparse-small-inputs` | 2026-10-29 |
| Proven | Agent: Claude (session eba8300c-b0a6-4596-a5ff-034f5941aaf0), approved by Yuri Olive | L2: `small_sparse_inputs_into_sparse_match_reference` (4,000 random inputs of 1 to 20 values, equal to the reference fold in estimate, count and serialized bytes at every 97th input, through flushes and the switch to dense, validated and trusted); the randomized `tests/accumulator.rs` equality suites run the new path; L1: `examples/single_value_fold.rs` | local `cargo test --release`, `cargo clippy --all-targets -- -D warnings`, 2026-10-29; CI pending | 2026-10-29 |
| Reviewed | Reviewer: Yuri Olive | | pending: merged after green CI under the standing instruction | |
| Shipped | CI | | pending: merge | |
| Verified | CI | | pending: green CI on main | |
