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
- Both methods (`estimate` plus a `&self` `peek_estimate`): possible later if a caller needs
  shared access. For results that are only read, `Accumulator::serialize(&self)` plus a decode
  already works.

## Impact

Callers that cache results hold them mutably or behind a lock. Revisit if the engine needs
concurrent reads of one live sketch.

## Who

Agent: Claude (session 8b72ee89-5aeb-409a-b8be-e395357ed26a), approved by Yuri Olive ("go ahead
and keep working and improving", 2026-10-28).
