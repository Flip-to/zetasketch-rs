//! Fast fold of many serialized sketches into one result.
//!
//! [`Accumulator`] gives the same estimate and the same serialized bytes as folding the same
//! inputs, in the same order, with [`HyperLogLogPlusPlus::merge_bytes`]. Inputs are parsed
//! without copying. The dense register array and the sparse buffers are kept across inputs, and
//! across [`Accumulator::reset`], so a steady-state fold allocates nothing.
//!
//! The fast path covers every input with sparse mode enabled while the accumulator is dense,
//! whatever its precision (Java's dense merge is deterministic: lower the registers once, or
//! downgrade the input on the fly), and same-precision inputs while it is sparse. A precision
//! change while sparse, sparse mode disabled, or an input carrying both dense and sparse data
//! runs that merge on the reference implementation, whose sparse buffering is order-dependent;
//! the accumulator returns to the fast path as soon as the reference state has no unflushed
//! buffer. That keeps the result exact.
//! DECISION(faithful_port_of_java_reference)

use crate::encoding::{self, Normal, Sparse};
use crate::error::{Error, Result};
use crate::hll::{self, HyperLogLogPlusPlus};
use crate::proto::{State, StateView, HYPERLOGLOG_PLUS_UNIQUE};

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
    /// Difference-encoded byte length of `sparse`, kept current across merges.
    sparse_bytes: usize,
    dense: bool,
    registers: Vec<u8>,
    sparse: Vec<u32>,
    incoming: Vec<u32>,
    merged: Vec<u32>,
    /// Scratch registers for lowering the dense precision.
    lowered: Vec<u8>,
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

    /// Normal and sparse precision of the result, or `None` when nothing was added.
    #[must_use]
    pub fn precisions(&self) -> Option<(i32, i32)> {
        match &self.fallback {
            Some(f) => Some((f.normal_precision(), f.sparse_precision())),
            None => self
                .started
                .then_some((self.precision, self.sparse_precision)),
        }
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
            f.merge_bytes(bytes)?;
            self.try_resume();
            return Ok(());
        }
        let view = StateView::parse(bytes)?;
        let allowed = hll::validate_header(&view)?;
        self.add_view(&view, allowed)
    }

    /// Merges an in-memory sketch, with the same result as `add_bytes(&sketch.serialize())`
    /// would give in the reference fold (`merge`), but without serializing or copying it.
    ///
    /// # Errors
    /// Incompatible precision or value type. On error the accumulator is unchanged.
    pub fn add_sketch(&mut self, sketch: &HyperLogLogPlusPlus) -> Result<()> {
        if let Some(f) = &mut self.fallback {
            f.merge_ref(sketch)?;
            self.try_resume();
            return Ok(());
        }
        if sketch.has_buffered_values() {
            // Unflushed adds: only the reference merge sees them exactly as Java does.
            if !self.started {
                self.fallback = Some(sketch.clone());
                return Ok(());
            }
            let mut f = HyperLogLogPlusPlus::from_state(self.build_state())?;
            f.merge_ref(sketch)?;
            self.fallback = Some(f);
            self.try_resume();
            return Ok(());
        }
        let view = sketch.state_view();
        let allowed = hll::validate_header(&view)?;
        self.add_view(&view, allowed)
    }

    fn add_view(&mut self, view: &StateView<'_>, allowed: u8) -> Result<()> {
        let fast = view.sparse_precision != hll::SPARSE_PRECISION_DISABLED
            && !(view.has_data() && view.sparse_data.is_some());
        if !self.started {
            if !fast {
                self.fallback = Some(HyperLogLogPlusPlus::from_state(view.to_state())?);
                return Ok(());
            }
            self.decode_incoming(view)?;
            self.start(view, allowed);
            return Ok(());
        }

        let types = self.allowed & allowed;
        if types == 0 {
            return Err(Error::IncompatibleType(format!(
                "aggregator of types {:#b} is incompatible with {allowed:#b}",
                self.allowed
            )));
        }
        let same_precision =
            view.precision == self.precision && view.sparse_precision == self.sparse_precision;
        if !fast || (!self.dense && !same_precision) {
            // The reference's sparse merge across precisions buffers values one by one; run it
            // there, then resume on the fast path once its buffer is flushed.
            let mut f = HyperLogLogPlusPlus::from_state(self.build_state())?;
            f.merge(HyperLogLogPlusPlus::from_state(view.to_state())?)?;
            self.fallback = Some(f);
            self.try_resume();
            return Ok(());
        }
        self.decode_incoming(view)?;
        self.allowed = types;
        self.num_values = self.num_values.wrapping_add(view.num_values);

        if self.dense && !same_precision {
            self.merge_into_dense_any_precision(view);
            return Ok(());
        }

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

    /// [`estimate`](Self::estimate) through a shared reference: the same value, with no
    /// mutation. Use it to read cached results.
    #[must_use]
    pub fn cardinality(&self) -> i64 {
        if let Some(f) = &self.fallback {
            return f.cardinality();
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
    /// `None` when nothing was added. Written straight from the accumulator's buffers: no copy of
    /// the registers and no re-validation.
    #[must_use]
    pub fn serialize(&self) -> Option<Vec<u8>> {
        if let Some(f) = &self.fallback {
            return Some(f.clone().serialize());
        }
        if !self.started {
            return None;
        }
        if self.dense {
            return Some(self.view(Some(&self.registers), None).encode());
        }
        let sparse = encoding::encode_sparse(&self.sparse);
        if sparse.len() >= self.max_sparse_bytes() {
            // Only reachable when a single oversized sparse input was never merged. The
            // reference compacts it to dense on serialize, so take that path.
            return self.to_sketch().map(|mut s| s.serialize());
        }
        Some(self.view(None, Some(&sparse)).encode())
    }

    fn view<'a>(&self, data: Option<&'a [u8]>, sparse_data: Option<&'a [u8]>) -> StateView<'a> {
        StateView {
            r#type: HYPERLOGLOG_PLUS_UNIQUE,
            num_values: self.num_values,
            encoding_version: hll::ENCODING_VERSION,
            value_type: self.value_type,
            sparse_size: self.sparse_size,
            precision: self.precision,
            sparse_precision: self.sparse_precision,
            data,
            sparse_data,
        }
    }

    fn max_sparse_bytes(&self) -> usize {
        ((1i64 << self.precision) as f32 * 0.75f32) as usize
    }

    /// Java `NormalRepresentation.merge` / `addSparseValues` for a dense accumulator and an input
    /// of any precision: lower the registers if the input's normal precision is lower
    /// (`maybeDowngrade`), take the lower sparse precision, then max the input in, downgrading
    /// its registers or sparse values when its precision is higher. Deterministic, no buffer.
    fn merge_into_dense_any_precision(&mut self, view: &StateView<'_>) {
        // maybeDowngrade(other.precision, other.sparse_precision)
        if !(self.precision <= view.precision && self.sparse_precision <= view.sparse_precision) {
            if self.precision > view.precision {
                let from = Normal {
                    precision: self.precision,
                };
                let to = Normal {
                    precision: view.precision,
                };
                self.lowered.clear();
                self.lowered.resize(1 << view.precision, 0);
                encoding::downgrade_registers_max_into(
                    &self.registers,
                    from.precision,
                    &mut self.lowered,
                    to.precision,
                );
                std::mem::swap(&mut self.registers, &mut self.lowered);
                self.precision = view.precision;
            }
            self.sparse_precision = self.sparse_precision.min(view.sparse_precision);
        }
        let target = Normal {
            precision: self.precision,
        };
        if let Some(data) = view.data.filter(|d| !d.is_empty()) {
            let source = Normal {
                precision: view.precision,
            };
            if source.precision == target.precision {
                max_into(&mut self.registers, data);
            } else {
                encoding::downgrade_registers_max_into(
                    data,
                    source.precision,
                    &mut self.registers,
                    target.precision,
                );
            }
        } else {
            let enc = Sparse::new(view.precision, view.sparse_precision);
            for &v in &self.incoming {
                let (idx, rho) = if target.precision < enc.normal_precision {
                    (
                        enc.decode_and_downgrade_normal_index(v, target),
                        enc.decode_and_downgrade_normal_rho_w(v, target),
                    )
                } else {
                    (enc.decode_normal_index(v), enc.decode_normal_rho_w(v))
                };
                if self.registers[idx] < rho {
                    self.registers[idx] = rho;
                }
            }
        }
    }

    /// Leaves the reference fallback when its state can be continued on the fast path: no
    /// unflushed buffer (see `HyperLogLogPlusPlus::resumable_state`).
    fn try_resume(&mut self) {
        let Some(f) = &self.fallback else { return };
        let Some((state, allowed)) = f.resumable_state() else {
            return;
        };
        let sparse = match &state.sparse_data {
            Some(sd) if !state.has_data() => match encoding::decode_sparse(sd) {
                Ok(v) => Some((v, sd.len())),
                Err(_) => return,
            },
            _ => None,
        };
        self.started = true;
        self.allowed = allowed;
        self.value_type = state.value_type;
        self.num_values = state.num_values;
        self.precision = state.precision;
        self.sparse_precision = state.sparse_precision;
        self.sparse_size = state.sparse_size;
        if let Some((values, bytes)) = sparse {
            self.dense = false;
            self.sparse = values;
            self.sparse_bytes = bytes;
        } else if let Some(data) = state.data.as_deref().filter(|d| !d.is_empty()) {
            self.dense = true;
            self.registers.clear();
            self.registers.extend_from_slice(data);
        } else {
            // Sparse mode with no stream yet.
            self.dense = false;
            self.sparse.clear();
            self.sparse_bytes = 0;
        }
        self.fallback = None;
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
            self.sparse_bytes = view.sparse_data.map_or(0, <[u8]>::len);
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
        // A few values into a big union: insert in place. Otherwise one full merge pass.
        if self.incoming.len() * 16 <= self.sparse.len() {
            encoding::insert_dedupe(
                enc,
                &mut self.sparse,
                &self.incoming,
                &mut self.sparse_bytes,
            );
        } else {
            self.sparse_bytes =
                encoding::merge_dedupe_into(enc, &self.sparse, &self.incoming, &mut self.merged);
            std::mem::swap(&mut self.sparse, &mut self.merged);
        }
        self.sparse_size = self.sparse.len() as i32;
        if self.sparse_bytes >= self.max_sparse_bytes() {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A single sparse input whose stream is already at the normalization threshold is never
    /// merged, so the accumulator still holds it sparse. `serialize` must then match the
    /// reference, which compacts it to dense. BigQuery never emits such a sketch, so this state
    /// is built by hand.
    #[test]
    fn oversized_single_sparse_input_serializes_like_reference() {
        let values: Vec<u32> = (0..800).map(|i| i * 32 + 1).collect();
        let state = State {
            r#type: HYPERLOGLOG_PLUS_UNIQUE,
            num_values: 800,
            encoding_version: hll::ENCODING_VERSION,
            value_type: 8,
            sparse_size: 800,
            precision: 10,
            sparse_precision: 15,
            data: None,
            sparse_data: Some(encoding::encode_sparse(&values)),
        };
        assert!(state.sparse_data.as_ref().unwrap().len() >= 768);
        let bytes = state.to_bytes();

        let mut reference = HyperLogLogPlusPlus::from_bytes(&bytes).unwrap();
        let mut acc = Accumulator::new();
        acc.add_bytes(&bytes).unwrap();
        assert_eq!(acc.estimate(), reference.estimate());
        assert_eq!(acc.serialize().unwrap(), reference.serialize());
    }
}
