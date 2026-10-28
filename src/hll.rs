//! The HyperLogLog++ sketch. A faithful port of ZetaSketch Java `HyperLogLogPlusPlus`,
//! `Representation`, `NormalRepresentation` and `SparseRepresentation`: the same state
//! transitions in the same order, so that serialized bytes match the reference exactly.
//! DECISION(faithful_port_of_java_reference)

use std::collections::HashSet;

use crate::encoding::{self, Normal, Sparse};
use crate::error::{Error, Result};
use crate::estimate;
use crate::hash::fingerprint2011;
use crate::proto::{State, StateView, HYPERLOGLOG_PLUS_UNIQUE};

/// Smallest supported normal precision.
pub const MINIMUM_PRECISION: i32 = 4;
/// Largest supported normal precision.
pub const MAXIMUM_PRECISION: i32 = 24;
/// Largest supported sparse precision.
pub const MAXIMUM_SPARSE_PRECISION: i32 = 25;
/// Normal precision used by BigQuery `HLL_COUNT.INIT` when none is given.
pub const DEFAULT_NORMAL_PRECISION: i32 = 15;
/// Default distance between sparse and normal precision.
pub const DEFAULT_SPARSE_PRECISION_DELTA: i32 = 5;
/// Sparse precision value that disables the sparse representation.
pub const SPARSE_PRECISION_DISABLED: i32 = 0;

pub(crate) const ENCODING_VERSION: i32 = 2;

const VALUE_TYPE_UNKNOWN: i32 = 0;
const VALUE_TYPE_UINT32: i32 = 7;
const VALUE_TYPE_UINT64: i32 = 8;
const VALUE_TYPE_BYTES_OR_UTF8_STRING: i32 = 11;

const TYPE_LONG: u8 = 1;
const TYPE_INTEGER: u8 = 2;
const TYPE_STRING: u8 = 4;
const TYPE_BYTES: u8 = 8;
const TYPE_ALL: u8 = TYPE_LONG | TYPE_INTEGER | TYPE_STRING | TYPE_BYTES;

/// Kind of value a sketch accepts. A sketch decoded from bytes without a value type accepts any
/// kind until the first value is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueKind {
    /// 64-bit integers (BigQuery `INT64`).
    Long,
    /// 32-bit integers.
    Integer,
    /// UTF-8 strings or raw bytes (BigQuery `STRING`, `BYTES`).
    BytesOrString,
}

impl ValueKind {
    fn value_type(self) -> i32 {
        match self {
            ValueKind::Long => VALUE_TYPE_UINT64,
            ValueKind::Integer => VALUE_TYPE_UINT32,
            ValueKind::BytesOrString => VALUE_TYPE_BYTES_OR_UTF8_STRING,
        }
    }
}

fn type_value_type(t: u8) -> i32 {
    match t {
        TYPE_LONG => VALUE_TYPE_UINT64,
        TYPE_INTEGER => VALUE_TYPE_UINT32,
        _ => VALUE_TYPE_BYTES_OR_UTF8_STRING,
    }
}

fn allowed_types(value_type: i32) -> Result<u8> {
    match value_type {
        VALUE_TYPE_UNKNOWN => Ok(TYPE_ALL),
        VALUE_TYPE_UINT64 => Ok(TYPE_LONG),
        VALUE_TYPE_UINT32 => Ok(TYPE_INTEGER),
        VALUE_TYPE_BYTES_OR_UTF8_STRING => Ok(TYPE_STRING | TYPE_BYTES),
        other => Err(Error::IncompatibleType(format!(
            "unsupported value type {other}"
        ))),
    }
}

fn check_normal_precision(p: i32) -> Result<()> {
    if (MINIMUM_PRECISION..=MAXIMUM_PRECISION).contains(&p) {
        Ok(())
    } else {
        Err(Error::InvalidState(format!(
            "normal precision must be in [{MINIMUM_PRECISION}, {MAXIMUM_PRECISION}] but was {p}"
        )))
    }
}

fn check_sparse_precision(p: i32, sp: i32) -> Result<()> {
    check_normal_precision(p)?;
    if p <= sp && sp <= MAXIMUM_SPARSE_PRECISION {
        Ok(())
    } else {
        Err(Error::InvalidState(format!(
            "sparse precision must be in [{p}, {MAXIMUM_SPARSE_PRECISION}] but was {sp}"
        )))
    }
}

/// Validates a parsed sketch the way the reference constructor does, plus bounds checks that
/// keep corrupt input from indexing out of range. Returns the allowed value types.
pub(crate) fn validate(view: &StateView<'_>) -> Result<u8> {
    let allowed = validate_header(view)?;
    // Sparse data is read by any later merge or normalization, so check it whenever present.
    if let Some(sd) = view.sparse_data.filter(|d| !d.is_empty()) {
        let enc = Sparse::new(view.precision, view.sparse_precision);
        encoding::decode_validated_into(sd, enc, &mut Vec::new())?;
    }
    Ok(allowed)
}

/// Everything [`validate`] checks except walking the sparse stream. When sparse data is
/// present the precision pair is checked, so `Sparse::new(p, sp)` is safe to build.
pub(crate) fn validate_header(view: &StateView<'_>) -> Result<u8> {
    if view.r#type != HYPERLOGLOG_PLUS_UNIQUE {
        return Err(Error::InvalidState(format!(
            "expected type HYPERLOGLOG_PLUS_UNIQUE but was {}",
            view.r#type
        )));
    }
    if view.encoding_version != ENCODING_VERSION {
        return Err(Error::InvalidState(format!(
            "expected encoding version {ENCODING_VERSION} but was {}",
            view.encoding_version
        )));
    }
    let allowed = allowed_types(view.value_type)?;
    if view.has_sparse_data() && view.sparse_precision == SPARSE_PRECISION_DISABLED {
        return Err(Error::InvalidState(
            "must have a sparse precision when sparse data is set".into(),
        ));
    }
    if view.has_data() || view.sparse_precision == SPARSE_PRECISION_DISABLED {
        check_normal_precision(view.precision)?;
        if let Some(data) = view.data.filter(|d| !d.is_empty()) {
            let want = 1usize << view.precision;
            if data.len() != want {
                return Err(Error::InvalidState(format!(
                    "expected data to consist of exactly {want} bytes but got {}",
                    data.len()
                )));
            }
            let max = (65 - view.precision) as u8;
            if data.iter().copied().max().unwrap_or(0) > max {
                return Err(Error::InvalidState("register value out of range".into()));
            }
        }
    } else {
        check_sparse_precision(view.precision, view.sparse_precision)?;
    }
    if view.has_sparse_data() {
        check_sparse_precision(view.precision, view.sparse_precision)?;
    }
    Ok(allowed)
}

/// HLL++ estimate over dense registers (Java `NormalRepresentation.estimate`). Sums in register
/// order on purpose: DECISION(dense_estimate_sequential_sum).
pub(crate) fn dense_estimate(data: &[u8], p: i32) -> i64 {
    let mut num_zeros = 0u32;
    let mut sum = 0.0f64;
    for &v in data {
        if v == 0 {
            num_zeros += 1;
        }
        // Exactly 1.0 / 2^v (v <= 61 after validation).
        sum += f64::from_bits((1023 - u64::from(v)) << 52);
    }
    let m = f64::from(1i32 << p);
    if num_zeros > 0 {
        let h = m * (m / f64::from(num_zeros)).ln();
        if h <= f64::from(estimate::linear_counting_threshold(p)) {
            return estimate::java_round(h);
        }
    }
    let e = estimate::alpha(p) * m * m / sum;
    estimate::java_round(e - estimate::estimate_bias(e, p))
}

/// Linear counting over the sparse buckets (Java `SparseRepresentation.estimate`).
pub(crate) fn sparse_estimate(sp: i32, sparse_size: i32) -> i64 {
    let buckets = 1i64 << sp;
    let num_zeros = buckets - i64::from(sparse_size);
    let e = buckets as f64 * (buckets as f64 / num_zeros as f64).ln();
    estimate::java_round(e)
}

/// Unsorted buffer of sparse values not yet merged into `sparse_data`.
#[derive(Debug, Clone, Default)]
struct SparseBuffer {
    values: HashSet<u32>,
}

#[derive(Debug, Clone)]
enum Mode {
    Normal,
    Sparse(SparseBuffer),
}

/// A HyperLogLog++ sketch in the ZetaSketch / BigQuery `HLL_COUNT` format.
///
/// ```
/// use zetasketch::HyperLogLogPlusPlus;
///
/// let mut a = HyperLogLogPlusPlus::for_strings(15).unwrap();
/// a.add_str("apple").unwrap();
/// a.add_str("banana").unwrap();
/// let bytes = a.serialize();
///
/// let mut b = HyperLogLogPlusPlus::from_bytes(&bytes).unwrap();
/// b.merge_bytes(&bytes).unwrap();
/// assert_eq!(b.estimate(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct HyperLogLogPlusPlus {
    state: State,
    allowed: u8,
    mode: Mode,
}

impl HyperLogLogPlusPlus {
    /// New empty sketch with the given precisions. `sparse_precision` of
    /// [`SPARSE_PRECISION_DISABLED`] starts dense.
    ///
    /// # Errors
    /// Precisions outside the supported range.
    pub fn new(normal_precision: i32, sparse_precision: i32, kind: ValueKind) -> Result<Self> {
        let state = State {
            r#type: HYPERLOGLOG_PLUS_UNIQUE,
            encoding_version: ENCODING_VERSION,
            precision: normal_precision,
            sparse_precision,
            value_type: kind.value_type(),
            ..State::default()
        };
        Self::from_state(state)
    }

    fn with_default_sparse(normal_precision: i32, kind: ValueKind) -> Result<Self> {
        let sp = (normal_precision + DEFAULT_SPARSE_PRECISION_DELTA).min(MAXIMUM_SPARSE_PRECISION);
        Self::new(normal_precision, sp, kind)
    }

    /// New sketch for strings or bytes, sparse precision `p + 5` (capped at 25).
    ///
    /// # Errors
    /// Precision outside `[4, 24]`.
    pub fn for_strings(normal_precision: i32) -> Result<Self> {
        Self::with_default_sparse(normal_precision, ValueKind::BytesOrString)
    }

    /// New sketch for 64-bit integers, sparse precision `p + 5` (capped at 25).
    ///
    /// # Errors
    /// Precision outside `[4, 24]`.
    pub fn for_longs(normal_precision: i32) -> Result<Self> {
        Self::with_default_sparse(normal_precision, ValueKind::Long)
    }

    /// New sketch for 32-bit integers, sparse precision `p + 5` (capped at 25).
    ///
    /// # Errors
    /// Precision outside `[4, 24]`.
    pub fn for_integers(normal_precision: i32) -> Result<Self> {
        Self::with_default_sparse(normal_precision, ValueKind::Integer)
    }

    /// Decodes a serialized `AggregatorStateProto` (e.g. the output of BigQuery
    /// `HLL_COUNT.INIT` or `HLL_COUNT.MERGE_PARTIAL`).
    ///
    /// # Errors
    /// Malformed proto, wrong aggregator type or encoding version, unsupported precision or value
    /// type, dense data of the wrong length.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::from_state(State::parse(bytes)?)
    }

    pub(crate) fn from_state(state: State) -> Result<Self> {
        let allowed = validate(&state.view())?;
        let mode = if state.has_data() || state.sparse_precision == SPARSE_PRECISION_DISABLED {
            Mode::Normal
        } else {
            Mode::Sparse(SparseBuffer::default())
        };
        Ok(HyperLogLogPlusPlus {
            state,
            allowed,
            mode,
        })
    }

    /// Normal (dense) precision.
    #[must_use]
    pub fn normal_precision(&self) -> i32 {
        self.state.precision
    }

    /// Sparse precision, or 0 when sparse mode is disabled.
    #[must_use]
    pub fn sparse_precision(&self) -> i32 {
        self.state.sparse_precision
    }

    /// Number of values added or merged in (not distinct).
    #[must_use]
    pub fn num_values(&self) -> i64 {
        self.state.num_values
    }

    /// True while the sketch is in its sparse representation.
    #[must_use]
    pub fn is_sparse(&self) -> bool {
        matches!(self.mode, Mode::Sparse(_))
    }

    // ---- adding values -------------------------------------------------------------------

    fn check_and_set_type(&mut self, t: u8) -> Result<()> {
        if self.allowed & t == 0 {
            return Err(Error::IncompatibleType(format!(
                "unable to add type {t:#b} to aggregator of types {:#b}",
                self.allowed
            )));
        }
        if self.allowed.count_ones() > 1 {
            self.allowed = t;
            self.state.value_type = type_value_type(t);
        }
        Ok(())
    }

    /// Adds a 64-bit integer.
    ///
    /// # Errors
    /// The sketch is typed for another value kind.
    pub fn add_long(&mut self, value: i64) -> Result<()> {
        self.check_and_set_type(TYPE_LONG)?;
        self.add_hash(fingerprint2011(&value.to_le_bytes()))
    }

    /// Adds a 32-bit integer.
    ///
    /// # Errors
    /// The sketch is typed for another value kind.
    pub fn add_int(&mut self, value: i32) -> Result<()> {
        self.check_and_set_type(TYPE_INTEGER)?;
        self.add_hash(fingerprint2011(&value.to_le_bytes()))
    }

    /// Adds a UTF-8 string.
    ///
    /// # Errors
    /// The sketch is typed for another value kind.
    pub fn add_str(&mut self, value: &str) -> Result<()> {
        self.check_and_set_type(TYPE_STRING)?;
        self.add_hash(fingerprint2011(value.as_bytes()))
    }

    /// Adds raw bytes.
    ///
    /// # Errors
    /// The sketch is typed for another value kind.
    pub fn add_bytes(&mut self, value: &[u8]) -> Result<()> {
        self.check_and_set_type(TYPE_BYTES)?;
        self.add_hash(fingerprint2011(value))
    }

    fn add_hash(&mut self, hash: u64) -> Result<()> {
        match &mut self.mode {
            Mode::Normal => {
                let enc = self.normal_encoding();
                let idx = enc.index(hash);
                let rho = enc.rho_w(hash);
                let data = self.ensure_data();
                if data[idx] < rho {
                    data[idx] = rho;
                }
            }
            Mode::Sparse(buf) => {
                let enc = Sparse::new(self.state.precision, self.state.sparse_precision);
                buf.values.insert(enc.encode_hash(hash));
                self.update_representation()?;
            }
        }
        self.state.num_values = self.state.num_values.wrapping_add(1);
        Ok(())
    }

    // ---- results -------------------------------------------------------------------------

    /// Takes `&mut self`: a sparse sketch flushes its buffer first, as Java does
    /// (DECISION(estimate_takes_mut_for_java_parity)).
    ///
    /// Cardinality estimate, identical to ZetaSketch Java `longResult()` and BigQuery
    /// `HLL_COUNT.EXTRACT`.
    pub fn estimate(&mut self) -> i64 {
        if self.is_sparse() {
            // Flushing re-encodes only; its errors cannot occur for in-memory values.
            let _ = self.flush_buffer();
            return sparse_estimate(self.state.sparse_precision, self.state.sparse_size);
        }
        if !self.state.has_data() {
            return 0;
        }
        dense_estimate(
            self.state.data.as_deref().unwrap_or(&[]),
            self.state.precision,
        )
    }

    /// The value [`estimate`](Self::estimate) would return right now, without changing the
    /// sketch, so it works through a shared reference (cached results, `Arc`, concurrent reads).
    ///
    /// `estimate` also flushes the sparse add-buffer, as Java does. That flush can change *later*
    /// behavior on interleaved add/estimate sequences
    /// (DECISION(estimate_takes_mut_for_java_parity)), never the value returned. Use
    /// `cardinality` for anything that is only read.
    #[must_use]
    pub fn cardinality(&self) -> i64 {
        match &self.mode {
            Mode::Sparse(buf) if !buf.values.is_empty() => {
                // What the flush would write: dedupe(stored + buffer). Stored data was validated.
                let merged = sorted_values(&self.state, buf)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let size = self.sparse_encoding().dedupe(&merged).len();
                sparse_estimate(self.state.sparse_precision, size as i32)
            }
            Mode::Sparse(_) => sparse_estimate(self.state.sparse_precision, self.state.sparse_size),
            Mode::Normal if !self.state.has_data() => 0,
            Mode::Normal => dense_estimate(
                self.state.data.as_deref().unwrap_or(&[]),
                self.state.precision,
            ),
        }
    }

    /// Serializes to `AggregatorStateProto` bytes, identical to ZetaSketch Java
    /// `serializeToByteArray()` and BigQuery `HLL_COUNT.MERGE_PARTIAL` output.
    pub fn serialize(&mut self) -> Vec<u8> {
        if self.is_sparse() {
            // compact(): flush, materialize an empty sparse stream, maybe normalize.
            let _ = self.flush_buffer();
            if self.state.sparse_data.is_none() {
                self.state.sparse_data = Some(Vec::new());
            }
            let _ = self.update_representation();
        }
        self.state.to_bytes()
    }

    // ---- merging -------------------------------------------------------------------------

    /// Merges a serialized sketch. Empty input is a no-op, like the reference.
    ///
    /// # Errors
    /// Malformed bytes, incompatible precision or value type.
    /// To fold many sketches, prefer [`Accumulator`](crate::Accumulator): same result, no
    /// per-sketch allocation.
    pub fn merge_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.merge(Self::from_bytes(bytes)?)
    }

    /// Merges `other` into `self`. The result has the lower of each precision.
    ///
    /// # Errors
    /// Incompatible precision (one sketch must be lower or equal on both precisions) or value
    /// type.
    pub fn merge(&mut self, other: HyperLogLogPlusPlus) -> Result<()> {
        let types = self.allowed & other.allowed;
        if types == 0 {
            return Err(Error::IncompatibleType(format!(
                "aggregator of types {:#b} is incompatible with {:#b}",
                self.allowed, other.allowed
            )));
        }
        self.allowed = types;
        let other_values = other.state.num_values;
        match (&self.mode, other.mode) {
            (Mode::Normal, Mode::Normal) => self.merge_normal_into_normal(&other.state),
            (Mode::Normal, Mode::Sparse(buf)) => {
                self.merge_sparse_into_normal(&other.state, &buf)?;
            }
            (Mode::Sparse(_), Mode::Normal) => {
                self.normalize()?;
                self.merge_normal_into_normal(&other.state);
            }
            (Mode::Sparse(_), Mode::Sparse(buf)) => {
                let enc = Sparse::new(other.state.precision, other.state.sparse_precision);
                let sorted = sorted_values(&other.state, &buf)?;
                self.add_sparse_values(enc, sorted)?;
            }
        }
        self.state.num_values = self.state.num_values.wrapping_add(other_values);
        Ok(())
    }

    // ---- normal representation -----------------------------------------------------------

    fn normal_encoding(&self) -> Normal {
        Normal {
            precision: self.state.precision,
        }
    }

    fn ensure_data(&mut self) -> &mut Vec<u8> {
        if !self.state.has_data() {
            self.state.data = Some(vec![0u8; 1 << self.state.precision]);
        }
        self.state.data.as_mut().expect("data allocated above")
    }

    /// Java `NormalRepresentation.maybeDowngrade`.
    fn normal_maybe_downgrade(&mut self, precision: i32, sparse_precision: i32) {
        if self.state.precision <= precision && self.state.sparse_precision <= sparse_precision {
            return;
        }
        if self.state.precision > precision {
            let source = self.state.data.take();
            let source_enc = self.normal_encoding();
            self.state.data = Some(vec![0u8; 1 << precision]);
            self.state.precision = precision;
            self.merge_normal_data(source.as_deref(), source_enc);
        }
        self.state.sparse_precision = self.state.sparse_precision.min(sparse_precision);
    }

    /// Java `mergeNormalDataMaybeDowngrading`: register-wise max of decoded values.
    fn merge_normal_data(&mut self, source: Option<&[u8]>, source_enc: Normal) {
        let Some(source) = source else { return };
        let target_enc = self.normal_encoding();
        let target = self.ensure_data();
        if target_enc.precision == source_enc.precision {
            for (t, &s) in target.iter_mut().zip(source) {
                if *t < s {
                    *t = s;
                }
            }
            return;
        }
        for (old_index, &old_rho) in source.iter().enumerate() {
            let new_index = source_enc.downgrade_index(old_index, target_enc);
            let new_rho = source_enc.downgrade_rho_w(old_index, old_rho, target_enc);
            if target[new_index] < new_rho {
                target[new_index] = new_rho;
            }
        }
    }

    fn merge_normal_into_normal(&mut self, other: &State) {
        self.normal_maybe_downgrade(other.precision, other.sparse_precision);
        let other_enc = Normal {
            precision: other.precision,
        };
        self.merge_normal_data(other.data.as_deref(), other_enc);
    }

    /// Java `SparseRepresentation.mergeInto(NormalRepresentation)`.
    fn merge_sparse_into_normal(&mut self, other: &State, buf: &SparseBuffer) -> Result<()> {
        let enc = Sparse::new(other.precision, other.sparse_precision);
        if other.has_sparse_data() {
            let values = encoding::decode_sparse(other.sparse_data.as_deref().unwrap_or(&[]))?;
            self.add_sparse_values(enc, Some(values))?;
        }
        let buffered: Vec<u32> = buf.values.iter().copied().collect();
        self.add_sparse_values(enc, Some(buffered))
    }

    fn normal_add_sparse_value(&mut self, source: Sparse, v: u32) {
        let target = self.normal_encoding();
        let (idx, rho) = if target.precision < source.normal_precision {
            (
                source.decode_and_downgrade_normal_index(v, target),
                source.decode_and_downgrade_normal_rho_w(v, target),
            )
        } else {
            (source.decode_normal_index(v), source.decode_normal_rho_w(v))
        };
        let data = self.ensure_data();
        if data[idx] < rho {
            data[idx] = rho;
        }
    }

    // ---- sparse representation -----------------------------------------------------------

    fn sparse_encoding(&self) -> Sparse {
        Sparse::new(self.state.precision, self.state.sparse_precision)
    }

    fn buffer(&mut self) -> &mut SparseBuffer {
        match &mut self.mode {
            Mode::Sparse(b) => b,
            Mode::Normal => unreachable!("sparse operation on a normal sketch"),
        }
    }

    fn max_sparse_data_bytes(&self) -> usize {
        ((1i64 << self.state.precision) as f32 * 0.75f32) as usize
    }

    fn max_buffer_elements(&self) -> usize {
        ((1i64 << self.state.precision) as f32 * (1.0f32 - 0.75f32)) as usize
    }

    /// Java `SparseRepresentation.updateRepresentation`.
    fn update_representation(&mut self) -> Result<()> {
        let Mode::Sparse(buf) = &self.mode else {
            return Ok(());
        };
        if buf.values.len() > self.max_buffer_elements() {
            self.flush_buffer()?;
        }
        let too_big = self
            .state
            .sparse_data
            .as_ref()
            .is_some_and(|d| d.len() >= self.max_sparse_data_bytes());
        if too_big {
            self.normalize()?;
        }
        Ok(())
    }

    fn flush_buffer(&mut self) -> Result<()> {
        let Mode::Sparse(buf) = &self.mode else {
            return Ok(());
        };
        if buf.values.is_empty() {
            return Ok(());
        }
        let enc = self.sparse_encoding();
        let merged = sorted_values(&self.state, buf)?.unwrap_or_default();
        let deduped = enc.dedupe(&merged);
        self.set_sparse(&deduped);
        Ok(())
    }

    /// Java `SparseRepresentation.set`: replace the sparse stream and clear the buffer.
    fn set_sparse(&mut self, values: &[u32]) {
        self.buffer().values.clear();
        self.state.sparse_data = Some(encoding::encode_sparse(values));
        self.state.sparse_size = values.len() as i32;
    }

    /// Java `SparseRepresentation.normalize`: switch to dense registers.
    fn normalize(&mut self) -> Result<()> {
        let Mode::Sparse(buf) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return Ok(());
        };
        check_normal_precision(self.state.precision)?;
        let snapshot = State {
            data: None,
            ..self.state.clone()
        };
        self.merge_sparse_into_normal(&snapshot, &buf)?;
        self.state.sparse_data = None;
        self.state.sparse_size = 0;
        Ok(())
    }

    /// Java `Representation.addSparseValue`, dispatched on the current representation.
    fn add_sparse_value(&mut self, enc: Sparse, v: u32) -> Result<()> {
        if !self.is_sparse() {
            self.normal_maybe_downgrade(enc.normal_precision, enc.sparse_precision);
            self.normal_add_sparse_value(enc, v);
            return Ok(());
        }
        let this = self.sparse_encoding();
        this.assert_compatible(enc)?;
        if enc.is_less_than(this) {
            self.sparse_downgrade(enc)?;
            return self.add_sparse_value(enc, v);
        }
        let value = if this.is_less_than(enc) {
            enc.downgrade(v, this)
        } else {
            v
        };
        self.buffer().values.insert(value);
        self.update_representation()
    }

    /// Java `Representation.addSparseValues`. `values` is sorted when `self` and `enc` are both
    /// sparse at the same precision (the only case in which order matters).
    fn add_sparse_values(&mut self, enc: Sparse, values: Option<Vec<u32>>) -> Result<()> {
        if !self.is_sparse() {
            self.normal_maybe_downgrade(enc.normal_precision, enc.sparse_precision);
            let Some(values) = values else { return Ok(()) };
            self.ensure_data();
            for v in values {
                self.normal_add_sparse_value(enc, v);
            }
            return Ok(());
        }
        let this = self.sparse_encoding();
        this.assert_compatible(enc)?;
        if enc.is_less_than(this) {
            self.sparse_downgrade(enc)?;
            return self.add_sparse_values(enc, values);
        }
        let Some(values) = values else { return Ok(()) };
        if this.is_less_than(enc) {
            for v in values {
                self.add_sparse_value(this, enc.downgrade(v, this))?;
            }
            return Ok(());
        }
        let Mode::Sparse(buf) = &self.mode else {
            unreachable!()
        };
        let own = sorted_values(&self.state, buf)?.unwrap_or_default();
        let merged = encoding::merge_sorted(&own, &values);
        let deduped = this.dedupe(&merged);
        self.set_sparse(&deduped);
        self.update_representation()
    }

    /// Java `SparseRepresentation.downgrade(Encoding.Sparse)`.
    fn sparse_downgrade(&mut self, enc: Sparse) -> Result<()> {
        let this = self.sparse_encoding();
        if !enc.is_less_than(this) {
            return Ok(());
        }
        let original = self.state.sparse_data.take();
        let mut buffered: Vec<u32> = self.buffer().values.drain().collect();
        buffered.sort_unstable();
        self.state.precision = this.normal_precision.min(enc.normal_precision);
        self.state.sparse_precision = this.sparse_precision.min(enc.sparse_precision);
        check_sparse_precision(self.state.precision, self.state.sparse_precision)?;
        self.mode = Mode::Sparse(SparseBuffer::default());
        if let Some(original) = original.filter(|d| !d.is_empty()) {
            for v in encoding::decode_sparse(&original)? {
                self.add_sparse_value(enc, this.downgrade(v, enc))?;
            }
        }
        // Java re-adds the buffered values as if they were already in `enc`, but they are still
        // in `this`. That throws (index out of range) or silently corrupts registers. Downgrade
        // them like the stored values instead; see docs/tech_debt/sparse_downgrade_buffer_encoding.md.
        for v in buffered {
            self.add_sparse_value(enc, this.downgrade(v, enc))?;
        }
        Ok(())
    }
}

/// Java `SparseRepresentation.sortedIterator`: stored values merged with the sorted buffer,
/// `None` when both are empty.
fn sorted_values(state: &State, buf: &SparseBuffer) -> Result<Option<Vec<u32>>> {
    let stored = if state.has_sparse_data() {
        Some(encoding::decode_sparse(
            state.sparse_data.as_deref().unwrap_or(&[]),
        )?)
    } else {
        None
    };
    let buffered = if buf.values.is_empty() {
        None
    } else {
        let mut v: Vec<u32> = buf.values.iter().copied().collect();
        v.sort_unstable();
        Some(v)
    };
    Ok(match (stored, buffered) {
        (Some(a), Some(b)) => Some(encoding::merge_sorted(&a, &b)),
        (a, b) => a.or(b),
    })
}
