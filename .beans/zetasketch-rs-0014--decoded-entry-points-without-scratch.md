---
# zetasketch-rs-0014
title: 'Decoded entry points without the scratch lend in the common case'
status: in-progress
type: task
created_at: 2026-11-04T23:30:00Z
updated_at: 2026-11-04T23:30:00Z
---

A consumer that stores sketches decoded calls `add_single_values` once per row (one value) or
`add_sparse_values` per small sketch. Each call lent and returned the per-thread scratch buffers
(thread-local access plus three swaps) even when it only raised registers or appended to the
pending values. `add_decoded_fast` does those two cases on `self` directly, with the same checks
and bookkeeping as `add_one_sketch`; anything else takes the previous path.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | "pode trabalhar" (continue on library performance for decoded-layout consumers) | chat | 2026-11-04 |
| Built | Agent: Claude (session 16c9303b-0952-4cdc-8d4d-28abfd518593), approved by Yuri Olive | `add_decoded_fast`; `examples/decoded_fold.rs` | this PR | 2026-11-04 |
| Proven | Agent: Claude (session 16c9303b-0952-4cdc-8d4d-28abfd518593), approved by Yuri Olive | L2: existing decoded-input tests compare with `add_trusted_bytes` across the sparse and dense phases; `decoded_fold` one value per call 18.4 to 7.8 ns (dense), 34-39 to 16-18 ns (sparse) | local `cargo test --release`, clippy, fmt, 2026-11-04; CI pending | 2026-11-04 |
| Reviewed | Reviewer: Yuri Olive | | pending | |
| Shipped | Developer: Yuri Olive | | pending | |
| Verified | Developer: Yuri Olive | | pending: consumer timing after release | |
