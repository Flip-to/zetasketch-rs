---
# zetasketch-rs-0008
title: 'Fold sparse sketches that were decoded beforehand'
status: in-progress
type: task
created_at: 2026-10-29T23:00:00Z
updated_at: 2026-10-29T23:00:00Z
---

Add `SparseHeader`, `Accumulator::add_single_values`, `add_sparse_values` and `add_sparse_batch`:
feed an accumulator the sparse values of sketches that an engine stored already decoded, with the
same result as `add_trusted_bytes` on the serialized sketches and no protobuf parsing.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | Prototype a pre-decoded sketch layout to see how far the raw path can go without precomputed aggregates | chat requests "Prototype it", "I want to avoid cubes" | 2026-10-29 |
| Built | Agent: Claude (session eba8300c-b0a6-4596-a5ff-034f5941aaf0), approved by Yuri Olive | One internal `add_one_sketch` (start, dense register loop, the deferral branch of `merge_sparse`, else the general path on the serialized sketch) behind three public entry points; a tight loop over the remaining values once dense | branch `feat/decoded-sparse-input` | 2026-10-29 |
| Proven | Agent: Claude (session eba8300c-b0a6-4596-a5ff-034f5941aaf0), approved by Yuri Olive | L2: `add_single_values_equals_the_one_value_sketches` (40,000 values, one per call and in batches, through the sparse and dense phases, a STRING sketch and precision changes); `add_sparse_values_equals_the_sketches` (4,000 sketches of 1 to 40 values, single and batched; estimate, count and serialized bytes equal); `add_sparse_batch_rejects_mismatched_lengths` | local `cargo test --release`, `cargo clippy --all-targets -- -D warnings`, 2026-10-29; CI pending | 2026-10-29 |
| Reviewed | Reviewer: Yuri Olive | | pending: merged after green CI under the standing instruction | |
| Shipped | CI | | pending: merge | |
| Verified | CI | | pending: green CI on main | |
