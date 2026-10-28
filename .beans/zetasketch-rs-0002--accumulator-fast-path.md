---
# zetasketch-rs-0002
title: 'Accumulator fast path for folding many serialized sketches'
status: completed
type: task
created_at: 2026-10-28T22:10:00Z
updated_at: 2026-10-28T23:00:00Z
---

Add `Accumulator`: a zero-copy, allocation-free fold of serialized sketches, equal byte for byte
to the reference fold. Also add differential, randomized, mixed-precision and robustness tests,
cargo-fuzz targets and a fold benchmark. The randomized tests found a Java-inherited downgrade
bug; it is fixed and checked against new BigQuery fixtures.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | Build the planned hot-path merge for many stored sketches | chat request "Build fast path" | 2026-10-28 |
| Built | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | Reference implementation stays the oracle and the fallback (`docs/decisions/20261028_faithful_port_of_java_reference.md`) | https://github.com/Flip-to/zetasketch-rs/pull/1 | 2026-10-28 |
| Proven | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | L2: equality with the reference fold (21 golden + 450 randomized folds), 3 new BigQuery lowering-chain fixtures, 40k corrupt inputs without a panic; L1: bench 2.1x | local `cargo test --release` and `cargo bench --bench fold`, 2026-10-28; CI pending on the PR | 2026-10-28 |
| Reviewed | Developer: Yuri Olive | Approved merge in session | chat request "Do 1, 2, 5, 6" | 2026-10-28 |
| Shipped | Developer: Yuri Olive | Squash-merged to main | https://github.com/Flip-to/zetasketch-rs/pull/1, commit 9bf283f | 2026-10-28 |
| Verified | CI | All 6 PR checks green incl. fuzz smoke; main CI re-runs on merge commit | PR #1 checks | 2026-10-28 |

## Todo

- [x] `Accumulator` with zero-copy parse, reusable buffers, reference fallback
- [x] Fix the buffered-downgrade bug, plus BigQuery lowering-chain fixtures
- [x] Bounds validation for sparse values and registers; robustness tests
- [x] `fuzz/` targets (decode, differential_fold) and a CI smoke run
- [x] `benches/fold.rs`
- [ ] Faster sparse inputs (branch-light varint decode, fused validate+apply) if a consumer bench needs it
