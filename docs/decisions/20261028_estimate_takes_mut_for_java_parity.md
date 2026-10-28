---
status: accepted
kind: developer
who: "Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive"
ticket: None
spec: docs/specs/20261028_hllpp_port_with_bigquery_parity.md
prs: []
---

# `estimate()` takes `&mut self`, like Java's `longResult()` mutates

## Context

`HyperLogLogPlusPlus::estimate` and `Accumulator::estimate` take `&mut self`, so a result can't
be estimated through a shared `&` reference. The obvious API fix is an `estimate(&self)` that
computes on a copy.

## Decision

Keep `&mut self`. Estimating a sparse sketch flushes its buffer into the sparse stream, exactly as
Java's `SparseRepresentation.estimate()` does.

## Why

The flush changes later behavior. After it, the sparse stream is larger, so the next `add` can
cross the `0.75 * 2^p` byte threshold and turn the sketch dense. Without the flush, the values stay
in the buffer and the sketch stays sparse. So an `add, estimate, add, estimate` sequence returns a
different second estimate (dense estimator vs linear counting) depending on whether the first
estimate flushed. Matching Java on such sequences requires the mutation.

## Alternatives

- `estimate(&self)` computing on a copy: breaks parity on interleaved add/estimate sequences, as
  above.
- **Both methods (adopted alongside this decision):** `cardinality(&self)` returns the same value
  `estimate` would, computed without the flush, so cached results can be read through a shared
  reference. `estimate(&mut self)` stays for exact Java behavior on interleaved add/estimate
  sequences. A test checks that `cardinality == estimate` at every step of interleaved adds.

## Impact

Callers that cache results read them with `cardinality(&self)`: no lock, no `&mut`. Only code
that keeps adding to a sketch after estimating needs `estimate(&mut self)`.

## Who

Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive ("go ahead
and keep working and improving", 2026-10-28).
