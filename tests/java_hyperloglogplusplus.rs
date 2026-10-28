//! Ports of the Java ZetaSketch public-API unit tests to the Rust public API:
//! `javatests/com/google/zetasketch/HyperLogLogPlusPlusTest.java`,
//! `HyperLogLogPlusPlusMergeTest.java` and `internal/hash/HashTest.java`.
//!
//! Java asserts exception messages; this crate reports `zetasketch::Error` variants with its own
//! wording, so the ports assert the variant only.

use zetasketch::hash::fingerprint2011;
use zetasketch::{
    Error, HyperLogLogPlusPlus, ValueKind, DEFAULT_NORMAL_PRECISION,
    DEFAULT_SPARSE_PRECISION_DELTA, MAXIMUM_PRECISION, MINIMUM_PRECISION,
    SPARSE_PRECISION_DISABLED,
};

// ---- proto helpers ------------------------------------------------------------------------

const HLL_TYPE: i32 = 112;
const SUM_TYPE: i32 = 100;
const VT_UINT32: i32 = 7;
const VT_UINT64: i32 = 8;
const VT_BYTES_OR_UTF8_STRING: i32 = 11;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Hll {
    sparse_size: Option<i32>,
    precision: Option<i32>,
    sparse_precision: Option<i32>,
    data: Option<Vec<u8>>,
    sparse_data: Option<Vec<u8>>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct State {
    r#type: Option<i32>,
    num_values: Option<i64>,
    encoding_version: Option<i32>,
    value_type: Option<i32>,
    hll: Option<Hll>,
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_int(out: &mut Vec<u8>, field: u32, v: i64) {
    put_varint(out, u64::from(field << 3));
    put_varint(out, v as u64);
}

fn put_len(out: &mut Vec<u8>, field: u32, b: &[u8]) {
    put_varint(out, u64::from(field << 3 | 2));
    put_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

/// Encodes in field-number order, which is what Java's proto serializer emits.
fn encode(s: &State) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(v) = s.r#type {
        put_int(&mut out, 1, i64::from(v));
    }
    if let Some(v) = s.num_values {
        put_int(&mut out, 2, v);
    }
    if let Some(v) = s.encoding_version {
        put_int(&mut out, 3, i64::from(v));
    }
    if let Some(v) = s.value_type {
        put_int(&mut out, 4, i64::from(v));
    }
    if let Some(h) = &s.hll {
        let mut b = Vec::new();
        if let Some(v) = h.sparse_size {
            put_int(&mut b, 2, i64::from(v));
        }
        if let Some(v) = h.precision {
            put_int(&mut b, 3, i64::from(v));
        }
        if let Some(v) = h.sparse_precision {
            put_int(&mut b, 4, i64::from(v));
        }
        if let Some(v) = &h.data {
            put_len(&mut b, 5, v);
        }
        if let Some(v) = &h.sparse_data {
            put_len(&mut b, 6, v);
        }
        put_len(&mut out, 112, &b);
    }
    out
}

fn read_varint(b: &[u8], i: &mut usize) -> u64 {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let byte = b[*i];
        *i += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return v;
        }
        shift += 7;
    }
}

/// Minimal parser for the fields this test file inspects (panics on anything else).
fn decode(b: &[u8]) -> State {
    fn fields(b: &[u8], mut f: impl FnMut(u32, u64, &[u8])) {
        let mut i = 0;
        while i < b.len() {
            let tag = read_varint(b, &mut i);
            let (field, wire) = ((tag >> 3) as u32, tag & 7);
            match wire {
                0 => {
                    let v = read_varint(b, &mut i);
                    f(field, v, &[]);
                }
                2 => {
                    let n = read_varint(b, &mut i) as usize;
                    f(field, 0, &b[i..i + n]);
                    i += n;
                }
                w => panic!("unexpected wire type {w}"),
            }
        }
    }
    let mut s = State::default();
    fields(b, |field, v, bytes| match field {
        1 => s.r#type = Some(v as i32),
        2 => s.num_values = Some(v as i64),
        3 => s.encoding_version = Some(v as i32),
        4 => s.value_type = Some(v as i32),
        112 => {
            let mut h = Hll::default();
            fields(bytes, |field, v, bytes| match field {
                2 => h.sparse_size = Some(v as i32),
                3 => h.precision = Some(v as i32),
                4 => h.sparse_precision = Some(v as i32),
                5 => h.data = Some(bytes.to_vec()),
                6 => h.sparse_data = Some(bytes.to_vec()),
                other => panic!("unexpected hll field {other}"),
            });
            s.hll = Some(h);
        }
        other => panic!("unexpected field {other}"),
    });
    s
}

/// HLL++ state with `encoding_version = 2`, the given value type and extension.
fn hll_state(value_type: Option<i32>, num_values: i64, hll: Option<Hll>) -> Vec<u8> {
    encode(&State {
        r#type: Some(HLL_TYPE),
        num_values: Some(num_values),
        encoding_version: Some(2),
        value_type,
        hll,
    })
}

fn precisions(p: i32, sp: Option<i32>) -> Hll {
    Hll {
        precision: Some(p),
        sparse_precision: sp,
        ..Hll::default()
    }
}

/// The `{1}` sparse sketch (p=15, sp=25, sparse_size=1, num_values=2) used throughout the Java tests.
fn one_value_sparse(value_type: Option<i32>) -> Vec<u8> {
    hll_state(
        value_type,
        2,
        Some(Hll {
            sparse_size: Some(1),
            sparse_data: Some(vec![1]),
            ..precisions(15, Some(25))
        }),
    )
}

/// Java `BYTE_OR_STRING_TYPE_STATE_PROTO`.
fn byte_or_string_type_state() -> Vec<u8> {
    hll_state(
        Some(VT_BYTES_OR_UTF8_STRING),
        0,
        Some(precisions(15, Some(25))),
    )
}

/// Java `UNKNOWN_TYPE_STATE_PROTO`.
fn unknown_type_state() -> Vec<u8> {
    hll_state(None, 0, Some(precisions(15, Some(25))))
}

/// Java's static `hllBuilder` (`sparsePrecision(25)`, default normal precision 15).
fn builder(kind: ValueKind) -> HyperLogLogPlusPlus {
    HyperLogLogPlusPlus::new(DEFAULT_NORMAL_PRECISION, 25, kind).unwrap()
}

/// `new HyperLogLogPlusPlus.Builder().noSparseMode().buildForIntegers()`.
fn no_sparse_integers(p: i32) -> HyperLogLogPlusPlus {
    HyperLogLogPlusPlus::new(p, SPARSE_PRECISION_DISABLED, ValueKind::Integer).unwrap()
}

#[allow(clippy::needless_pass_by_value)] // takes the call result directly
fn assert_incompatible_type<T: std::fmt::Debug>(r: Result<T, Error>) {
    assert!(
        matches!(r, Err(Error::IncompatibleType(_))),
        "expected IncompatibleType, got {r:?}"
    );
}

#[allow(clippy::needless_pass_by_value)] // takes the call result directly
fn assert_invalid_state<T: std::fmt::Debug>(r: Result<T, Error>) {
    assert!(
        matches!(r, Err(Error::InvalidState(_))),
        "expected InvalidState, got {r:?}"
    );
}

/// `java.util.Random`, for `HyperLogLogPlusPlusMergeTest`.
struct JavaRandom(u64);

impl JavaRandom {
    const MULT: u64 = 0x5_DEEC_E66D;
    const MASK: u64 = (1 << 48) - 1;

    fn new(seed: i64) -> Self {
        JavaRandom((seed as u64 ^ Self::MULT) & Self::MASK)
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(Self::MULT).wrapping_add(0xB)) & Self::MASK;
        (self.0 >> (48 - bits)) as i64 as i32
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        assert!(bound > 0);
        if bound & -bound == bound {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    fn next_long(&mut self) -> i64 {
        (i64::from(self.next(32)) << 32).wrapping_add(i64::from(self.next(32)))
    }
}

// ---- HyperLogLogPlusPlusTest --------------------------------------------------------------

#[test]
fn add_bytes() {
    let mut a = builder(ValueKind::BytesOrString);
    a.add_bytes(&[12]).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 1);
}

#[test]
fn add_bytes_throws_when_other_type() {
    let mut a = builder(ValueKind::Long);
    assert_incompatible_type(a.add_bytes(&[12]));
}

#[test]
fn add_bytes_to_byte_or_string_type() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&byte_or_string_type_state()).unwrap();
    a.add_bytes(&[12]).unwrap();
    assert_incompatible_type(a.add_str("foo"));
}

#[test]
fn add_bytes_to_uninitalized() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&unknown_type_state()).unwrap();
    a.add_bytes(&[12]).unwrap();
    assert_incompatible_type(a.add_long(42));
}

#[test]
fn add_integer() {
    let mut a = builder(ValueKind::Integer);
    a.add_int(1).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 1);
}

#[test]
fn add_integer_throws_when_other_type() {
    let mut a = builder(ValueKind::Long);
    assert_incompatible_type(a.add_int(1));
}

#[test]
fn add_integer_to_uninitalized() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&unknown_type_state()).unwrap();
    a.add_int(42).unwrap();
    assert_incompatible_type(a.add_long(42));
}

#[test]
fn add_long() {
    let mut a = builder(ValueKind::Long);
    a.add_long(1).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 1);
}

#[test]
fn add_long_throws_when_other_type() {
    let mut a = builder(ValueKind::Integer);
    assert_incompatible_type(a.add_long(1));
}

#[test]
fn add_long_to_uninitalized() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&unknown_type_state()).unwrap();
    a.add_long(42).unwrap();
    assert_incompatible_type(a.add_int(42));
}

// addObject_Bytes / addObject_Integer / addObject_Long: Java's generic add(Object) maps to the
// typed adders, so these port to the same calls as addBytes / addInteger / addLong.
#[test]
fn add_object_bytes() {
    let mut a = builder(ValueKind::BytesOrString);
    a.add_bytes(&[12]).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 1);
}

#[test]
fn add_object_integer() {
    let mut a = builder(ValueKind::Integer);
    a.add_int(1).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 1);
}

#[test]
fn add_object_long() {
    let mut a = builder(ValueKind::Long);
    a.add_long(1).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 1);
}

// Not applicable: addObject_ThrowsOnWrongObject (Rust adders are statically typed; no add(Object)).

#[test]
fn add_string() {
    let mut a = builder(ValueKind::BytesOrString);
    a.add_str("foo").unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 1);
}

#[test]
fn add_string_to_byte_or_string_type() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&byte_or_string_type_state()).unwrap();
    a.add_str("foo").unwrap();
    assert_incompatible_type(a.add_bytes(&[1]));
}

#[test]
fn add_string_to_uninitalized() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&unknown_type_state()).unwrap();
    a.add_str("foo").unwrap();
    assert_incompatible_type(a.add_int(42));
}

#[test]
fn create_throws_when_precision_too_large() {
    assert_invalid_state(HyperLogLogPlusPlus::new(
        MAXIMUM_PRECISION + 1,
        25,
        ValueKind::Integer,
    ));
}

#[test]
fn create_throws_when_precision_too_small() {
    assert_invalid_state(HyperLogLogPlusPlus::new(
        MINIMUM_PRECISION - 1,
        25,
        ValueKind::Integer,
    ));
}

#[test]
fn from_proto_throws_when_no_extension() {
    assert_invalid_state(HyperLogLogPlusPlus::from_bytes(&hll_state(None, 0, None)));
}

#[test]
fn from_proto_throws_when_normal_precision_too_large() {
    let b = hll_state(Some(VT_UINT32), 0, Some(precisions(25, None)));
    assert_invalid_state(HyperLogLogPlusPlus::from_bytes(&b));
}

#[test]
fn from_proto_throws_when_normal_precision_too_small() {
    let b = hll_state(Some(VT_UINT32), 0, Some(precisions(3, None)));
    assert_invalid_state(HyperLogLogPlusPlus::from_bytes(&b));
}

#[test]
fn from_proto_throws_when_not_hyper_log_log_plus_plus() {
    let b = encode(&State {
        r#type: Some(SUM_TYPE),
        num_values: Some(0),
        ..State::default()
    });
    assert_invalid_state(HyperLogLogPlusPlus::from_bytes(&b));
}

#[test]
fn from_proto_throws_when_sparse_is_missing_sparse_precision() {
    let b = hll_state(
        Some(VT_UINT32),
        0,
        Some(Hll {
            sparse_data: Some(vec![1]),
            ..precisions(15, Some(0))
        }),
    );
    assert_invalid_state(HyperLogLogPlusPlus::from_bytes(&b));
}

#[test]
fn from_proto_throws_when_sparse_precision_too_large() {
    let b = hll_state(Some(VT_UINT32), 0, Some(precisions(15, Some(26))));
    assert_invalid_state(HyperLogLogPlusPlus::from_bytes(&b));
}

#[test]
fn from_proto_throws_when_sparse_precision_too_small() {
    let b = hll_state(Some(VT_UINT32), 0, Some(precisions(15, Some(14))));
    assert_invalid_state(HyperLogLogPlusPlus::from_bytes(&b));
}

#[test]
fn from_proto_when_normal() {
    let b = hll_state(
        Some(VT_UINT32),
        1,
        Some(Hll {
            data: Some(vec![0; 1 << 15]),
            ..precisions(15, None)
        }),
    );
    let mut a = HyperLogLogPlusPlus::from_bytes(&b).unwrap();
    assert_eq!(a.estimate(), 0);
    assert_eq!(a.num_values(), 1);
}

#[test]
fn from_proto_when_sparse() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&one_value_sparse(Some(VT_UINT32))).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 2);
}

// fromProtoByteString / fromProtoBytes: ByteString and byte[] overloads both map to from_bytes.
#[test]
fn from_proto_byte_string() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&one_value_sparse(Some(VT_UINT32))).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 2);
}

#[test]
fn from_proto_byte_string_throws_when_invalid() {
    let r = HyperLogLogPlusPlus::from_bytes(&[1]);
    assert!(matches!(r, Err(Error::Proto(_))), "got {r:?}");
}

#[test]
fn from_proto_bytes() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&one_value_sparse(Some(VT_UINT32))).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 2);
}

#[test]
fn from_proto_bytes_throws_when_invalid() {
    let r = HyperLogLogPlusPlus::from_bytes(&[1]);
    assert!(matches!(r, Err(Error::Proto(_))), "got {r:?}");
}

#[test]
fn long_result() {
    let mut a = builder(ValueKind::Integer);
    for v in [1, 2, 3, 2, 3] {
        a.add_int(v).unwrap();
    }
    assert_eq!(a.estimate(), 3);
}

#[test]
fn long_result_zero_when_empty() {
    let mut a = builder(ValueKind::Integer);
    assert_eq!(a.estimate(), 0);
}

#[test]
fn merge_from_proto() {
    let mut a = builder(ValueKind::Integer);
    a.merge_bytes(&one_value_sparse(Some(VT_UINT32))).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 2);
}

#[test]
fn merge_incompatible_types() {
    let mut a = builder(ValueKind::Integer);
    let b = HyperLogLogPlusPlus::from_bytes(&one_value_sparse(Some(VT_UINT64))).unwrap();
    assert_incompatible_type(a.merge(b));
}

#[test]
fn merge_known_into_unknown_type() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&unknown_type_state()).unwrap();
    let b = builder(ValueKind::Long);
    a.merge(b).unwrap();
    // The type has been fixed.
    assert_incompatible_type(a.add_int(12));
}

fn check_merge_counts(
    mut a: HyperLogLogPlusPlus,
    mut b: HyperLogLogPlusPlus,
) -> HyperLogLogPlusPlus {
    for v in [1, 2, 3] {
        a.add_int(v).unwrap();
    }
    for v in [3, 4] {
        b.add_int(v).unwrap();
    }
    a.merge(b.clone()).unwrap();
    assert_eq!(a.estimate(), 4);
    assert_eq!(a.num_values(), 5);
    assert_eq!(b.estimate(), 2);
    assert_eq!(b.num_values(), 2);
    a
}

#[test]
fn merge_normal_into_normal() {
    check_merge_counts(
        no_sparse_integers(DEFAULT_NORMAL_PRECISION),
        no_sparse_integers(DEFAULT_NORMAL_PRECISION),
    );
}

#[test]
fn merge_normal_into_normal_with_higher_precision() {
    let a = check_merge_counts(
        no_sparse_integers(DEFAULT_NORMAL_PRECISION),
        no_sparse_integers(13),
    );
    assert_eq!(a.normal_precision(), 13);
    assert_eq!(a.sparse_precision(), 0);
}

#[test]
fn merge_normal_into_normal_with_lower_precision() {
    let a = check_merge_counts(
        no_sparse_integers(13),
        no_sparse_integers(DEFAULT_NORMAL_PRECISION),
    );
    assert_eq!(a.normal_precision(), 13);
    assert_eq!(a.sparse_precision(), 0);
}

#[test]
fn merge_normal_into_sparse() {
    check_merge_counts(
        builder(ValueKind::Integer),
        no_sparse_integers(DEFAULT_NORMAL_PRECISION),
    );
}

#[test]
fn merge_sparse_into_normal() {
    check_merge_counts(
        no_sparse_integers(DEFAULT_NORMAL_PRECISION),
        builder(ValueKind::Integer),
    );
}

#[test]
fn merge_sparse_into_sparse() {
    check_merge_counts(builder(ValueKind::Integer), builder(ValueKind::Integer));
}

#[test]
fn merge_from_bytes() {
    let mut a = builder(ValueKind::Integer);
    a.merge_bytes(&one_value_sparse(Some(VT_UINT32))).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 2);
}

#[test]
fn merge_from_bytes_does_nothing_when_empty() {
    let mut a = builder(ValueKind::Integer);
    a.merge_bytes(&[]).unwrap();
    assert_eq!(a.estimate(), 0);
    assert_eq!(a.num_values(), 0);
}

// Not applicable: mergeFromBytes_DoesNothingWhenNull (no null slices in Rust; empty is covered above).

#[test]
fn merge_from_bytes_throws_when_invalid() {
    let mut a = builder(ValueKind::Integer);
    let r = a.merge_bytes(&[1]);
    assert!(matches!(r, Err(Error::Proto(_))), "got {r:?}");
}

// mergeFromByteString*: the ByteString overloads map to merge_bytes.
#[test]
fn merge_from_byte_string() {
    let mut a = builder(ValueKind::Integer);
    a.merge_bytes(&one_value_sparse(Some(VT_UINT32))).unwrap();
    assert_eq!(a.estimate(), 1);
    assert_eq!(a.num_values(), 2);
}

#[test]
fn merge_from_byte_string_does_nothing_when_empty() {
    let mut a = builder(ValueKind::Integer);
    a.merge_bytes(&[]).unwrap();
    assert_eq!(a.estimate(), 0);
    assert_eq!(a.num_values(), 0);
}

// Not applicable: mergeFromByteString_DoesNothingWhenNull (no null slices in Rust).

#[test]
fn merge_from_byte_string_throws_when_invalid() {
    let mut a = builder(ValueKind::Integer);
    let r = a.merge_bytes(&[1]);
    assert!(matches!(r, Err(Error::Proto(_))), "got {r:?}");
}

#[test]
fn merge_from_proto_known_into_unknown_type() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&unknown_type_state()).unwrap();
    a.merge_bytes(&hll_state(
        Some(VT_UINT64),
        0,
        Some(precisions(15, Some(25))),
    ))
    .unwrap();
    assert_incompatible_type(a.add_int(12));
}

#[test]
fn merge_from_proto_throws_when_incompatible_type() {
    let mut a = builder(ValueKind::Integer);
    assert_incompatible_type(a.merge_bytes(&one_value_sparse(Some(VT_UINT64))));
}

#[test]
fn merge_from_proto_unknown_into_known_type() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&hll_state(
        Some(VT_UINT64),
        0,
        Some(precisions(15, Some(25))),
    ))
    .unwrap();
    a.merge_bytes(&unknown_type_state()).unwrap();
    assert_incompatible_type(a.add_int(12));
}

#[test]
fn merge_from_proto_unknown_into_unknown_type() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&unknown_type_state()).unwrap();
    a.merge_bytes(&unknown_type_state()).unwrap();
    // The type is still open.
    a.add_int(12).unwrap();
}

#[test]
fn num_values() {
    let mut a = builder(ValueKind::Integer);
    for v in [1, 2, 3, 2, 3] {
        a.add_int(v).unwrap();
    }
    assert_eq!(a.num_values(), 5);
}

#[test]
fn num_values_zero_when_empty() {
    let a = builder(ValueKind::Integer);
    assert_eq!(a.num_values(), 0);
}

/// Shared body of serializeToBytes / serializeToByteString / serializeToProto: all three Java
/// serializers map to `serialize`. Java ignores `sparse_data` content and field absence.
fn check_serialized_three_integers() {
    let mut a = builder(ValueKind::Integer);
    for v in [1, 2, 3] {
        a.add_int(v).unwrap();
    }
    let actual = decode(&a.serialize());
    let hll = actual.hll.clone().expect("hll extension");
    assert!(hll.sparse_data.is_some());
    assert!(hll.data.as_deref().unwrap_or_default().is_empty());
    assert_eq!(
        actual,
        State {
            r#type: Some(HLL_TYPE),
            num_values: Some(3),
            encoding_version: Some(2),
            value_type: Some(VT_UINT32),
            hll: Some(Hll {
                sparse_size: Some(3),
                data: hll.data.clone(),
                sparse_data: hll.sparse_data.clone(),
                ..precisions(15, Some(25))
            }),
        }
    );
}

#[test]
fn serialize_to_bytes() {
    check_serialized_three_integers();
}

#[test]
fn serialize_to_byte_string() {
    check_serialized_three_integers();
}

#[test]
fn serialize_to_proto() {
    check_serialized_three_integers();
}

#[test]
fn serialize_to_proto_empty_aggregator_sets_empty_sparse_data_field() {
    let mut a = HyperLogLogPlusPlus::new(13, 16, ValueKind::BytesOrString).unwrap();
    let hll = decode(&a.serialize()).hll.expect("hll extension");
    assert_eq!(hll.sparse_data, Some(vec![]));
}

#[test]
fn serialize_to_proto_type_when_bytes_and_string_type() {
    let mut a = HyperLogLogPlusPlus::from_bytes(&byte_or_string_type_state()).unwrap();
    assert_eq!(
        decode(&a.serialize()).value_type,
        Some(VT_BYTES_OR_UTF8_STRING)
    );
}

#[test]
fn lowest_precision_basic_ops_normal_only() {
    let mut a = HyperLogLogPlusPlus::new(4, SPARSE_PRECISION_DISABLED, ValueKind::Long).unwrap();
    assert_eq!(a.normal_precision(), 4);
    assert_eq!(a.sparse_precision(), 0);
    a.add_long(42).unwrap();
    assert_eq!(a.estimate(), 1);
    let bytes = a.serialize();
    let mut data = vec![0u8; 16];
    data[5] = 1;
    let expected = hll_state(
        Some(VT_UINT64),
        1,
        Some(Hll {
            data: Some(data),
            ..precisions(4, None)
        }),
    );
    assert_eq!(bytes, expected);
    let mut from = HyperLogLogPlusPlus::from_bytes(&bytes).unwrap();
    assert_eq!(from.normal_precision(), 4);
    assert_eq!(from.sparse_precision(), 0);
    assert_eq!(from.estimate(), 1);
}

#[test]
fn lowest_precision_basic_ops_with_sparse() {
    let mut a = HyperLogLogPlusPlus::new(4, 4, ValueKind::Long).unwrap();
    assert_eq!(a.normal_precision(), 4);
    assert_eq!(a.sparse_precision(), 4);
    a.add_long(42).unwrap();
    assert_eq!(a.estimate(), 1);
    let bytes = a.serialize();
    let expected = hll_state(
        Some(VT_UINT64),
        1,
        Some(Hll {
            sparse_size: Some(1),
            sparse_data: Some(vec![0xC1, 0x0A]),
            ..precisions(4, Some(4))
        }),
    );
    assert_eq!(bytes, expected);
    let mut from = HyperLogLogPlusPlus::from_bytes(&bytes).unwrap();
    assert_eq!(from.normal_precision(), 4);
    assert_eq!(from.sparse_precision(), 4);
    assert_eq!(from.estimate(), 1);
}

// Builder tests map to the `for_*` constructors (default sparse precision) and `new`.
#[test]
fn builder_uses_both_precision_defaults_when_unspecified() {
    let a = HyperLogLogPlusPlus::for_strings(DEFAULT_NORMAL_PRECISION).unwrap();
    assert_eq!(a.normal_precision(), DEFAULT_NORMAL_PRECISION);
    assert_eq!(
        a.sparse_precision(),
        DEFAULT_NORMAL_PRECISION + DEFAULT_SPARSE_PRECISION_DELTA
    );
}

#[test]
fn builder_uses_normal_precision_default_when_unspecified() {
    let a = HyperLogLogPlusPlus::new(DEFAULT_NORMAL_PRECISION, 18, ValueKind::Integer).unwrap();
    assert_eq!(a.normal_precision(), DEFAULT_NORMAL_PRECISION);
    assert_eq!(a.sparse_precision(), 18);
}

#[test]
fn builder_uses_sparse_precision_default_when_unspecified() {
    let a = HyperLogLogPlusPlus::for_longs(18).unwrap();
    assert_eq!(a.normal_precision(), 18);
    assert_eq!(a.sparse_precision(), 18 + DEFAULT_SPARSE_PRECISION_DELTA);
}

#[test]
fn builder_uses_both_precisions_as_specified() {
    let a = HyperLogLogPlusPlus::new(14, 17, ValueKind::BytesOrString).unwrap();
    assert_eq!(a.normal_precision(), 14);
    assert_eq!(a.sparse_precision(), 17);
}

// Not applicable: builder_invocationOrderDoesNotMatter (no builder; `new` takes positional args).

#[test]
fn builder_no_sparse_mode() {
    let a =
        HyperLogLogPlusPlus::new(16, SPARSE_PRECISION_DISABLED, ValueKind::BytesOrString).unwrap();
    assert_eq!(a.sparse_precision(), SPARSE_PRECISION_DISABLED);
    assert_eq!(a.normal_precision(), 16);
}

// builder_sparsePrecision_setMultipleTimes: no mutable builder; mapped to the three resulting
// constructor calls.
#[test]
fn builder_sparse_precision_set_multiple_times() {
    let a =
        HyperLogLogPlusPlus::new(16, SPARSE_PRECISION_DISABLED, ValueKind::BytesOrString).unwrap();
    assert_eq!(a.sparse_precision(), SPARSE_PRECISION_DISABLED);
    assert_eq!(a.normal_precision(), 16);
    let b = HyperLogLogPlusPlus::new(16, 20, ValueKind::BytesOrString).unwrap();
    assert_eq!(b.sparse_precision(), 20);
    assert_eq!(b.normal_precision(), 16);
    let c = HyperLogLogPlusPlus::new(16, SPARSE_PRECISION_DISABLED, ValueKind::Long).unwrap();
    assert_eq!(c.sparse_precision(), SPARSE_PRECISION_DISABLED);
    assert_eq!(c.normal_precision(), 16);
}

#[test]
fn builder_default_sparse_precision_with_high_normal_precision_caps_sparse_precision() {
    let a = HyperLogLogPlusPlus::for_strings(22).unwrap();
    assert_eq!(a.sparse_precision(), 25);
    assert_eq!(a.normal_precision(), 22);
}

// builder_reuse: no reusable builder; mapped to the equivalent constructor calls.
#[test]
fn builder_reuse() {
    let mut bytes = HyperLogLogPlusPlus::new(13, 16, ValueKind::BytesOrString).unwrap();
    bytes.add_bytes(&[12]).unwrap();
    assert_eq!(bytes.estimate(), 1);
    assert_eq!(bytes.num_values(), 1);
    assert_eq!(bytes.normal_precision(), 13);
    assert_eq!(bytes.sparse_precision(), 16);

    let mut longs = HyperLogLogPlusPlus::new(13, 16, ValueKind::Long).unwrap();
    longs.add_long(1).unwrap();
    assert_eq!(longs.estimate(), 1);
    assert_eq!(longs.num_values(), 1);
    assert_eq!(longs.normal_precision(), 13);
    assert_eq!(longs.sparse_precision(), 16);

    let mut strings = HyperLogLogPlusPlus::new(18, 20, ValueKind::BytesOrString).unwrap();
    strings.add_str("foo").unwrap();
    assert_eq!(strings.estimate(), 1);
    assert_eq!(strings.num_values(), 1);
    assert_eq!(strings.normal_precision(), 18);
    assert_eq!(strings.sparse_precision(), 20);
}

// ---- HyperLogLogPlusPlusMergeTest ---------------------------------------------------------

#[test]
fn merge_multiple_sparse_representations_into_a_normal_one() {
    let normal_precision = 13;
    let build = || HyperLogLogPlusPlus::new(normal_precision, 16, ValueKind::Long).unwrap();
    let mut random = JavaRandom::new(123);
    let mut protos = Vec::new();
    let mut overall = build();
    for _ in 0..100 {
        let num_values = random.next_int((1 << normal_precision) / 2) + 1;
        let mut agg = build();
        for _ in 0..num_values {
            let value = random.next_long();
            agg.add_long(value).unwrap();
            overall.add_long(value).unwrap();
        }
        let proto = agg.serialize();
        assert!(decode(&proto).hll.unwrap().sparse_data.is_some());
        protos.push(proto);
    }
    let expected = overall.serialize();
    assert!(decode(&expected).hll.unwrap().sparse_data.is_none());

    let mut agg = HyperLogLogPlusPlus::from_bytes(&protos[0]).unwrap();
    for p in &protos[1..] {
        agg.merge_bytes(p).unwrap();
    }
    assert_eq!(agg.serialize(), expected);
}

// ---- internal/hash/HashTest ---------------------------------------------------------------

#[test]
fn of_bytes() {
    assert_eq!(fingerprint2011(&[]), 0x23ad_7c90_4aa6_65e3);
    assert_eq!(
        fingerprint2011(&[0x66, 0x6f, 0x6f, 0x62, 0x61, 0x72]),
        0x36a1_e57a_138e_4467
    );
}

#[test]
fn of_int() {
    // Guava hashInt hashes the 4 little-endian bytes.
    assert_eq!(fingerprint2011(&0i32.to_le_bytes()), 0x1f6e_43ff_4b52_70ee);
    assert_eq!(fingerprint2011(&42i32.to_le_bytes()), 0x5cbd_ed94_3bff_ddd3);
    assert_eq!(
        fingerprint2011(&(-15i32).to_le_bytes()),
        0xfd5a_96b7_6694_22c1
    );
}

#[test]
fn of_long() {
    assert_eq!(fingerprint2011(&0i64.to_le_bytes()), 0x853a_22bd_6e14_a48f);
    assert_eq!(fingerprint2011(&42i64.to_le_bytes()), 0x583b_2b9d_f8e0_eb60);
    assert_eq!(
        fingerprint2011(&(-15i64).to_le_bytes()),
        0x5394_14f2_87f1_1e37
    );
}

#[test]
fn of_string() {
    assert_eq!(fingerprint2011("".as_bytes()), 0x23ad_7c90_4aa6_65e3);
    assert_eq!(fingerprint2011("foo".as_bytes()), 0xd0bc_bfe2_61b3_6504);
    assert_eq!(
        fingerprint2011("Z\u{fc}rich".as_bytes()),
        0x27ef_c00f_7d2c_e548
    );
    // Unicode strings must not be normalized.
    assert_eq!(
        fingerprint2011("Zu\u{308}rich".as_bytes()),
        0x7dfa_3067_e55c_7e8a
    );
}
