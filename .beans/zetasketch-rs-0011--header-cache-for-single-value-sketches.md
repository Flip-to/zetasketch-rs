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
way (sixteen slots, by length), and a later sketch with the same header bytes skips the parse and the
structural validation, which depend only on those bytes.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | "work on the zetasketch-rs improvements first that you recommend" | chat | 2026-10-30 |
| Built | Agent: Claude (session dee39dd0-a741-443f-83d1-af955367f015), approved by Yuri Olive | `Header`, `cached_header`, `learn_header`, `Accumulator::add_cached`; the cache is thread-local, so no API change | branch `perf/header-cache` | 2026-10-30 |
| Proven | Agent: Claude (session dee39dd0-a741-443f-83d1-af955367f015), approved by Yuri Olive | L2: `header_cache_changes_nothing` (40 runs x 1,500 inputs; cache warm against the cache emptied before every input; success, error text, panics and serialized state equal; mutations that skip the prefix compare or the count update make it fail); benchmark `examples/single_value_fold.rs` 33.6 to 21.4 ns (dense, `add_trusted_bytes`), 31.9 to 18.3 ns (sparse, 300), best of four runs, per sketch | local `cargo test` (debug), `cargo clippy --all-targets -- -D warnings`, 2026-10-30; CI pending | 2026-10-30 |
| Reviewed | Reviewer: Yuri Olive | | pending | |
| Shipped | CI | | pending: merge, then a release only on the owner's approval | |
| Verified | CI | | pending | |

## Extended after review

The first version cached one-value sketches for `add_trusted_bytes` only. After an independent review
(no correctness bug; test gaps) it now also covers sketches of one to eight values (the stream is
decoded as exactly the remembered number of varints of at most four bytes, sums over `i32::MAX` go to
the general path) and `add_bytes` (headers learned with full validation, values range-checked by
`encoding::small_values_valid`, which restates the validating decode's checks). The tests were
rewritten for the gaps the review found: the cache hits are counted (a thread-local counter) instead of
inputs that could hit, the reference side runs with the cache emptied before and after so the cached
side only hits what it learned, both entry points, sketches of another precision or value type, a
test of stream shapes. Skipping the value check makes the differential test fail. The `validated`
flag is a precaution: for sketches without dense data `validate_header` equals `validate_structure`, so
no test can tell, and skipping the flag is not caught.

## Second review

A second review of the extension found no correctness bug and two weaknesses in the tests, both fixed:
`header_cache_stream_shapes` built its streams with a value type the accumulator rejects (every fold
failed the same way on both sides, so it proved little) and could not tell a hit from the full path
(the reference side's second fold also hit). It now uses the INT64 value type, the reference side
empties the cache between its folds, and a new test (`header_cache_refuses_what_validation_refuses`)
learns a valid stream and then folds an invalid one of the same length: an invalid stream is never
learned, so only that sequence reaches the value check. Mutations: always-true `small_values_valid`
fails the differential and the refusal tests; dropping the last value of a multi-value hit fails the
differential test; ignoring the `validated` flag fails nothing (see above).
