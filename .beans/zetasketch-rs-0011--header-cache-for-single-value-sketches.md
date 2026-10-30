---
# zetasketch-rs-0011
title: 'Skip the parse of single-value sketches with a remembered header'
status: in-progress
type: task
created_at: 2026-10-30T22:00:00Z
updated_at: 2026-10-30T22:00:00Z
---

Most non-null sketches in a day-grain event root hold one value. Their bytes are a fixed header
(type, counts, versions, precisions and the lengths a value's varint size fixes) and the varint.
`add_trusted_bytes` now remembers, per thread, the header of a one-value sketch it folded the long
way (eight slots, by length), and a later sketch with the same header bytes skips the parse and the
structural validation, which depend only on those bytes.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | "work on the zetasketch-rs improvements first that you recommend" | chat | 2026-10-30 |
| Built | Agent: Claude (session dee39dd0-a741-443f-83d1-af955367f015), approved by Yuri Olive | `Header`, `cached_header`, `learn_header`, `Accumulator::add_cached_single`; the cache is thread-local, so no API change | branch `perf/header-cache` | 2026-10-30 |
| Proven | Agent: Claude (session dee39dd0-a741-443f-83d1-af955367f015), approved by Yuri Olive | L2: `header_cache_changes_nothing` (40 runs x 1,500 inputs; cache warm against the cache emptied before every input; success, error text, panics and serialized state equal; mutations that skip the prefix compare or the count update make it fail); benchmark `examples/single_value_fold.rs` 33.6 to 19.1 ns (dense), 31.9 to 18.1 ns (sparse, 300) per sketch | local `cargo test` (debug), `cargo clippy --all-targets -- -D warnings`, 2026-10-30; CI pending | 2026-10-30 |
| Reviewed | Reviewer: Yuri Olive | | pending | |
| Shipped | CI | | pending: merge, then a release only on the owner's approval | |
| Verified | CI | | pending | |
