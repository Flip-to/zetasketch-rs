---
status: accepted
kind: developer
who: "Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive"
ticket: None
spec: docs/specs/20261028_hllpp_port_with_bigquery_parity.md
prs: []
---

# The dense estimate sums a register histogram, exactly, with the sequential loop as fallback

## Context

`docs/decisions/20261028_dense_estimate_sequential_sum.md` kept Java's sequential f64 loop over
all `2^p` registers, because floating-point addition is not associative and any other order
could change the rounded estimate. The loop is latency-bound: about 19.9 us per estimate at
p = 15 (`examples/estimate_cost.rs`, 2026-10-29), as much as folding several dense inputs.

## Decision

Compute `sum(2^-v)` from a register-value histogram, accumulated in integers and converted to f64
once, whenever `p + vmax <= 53`. Otherwise use the sequential loop.

## Why

The histogram result is bit-identical to Java's loop whenever `p + vmax <= 53`, not just close:

- Every term is `2^-v`, so every partial sum is a multiple of `2^-vmax` and no larger than
  `2^p`.
- Such a number needs at most `p + vmax` significand bits. When `p + vmax <= 53`, every partial
  sum of the sequential loop is exactly representable, so the loop never rounds and returns the
  exact sum.
- The histogram computes that same exact sum: an integer below `2^53` times a power of two, both
  exact in f64.

`vmax > 53 - p` (38 at p = 15) needs a hash with that many leading zeros after the index. That is
about 2^-38 per value, so the fallback effectively never runs. A unit test compares the f64 bits
with the sequential loop across p in (4, 10, 12, 15, 18), including cases with large registers
that force the fallback. All golden and BigQuery suites still pass.

Result: 19.9 to 8.0 us per estimate at p = 15 (2.5x), with four interleaved sub-histograms.

## Alternatives

- Keep the sequential loop: correct, but 2.5x slower for no correctness benefit.
- SIMD or reordered f64 sums: not exact in general. Rejected for the reason in the superseded
  decision.

## Impact

Estimates and cardinalities of dense sketches are 2.5x cheaper, with unchanged results. If a
future change adds terms that are not dyadic or adds a scaling, this proof no longer holds and
must be redone.

## Who

Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive ("prioritize
correctness and performance", 2026-10-29).
