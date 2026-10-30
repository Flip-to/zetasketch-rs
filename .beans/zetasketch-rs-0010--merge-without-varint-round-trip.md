---
# zetasketch-rs-0010
title: 'Merge sparse accumulators without a varint round trip'
status: in-progress
type: task
created_at: 2026-10-30T21:00:00Z
updated_at: 2026-10-30T21:00:00Z
---

`Accumulator::merge` of a sparse source encoded its sorted values to varint bytes and `add_view`
decoded them again before merging. For a started target with the same precisions the sorted values
now go straight in (into the deferral or merge path, or into the registers of a dense target).

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | "work on the zetasketch-rs improvements first that you recommend" | chat | 2026-10-30 |
| Built | Agent: Claude (session dee39dd0-a741-443f-83d1-af955367f015), approved by Yuri Olive | A fast branch at the top of `merge`; every other case takes the old route | branch `perf/merge-direct` | 2026-10-30 |
| Proven | Agent: Claude (session dee39dd0-a741-443f-83d1-af955367f015), approved by Yuri Olive | L2: `merge_equals_adding_the_serialized_source` (5 precision pairs x 3 source precisions x 60 random trials, empty to past the threshold, deferred values, a value-type error with the target unchanged); dropping the last value of the merged slice makes it fail; benchmark `examples/merge_probe.rs` 24 to 33 % faster per merge | local `cargo test` (debug), `cargo clippy --all-targets -- -D warnings`, 2026-10-30; CI pending | 2026-10-30 |
| Reviewed | Reviewer: Yuri Olive | | pending | |
| Shipped | CI | | pending: merge, then a release only on the owner's approval | |
| Verified | CI | | pending | |
