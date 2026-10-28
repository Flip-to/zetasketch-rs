---
status: accepted
area: merge
impact: low
effort: s
---

# Sparse downgrade re-adds buffered values under the target encoding (reference quirk)

## What

Java `SparseRepresentation.downgrade(encoding)` converts the stored sparse stream to the lower
encoding. It then re-adds the unflushed buffer as if those values were already in the lower
encoding, but they are still in the old one. `hll.rs` mirrors this.

## Why it exists

`DECISION(faithful_port_of_java_reference)`: byte parity with the reference beats local
correctness.

## Consequence

The quirk shows up only when a sketch that has unflushed in-memory adds merges a sketch with a
lower precision. Sketches decoded from bytes have an empty buffer, so merging BigQuery sketches
never reaches it.

## Fix

If needed: flush before downgrading, and check whether BigQuery agrees. That needs a fixture
from a mixed-precision merge of freshly built sketches.

## Workaround meanwhile

Call `serialize()` (which flushes) before merging a locally built sketch into a lower-precision
one.
