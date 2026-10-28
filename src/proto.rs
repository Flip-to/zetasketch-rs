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

    pub fn parse(bytes: &[u8]) -> Result<State> {
        let mut state = State::default();
        let mut r = Reader { buf: bytes, pos: 0 };
        while !r.at_end() {
            let tag = r.tag()?;
            match tag {
                TYPE_TAG => state.r#type = r.varint32()?,
                NUM_VALUES_TAG => state.num_values = r.varint64()? as i64,
                ENCODING_VERSION_TAG => state.encoding_version = r.varint32()?,
                VALUE_TYPE_TAG => state.value_type = r.varint32()?,
                HLL_STATE_TAG => {
                    let nested = r.length_delimited()?;
                    state.parse_hll(nested)?;
                }
                _ => r.skip(tag)?,
            }
        }
        Ok(state)
    }

    fn parse_hll(&mut self, bytes: &[u8]) -> Result<()> {
        let mut r = Reader { buf: bytes, pos: 0 };
        while !r.at_end() {
            let tag = r.tag()?;
            match tag {
                SPARSE_SIZE_TAG => self.sparse_size = r.varint32()?,
                PRECISION_TAG => self.precision = r.varint32()?,
                SPARSE_PRECISION_TAG => self.sparse_precision = r.varint32()?,
                DATA_TAG => self.data = Some(r.length_delimited()?.to_vec()),
                SPARSE_DATA_TAG => self.sparse_data = Some(r.length_delimited()?.to_vec()),
                _ => r.skip(tag)?,
            }
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut hll = Vec::new();
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
        if let Some(data) = &self.data {
            put_varint(&mut hll, u64::from(DATA_TAG));
            put_varint(&mut hll, data.len() as u64);
            hll.extend_from_slice(data);
        }
        if let Some(sparse) = &self.sparse_data {
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

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn varint64(&mut self) -> Result<u64> {
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
