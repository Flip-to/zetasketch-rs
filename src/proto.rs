//! Hand-rolled reader and writer for the two protobuf messages a sketch consists of:
//! `AggregatorStateProto` with its `HyperLogLogPlusUniqueStateProto` extension (field 112).
//!
//! Mirrors ZetaSketch Java `internal/hllplus/State.java` field by field, including which fields
//! are omitted when they hold their default, so serialization is byte-identical.

use crate::error::{Error, Result};

/// `AggregatorType.HYPERLOGLOG_PLUS_UNIQUE`.
pub(crate) const HYPERLOGLOG_PLUS_UNIQUE: i32 = 112;

const WIRE_VARINT: u32 = 0;
const WIRE_FIXED64: u32 = 1;
const WIRE_LEN: u32 = 2;
const WIRE_FIXED32: u32 = 5;

const TYPE_TAG: u32 = 1 << 3 | WIRE_VARINT;
const NUM_VALUES_TAG: u32 = 2 << 3 | WIRE_VARINT;
const ENCODING_VERSION_TAG: u32 = 3 << 3 | WIRE_VARINT;
const VALUE_TYPE_TAG: u32 = 4 << 3 | WIRE_VARINT;
const HLL_STATE_TAG: u32 = 112 << 3 | WIRE_LEN;

const SPARSE_SIZE_TAG: u32 = 2 << 3 | WIRE_VARINT;
const PRECISION_TAG: u32 = 3 << 3 | WIRE_VARINT;
const SPARSE_PRECISION_TAG: u32 = 4 << 3 | WIRE_VARINT;
const DATA_TAG: u32 = 5 << 3 | WIRE_LEN;
const SPARSE_DATA_TAG: u32 = 6 << 3 | WIRE_LEN;

const DEFAULT_ENCODING_VERSION: i32 = 1;
const DEFAULT_VALUE_TYPE: i32 = 0;

/// Fields of a serialized sketch, borrowing `data` and `sparse_data` from the input bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateView<'a> {
    pub r#type: i32,
    pub num_values: i64,
    pub encoding_version: i32,
    pub value_type: i32,
    pub sparse_size: i32,
    pub precision: i32,
    pub sparse_precision: i32,
    pub data: Option<&'a [u8]>,
    pub sparse_data: Option<&'a [u8]>,
}

impl<'a> StateView<'a> {
    /// Java `State.hasData`: present and non-empty.
    pub fn has_data(&self) -> bool {
        self.data.is_some_and(|d| !d.is_empty())
    }

    /// Java `State.hasSparseData`: present and non-empty.
    pub fn has_sparse_data(&self) -> bool {
        self.sparse_data.is_some_and(|d| !d.is_empty())
    }

    pub fn parse(bytes: &'a [u8]) -> Result<StateView<'a>> {
        let mut view = StateView {
            r#type: HYPERLOGLOG_PLUS_UNIQUE,
            num_values: 0,
            encoding_version: DEFAULT_ENCODING_VERSION,
            value_type: DEFAULT_VALUE_TYPE,
            sparse_size: 0,
            precision: 0,
            sparse_precision: 0,
            data: None,
            sparse_data: None,
        };
        let mut r = Reader { buf: bytes, pos: 0 };
        while !r.at_end() {
            let tag = r.tag()?;
            match tag {
                TYPE_TAG => view.r#type = r.varint32()?,
                NUM_VALUES_TAG => view.num_values = r.varint64()? as i64,
                ENCODING_VERSION_TAG => view.encoding_version = r.varint32()?,
                VALUE_TYPE_TAG => view.value_type = r.varint32()?,
                HLL_STATE_TAG => {
                    let nested = r.length_delimited()?;
                    view.parse_hll(nested)?;
                }
                _ => r.skip(tag)?,
            }
        }
        Ok(view)
    }

    fn parse_hll(&mut self, bytes: &'a [u8]) -> Result<()> {
        let mut r = Reader { buf: bytes, pos: 0 };
        while !r.at_end() {
            let tag = r.tag()?;
            match tag {
                SPARSE_SIZE_TAG => self.sparse_size = r.varint32()?,
                PRECISION_TAG => self.precision = r.varint32()?,
                SPARSE_PRECISION_TAG => self.sparse_precision = r.varint32()?,
                DATA_TAG => self.data = Some(r.length_delimited()?),
                SPARSE_DATA_TAG => self.sparse_data = Some(r.length_delimited()?),
                _ => r.skip(tag)?,
            }
        }
        Ok(())
    }

    pub fn to_state(self) -> State {
        State {
            r#type: self.r#type,
            num_values: self.num_values,
            encoding_version: self.encoding_version,
            value_type: self.value_type,
            sparse_size: self.sparse_size,
            precision: self.precision,
            sparse_precision: self.sparse_precision,
            data: self.data.map(<[u8]>::to_vec),
            sparse_data: self.sparse_data.map(<[u8]>::to_vec),
        }
    }
}

/// Decoded fields of a sketch. `data` holds the dense registers (one byte per register),
/// `sparse_data` the difference/varint-encoded sorted sparse values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct State {
    pub r#type: i32,
    pub num_values: i64,
    pub encoding_version: i32,
    pub value_type: i32,
    pub sparse_size: i32,
    pub precision: i32,
    pub sparse_precision: i32,
    pub data: Option<Vec<u8>>,
    pub sparse_data: Option<Vec<u8>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            r#type: HYPERLOGLOG_PLUS_UNIQUE,
            num_values: 0,
            encoding_version: DEFAULT_ENCODING_VERSION,
            value_type: DEFAULT_VALUE_TYPE,
            sparse_size: 0,
            precision: 0,
            sparse_precision: 0,
            data: None,
            sparse_data: None,
        }
    }
}

impl State {
    /// Java `State.hasData`: present and non-empty.
    pub fn has_data(&self) -> bool {
        self.data.as_ref().is_some_and(|d| !d.is_empty())
    }

    /// Java `State.hasSparseData`: present and non-empty.
    pub fn has_sparse_data(&self) -> bool {
        self.sparse_data.as_ref().is_some_and(|d| !d.is_empty())
    }

    pub fn view(&self) -> StateView<'_> {
        StateView {
            r#type: self.r#type,
            num_values: self.num_values,
            encoding_version: self.encoding_version,
            value_type: self.value_type,
            sparse_size: self.sparse_size,
            precision: self.precision,
            sparse_precision: self.sparse_precision,
            data: self.data.as_deref(),
            sparse_data: self.sparse_data.as_deref(),
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<State> {
        StateView::parse(bytes).map(StateView::to_state)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.view().encode()
    }
}

impl StateView<'_> {
    /// Java `State.writeTo`: the serialized sketch.
    pub fn encode(self) -> Vec<u8> {
        let mut hll = Vec::with_capacity(
            self.data.map_or(0, <[u8]>::len) + self.sparse_data.map_or(0, <[u8]>::len) + 32,
        );
        if self.sparse_size != 0 {
            put_varint(&mut hll, u64::from(SPARSE_SIZE_TAG));
            put_int32(&mut hll, self.sparse_size);
        }
        if self.precision != 0 {
            put_varint(&mut hll, u64::from(PRECISION_TAG));
            put_int32(&mut hll, self.precision);
        }
        if self.sparse_precision != 0 {
            put_varint(&mut hll, u64::from(SPARSE_PRECISION_TAG));
            put_int32(&mut hll, self.sparse_precision);
        }
        if let Some(data) = self.data {
            put_varint(&mut hll, u64::from(DATA_TAG));
            put_varint(&mut hll, data.len() as u64);
            hll.extend_from_slice(data);
        }
        if let Some(sparse) = self.sparse_data {
            put_varint(&mut hll, u64::from(SPARSE_DATA_TAG));
            put_varint(&mut hll, sparse.len() as u64);
            hll.extend_from_slice(sparse);
        }

        let mut out = Vec::with_capacity(hll.len() + 24);
        put_varint(&mut out, u64::from(TYPE_TAG));
        put_int32(&mut out, self.r#type);
        put_varint(&mut out, u64::from(NUM_VALUES_TAG));
        put_varint(&mut out, self.num_values as u64);
        if self.encoding_version != DEFAULT_ENCODING_VERSION {
            put_varint(&mut out, u64::from(ENCODING_VERSION_TAG));
            put_int32(&mut out, self.encoding_version);
        }
        if self.value_type != DEFAULT_VALUE_TYPE {
            put_varint(&mut out, u64::from(VALUE_TYPE_TAG));
            put_int32(&mut out, self.value_type);
        }
        put_varint(&mut out, u64::from(HLL_STATE_TAG));
        put_varint(&mut out, hll.len() as u64);
        out.extend_from_slice(&hll);
        out
    }
}

/// Protobuf `int32` encoding: negative values are sign-extended to 10 bytes.
fn put_int32(out: &mut Vec<u8>, v: i32) {
    put_varint(out, i64::from(v) as u64);
}

pub(crate) fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Encoded length of `v` as a varint.
#[inline]
pub(crate) fn varint_len(v: u32) -> usize {
    match v {
        0..=0x7f => 1,
        0x80..=0x3fff => 2,
        0x4000..=0x1f_ffff => 3,
        0x20_0000..=0x0fff_ffff => 4,
        _ => 5,
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn varint64(&mut self) -> Result<u64> {
        // Tags and most header fields are one byte.
        if let Some(&b) = self.buf.get(self.pos) {
            if b < 0x80 {
                self.pos += 1;
                return Ok(u64::from(b));
            }
        }
        let mut result = 0u64;
        for i in 0..10 {
            let b = *self
                .buf
                .get(self.pos)
                .ok_or(Error::Proto("truncated varint"))?;
            self.pos += 1;
            result |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(result);
            }
        }
        Err(Error::Proto("malformed varint"))
    }

    /// Protobuf `readInt32`/`readEnum`: a varint truncated to its low 32 bits.
    fn varint32(&mut self) -> Result<i32> {
        Ok(self.varint64()? as i32)
    }

    fn tag(&mut self) -> Result<u32> {
        let tag = self.varint64()?;
        if tag > u64::from(u32::MAX) || (tag >> 3) == 0 {
            return Err(Error::Proto("invalid tag"));
        }
        Ok(tag as u32)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or(Error::Proto("truncated field"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn length_delimited(&mut self) -> Result<&'a [u8]> {
        let len = self.varint64()?;
        let len = usize::try_from(len).map_err(|_| Error::Proto("length overflow"))?;
        self.take(len)
    }

    fn skip(&mut self, tag: u32) -> Result<()> {
        match tag & 7 {
            WIRE_VARINT => self.varint64().map(|_| ()),
            WIRE_FIXED64 => self.take(8).map(|_| ()),
            WIRE_LEN => self.length_delimited().map(|_| ()),
            WIRE_FIXED32 => self.take(4).map(|_| ()),
            _ => Err(Error::Proto("unsupported wire type")),
        }
    }
}

/// Raw fields of a `KLL_QUANTILES` sketch (aggregator type 113): see `crate::kll`.
#[derive(Debug, Clone)]
pub(crate) struct KllRaw {
    pub num_values: i64,
    pub value_type: i32,
    pub k: i64,
    min: Vec<u8>,
    max: Vec<u8>,
    /// One entry per compactor level, in order: packed doubles (field 1) and/or packed varint
    /// deltas (field 2). Empty levels still occupy their position.
    levels: Vec<(Vec<u8>, Vec<u8>)>,
}

impl KllRaw {
    pub const VALUE_TYPE_INT64: i32 = 4;
    pub const VALUE_TYPE_FLOAT64: i32 = 10;

    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    pub fn int_levels(&self) -> Result<Vec<Vec<i64>>> {
        self.levels
            .iter()
            .map(|(_, packed)| {
                let mut r = Reader {
                    buf: packed,
                    pos: 0,
                };
                let mut out = Vec::new();
                let mut acc = 0i64;
                while !r.at_end() {
                    // First delta is a protobuf int64 (two's complement), later ones positive.
                    acc = acc.wrapping_add(r.varint64()? as i64);
                    out.push(acc);
                }
                Ok(out)
            })
            .collect()
    }

    pub fn float_levels(&self) -> Result<Vec<Vec<f64>>> {
        self.levels
            .iter()
            .map(|(packed, _)| {
                if packed.len() % 8 != 0 {
                    return Err(Error::Proto("packed doubles not a multiple of 8 bytes"));
                }
                Ok(packed
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().expect("8 bytes")))
                    .collect())
            })
            .collect()
    }

    fn int_field(bytes: &[u8]) -> Result<i64> {
        let mut r = Reader { buf: bytes, pos: 0 };
        let v = r.varint64()? as i64;
        if !r.at_end() {
            return Err(Error::Proto("trailing bytes after KLL integer bound"));
        }
        Ok(v)
    }

    fn float_field(bytes: &[u8]) -> Result<f64> {
        let b: [u8; 8] = bytes
            .try_into()
            .map_err(|_| Error::Proto("KLL float bound is not 8 bytes"))?;
        Ok(f64::from_le_bytes(b))
    }

    pub fn int_min(&self) -> Result<i64> {
        Self::int_field(&self.min)
    }

    pub fn int_max(&self) -> Result<i64> {
        Self::int_field(&self.max)
    }

    pub fn float_min(&self) -> Result<f64> {
        Self::float_field(&self.min)
    }

    pub fn float_max(&self) -> Result<f64> {
        Self::float_field(&self.max)
    }
}

const KLL_TYPE: i32 = 113;
const KLL_STATE_TAG: u32 = 113 << 3 | WIRE_LEN;

/// Parses a `KLL_QUANTILES` sketch envelope and its KLL state.
pub(crate) fn parse_kll(bytes: &[u8]) -> Result<KllRaw> {
    let mut raw = KllRaw {
        num_values: 0,
        value_type: DEFAULT_VALUE_TYPE,
        k: 0,
        min: Vec::new(),
        max: Vec::new(),
        levels: Vec::new(),
    };
    let mut ty = 0;
    let mut r = Reader { buf: bytes, pos: 0 };
    while !r.at_end() {
        let tag = r.tag()?;
        match tag {
            TYPE_TAG => ty = r.varint32()?,
            NUM_VALUES_TAG => raw.num_values = r.varint64()? as i64,
            VALUE_TYPE_TAG => raw.value_type = r.varint32()?,
            KLL_STATE_TAG => {
                let mut s = Reader {
                    buf: r.length_delimited()?,
                    pos: 0,
                };
                while !s.at_end() {
                    let t = s.tag()?;
                    match t {
                        // Field 1: unexplained (about 3k); not needed to extract quantiles.
                        x if x == (1 << 3 | WIRE_VARINT) => {
                            s.varint64()?;
                        }
                        x if x == (2 << 3 | WIRE_VARINT) => raw.k = s.varint64()? as i64,
                        x if x == (3 << 3 | WIRE_LEN) => raw.min = s.length_delimited()?.to_vec(),
                        x if x == (4 << 3 | WIRE_LEN) => raw.max = s.length_delimited()?.to_vec(),
                        x if x == (5 << 3 | WIRE_LEN) => {
                            let mut c = Reader {
                                buf: s.length_delimited()?,
                                pos: 0,
                            };
                            let mut level = (Vec::new(), Vec::new());
                            while !c.at_end() {
                                let ct = c.tag()?;
                                match ct {
                                    x if x == (1 << 3 | WIRE_LEN) => {
                                        level.0 = c.length_delimited()?.to_vec();
                                    }
                                    x if x == (2 << 3 | WIRE_LEN) => {
                                        level.1 = c.length_delimited()?.to_vec();
                                    }
                                    _ => c.skip(ct)?,
                                }
                            }
                            raw.levels.push(level);
                        }
                        _ => s.skip(t)?,
                    }
                }
            }
            _ => r.skip(tag)?,
        }
    }
    if ty != KLL_TYPE {
        return Err(Error::InvalidState(format!(
            "expected aggregator type 113 (KLL_QUANTILES) but was {ty}"
        )));
    }
    Ok(raw)
}
