---
status: accepted
kind: developer
who: "Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive"
ticket: None
spec: docs/specs/20261028_hllpp_port_with_bigquery_parity.md
prs: []
---

# Port Java ZetaSketch function by function, quirks included

## Context

Byte parity with `HLL_COUNT.MERGE_PARTIAL` depends on more than the register math. It also
depends on when the sparse buffer flushes, when a sketch turns dense, how values are deduped and
downgraded, and which proto fields are written. A clean-room design would match estimates but
drift on bytes.

## Decision

Port each Java method with the same state transitions in the same order. Mirror reference
quirks, and record each one in `docs/tech_debt/`. Deviate only where Java would crash or read
stale state (see the spec). Never deviate on an input that Java serializes successfully.

## Why

The golden corpus and the BigQuery fixtures are byte-level oracles. A faithful port passed all
of them on the first run (2026-10-28). Any "improvement" to the transitions would have to be
proven against the same oracles anyway.

## Alternatives

- Clean-room implementation from the HLL++ paper: estimates match, bytes would not.
- Port zetasketch-cpp instead: it is itself a corrected port of Java. Going to the source avoids
  inheriting its divergences.

## Impact

Performance work (zero-copy decode, a reusable dense accumulator, SIMD max) must sit beside the
faithful path and be differentially tested against it. It must not replace it.

## Who

Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive.
