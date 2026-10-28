---
status: accepted
kind: product
who: "Developer: Yuri Olive"
ticket: None
spec: docs/specs/20261028_hllpp_port_with_bigquery_parity.md
prs: []
---

# Test fixtures are synthetic only; no real data in this public repo

## Context

The repository is public and open source. Sketches built from real data leak information about
that data: register patterns, approximate cardinalities and value hashes. So do project, dataset
and table names.

## Decision

Every fixture is generated from synthetic inputs:

- BigQuery fixtures come from `GENERATE_ARRAY` and literal strings inside the query. The
  generator reads no table.
- The billing project is a command-line argument, never written into the repo.
- Third-party corpora (the zetasketch-cpp golden corpus) are synthetic too.

## Why

This keeps the repo safe to publish and the fixtures reproducible by anyone with BigQuery access.

## Alternatives

- Sampled production sketches: more realistic, but cannot be published. Validation on real
  sketches belongs in the consumer's private test suite.

## Impact

Parity with real-world sketch shapes is not proven here, only parity across a cardinality and
merge-shape grid. Consumers should run their own private parity checks.

## Who

Developer: Yuri Olive (stated in the 2026-10-28 session: "our data must remain private and the
repo will be opensource public").
