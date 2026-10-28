---
# zetasketch-rs-0003
title: '0.2.0: API hardening, allocation cleanups, CI checks, nightly fuzz'
status: in-progress
type: task
created_at: 2026-10-28T23:30:00Z
updated_at: 2026-10-28T23:59:00Z
---

Changes from the debt audit:

- `#[non_exhaustive]` on `Error` and `ValueKind` (breaking, so 0.2.0);
- bias correction without heap allocation;
- `Accumulator::serialize` without copying the registers or re-validating;
- CI checks for docs, packaging and bias-table drift against pinned Java;
- a nightly fuzz job that keeps its corpus;
- decisions for the two items deliberately left as they are, and tech-debt records for the two
  open performance items.

## A2V track

| Step | Who | Why | Evidence | When |
|---|---|---|---|---|
| Asked | Developer: Yuri Olive | Keep improving: remaining performance work and tech debt | chat request "go ahead and keep working and improving" | 2026-10-28 |
| Built | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | `estimate` stays `&mut` (`docs/decisions/20261028_estimate_takes_mut_for_java_parity.md`); the dense sum stays sequential (`docs/decisions/20261028_dense_estimate_sequential_sum.md`) | branch `release/0.2.0-cleanup` | 2026-10-28 |
| Proven | Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive | L2: all parity suites unchanged and green, new unit test for the oversized-sparse serialize path, bias tables regenerate identically from Java `d098c49` | local `cargo test --release`, `cargo doc` with `-D warnings`, `cargo package`, 2026-10-28; CI pending | 2026-10-28 |
| Reviewed | Reviewer: Yuri Olive | | pending: PR review | |
| Shipped | Developer: Yuri Olive | | pending: merge, then tag v0.2.0 | |
| Verified | CI | | pending: release run publishes 0.2.0 through trusted publishing | |

## Todo

- [x] non_exhaustive enums, version 0.2.0, CHANGELOG
- [x] Allocation-free `estimate_bias`; `serialize` without the register copy
- [x] CI: `cargo doc -D warnings`, `cargo package`, bias-table drift; nightly fuzz with corpus
- [ ] `docs/tech_debt/sparse_merge_multi_pass.md`
- [ ] `docs/tech_debt/mixed_precision_fallback_path.md`
