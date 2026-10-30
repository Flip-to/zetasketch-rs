---
# zetasketch-rs-0012
title: 'Merge the plain sparse values with a branch-free loop'
status: in-progress
type: task
created_at: 2026-10-30T23:00:00Z
updated_at: 2026-10-30T23:00:00Z
---

Instrumenting `merge` showed the merge-and-dedupe pass at 3 to 6 ns per element, re-reading a growing
union for every source. `merge_dedupe_into` now merges the values below the rho-encoded flag (plain
whatever the input, since the flag is one bit; almost every value) with a branch-free loop that
drops repeats and counts the encoded length, then runs the old general loop over the rest.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | "keep working on profile one merge at about 3,000 values" | chat | 2026-10-30 |
| Built | Agent: Claude (session dee39dd0-a741-443f-83d1-af955367f015), approved by Yuri Olive | `merge_plain`, `gap_len`, `merge_dedupe_rest` | branch `perf/merge-loop` (on top of `perf/merge-direct`) | 2026-10-30 |
| Proven | Agent: Claude (session dee39dd0-a741-443f-83d1-af955367f015), approved by Yuri Olive | L2: `fused_merge_dedupe_matches_three_pass` (existing), `plain_merge_matches_three_pass_with_duplicates` (repeats inside runs, empty and one-value runs, 0, rho-encoded values among plain ones, six precision pairs), `gap_len_is_varint_len`; all accumulator differential tests unchanged; dropping the within-run repeat check fails both merge tests; `merge_probe` 31.7 to 22.0 us and 104.5 to 59.4 us per merge | local `cargo test` (debug and release), `cargo clippy --all-targets -- -D warnings`, 2026-10-30; CI pending | 2026-10-30 |
| Reviewed | Reviewer: Yuri Olive | | pending | |
| Shipped | CI | | pending: merge, then a release only on the owner's approval | |
| Verified | CI | | pending | |
