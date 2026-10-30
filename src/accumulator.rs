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

/// What the sketches of one pre-decoded input have in common: the value type and the two
/// precisions, validated once (see [`Accumulator::add_single_values`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparseHeader {
    value_type: i32,
    precision: i32,
    sparse_precision: i32,
    allowed: u8,
}

impl SparseHeader {
    /// Validates the value type and precisions the way a sketch carrying sparse data is validated.
    ///
    /// # Errors
    /// An unknown value type, or precisions the format does not allow (normal precision 4 to 24,
    /// sparse precision up to 25 and not below the normal one).
    pub fn new(value_type: i32, precision: i32, sparse_precision: i32) -> Result<Self> {
        let probe = StateView {
            r#type: HYPERLOGLOG_PLUS_UNIQUE,
            num_values: 1,
            encoding_version: hll::ENCODING_VERSION,
            value_type,
            sparse_size: 1,
            precision,
            sparse_precision,
            data: None,
            sparse_data: Some(&[1]),
        };
        let allowed = hll::validate_structure(&probe)?;
        Ok(SparseHeader {
            value_type,
            precision,
            sparse_precision,
            allowed,
        })
    }
}

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
    /// Sparse values from same-precision inputs not yet merged into `sparse` (unsorted, may
    /// contain duplicates). Whether the merges they stand for crossed the normalization threshold
    /// is decided when they are merged; see `settle`.
    pending: Vec<u32>,
    /// Length of each sketch's values in `pending`, in order (to replay them one by one).
    pending_lens: Vec<u32>,
    dense: bool,
    registers: Vec<u8>,
    sparse: Vec<u32>,
    // Scratch buffers. They are lent from a per-thread pool for the duration of one call
    // (`with_scratch`), so an idle accumulator holds no scratch memory and thousands of live
    // accumulators (one per group and metric) share one set of warm buffers per thread.
    incoming: Vec<u32>,
    merged: Vec<u32>,
    /// Scratch for the sorted copy of `pending` while it is merged.
    sorted: Vec<u32>,
    /// Scratch registers for lowering the dense precision.
    lowered: Vec<u8>,
    fallback: Option<Box<HyperLogLogPlusPlus>>,
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
        self.pending.clear();
        self.pending_lens.clear();
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
        if self.started && self.fallback.is_none() && !bytes.is_empty() {
            let view = StateView::parse(bytes)?;
            let allowed = hll::validate_header(&view)?;
            if self.add_small_sparse(&view, allowed, false)? {
                return Ok(());
            }
            return self.with_scratch(|acc| acc.add_view(&view, allowed, false));
        }
        self.with_scratch(|acc| acc.add_bytes_inner(bytes))
    }

    fn add_bytes_inner(&mut self, bytes: &[u8]) -> Result<()> {
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
        self.add_view(&view, allowed, false)
    }

    /// Like [`add_bytes`](Self::add_bytes), for bytes that were already validated (for example by
    /// `add_bytes` or [`HyperLogLogPlusPlus::from_bytes`] when they were stored). It skips the
    /// per-value range checks: scanning dense registers and checking each sparse value. The
    /// structural checks stay: type, version, precisions, data length and varint framing.
    ///
    /// Same result as `add_bytes` on valid input. On corrupt input it may return an error, give a
    /// wrong result, or panic (an index out of range). It never causes undefined behavior: the
    /// crate forbids unsafe code.
    ///
    /// # Errors
    /// Malformed structure, incompatible precision or value type.
    pub fn add_trusted_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if self.started && self.fallback.is_none() && !bytes.is_empty() {
            let view = StateView::parse(bytes)?;
            let allowed = hll::validate_structure(&view)?;
            if self.add_small_sparse(&view, allowed, true)? {
                return Ok(());
            }
            return self.with_scratch(|acc| acc.add_view(&view, allowed, true));
        }
        self.with_scratch(|acc| {
            if bytes.is_empty() {
                return Ok(());
            }
            if acc.fallback.is_some() {
                return acc.add_bytes_inner(bytes);
            }
            let view = StateView::parse(bytes)?;
            let allowed = hll::validate_structure(&view)?;
            acc.add_view(&view, allowed, true)
        })
    }

    /// Adds sparse values that were decoded and validated beforehand, each as a sketch of its own
    /// that holds that one value: the same result as `add_trusted_bytes` on the serialized
    /// one-value sparse sketch (`num_values` 1, `sparse_size` 1) of each value, in order, without
    /// building or parsing those bytes. A value is a sparse-encoded value as stored in the sketch's
    /// sparse stream. Meant for engines that keep sketches pre-decoded next to their data.
    ///
    /// A value outside the range the sparse precision allows is not checked here: the caller
    /// validated it when it decoded the sketch (like [`add_trusted_bytes`](Self::add_trusted_bytes),
    /// bad input may give a wrong result or a panic, never undefined behavior).
    ///
    /// # Errors
    /// Incompatible value type or precision with what is already accumulated, as for
    /// [`add_bytes`](Self::add_bytes). Values before the failing one stay added.
    pub fn add_single_values(&mut self, header: &SparseHeader, values: &[u32]) -> Result<()> {
        self.with_scratch(|acc| {
            for v in values {
                acc.add_one_sketch(header, std::slice::from_ref(v))?;
            }
            Ok(())
        })
    }

    /// Adds one sketch whose sparse stream was decoded and validated beforehand: the same result as
    /// `add_trusted_bytes` on that sketch serialized with `sparse_size` and `num_values` equal to
    /// `values.len()`. `values` are the stream's sparse-encoded values, ascending, as a valid
    /// stream holds them, and not empty (an empty slice adds nothing). The same trust applies as
    /// for [`add_single_values`](Self::add_single_values).
    ///
    /// # Errors
    /// As [`add_single_values`](Self::add_single_values).
    pub fn add_sparse_values(&mut self, header: &SparseHeader, values: &[u32]) -> Result<()> {
        self.with_scratch(|acc| acc.add_one_sketch(header, values))
    }

    /// Adds a run of pre-decoded sparse sketches: sketch `i` has `counts[i]` values, taken in order
    /// from the flattened `values`. The same result as [`add_sparse_values`](Self::add_sparse_values)
    /// on each sketch in turn, with the scratch buffers lent once, and a tight loop over the
    /// remaining values once the accumulator is dense. `values.len()` must equal the sum of
    /// `counts`.
    ///
    /// # Errors
    /// As [`add_single_values`](Self::add_single_values), and an error, with nothing added, when
    /// `values.len()` is not the sum of `counts`.
    pub fn add_sparse_batch(
        &mut self,
        header: &SparseHeader,
        counts: &[u8],
        values: &[u32],
    ) -> Result<()> {
        let total: usize = counts.iter().map(|&c| usize::from(c)).sum();
        if total != values.len() {
            return Err(Error::InvalidState(format!(
                "counts add up to {total} values but {} were given",
                values.len()
            )));
        }
        self.with_scratch(|acc| {
            let mut offset = 0usize;
            for &count in counts {
                if acc.dense
                    && acc.fallback.is_none()
                    && acc.precision == header.precision
                    && acc.sparse_precision == header.sparse_precision
                    && acc.allowed & header.allowed != 0
                {
                    // Dense and compatible: every remaining sketch only raises registers, and each
                    // adds as many values to the count as it holds. (Sketches of no values add
                    // nothing, and are in `counts` only.)
                    let rest = &values[offset..];
                    acc.allowed &= header.allowed;
                    acc.num_values = acc.num_values.wrapping_add(rest.len() as i64);
                    let enc = acc.encoding();
                    for &v in rest {
                        set_register(&mut acc.registers, enc, v);
                    }
                    return Ok(());
                }
                let n = usize::from(count);
                acc.add_one_sketch(header, &values[offset..offset + n])?;
                offset += n;
            }
            Ok(())
        })
    }

    /// One pre-decoded sparse sketch, scratch buffers already lent.
    fn add_one_sketch(&mut self, h: &SparseHeader, values: &[u32]) -> Result<()> {
        if values.is_empty() {
            return Ok(());
        }
        let n = values.len();
        if !self.started && self.fallback.is_none() {
            let view = StateView {
                r#type: HYPERLOGLOG_PLUS_UNIQUE,
                num_values: n as i64,
                encoding_version: hll::ENCODING_VERSION,
                value_type: h.value_type,
                sparse_size: n as i32,
                precision: h.precision,
                sparse_precision: h.sparse_precision,
                data: None,
                sparse_data: None,
            };
            self.incoming.clear();
            self.incoming.extend_from_slice(values);
            self.start(&view, h.allowed);
            let mut last = 0u32;
            self.sparse_bytes = values
                .iter()
                .map(|&v| {
                    let delta = v - last;
                    last = v;
                    crate::proto::varint_len(delta)
                })
                .sum();
            return Ok(());
        }
        if self.fallback.is_none()
            && self.precision == h.precision
            && self.sparse_precision == h.sparse_precision
        {
            let types = self.allowed & h.allowed;
            if types != 0 {
                self.allowed = types;
                self.num_values = self.num_values.wrapping_add(n as i64);
                if self.dense {
                    let enc = self.encoding();
                    for &v in values {
                        set_register(&mut self.registers, enc, v);
                    }
                    return Ok(());
                }
                if self.pending.len() + n <= self.pending_cap() {
                    self.pending.extend_from_slice(values);
                    self.pending_lens.push(n as u32);
                    return Ok(());
                }
                self.incoming.clear();
                self.incoming.extend_from_slice(values);
                self.merge_sparse();
                return Ok(());
            }
        }
        // Anything else (a precision change, incompatible types, the reference fallback): the
        // general path, on the serialized sketch.
        let state = State {
            r#type: HYPERLOGLOG_PLUS_UNIQUE,
            num_values: n as i64,
            encoding_version: hll::ENCODING_VERSION,
            value_type: h.value_type,
            sparse_size: n as i32,
            precision: h.precision,
            sparse_precision: h.sparse_precision,
            data: None,
            sparse_data: Some(encoding::encode_sparse(values)),
        };
        let bytes = state.view().encode();
        self.add_bytes_inner(&bytes)
    }

    /// Merges another accumulator's result into this one: exactly
    /// `self.add_bytes(&other.serialize().unwrap())`, without the serialize/decode round trip.
    /// That is the reference merge of two partial results, which is also how BigQuery combines
    /// partial aggregates (`HLL_COUNT.MERGE_PARTIAL` of partials).
    ///
    /// Use it to fold chunks of a large input on several threads and then combine the
    /// per-thread accumulators. Merging an empty accumulator is a no-op.
    ///
    /// # Errors
    /// Incompatible precision or value type. On error `self` is unchanged.
    pub fn merge(&mut self, other: &Accumulator) -> Result<()> {
        if !other.started && other.fallback.is_none() {
            return Ok(());
        }
        let resolved = other.resolved();
        let other = &*resolved;
        if self.fallback.is_some() || other.fallback.is_some() {
            let Some(sketch) = other.to_sketch() else {
                return Ok(());
            };
            return self.add_sketch(&sketch);
        }
        if other.dense {
            let view = other.view(Some(&other.registers), None);
            return self.with_scratch(|acc| acc.add_view(&view, other.allowed, true));
        }
        let sparse = encoding::encode_sparse(&other.sparse);
        if sparse.len() >= other.max_sparse_bytes() {
            // A single oversized sparse input that was never merged: `serialize` compacts it to
            // dense, so go through the same path.
            let Some(sketch) = other.to_sketch() else {
                return Ok(());
            };
            return self.add_sketch(&sketch);
        }
        let view = other.view(None, Some(&sparse));
        self.with_scratch(|acc| acc.add_view(&view, other.allowed, true))
    }

    /// Merges an in-memory sketch, with the same result as `add_bytes(&sketch.serialize())`
    /// would give in the reference fold (`merge`), but without serializing or copying it.
    ///
    /// # Errors
    /// Incompatible precision or value type. On error the accumulator is unchanged.
    pub fn add_sketch(&mut self, sketch: &HyperLogLogPlusPlus) -> Result<()> {
        self.with_scratch(|acc| acc.add_sketch_inner(sketch))
    }

    fn add_sketch_inner(&mut self, sketch: &HyperLogLogPlusPlus) -> Result<()> {
        if let Some(f) = &mut self.fallback {
            f.merge_ref(sketch)?;
            self.try_resume();
            return Ok(());
        }
        if sketch.has_buffered_values() {
            // Unflushed adds: only the reference merge sees them exactly as Java does.
            if !self.started {
                self.fallback = Some(Box::new(sketch.clone()));
                return Ok(());
            }
            self.resolve_inner();
            let mut f = HyperLogLogPlusPlus::from_state(self.build_state())?;
            f.merge_ref(sketch)?;
            self.fallback = Some(Box::new(f));
            self.try_resume();
            return Ok(());
        }
        let view = sketch.state_view();
        // In-memory sketches were validated when decoded or built.
        let allowed = hll::validate_structure(&view)?;
        self.add_view(&view, allowed, true)
    }

    /// The common case in event-grain data, without touching the scratch buffers: a sparse input
    /// of at most [`encoding::SMALL`] values with the accumulator's precisions. Decodes and
    /// validates into a stack array first, so on error the accumulator is unchanged, then does
    /// exactly what `add_view` does for this case:
    ///
    /// - into a dense accumulator: take the allowed types and the value count, and raise one
    ///   register per value;
    /// - into a sparse one: the deferral branch of `merge_sparse`, which appends the values to the
    ///   pending buffer while it stays under its cap.
    ///
    /// Returns `false`, with nothing changed, for any other input and for the sparse case that has
    /// to flush (the general path does that).
    fn add_small_sparse(
        &mut self,
        view: &StateView<'_>,
        allowed: u8,
        trusted: bool,
    ) -> Result<bool> {
        let Some(sparse) = view.sparse_data.filter(|d| !d.is_empty()) else {
            return Ok(false);
        };
        let types = self.allowed & allowed;
        if view.has_data()
            || view.sparse_precision == hll::SPARSE_PRECISION_DISABLED
            || view.precision != self.precision
            || view.sparse_precision != self.sparse_precision
            || types == 0
        {
            return Ok(false);
        }
        let enc = self.encoding();
        let mut values = [0u32; encoding::SMALL];
        let decoded = if trusted {
            encoding::decode_unvalidated_small(sparse, &mut values)?
        } else {
            encoding::decode_validated_small(sparse, enc, &mut values)?
        };
        let Some(n) = decoded else {
            return Ok(false);
        };
        if self.dense {
            self.allowed = types;
            self.num_values = self.num_values.wrapping_add(view.num_values);
            for &v in &values[..n] {
                set_register(&mut self.registers, enc, v);
            }
            return Ok(true);
        }
        // Sparse accumulator: only the deferral branch (see `merge_sparse`).
        if self.pending.len() + n > self.pending_cap() {
            return Ok(false);
        }
        self.allowed = types;
        self.num_values = self.num_values.wrapping_add(view.num_values);
        self.pending.extend_from_slice(&values[..n]);
        self.pending_lens.push(n as u32);
        Ok(true)
    }

    fn add_view(&mut self, view: &StateView<'_>, allowed: u8, trusted: bool) -> Result<()> {
        let fast = view.sparse_precision != hll::SPARSE_PRECISION_DISABLED
            && !(view.has_data() && view.sparse_data.is_some());
        if !self.started {
            if !fast {
                self.fallback = Some(Box::new(HyperLogLogPlusPlus::from_state(view.to_state())?));
                return Ok(());
            }
            self.decode_incoming(view, trusted)?;
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
            self.resolve_inner();
            let mut f = HyperLogLogPlusPlus::from_state(self.build_state())?;
            f.merge(HyperLogLogPlusPlus::from_state(view.to_state())?)?;
            self.fallback = Some(Box::new(f));
            self.try_resume();
            return Ok(());
        }
        self.decode_incoming(view, trusted)?;
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
        self.resolve();
        self.cardinality()
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
        if !self.pending.is_empty() {
            return self.resolved().cardinality();
        }
        if self.dense {
            hll::dense_estimate(&self.registers, self.precision)
        } else {
            hll::sparse_estimate(self.sparse_precision, self.current_sparse_size())
        }
    }

    /// `sparse_size` as the reference would hold it: the merged union's length once any merge
    /// happened. Needs no deferred values (`resolved`).
    fn current_sparse_size(&self) -> i32 {
        debug_assert!(self.pending.is_empty());
        self.sparse_size
    }

    /// Most values deferred at once: the union's size over two (so a merge pass is amortized over
    /// at least half as many values as it reads), and never fewer than [`MAX_PENDING`].
    fn pending_cap(&self) -> usize {
        MAX_PENDING.max(self.sparse.len() / 2)
    }

    /// This accumulator with its deferred values merged: borrowed when there are none, else a
    /// resolved copy (what a method that takes `&self` reads).
    fn resolved(&self) -> std::borrow::Cow<'_, Accumulator> {
        if self.pending.is_empty() {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut copy = self.clone();
        copy.resolve();
        std::borrow::Cow::Owned(copy)
    }

    /// Merges the deferred values (see `settle`).
    fn resolve(&mut self) {
        if !self.pending.is_empty() {
            self.with_scratch(Self::resolve_inner);
        }
    }

    /// [`resolve`](Self::resolve) with the scratch buffers already lent.
    fn resolve_inner(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending);
        let lens = std::mem::take(&mut self.pending_lens);
        self.settle(&pending, &lens);
        self.pending = pending;
        self.pending.clear();
        self.pending_lens = lens;
        self.pending_lens.clear();
    }

    /// Merges deferred sketches (`values`, one after the other, `lens` long each) into the union
    /// with one sorted merge, with the result of merging them one at a time as the reference does.
    ///
    /// The reference normalizes (goes dense) at the first merge after which the union's encoded
    /// length reaches the threshold `T`. Take `B`, the length after all the sketches, and `D`, the
    /// number of values that collapsed into another (same index). Adding a value to the union never
    /// shortens its encoding (a varint's length is at most the sum of its two halves'), and
    /// replacing a rho-encoded value by a larger one in its group shortens it by at most one byte,
    /// which happens at most `D` times. So after every merge before the last the length was at
    /// most `B + D`:
    ///
    /// - `B >= T`: the last merge normalized, so the result is dense, whatever happened before (a
    ///   dense accumulator holds the maximum of all values, however it got there);
    /// - `B + D < T`: no merge reached `T`, the union is the sparse result;
    /// - otherwise the sketches are replayed one by one, which is the reference exactly.
    ///
    /// The union is below `T` before the first of them (every earlier check passed).
    // DECISION(deferred_sparse_merges)
    fn settle(&mut self, values: &[u32], lens: &[u32]) {
        let enc = self.encoding();
        let threshold = self.max_sparse_bytes();
        self.sorted.clear();
        self.sorted.extend_from_slice(values);
        sort_runs(&mut self.sorted);
        let bytes = encoding::merge_dedupe_into(enc, &self.sparse, &self.sorted, &mut self.merged);
        let collapsed = self.sparse.len() + self.sorted.len() - self.merged.len();
        if bytes >= threshold || bytes + collapsed < threshold {
            std::mem::swap(&mut self.sparse, &mut self.merged);
            self.sparse_bytes = bytes;
            self.sparse_size = self.sparse.len() as i32;
            if bytes >= threshold {
                self.densify_now();
            }
            return;
        }
        #[cfg(test)]
        REPLAYS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut offset = 0;
        for &len in lens {
            let sketch = &values[offset..offset + len as usize];
            offset += len as usize;
            if self.dense {
                let enc = self.encoding();
                for &v in sketch {
                    set_register(&mut self.registers, enc, v);
                }
            } else if !sketch.is_empty() {
                self.merge_step_exact(sketch);
            }
        }
    }

    /// One reference merge of a sparse input into the union (nothing deferred): the sorted union,
    /// deduped, then normalized if the encoded stream reaches the threshold.
    fn merge_step_exact(&mut self, values: &[u32]) {
        debug_assert!(self.pending.is_empty());
        let enc = self.encoding();
        if values.len() <= MAX_INSERT && values.len() * 16 <= self.sparse.len() {
            // A few values into a big union: insert in place. Each insert shifts the union's tail,
            // so this only beats one full merge pass for a handful of values, whatever the union's
            // size (measured: 300 values into a 7k union cost ~50 us/sketch by insertion).
            encoding::insert_dedupe(enc, &mut self.sparse, values, &mut self.sparse_bytes);
        } else {
            self.sparse_bytes =
                encoding::merge_dedupe_into(enc, &self.sparse, values, &mut self.merged);
            std::mem::swap(&mut self.sparse, &mut self.merged);
        }
        self.sparse_size = self.sparse.len() as i32;
        if self.sparse_bytes >= self.max_sparse_bytes() {
            self.densify_now();
        }
    }

    /// The accumulated sketch, or `None` when nothing was added.
    ///
    /// # Panics
    /// Never for states built by this accumulator. The internal state is always valid.
    #[must_use]
    pub fn to_sketch(&self) -> Option<HyperLogLogPlusPlus> {
        if let Some(f) = &self.fallback {
            return Some((**f).clone());
        }
        if !self.started {
            return None;
        }
        if !self.pending.is_empty() {
            return self.resolved().to_sketch();
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
            return Some((**f).clone().serialize());
        }
        if !self.started {
            return None;
        }
        if !self.pending.is_empty() {
            return self.resolved().serialize();
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
            sparse_size: self.current_sparse_size(),
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
        self.pending.clear();
        self.pending_lens.clear();
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

    /// Runs `f` with the per-thread scratch buffers moved into `self`, then moves them back.
    fn with_scratch<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        SCRATCH.with(|cell| {
            let mut scratch = cell.take();
            std::mem::swap(&mut self.incoming, &mut scratch.incoming);
            std::mem::swap(&mut self.merged, &mut scratch.merged);
            std::mem::swap(&mut self.sorted, &mut scratch.sorted);
            std::mem::swap(&mut self.lowered, &mut scratch.lowered);
            let result = f(self);
            std::mem::swap(&mut self.incoming, &mut scratch.incoming);
            std::mem::swap(&mut self.merged, &mut scratch.merged);
            std::mem::swap(&mut self.sorted, &mut scratch.sorted);
            std::mem::swap(&mut self.lowered, &mut scratch.lowered);
            cell.set(scratch);
            result
        })
    }

    /// Decodes and validates the input's sparse stream into `incoming`, before any state change.
    fn decode_incoming(&mut self, view: &StateView<'_>, trusted: bool) -> Result<()> {
        self.incoming.clear();
        if let Some(sd) = view.sparse_data.filter(|d| !d.is_empty()) {
            if trusted {
                encoding::decode_unvalidated_into(sd, &mut self.incoming)?;
            } else {
                let enc = Sparse::new(view.precision, view.sparse_precision);
                encoding::decode_validated_into(sd, enc, &mut self.incoming)?;
            }
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
        self.pending.clear();
        self.pending_lens.clear();
        if let Some(data) = view.data.filter(|d| !d.is_empty()) {
            self.dense = true;
            self.registers.clear();
            self.registers.extend_from_slice(data);
        } else {
            self.dense = false;
            self.sparse.clear();
            self.sparse.extend_from_slice(&self.incoming);
            self.sparse_bytes = view.sparse_data.map_or(0, <[u8]>::len);
        }
    }

    /// Java `SparseRepresentation.normalize`, with deferred values merged first.
    fn densify(&mut self) {
        self.resolve_inner();
        if !self.dense {
            self.densify_now();
        }
    }

    /// `normalize` of the union as it is.
    fn densify_now(&mut self) {
        debug_assert!(self.pending.is_empty());
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

    /// Java sparse + sparse merge of the values in `incoming`. They are deferred (appended to
    /// `pending`) while that stays under the cap, and merged later all at once by `settle`, which
    /// decides exactly whether any of the merges normalized. A merge pass costs the size of the
    /// union, so merging per input costs far more than per batch of inputs.
    fn merge_sparse(&mut self) {
        let n = self.incoming.len();
        if self.pending.len() + n <= self.pending_cap() {
            self.pending.extend_from_slice(&self.incoming);
            self.pending_lens.push(n as u32);
            return;
        }
        self.resolve_inner();
        if self.dense {
            let enc = self.encoding();
            for &v in &self.incoming {
                set_register(&mut self.registers, enc, v);
            }
        } else if n <= self.pending_cap() {
            self.pending.extend_from_slice(&self.incoming);
            self.pending_lens.push(n as u32);
        } else {
            let values = std::mem::take(&mut self.incoming);
            self.merge_step_exact(&values);
            self.incoming = values;
        }
    }

    fn build_state(&self) -> State {
        debug_assert!(self.pending.is_empty());
        State {
            r#type: HYPERLOGLOG_PLUS_UNIQUE,
            num_values: self.num_values,
            encoding_version: hll::ENCODING_VERSION,
            value_type: self.value_type,
            sparse_size: self.current_sparse_size(),
            precision: self.precision,
            sparse_precision: self.sparse_precision,
            data: self.dense.then(|| self.registers.clone()),
            sparse_data: (!self.dense).then(|| encoding::encode_sparse(&self.sparse)),
        }
    }
}

/// Largest input inserted value by value into the sparse union instead of a full merge pass.
const MAX_INSERT: usize = 16;

/// Times `settle` replayed sketches one by one (tests check that path ran).
#[cfg(test)]
static REPLAYS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Fewest deferred sparse values an accumulator holds before merging them (see `pending_cap`).
const MAX_PENDING: usize = 2048;

/// Per-thread scratch buffers lent to accumulators (see `Accumulator::with_scratch`).
#[derive(Default)]
struct Scratch {
    incoming: Vec<u32>,
    merged: Vec<u32>,
    sorted: Vec<u32>,
    lowered: Vec<u8>,
}

thread_local! {
    static SCRATCH: std::cell::Cell<Scratch> = std::cell::Cell::new(Scratch::default());
}

/// Sorts `pending`, a concatenation of short ascending runs (each decoded input stream is
/// ascending). Equal values are identical, so an unstable sort gives the same result as a stable
/// one; it measured about twice as fast here as the run-detecting stable sort, whose runs are a
/// couple of values long.
fn sort_runs(values: &mut [u32]) {
    values.sort_unstable();
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

    /// Small sparse inputs into a dense accumulator take a scratch-free path. They must equal the
    /// reference fold (also with inputs of more than `encoding::SMALL` values, which take the
    /// general path), and a corrupt one must be rejected with the accumulator unchanged.
    #[test]
    fn small_sparse_inputs_into_dense_match_reference() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut big = HyperLogLogPlusPlus::for_longs(15).unwrap();
        for v in 0..50_000 {
            big.add_long(v).unwrap();
        }
        let big = big.serialize(); // dense
        for trusted in [false, true] {
            let mut acc = Accumulator::new();
            let mut reference = HyperLogLogPlusPlus::from_bytes(&big).unwrap();
            let add = |acc: &mut Accumulator, b: &[u8]| {
                if trusted {
                    acc.add_trusted_bytes(b)
                } else {
                    acc.add_bytes(b)
                }
            };
            add(&mut acc, &big).unwrap();
            assert!(acc.dense);
            for _ in 0..600 {
                let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
                let n = 1 + next() % 24; // 17 and more values leave the small path
                let start = (next() % 1_000_000_000) as i64;
                for v in start..start + n as i64 {
                    s.add_long(v).unwrap();
                }
                let bytes = s.serialize();
                add(&mut acc, &bytes).unwrap();
                reference.merge_bytes(&bytes).unwrap();
            }
            assert_eq!(acc.estimate(), reference.estimate());
            assert_eq!(acc.num_values(), reference.num_values());
            assert_eq!(acc.serialize().unwrap(), reference.serialize());
        }

        // A tiny input whose value is out of range for its precisions is rejected, and the
        // accumulator keeps its state.
        let mut acc = Accumulator::new();
        acc.add_bytes(&big).unwrap();
        let before = acc.serialize().unwrap();
        let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
        s.add_long(7).unwrap();
        // Well framed, one value of 2^32 - 1: out of range for precisions 15 / 20.
        let mut state = State::parse(&s.serialize()).unwrap();
        state.sparse_data = Some(vec![0xff, 0xff, 0xff, 0xff, 0x0f]);
        let bad = state.view().encode();
        assert!(StateView::parse(&bad).is_ok());
        assert!(matches!(
            acc.add_bytes(&bad),
            Err(Error::CorruptSparseData(_))
        ));
        assert_eq!(acc.serialize().unwrap(), before);
    }

    /// A xorshift generator, so the tests below need no dependency.
    fn rng(seed: u64) -> impl FnMut() -> u64 {
        let mut state = seed;
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        }
    }

    fn sketch_of(next: &mut impl FnMut() -> u64, values: u64) -> Vec<u8> {
        let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
        let start = (next() % 1_000_000_000) as i64;
        for v in start..start + values as i64 {
            s.add_long(v).unwrap();
        }
        s.serialize()
    }

    /// Small sparse inputs into a sparse accumulator take the deferral path without the scratch
    /// buffers. The state must equal the reference fold at every checkpoint, through the flushes
    /// and the switch to dense.
    #[test]
    fn small_sparse_inputs_into_sparse_match_reference() {
        for trusted in [false, true] {
            let mut next = rng(0x9e37_79b9_7f4a_7c15);
            let mut acc = Accumulator::new();
            let mut reference: Option<HyperLogLogPlusPlus> = None;
            let mut saw_sparse_after_first = false;
            let mut saw_pending = false;
            for i in 0..4000 {
                let values = 1 + next() % 20; // 17 to 20 values leave the small path
                let bytes = sketch_of(&mut next, values);
                if trusted {
                    acc.add_trusted_bytes(&bytes).unwrap();
                } else {
                    acc.add_bytes(&bytes).unwrap();
                }
                match &mut reference {
                    Some(r) => r.merge_bytes(&bytes).unwrap(),
                    None => reference = Some(HyperLogLogPlusPlus::from_bytes(&bytes).unwrap()),
                }
                saw_sparse_after_first |= i > 0 && !acc.dense;
                saw_pending |= !acc.pending.is_empty();
                if i % 97 == 0 || i == 3999 {
                    let mut want = reference.clone().unwrap();
                    assert_eq!(
                        acc.estimate(),
                        want.estimate(),
                        "input {i} trusted {trusted}"
                    );
                    assert_eq!(acc.num_values(), want.num_values(), "input {i}");
                    assert_eq!(acc.serialize().unwrap(), want.serialize(), "input {i}");
                }
            }
            assert!(saw_sparse_after_first, "the sparse phase was exercised");
            assert!(saw_pending, "inputs were deferred, so the new branch ran");
            assert!(acc.dense, "4,000 inputs end dense");
        }
    }

    /// A p = 15 INT64 sketch of `values` consecutive values from a pseudo-random start.
    fn sketch_n(next: &mut impl FnMut() -> u64, values: u64) -> Vec<u8> {
        sketch_of(next, values)
    }

    /// Feeds `inputs` to an accumulator and to the reference fold, step by step. Every step must
    /// agree on success or failure and, once something has been added, on the serialized bytes, the
    /// estimate and the value count (also right after a rejected input, which must change nothing).
    /// Returns the accumulator.
    fn fold_and_compare(inputs: &[Vec<u8>], trusted: bool, every_step: bool) -> Accumulator {
        let mut acc = Accumulator::new();
        let mut reference: Option<HyperLogLogPlusPlus> = None;
        for (i, bytes) in inputs.iter().enumerate() {
            let got = if trusted {
                acc.add_trusted_bytes(bytes)
            } else {
                acc.add_bytes(bytes)
            };
            let want = if let Some(r) = reference.as_mut() {
                r.merge_bytes(bytes)
            } else {
                HyperLogLogPlusPlus::from_bytes(bytes).map(|s| reference = Some(s))
            };
            assert_eq!(got.is_ok(), want.is_ok(), "step {i}, trusted {trusted}");
            if every_step || i + 1 == inputs.len() {
                if let Some(r) = &reference {
                    let mut want = r.clone();
                    assert_eq!(acc.estimate(), want.estimate(), "estimate, step {i}");
                    assert_eq!(acc.num_values(), want.num_values(), "count, step {i}");
                    assert_eq!(
                        acc.serialize().unwrap(),
                        want.serialize(),
                        "bytes, step {i}"
                    );
                }
            }
        }
        acc
    }

    /// The cases around the small-input path: 16 and 17 values (the limit), an input of another
    /// value type or precision arriving while the accumulator is sparse, dense and empty inputs,
    /// the 2,048-value pending cap checked at every step, and reuse after `reset`.
    #[test]
    fn small_sparse_edge_cases_match_reference() {
        let mut next = rng(0x1234_5678_9abc_def1);
        let strings = {
            let mut s = HyperLogLogPlusPlus::for_strings(15).unwrap();
            s.add_str("a string value").unwrap();
            s.serialize()
        };
        let other_precision = {
            let mut s = HyperLogLogPlusPlus::for_longs(14).unwrap();
            for v in 0..4 {
                s.add_long(v).unwrap();
            }
            s.serialize()
        };
        for trusted in [false, true] {
            // 16 values take the small path, 17 do not; a STRING sketch is rejected while sparse.
            let a = [
                sketch_n(&mut next, 5),
                sketch_n(&mut next, 16),
                sketch_n(&mut next, 17),
                strings.clone(),
                sketch_n(&mut next, 3),
                sketch_n(&mut next, 1),
            ];
            let mut acc = fold_and_compare(&a, trusted, true);
            // Reuse after reset equals a fresh accumulator.
            acc.reset();
            let again = [sketch_n(&mut next, 4), sketch_n(&mut next, 4)];
            for b in &again {
                acc.add_bytes(b).unwrap();
            }
            let fresh = fold_and_compare(&again, trusted, true);
            assert_eq!(acc.serialize(), fresh.serialize());

            // Another precision while sparse, then more small inputs.
            let b = [
                sketch_n(&mut next, 5),
                other_precision.clone(),
                sketch_n(&mut next, 3),
                sketch_n(&mut next, 2),
            ];
            fold_and_compare(&b, trusted, true);

            // A dense input first, then small ones; and small ones, a dense input, small ones.
            let c = [
                sketch_n(&mut next, 50_000),
                sketch_n(&mut next, 3),
                sketch_n(&mut next, 16),
                sketch_n(&mut next, 17),
            ];
            fold_and_compare(&c, trusted, true);
            let d = [
                sketch_n(&mut next, 2),
                sketch_n(&mut next, 50_000),
                sketch_n(&mut next, 3),
            ];
            fold_and_compare(&d, trusted, true);

            // An empty input in the middle changes nothing.
            let e = [sketch_n(&mut next, 3), Vec::new(), sketch_n(&mut next, 2)];
            fold_and_compare(&e, trusted, true);

            // Singles across the pending cap (2,048 values) and past it, compared at every step.
            let singles: Vec<Vec<u8>> = (0..2_150).map(|_| sketch_n(&mut next, 1)).collect();
            let acc = fold_and_compare(&singles, trusted, true);
            assert!(
                !acc.dense,
                "2,150 singles are still below the dense threshold"
            );
        }
    }

    /// The header and the one sparse value of a serialized one-value sparse sketch.
    fn decoded_single(bytes: &[u8]) -> (SparseHeader, u32) {
        let view = StateView::parse(bytes).unwrap();
        let values = encoding::decode_sparse(view.sparse_data.unwrap()).unwrap();
        assert_eq!(values.len(), 1, "a one-value sketch");
        let header =
            SparseHeader::new(view.value_type, view.precision, view.sparse_precision).unwrap();
        (header, values[0])
    }

    /// `add_single_values` is `add_trusted_bytes` on the serialized one-value sketch of each value:
    /// same estimate, count and bytes at checkpoints, through the sparse phase, the pending flushes
    /// and the switch to dense, with a STRING sketch (incompatible) and a precision change (the
    /// reference fallback) in the stream, one value per call and in batches.
    #[test]
    fn add_single_values_equals_the_one_value_sketches() {
        let mut next = rng(0x0bad_cafe_dead_beef);
        let strings = {
            let mut s = HyperLogLogPlusPlus::for_strings(15).unwrap();
            s.add_str("one string").unwrap();
            s.serialize()
        };
        let other_precision = {
            let mut s = HyperLogLogPlusPlus::for_longs(14).unwrap();
            s.add_long(7).unwrap();
            s.serialize()
        };
        let inputs: Vec<Vec<u8>> = (0..40_000)
            .map(|i| match i {
                1500 => strings.clone(),
                3000 | 30_000 => other_precision.clone(),
                _ => sketch_of(&mut next, 1),
            })
            .collect();
        let decoded: Vec<(SparseHeader, u32)> = inputs.iter().map(|b| decoded_single(b)).collect();

        let mut by_bytes = Accumulator::new();
        let mut one_at_a_time = Accumulator::new();
        let (mut saw_sparse, mut saw_dense) = (false, false);
        for (i, bytes) in inputs.iter().enumerate() {
            let (h, v) = decoded[i];
            let want = by_bytes.add_trusted_bytes(bytes);
            let got = one_at_a_time.add_single_values(&h, &[v]);
            assert_eq!(got.is_ok(), want.is_ok(), "input {i}");
            saw_sparse |= i > 0 && !one_at_a_time.dense && one_at_a_time.started;
            saw_dense |= one_at_a_time.dense;
            if i % 997 == 0 || i == inputs.len() - 1 {
                assert_eq!(
                    one_at_a_time.estimate(),
                    by_bytes.estimate(),
                    "estimate, input {i}"
                );
                assert_eq!(
                    one_at_a_time.num_values(),
                    by_bytes.num_values(),
                    "count, input {i}"
                );
                assert_eq!(
                    one_at_a_time.serialize(),
                    by_bytes.serialize(),
                    "bytes, input {i}"
                );
            }
        }

        assert!(saw_sparse && saw_dense, "both phases were exercised");

        // The same values in batches of equal headers.
        let mut batched = Accumulator::new();
        let mut at = 0;
        while at < decoded.len() {
            let h = decoded[at].0;
            let mut end = at + 1;
            while end < decoded.len()
                && decoded[end].0 == h
                && end - at < 1 + (next() % 400) as usize
            {
                end += 1;
            }
            let values: Vec<u32> = decoded[at..end].iter().map(|d| d.1).collect();
            let _ = batched.add_single_values(&h, &values);
            at = end;
        }
        assert_eq!(batched.estimate(), by_bytes.estimate(), "batched estimate");
        assert_eq!(batched.num_values(), by_bytes.num_values(), "batched count");
        assert_eq!(batched.serialize(), by_bytes.serialize(), "batched bytes");
    }

    /// `add_sparse_values` is `add_trusted_bytes` on the serialized sketch whose `sparse_size` and
    /// `num_values` equal the number of values: same estimate, count and bytes at checkpoints,
    /// through the sparse phase, flushes and the switch to dense, with a STRING sketch and a
    /// precision change in the stream.
    #[test]
    fn add_sparse_values_equals_the_sketches() {
        let mut next = rng(0x5eed_0000_1234_5678);
        let strings = {
            let mut s = HyperLogLogPlusPlus::for_strings(15).unwrap();
            s.add_str("one string").unwrap();
            s.serialize()
        };
        let other_precision = {
            let mut s = HyperLogLogPlusPlus::for_longs(14).unwrap();
            for v in 0..5 {
                s.add_long(v).unwrap();
            }
            s.serialize()
        };
        let mut inputs: Vec<(Vec<u8>, SparseHeader, Vec<u32>)> = Vec::new();
        while inputs.len() < 4000 {
            let i = inputs.len();
            let bytes = match i {
                700 => strings.clone(),
                1500 | 3000 => other_precision.clone(),
                _ => {
                    let n = 1 + next() % 40;
                    sketch_of(&mut next, n)
                }
            };
            let view = StateView::parse(&bytes).unwrap();
            let values = encoding::decode_sparse(view.sparse_data.unwrap()).unwrap();
            // The API sets both counters to the number of values, so keep sketches that agree
            // (a repeated sparse index would make them differ, which is rare).
            if view.num_values != values.len() as i64 || view.sparse_size != values.len() as i32 {
                continue;
            }
            let header =
                SparseHeader::new(view.value_type, view.precision, view.sparse_precision).unwrap();
            inputs.push((bytes, header, values));
        }
        let mut by_bytes = Accumulator::new();
        let mut by_values = Accumulator::new();
        let (mut saw_sparse, mut saw_dense) = (false, false);
        for (i, (bytes, header, values)) in inputs.iter().enumerate() {
            let want = by_bytes.add_trusted_bytes(bytes);
            let got = by_values.add_sparse_values(header, values);
            assert_eq!(got.is_ok(), want.is_ok(), "input {i}");
            saw_sparse |= i > 0 && !by_values.dense && by_values.started;
            saw_dense |= by_values.dense;
            if i % 101 == 0 || i == inputs.len() - 1 {
                assert_eq!(
                    by_values.estimate(),
                    by_bytes.estimate(),
                    "estimate, input {i}"
                );
                assert_eq!(
                    by_values.num_values(),
                    by_bytes.num_values(),
                    "count, input {i}"
                );
                assert_eq!(
                    by_values.serialize(),
                    by_bytes.serialize(),
                    "bytes, input {i}"
                );
            }
        }
        assert!(saw_sparse && saw_dense, "both phases were exercised");

        // The same sketches as batches of equal headers: counts plus flattened values.
        let mut batched = Accumulator::new();
        let mut at = 0;
        while at < inputs.len() {
            let h = inputs[at].1;
            let mut end = at + 1;
            while end < inputs.len() && inputs[end].1 == h && end - at < 1 + (next() % 300) as usize
            {
                end += 1;
            }
            let counts: Vec<u8> = inputs[at..end].iter().map(|i| i.2.len() as u8).collect();
            let values: Vec<u32> = inputs[at..end]
                .iter()
                .flat_map(|i| i.2.iter().copied())
                .collect();
            let _ = batched.add_sparse_batch(&h, &counts, &values);
            at = end;
        }
        assert_eq!(batched.estimate(), by_bytes.estimate(), "batched estimate");
        assert_eq!(batched.num_values(), by_bytes.num_values(), "batched count");
        assert_eq!(batched.serialize(), by_bytes.serialize(), "batched bytes");
    }

    /// A sketch of `values` pseudo-random INT64 values at precisions `p` and `sp`.
    fn sketch_at(next: &mut impl FnMut() -> u64, p: i32, sp: i32, values: u64) -> Vec<u8> {
        let mut s = HyperLogLogPlusPlus::new(p, sp, crate::hll::ValueKind::Long).unwrap();
        for _ in 0..values {
            s.add_long(next() as i64).unwrap();
        }
        s.serialize()
    }

    /// Merging is deferred and decided later (`settle`): with the reference compared only every
    /// so many inputs, so that long stretches of inputs are deferred and then settled at once, the
    /// state must equal the reference fold exactly, at small precisions where the normalization
    /// threshold is crossed after a few hundred values, for both ways of adding, and through
    /// every read: `estimate`, `cardinality`, `serialize`, `to_sketch` and `merge` into another
    /// accumulator (the `&self` ones read a resolved copy and must not change the accumulator).
    #[test]
    fn deferred_merges_match_reference_at_checkpoints() {
        let mut next = rng(0x2545_f491_4f6c_dd1d);
        // (p, sp): sp close to p makes many values share an index (collapse, rho replacement).
        for (p, sp) in [
            (10, 11),
            (10, 14),
            (10, 25),
            (12, 13),
            (12, 20),
            (15, 16),
            (15, 25),
        ] {
            for trusted in [false, true] {
                for trial in 0..24 {
                    let inputs: Vec<Vec<u8>> = (0..(40 + next() % 900))
                        .map(|_| {
                            let values = 1 + next() % 24;
                            sketch_at(&mut next, p, sp, values)
                        })
                        .collect();
                    let every = 1 + (next() % 150) as usize;
                    let mut acc = Accumulator::new();
                    let mut reference: Option<HyperLogLogPlusPlus> = None;
                    for (i, bytes) in inputs.iter().enumerate() {
                        if trusted {
                            acc.add_trusted_bytes(bytes).unwrap();
                        } else {
                            acc.add_bytes(bytes).unwrap();
                        }
                        match &mut reference {
                            Some(r) => r.merge_bytes(bytes).unwrap(),
                            None => {
                                reference = Some(HyperLogLogPlusPlus::from_bytes(bytes).unwrap());
                            }
                        }
                        if (i + 1) % every != 0 && i + 1 != inputs.len() {
                            continue;
                        }
                        let mut want = reference.clone().unwrap();
                        let ctx =
                            format!("p {p} sp {sp} trusted {trusted} trial {trial} input {i}");
                        let before = acc.pending.len();
                        // `&self` reads leave the deferred values alone.
                        assert_eq!(acc.cardinality(), want.cardinality(), "cardinality, {ctx}");
                        assert_eq!(acc.pending.len(), before, "{ctx}");
                        assert_eq!(acc.serialize().unwrap(), want.clone().serialize(), "{ctx}");
                        assert_eq!(
                            acc.to_sketch().unwrap().serialize(),
                            want.clone().serialize()
                        );
                        assert_eq!(acc.pending.len(), before, "{ctx}");
                        let mut into = Accumulator::new();
                        into.merge(&acc).unwrap();
                        assert_eq!(into.serialize().unwrap(), want.clone().serialize(), "{ctx}");
                        if (i + 1) % 2 == 0 {
                            assert_eq!(acc.estimate(), want.estimate(), "estimate, {ctx}");
                            assert!(acc.pending.is_empty(), "{ctx}");
                        }
                        assert_eq!(acc.num_values(), want.num_values(), "count, {ctx}");
                    }
                }
            }
        }
        assert!(
            REPLAYS.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "the replay path ran (a merge reached the threshold before the last, yet the last did not)"
        );
    }

    /// An input that cannot be added (another value type) while values are deferred changes
    /// nothing, and the deferred values still settle to the reference result.
    #[test]
    fn rejected_input_with_deferred_values_changes_nothing() {
        let mut next = rng(0x9e37_79b9_7f4a_7c15);
        let strings = {
            let mut s = HyperLogLogPlusPlus::for_strings(10).unwrap();
            s.add_str("x").unwrap();
            s.serialize()
        };
        let mut acc = Accumulator::new();
        let mut reference: Option<HyperLogLogPlusPlus> = None;
        for i in 0..300 {
            let values = 1 + next() % 6;
            let bytes = sketch_at(&mut next, 10, 25, values);
            acc.add_bytes(&bytes).unwrap();
            match &mut reference {
                Some(r) => r.merge_bytes(&bytes).unwrap(),
                None => reference = Some(HyperLogLogPlusPlus::from_bytes(&bytes).unwrap()),
            }
            if i % 7 == 3 && !acc.dense {
                assert!(acc.add_bytes(&strings).is_err());
                assert!(reference.clone().unwrap().merge_bytes(&strings).is_err());
            }
        }
        let mut want = reference.unwrap();
        assert_eq!(acc.estimate(), want.estimate());
        assert_eq!(acc.serialize().unwrap(), want.serialize());
    }

    /// A batch whose counts do not add up to its values is rejected before anything is added.
    #[test]
    fn add_sparse_batch_rejects_mismatched_lengths() {
        let header = SparseHeader::new(8, 15, 20).unwrap();
        let mut acc = Accumulator::new();
        assert!(acc.add_sparse_batch(&header, &[2, 1], &[1, 2]).is_err());
        assert!(acc.add_sparse_batch(&header, &[1], &[1, 2]).is_err());
        assert!(acc.is_empty(), "nothing was added");
        acc.add_sparse_batch(&header, &[2, 1], &[1, 2, 3]).unwrap();
        assert!(!acc.is_empty());
    }
}
