//! **Experimental.** Decoding and quantile extraction for BigQuery `KLL_QUANTILES` sketches.
//!
//! BigQuery's KLL implementation is not open source. It lives in the same `AggregatorStateProto`
//! envelope as HLL++ (aggregator type 113, reserved in the public ZetaSketch protos). This module
//! follows the format and extraction rule worked out from BigQuery itself, as recorded in
//! `docs/findings/20261029_kll_quantiles_feasibility_spike.md`:
//!
//! - **Exact** for `KLL_QUANTILES.EXTRACT_*` / `EXTRACT_POINT_*` on stored sketches across the
//!   golden fixtures, except a few extreme-tail lookups at very small `k` (k = 10, heavy
//!   compaction), where BigQuery picks a neighbouring item. Those known differences are listed
//!   in `tests/kll_golden.rs`.
//! - **No merge.** BigQuery's KLL compaction is randomized per execution, so merged bytes cannot
//!   match BigQuery's; a merge would need its own statistical validation.
//!
//! The API may change in any release while it is experimental.

use crate::error::{Error, Result};
use crate::proto::{parse_kll, KllRaw};

/// A value stored in a KLL sketch.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum KllValue {
    /// From `KLL_QUANTILES.INIT_INT64`.
    Int64(i64),
    /// From `KLL_QUANTILES.INIT_FLOAT64`.
    Float64(f64),
}

#[derive(Debug, Clone)]
enum Items {
    Int64(Vec<(i64, u64)>),
    Float64(Vec<(f64, u64)>),
}

/// A decoded `KLL_QUANTILES` sketch: weighted samples plus the exact minimum and maximum.
#[derive(Debug, Clone)]
pub struct KllSketch {
    num_values: i64,
    k: i64,
    min: KllValue,
    max: KllValue,
    total_weight: u64,
    /// Sorted by value; each item's weight is `2^level`.
    items: Items,
}

impl KllSketch {
    /// Decodes the bytes of `KLL_QUANTILES.INIT_INT64` / `INIT_FLOAT64` / `MERGE_PARTIAL`.
    ///
    /// # Errors
    /// Malformed proto, a non-KLL aggregator type, an unsupported value type, or inconsistent
    /// contents (no items, a level too high to weigh, min/max of the wrong type).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let raw = parse_kll(bytes)?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: KllRaw) -> Result<Self> {
        if raw.level_count() > 63 {
            return Err(Error::InvalidState("too many KLL levels".into()));
        }
        let weight = |level: usize| 1u64 << level;
        let (items, min, max) = match raw.value_type {
            KllRaw::VALUE_TYPE_INT64 => {
                let mut items = Vec::new();
                for (level, values) in raw.int_levels()?.into_iter().enumerate() {
                    items.extend(values.into_iter().map(|v| (v, weight(level))));
                }
                items.sort_by_key(|&(v, _)| v);
                let (min, max) = (raw.int_min()?, raw.int_max()?);
                (
                    Items::Int64(items),
                    KllValue::Int64(min),
                    KllValue::Int64(max),
                )
            }
            KllRaw::VALUE_TYPE_FLOAT64 => {
                let mut items = Vec::new();
                for (level, values) in raw.float_levels()?.into_iter().enumerate() {
                    items.extend(values.into_iter().map(|v| (v, weight(level))));
                }
                items.sort_by(|a, b| a.0.total_cmp(&b.0));
                let (min, max) = (raw.float_min()?, raw.float_max()?);
                (
                    Items::Float64(items),
                    KllValue::Float64(min),
                    KllValue::Float64(max),
                )
            }
            other => {
                return Err(Error::IncompatibleType(format!(
                    "unsupported KLL value type {other}"
                )))
            }
        };
        let total_weight = match &items {
            Items::Int64(v) => v.iter().map(|&(_, w)| w).sum(),
            Items::Float64(v) => v.iter().map(|&(_, w)| w).sum(),
        };
        if total_weight == 0 {
            return Err(Error::InvalidState("KLL sketch has no items".into()));
        }
        Ok(KllSketch {
            num_values: raw.num_values,
            k: raw.k,
            min,
            max,
            total_weight,
            items,
        })
    }

    /// Number of values the sketch summarizes.
    #[must_use]
    pub fn num_values(&self) -> i64 {
        self.num_values
    }

    /// The precision argument the sketch was built with (`KLL_QUANTILES.INIT_*(x, k)`).
    #[must_use]
    pub fn k(&self) -> i64 {
        self.k
    }

    /// Exact minimum of the summarized values.
    #[must_use]
    pub fn min(&self) -> KllValue {
        self.min
    }

    /// Exact maximum of the summarized values.
    #[must_use]
    pub fn max(&self) -> KllValue {
        self.max
    }

    /// `KLL_QUANTILES.EXTRACT_*(sketch, q)`: `q + 1` values, the exact minimum, the `q - 1`
    /// inner `j/q` quantiles, and the exact maximum.
    ///
    /// # Errors
    /// `q` is 0.
    pub fn quantiles(&self, q: usize) -> Result<Vec<KllValue>> {
        if q == 0 {
            return Err(Error::InvalidState(
                "number of quantiles must be > 0".into(),
            ));
        }
        let w = self.total_weight as f64;
        let mut out = Vec::with_capacity(q + 1);
        out.push(self.min);
        for j in 1..q {
            out.push(self.at_weight(j as f64 * w / q as f64));
        }
        out.push(self.max);
        Ok(out)
    }

    /// `KLL_QUANTILES.EXTRACT_POINT_*(sketch, phi)` for `phi` in `[0, 1]`.
    ///
    /// # Errors
    /// `phi` outside `[0, 1]` (or NaN).
    pub fn point(&self, phi: f64) -> Result<KllValue> {
        if !(0.0..=1.0).contains(&phi) {
            return Err(Error::InvalidState(format!(
                "phi must be in [0, 1], got {phi}"
            )));
        }
        if phi == 0.0 {
            return Ok(self.min);
        }
        if phi == 1.0 {
            return Ok(self.max);
        }
        Ok(self.at_weight(phi * self.total_weight as f64))
    }

    /// The first item (by value) whose cumulative weight reaches `target`.
    fn at_weight(&self, target: f64) -> KllValue {
        fn pick<T: Copy>(items: &[(T, u64)], target: f64) -> T {
            let mut cum = 0u64;
            for &(v, w) in items {
                cum += w;
                if cum as f64 >= target {
                    return v;
                }
            }
            items[items.len() - 1].0
        }
        match &self.items {
            Items::Int64(v) => KllValue::Int64(pick(v, target)),
            Items::Float64(v) => KllValue::Float64(pick(v, target)),
        }
    }
}
