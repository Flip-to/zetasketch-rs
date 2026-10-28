---
# zetasketch-rs-0005
title: 'Mixed-precision folds on the Accumulator fast path'
status: in-progress
type: task
created_at: 2026-10-29T02:00:00Z
updated_at: 2026-10-29T03:00:00Z
---

Pay `docs/tech_debt/mixed_precision_fallback_path.md`: keep mixed-precision folds on the fast
path without losing exactness.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | Next improvement: the mixed-precision path; prioritize correctness and performance | chat requests "Mixed-precision path", "prioritize correctness and performance" | 2026-10-29 |
| Built | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | Dense merges across precisions are deterministic, so they go on the fast path; sparse ones use one reference merge, then resume; per-block register downgrade | branch `perf/mixed-precision-fast-path` | 2026-10-29 |
| Proven | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | L2: 400 random mixed-precision folds equal the reference (bytes, estimate, cardinality, count), release and debug; block downgrade equals Java's per-register form for p 4 to 16; L1: bench 40.9 to 17.2 us per sketch | local `cargo test --release`, `cargo test`, `cargo bench --bench fold`, 2026-10-29; CI pending | 2026-10-29 |
| Reviewed | Reviewer: Yuri Olive | | pending: merged after green CI under the standing instruction | |
| Shipped | CI | | pending: merge | |
| Verified | CI | | pending: green CI on main | |
