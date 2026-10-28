//! Byte-compatible Rust port of [Google ZetaSketch](https://github.com/google/zetasketch)
//! HyperLogLog++, the sketch format produced and consumed by BigQuery `HLL_COUNT.*`.
//!
//! - Decode sketches (`AggregatorStateProto`, sparse and dense) with
//!   [`HyperLogLogPlusPlus::from_bytes`].
//! - Merge them with [`HyperLogLogPlusPlus::merge`] / [`HyperLogLogPlusPlus::merge_bytes`]:
//!   registers are merged by max on **decoded** values, with sparse-to-dense promotion and
//!   precision lowering following the reference.
//! - Fold many serialized sketches fast with [`Accumulator`] (no per-sketch allocation, same
//!   bytes and estimate as the reference fold).
//! - Estimate with [`HyperLogLogPlusPlus::estimate`] (equal to `HLL_COUNT.EXTRACT`).
//! - Re-serialize with [`HyperLogLogPlusPlus::serialize`] (equal to `HLL_COUNT.MERGE_PARTIAL`).
//! - Build sketches from values (`add_long`, `add_str`, ...) using the reference
//!   `Fingerprint2011` hash.
//!
//! Parity is checked against the reference Java implementation's golden corpus and against
//! BigQuery outputs over synthetic inputs (see `tests/`).

mod accumulator;
mod bias_data;
mod encoding;
mod error;
mod estimate;
pub mod hash;
mod hll;
mod proto;

pub use accumulator::Accumulator;
pub use error::{Error, Result};
pub use hll::{
    HyperLogLogPlusPlus, ValueKind, DEFAULT_NORMAL_PRECISION, DEFAULT_SPARSE_PRECISION_DELTA,
    MAXIMUM_PRECISION, MAXIMUM_SPARSE_PRECISION, MINIMUM_PRECISION, SPARSE_PRECISION_DISABLED,
};
