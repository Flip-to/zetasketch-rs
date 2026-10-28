---
status: accepted
kind: developer
who: "Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive"
ticket: None
spec: docs/specs/20261028_hllpp_port_with_bigquery_parity.md
prs: []
---

# The dense estimate sums registers in order, in f64, and stays unoptimized

## Context

`dense_estimate` sums `2^-v` over every register, 32,768 of them at p = 15, one at a time. It is
the obvious target for SIMD or a histogram (count registers per `v`, then sum 65 terms).

## Decision

Keep the sequential f64 sum in register order, exactly as Java's `NormalRepresentation.estimate`
does.

## Why

Floating-point addition is not associative. Partial sums need up to about 15 + 61 = 76 bits of
mantissa, more than f64's 53, so the sequential sum rounds along the way. Any other order (SIMD
lanes, a histogram) can give a different last bit. Near a `.5` boundary that changes the rounded
estimate, which would break exact parity with Java and BigQuery `HLL_COUNT.EXTRACT`. The cost is
one pass over 32 KiB per estimate, small next to a fold.

## Alternatives

- SIMD or a histogram sum: faster, but not bit-exact with the reference.
- An exact accumulator (integer fixed-point, then one rounding): exact, but it would be a different
  rounding from Java's sequential f64 sum, so equally unsafe.

## Impact

Estimates stay bit-exact. Revisit only if estimating dominates a measured workload. Even then, the
safe speedup is fewer estimates (caching), not a faster sum.

## Who

Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive.
