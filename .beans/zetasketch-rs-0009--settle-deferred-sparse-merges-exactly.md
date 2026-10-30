---
# zetasketch-rs-0009
title: 'Settle deferred sparse merges exactly, in one pass'
status: in-progress
type: task
created_at: 2026-10-30T04:00:00Z
updated_at: 2026-10-30T04:00:00Z
---

Replace the 5-bytes-per-value deferral bound with deferral up to a cap that follows the union's size,
and decide exactly at the merge whether any of the deferred merges reached the normalization
threshold. The sparse phase rewrote the whole sorted union every few hundred deferred values;
profiling a day-grain fold (many groups, accumulators that stay sparse for long) showed those
rewrites and the sorts feeding them as most of the accumulator's time.

## Design

`settle` merges the deferred sketches with one sorted merge. With `B` the encoded length after all of
them and `D` the number of values that collapsed into another, a merge before the last cannot have
been above `B + D` (adding a value never shortens the encoding; replacing a rho-encoded value by a
larger one shortens it by at most one byte, at most `D` times). So `B >= T` is dense, `B + D < T` is
sparse, and anything in between replays the sketches one by one, which is the reference exactly.
Readers that take `&self` work on a resolved copy. Pending values are kept with the length of each
sketch so that the replay is possible.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | "any space for improvements in performance?" / sparse-phase work after the profile of the day-grain fold | chat | 2026-10-30 |
| Built | Agent: Claude (session eba8300c-b0a6-4596-a5ff-034f5941aaf0), approved by Yuri Olive | `settle`, `resolve`, `resolved`, `merge_step_exact`, `pending_cap`; unstable sort of the deferred values | branch `perf/sparse-phase` (on top of `feat/decoded-sparse-input`) | 2026-10-30 |
| Proven | Agent: Claude (session eba8300c-b0a6-4596-a5ff-034f5941aaf0), approved by Yuri Olive | L2: `deferred_merges_match_reference_at_checkpoints` (small precisions, both ways of adding, every read path, asserts the replay path ran; dropping the `D` slack makes it fail) and `rejected_input_with_deferred_values_changes_nothing`; all earlier tests unchanged; byte-identical engine output on a large real fold (8 entities, two spans) against the previous library | local `cargo test` (debug and release), `cargo clippy --all-targets -- -D warnings`, 2026-10-30; CI pending | 2026-10-30 |
| Reviewed | Reviewer: Yuri Olive | | pending: independent review not yet done | |
| Shipped | CI | | not merged, not released: see the note below | |
| Verified | CI | | pending | |

## Not shipped yet: no end-to-end gain was shown

In a consumer that folds pre-decoded sparse values (the path of `add_sparse_batch`), a day-grain fold
of one large entity ran about 2x faster on one thread, and the merge passes fell about 3x. In the
consumer that parses stored sketch bytes (`add_trusted_bytes`) no difference was measurable at 8
threads (24 entities, interleaved runs, noise larger than the effect; the fine grouping level looked
no better, possibly worse). That consumer's time is dominated by Parquet decoding, not by the
accumulator. Until a consumer shows a gain, the branch stays unmerged; the tests and the design are
worth keeping.
