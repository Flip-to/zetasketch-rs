//! Fast fold of many serialized sketches into one result.
//!
//! [`Accumulator`] gives the same estimate and the same serialized bytes as folding the same
//! inputs, in the same order, with [`HyperLogLogPlusPlus::merge_bytes`]. Inputs are parsed
//! without copying. The dense register array and the sparse buffers are kept across inputs, and
//! across [`Accumulator::reset`], so a steady-state fold allocates nothing.
//!
//! The fast path covers inputs that share the first input's normal and sparse precision and have
//! sparse mode enabled. Anything else (a precision change, sparse mode disabled, an input that
//! carries both dense and sparse data) switches the accumulator to the reference
//! implementation for the rest of the fold. That keeps the result exact.
//! DECISION(faithful_port_of_java_reference)

use crate::encoding::{self, Sparse};
use crate::error::{Error, Result};
use crate::hll::{self, HyperLogLogPlusPlus};
use crate::proto::{varint_len, State, StateView, HYPERLOGLOG_PLUS_UNIQUE};

/// Reusable accumulator for merging many serialized sketches.
///
/// ```
/// use zetasketch::{Accumulator, HyperLogLogPlusPlus};
///
/// let mut a = HyperLogLogPlusPlus::for_longs(15).unwrap();
/// let mut b = HyperLogLogPlusPlus::for_longs(15).unwrap();
/// for v in 0..100 { a.add_long(v).unwrap(); }
/// for v in 50..150 { b.add_long(v).unwrap(); }
///
/// let mut acc = Accumulator::new();
/// acc.add_bytes(&a.serialize()).unwrap();
/// acc.add_bytes(&b.serialize()).unwrap();
/// assert_eq!(acc.estimate(), 150);
/// acc.reset(); // reuse the buffers for the next group
/// ```
#[derive(Debug, Clone, Default)]
pub struct Accumulator {
    started: bool,
    num_values: i64,
    allowed: u8,
    value_type: i32,
    precision: i32,
    sparse_precision: i32,
    /// Java `sparse_size` field: the first input's value until a sparse merge rewrites it.
    sparse_size: i32,
    dense: bool,
    registers: Vec<u8>,
    sparse: Vec<u32>,
    incoming: Vec<u32>,
    merged: Vec<u32>,
    fallback: Option<HyperLogLogPlusPlus>,
}

impl Accumulator {
    /// Empty accumulator. Buffers are allocated on first use and then reused.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the accumulated state but keeps the allocated buffers.
    pub fn reset(&mut self) {
        self.started = false;
        self.num_values = 0;
        self.dense = false;
        self.sparse.clear();
        self.fallback = None;
    }

    /// True when no sketch has been added since creation or the last [`reset`](Self::reset).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.started && self.fallback.is_none()
    }

    /// Number of values across all added sketches (not distinct).
    #[must_use]
    pub fn num_values(&self) -> i64 {
        match &self.fallback {
            Some(f) => f.num_values(),
            None => self.num_values,
        }
    }

    /// Merges one serialized sketch. Empty input is a no-op, like
    /// [`HyperLogLogPlusPlus::merge_bytes`].
    ///
    /// # Errors
    /// Malformed bytes, incompatible precision or value type. The sketch is fully validated
    /// before anything is merged, so on error the accumulator is unchanged.
    pub fn add_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        if let Some(f) = &mut self.fallback {
            return f.merge_bytes(bytes);
        }
        let view = StateView::parse(bytes)?;
        let allowed = hll::validate_header(&view)?;

        let fast = view.sparse_precision != hll::SPARSE_PRECISION_DISABLED
            && !(view.has_data() && view.sparse_data.is_some());
        if !self.started {
            if !fast {
                self.fallback = Some(HyperLogLogPlusPlus::from_state(view.to_state())?);
                return Ok(());
            }
            self.decode_incoming(&view)?;
            self.start(&view, allowed);
            return Ok(());
        }

        let types = self.allowed & allowed;
        if types == 0 {
            return Err(Error::IncompatibleType(format!(
                "aggregator of types {:#b} is incompatible with {allowed:#b}",
                self.allowed
            )));
        }
        if !fast
            || view.precision != self.precision
            || view.sparse_precision != self.sparse_precision
        {
            let mut f = HyperLogLogPlusPlus::from_state(self.build_state())?;
            f.merge(HyperLogLogPlusPlus::from_state(view.to_state())?)?;
            self.fallback = Some(f);
            return Ok(());
        }
        self.decode_incoming(&view)?;
        self.allowed = types;
        self.num_values = self.num_values.wrapping_add(view.num_values);

        if let Some(data) = view.data.filter(|d| !d.is_empty()) {
            if !self.dense {
                self.densify();
            }
            max_into(&mut self.registers, data);
        } else if view.has_sparse_data() {
            if self.dense {
                let enc = self.encoding();
                for &v in &self.incoming {
                    set_register(&mut self.registers, enc, v);
                }
            } else {
                self.merge_sparse();
            }
        }
        Ok(())
    }

    /// Cardinality estimate of everything added, equal to the reference fold's `estimate()`.
    /// Returns 0 when nothing was added.
    pub fn estimate(&mut self) -> i64 {
        if let Some(f) = &mut self.fallback {
            return f.estimate();
        }
        if !self.started {
            return 0;
        }
        if self.dense {
            hll::dense_estimate(&self.registers, self.precision)
        } else {
            hll::sparse_estimate(self.sparse_precision, self.sparse_size)
        }
    }

    /// The accumulated sketch, or `None` when nothing was added.
    ///
    /// # Panics
    /// Never for states built by this accumulator. The internal state is always valid.
    #[must_use]
    pub fn to_sketch(&self) -> Option<HyperLogLogPlusPlus> {
        if let Some(f) = &self.fallback {
            return Some(f.clone());
        }
        if !self.started {
            return None;
        }
        Some(
            HyperLogLogPlusPlus::from_state(self.build_state())
                .expect("accumulator state is valid"),
        )
    }

    /// Serialized result, byte-identical to the reference fold (`HLL_COUNT.MERGE_PARTIAL`), or
    /// `None` when nothing was added.
    #[must_use]
    pub fn serialize(&self) -> Option<Vec<u8>> {
        self.to_sketch().map(|mut s| s.serialize())
    }

    /// Decodes and validates the input's sparse stream into `incoming`, before any state change.
    fn decode_incoming(&mut self, view: &StateView<'_>) -> Result<()> {
        self.incoming.clear();
        if let Some(sd) = view.sparse_data.filter(|d| !d.is_empty()) {
            let enc = Sparse::new(view.precision, view.sparse_precision);
            encoding::decode_validated_into(sd, enc, &mut self.incoming)?;
        }
        Ok(())
    }

    fn encoding(&self) -> Sparse {
        Sparse::new(self.precision, self.sparse_precision)
    }

    fn start(&mut self, view: &StateView<'_>, allowed: u8) {
        self.started = true;
        self.allowed = allowed;
        self.value_type = view.value_type;
        self.num_values = view.num_values;
        self.precision = view.precision;
        self.sparse_precision = view.sparse_precision;
        self.sparse_size = view.sparse_size;
        self.sparse.clear();
        if let Some(data) = view.data.filter(|d| !d.is_empty()) {
            self.dense = true;
            self.registers.clear();
            self.registers.extend_from_slice(data);
        } else {
            self.dense = false;
            std::mem::swap(&mut self.sparse, &mut self.incoming);
        }
    }

    /// Java `SparseRepresentation.normalize`.
    fn densify(&mut self) {
        let enc = self.encoding();
        self.registers.clear();
        self.registers.resize(1 << self.precision, 0);
        for &v in &self.sparse {
            set_register(&mut self.registers, enc, v);
        }
        self.sparse.clear();
        self.dense = true;
        self.sparse_size = 0;
    }

    /// Java sparse + sparse merge: sorted union, dedupe, then normalize if the encoded stream
    /// reaches `0.75 * 2^p` bytes.
    fn merge_sparse(&mut self) {
        let enc = self.encoding();
        encoding::merge_sorted_into(&self.sparse, &self.incoming, &mut self.merged);
        enc.dedupe_in_place(&mut self.merged);
        std::mem::swap(&mut self.sparse, &mut self.merged);
        self.sparse_size = self.sparse.len() as i32;

        let mut bytes = 0usize;
        let mut last = 0u32;
        for &v in &self.sparse {
            bytes += varint_len(v - last);
            last = v;
        }
        let max_bytes = ((1i64 << self.precision) as f32 * 0.75f32) as usize;
        if bytes >= max_bytes {
            self.densify();
        }
    }

    fn build_state(&self) -> State {
        State {
            r#type: HYPERLOGLOG_PLUS_UNIQUE,
            num_values: self.num_values,
            encoding_version: hll::ENCODING_VERSION,
            value_type: self.value_type,
            sparse_size: self.sparse_size,
            precision: self.precision,
            sparse_precision: self.sparse_precision,
            data: self.dense.then(|| self.registers.clone()),
            sparse_data: (!self.dense).then(|| encoding::encode_sparse(&self.sparse)),
        }
    }
}

#[inline]
fn set_register(registers: &mut [u8], enc: Sparse, v: u32) {
    let idx = enc.decode_normal_index(v);
    let rho = enc.decode_normal_rho_w(v);
    if registers[idx] < rho {
        registers[idx] = rho;
    }
}

/// Register-wise max. Written as a plain zip so the compiler vectorizes it.
#[inline]
fn max_into(target: &mut [u8], source: &[u8]) {
    for (t, &s) in target.iter_mut().zip(source) {
        *t = (*t).max(s);
    }
}
