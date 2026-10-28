//! Ported from Java ZetaSketch `javatests/.../internal/` (HEAD d098c49): unit tests of internal
//! modules. Every assertion this crate deliberately breaks is a named test documenting the
//! difference.

#![allow(clippy::unreadable_literal)]
// Bit patterns are written out as in Java, including `0 << n` index fields.
#![allow(clippy::identity_op)]
// Java's bit patterns are kept verbatim (decimal operands in `|`); the lint is newer than
// some toolchains, hence `unknown_lints`.
#![allow(unknown_lints, clippy::decimal_bitwise_operands)]

// Not applicable: ByteSliceTest (all tests): Java buffer class with no Rust counterpart (Rust
// uses `&[u8]` / `Vec<u8>`).
// Not applicable: GrowingByteSliceTest (all tests): Java buffer class with no Rust counterpart.

/// Varint encoding of each value, concatenated (Java test helper `varints`).
fn varints(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &v in values {
        crate::proto::put_varint(&mut out, u64::from(v));
    }
    out
}

mod difference_encoder_test {
    use super::varints;
    use crate::encoding::encode_sparse;

    #[test]
    fn put_int_correctly_writes_equal_elements() {
        assert_eq!(encode_sparse(&[42, 42]), varints(&[42, 0]));
    }

    #[test]
    fn put_int_correctly_writes_multiple() {
        assert_eq!(
            encode_sparse(&[42, 170, 2903]),
            varints(&[42, 170 - 42, 2903 - 170])
        );
    }

    #[test]
    fn put_int_correctly_writes_single() {
        assert_eq!(encode_sparse(&[42]), varints(&[42]));
    }

    #[test]
    fn put_int_correctly_writes_zero() {
        assert_eq!(encode_sparse(&[0]), varints(&[0]));
    }

    // Not applicable: putInt_ThrowsWhenNegative: `encode_sparse` takes `u32`, negative values
    // cannot be expressed.

    /// Java throws `IllegalArgumentException` on unsorted input. Rust only checks with a
    /// `debug_assert!` (the encoder is internal and always fed sorted, deduped values), so in
    /// release builds the input is accepted without a check.
    #[test]
    fn put_int_throws_when_not_sorted_differs_from_java() {
        let result = std::panic::catch_unwind(|| encode_sparse(&[42, 12]));
        assert_eq!(result.is_err(), cfg!(debug_assertions));
    }
}

mod difference_decoder_test {
    use super::varints;
    use crate::encoding::SparseIter;

    // Not applicable: create_ThrowsWhenNull: Rust slices cannot be null.

    #[test]
    fn has_next_returns_false_when_empty() {
        let mut iter = SparseIter::new(&[]);
        assert_eq!(iter.next(), None);
        assert!(iter.error().is_ok());
    }

    #[test]
    fn next_decodes_integers() {
        let bytes = varints(&[42, 170 - 42, 2903 - 170, 20160531 - 2903]);
        let mut iter = SparseIter::new(&bytes);
        assert_eq!(iter.next(), Some(42));
        assert_eq!(iter.next(), Some(170));
        assert_eq!(iter.next(), Some(2903));
        assert_eq!(iter.next(), Some(20160531));
        assert_eq!(iter.next(), None);
        assert!(iter.error().is_ok());
    }

    /// Java throws `NoSuchElementException`; the Rust iterator equivalent is `None`.
    #[test]
    fn next_throws_when_empty() {
        let mut iter = SparseIter::new(&[]);
        assert_eq!(iter.next(), None);
    }
}

mod merged_int_iterator_test {
    use crate::encoding::merge_sorted;

    #[test]
    fn next_returns_values_in_sorted_order() {
        assert_eq!(
            merge_sorted(&[1, 2, 4], &[2, 3, 4, 5, 6, 7]),
            vec![1, 2, 2, 3, 4, 4, 5, 6, 7]
        );
    }
}

mod data_test {
    use crate::estimate::{alpha, estimate_bias, linear_counting_threshold};

    const TOLERANCE: f64 = 0.0001;

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= TOLERANCE,
            "expected {expected} but got {actual}"
        );
    }

    #[test]
    fn alpha_() {
        assert_near(alpha(14), 0.7213);
    }

    #[test]
    fn estimate_bias_when_exactly_defined() {
        assert_near(estimate_bias(193.8044, 6), 1.8044);
        assert_near(estimate_bias(738.1256, 10), 737.1256);
        assert_near(estimate_bias(14573.7784, 14), 9248.7784);
    }

    #[test]
    fn estimate_bias_when_interpolation_needed() {
        assert_near(estimate_bias(25.0, 5), 19.5258);
        assert_near(estimate_bias(1490.0, 11), 1456.8144);
        assert_near(estimate_bias(16300.0, 14), 8005.2257);
        assert_near(estimate_bias(653000.0, 17), -411.7805);
    }

    #[test]
    fn estimate_bias_returns_zero_when_mean_out_of_range() {
        assert_near(estimate_bias(78.0, 4), 0.0);
        assert_near(estimate_bias(738.0, 10), 0.0);
        assert_near(estimate_bias(1310000.0, 18), 0.0);
    }

    #[test]
    fn estimate_bias_returns_zero_when_precision_out_of_range() {
        // Data.MINIMUM_PRECISION = 4, Data.MAXIMUM_PRECISION = 18.
        assert_near(estimate_bias(1000.0, 3), 0.0);
        assert_near(estimate_bias(1000.0, 19), 0.0);
    }

    #[test]
    fn linear_counting_threshold_when_precisely_defined() {
        assert_eq!(linear_counting_threshold(7), 80);
        assert_eq!(linear_counting_threshold(14), 11500);
    }

    #[test]
    fn linear_counting_threshold_when_computed() {
        assert_eq!(linear_counting_threshold(19), 1310720);
    }
}

mod encoding_normal_test {
    use crate::encoding::Normal;

    #[test]
    fn index() {
        let encoding = Normal { precision: 5 };
        assert_eq!(encoding.index(0b101110001u64 << 55), 0b10111);
    }

    #[test]
    fn rho_w() {
        let encoding = Normal { precision: 5 };
        // Number of leading zero bits after the index (= the first 5 bits) is 3, rhoW is 4.
        assert_eq!(encoding.rho_w(0b101110001u64 << 55), 4);
    }

    #[test]
    fn downgrade_index() {
        let source = Normal { precision: 5 };
        let target = Normal { precision: 3 };
        assert_eq!(source.downgrade_index(0b10111, target), 0b101);
    }

    #[test]
    fn downgrade_rho_w_none() {
        let source = Normal { precision: 5 };
        let target = Normal { precision: 3 };
        assert_eq!(source.downgrade_rho_w(0b10001, 0, target), 0);
    }

    #[test]
    fn downgrade_rho_w_non_zero() {
        let source = Normal { precision: 5 };
        let target = Normal { precision: 3 };
        assert_eq!(source.downgrade_rho_w(0b10001, 4, target), 2);
    }

    #[test]
    fn downgrade_rho_w_zero() {
        let source = Normal { precision: 5 };
        let target = Normal { precision: 3 };
        assert_eq!(source.downgrade_rho_w(0b10000, 4, target), 6);
    }
}

mod encoding_sparse_test {
    use crate::encoding::{Normal, Sparse};
    use crate::error::Error;

    #[test]
    fn assert_compatible_matching_precisions() {
        let a = Sparse::new(6, 11);
        let b = Sparse::new(6, 11);
        a.assert_compatible(b).unwrap();
    }

    #[test]
    fn assert_compatible_downgrade_normal_precision() {
        let a = Sparse::new(6, 11);
        let b = Sparse::new(7, 11);
        a.assert_compatible(b).unwrap();
        b.assert_compatible(a).unwrap();
    }

    #[test]
    fn assert_compatible_downgrade_sparse_precision() {
        let a = Sparse::new(6, 11);
        let b = Sparse::new(6, 12);
        a.assert_compatible(b).unwrap();
        b.assert_compatible(a).unwrap();
    }

    /// Same check; the Rust message starts with a lower-case "precisions" (Rust error style), so
    /// the comparison ignores case.
    #[test]
    fn assert_compatible_incompatible_downgrade() {
        let a = Sparse::new(7, 11);
        let b = Sparse::new(6, 12);

        let e = a.assert_compatible(b).unwrap_err();
        assert_eq!(
            e,
            Error::IncompatiblePrecision {
                this: (7, 11),
                other: (6, 12)
            }
        );
        assert!(e
            .to_string()
            .to_lowercase()
            .contains("precisions (p=7, sp=11) are not compatible to (p=6, sp=12)"));

        let e = b.assert_compatible(a).unwrap_err();
        assert!(e
            .to_string()
            .to_lowercase()
            .contains("precisions (p=6, sp=12) are not compatible to (p=7, sp=11)"));
    }

    #[test]
    fn decode_normal_index_when_not_rho_encoded() {
        let encoding = Sparse::new(4, 7);
        assert_eq!(encoding.decode_normal_index(0b1010100), 0b1010);
    }

    #[test]
    fn decode_normal_index_when_rho_encoded() {
        let encoding = Sparse::new(4, 7);
        assert_eq!(encoding.decode_normal_index(0b11010001100), 0b1010);
    }

    #[test]
    fn decode_and_downgrade_normal_index_when_not_rho_encoded() {
        let source = Sparse::new(4, 7);
        let target = Normal { precision: 3 };
        assert_eq!(
            source.decode_and_downgrade_normal_index(0b1010100, target),
            0b101
        );
    }

    #[test]
    fn decode_and_downgrade_normal_index_when_rho_encoded() {
        let source = Sparse::new(4, 7);
        let target = Normal { precision: 3 };
        assert_eq!(
            source.decode_and_downgrade_normal_index(0b11010001100, target),
            0b101
        );
    }

    #[test]
    fn decode_normal_rho_w_when_not_rho_encoded() {
        let encoding = Sparse::new(4, 7);
        assert_eq!(encoding.decode_normal_rho_w(0b1010100), 1);
    }

    #[test]
    fn decode_normal_rho_w_when_rho_encoded() {
        let encoding = Sparse::new(4, 7);
        assert_eq!(encoding.decode_normal_rho_w(0b11010001100), 0b1100 + 3);
    }

    #[test]
    fn decode_and_downgrade_normal_rho_w_when_not_rho_encoded() {
        let source = Sparse::new(4, 7);
        let target = Normal { precision: 3 };
        assert_eq!(
            source.decode_and_downgrade_normal_rho_w(0b1010100, target),
            2
        );
    }

    #[test]
    fn decode_and_downgrade_normal_rho_w_when_rho_encoded() {
        let source = Sparse::new(4, 7);
        let target = Normal { precision: 3 };
        assert_eq!(
            source.decode_and_downgrade_normal_rho_w(0b11010001100, target),
            0b1100 + 4
        );
    }

    #[test]
    fn decode_sparse_index_when_not_rho_encoded() {
        let encoding = Sparse::new(4, 7);
        assert_eq!(encoding.decode_sparse_index(0b1010100), 0b1010100);
    }

    #[test]
    fn decode_sparse_index_when_rho_encoded() {
        let encoding = Sparse::new(4, 7);
        assert_eq!(encoding.decode_sparse_index(0b11010001100), 0b1010000);
    }

    // `decode_sparse_rho_w_if_present` is private to `encoding`. Checked through `downgrade` to
    // the same encoding, which re-encodes (index, decoded rhoW') unchanged.
    #[test]
    fn decode_sparse_rho_w_if_present_when_rho_encoded() {
        let encoding = Sparse::new(4, 7);
        // rhoW' = 0b1100 survives the round trip only if it was decoded as 0b1100.
        assert_eq!(encoding.downgrade(0b11010001100, encoding), 0b11010001100);
    }

    // Not applicable: decodeSparseRhoWIfPresent_whenNotRhoEncoded: the decoder is private to
    // `encoding`, and a non-rho-encoded value always has non-zero low bits, so its (unknown,
    // 0) rhoW' never reaches any pub(crate) output.

    #[test]
    fn dedupe() {
        let input = [
            0b00000010100,
            0b00001010100,
            0b00001010101,
            0b11010001100,
            0b11010010000,
            0b11110000000,
        ];
        let encoding = Sparse::new(4, 7);
        assert_eq!(
            encoding.dedupe(&input),
            vec![
                0b00000010100,
                0b00001010100,
                0b00001010101,
                0b11010010000,
                0b11110000000
            ]
        );
    }

    #[test]
    fn dedupe_exact_duplicates() {
        let input = [
            0b00000010100,
            0b00000010100,
            0b00000010100,
            0b11010001100,
            0b11010001100,
            0b11010001100,
        ];
        let encoding = Sparse::new(4, 7);
        assert_eq!(encoding.dedupe(&input), vec![0b00000010100, 0b11010001100]);
    }

    #[test]
    fn encode_without_rho_w() {
        let encoding = Sparse::new(4, 7);
        assert_eq!(encoding.encode_hash(0b101100101u64 << 55), 0b1011001);
    }

    #[test]
    fn encode_without_rho_w_at_maximum_sparse_precision() {
        let encoding = Sparse::new(4, 30);
        assert_eq!(
            encoding.encode_hash(0b101100101u64 << 55),
            0b101100101 << 21
        );
    }

    #[test]
    fn encode_with_rho_w_at_maximum_normal_precision() {
        let encoding = Sparse::new(24, 26);
        assert_eq!(
            encoding.encode_hash(0b101u64 << 61),
            (1 << 30) | (0b101 << 27) | 39
        );
    }

    #[test]
    fn encode_with_rho_w_at_minimum_normal_precision() {
        let encoding = Sparse::new(1, 5);
        assert_eq!(encoding.encode_hash(1u64 << 63), (1 << 7) | (0b1 << 6) | 60);
    }

    #[test]
    fn encode_with_rho_w_when_length_dominated_by_normal_index() {
        let encoding = Sparse::new(4, 7);
        assert_eq!(
            encoding.encode_hash(0b101100001u64 << 55),
            (1 << 10) | (0b1011 << 6) | 2
        );
    }

    #[test]
    fn encode_with_rho_w_when_length_dominated_by_sparse_index() {
        let encoding = Sparse::new(2, 9);
        assert_eq!(
            encoding.encode_hash(0b110000000001u64 << 52),
            (1 << 9) | (0b11 << 6) | 3
        );
    }

    #[test]
    fn encode_with_rho_w_when_sparse_precision_is_equal_to_normal_precision() {
        let encoding = Sparse::new(3, 3);
        assert_eq!(
            encoding.encode_hash(0b10111u64 << 59),
            (1 << 9) | (0b101 << 6) | 1
        );
    }

    #[test]
    fn downgrade_rho_w_to_non_rho_w() {
        let source = Sparse::new(3, 5);
        let target = Sparse::new(2, 5);
        assert_eq!(
            source.downgrade((1 << 9) | (0b111 << 6) | 2, target),
            0b11100
        );
    }

    #[test]
    fn downgrade_non_rho_w_to_rho_w() {
        let source = Sparse::new(3, 5);
        let target = Sparse::new(3, 4);
        assert_eq!(
            source.downgrade(0b11101, target),
            (1 << 9) | (0b111 << 6) | 1
        );
    }

    #[test]
    fn downgrade_iterator() {
        let source = Sparse::new(11, 15);
        let target = Sparse::new(10, 13);
        // Preserves ordering of input values rather than sorting values.
        let out: Vec<u32> = [0b000000000000001, 0b000000000011111]
            .iter()
            .map(|&v| source.downgrade(v, target))
            .collect();
        assert_eq!(out, vec![0b10000000000000010, 0b000000000111]);
    }

    // Rust `Sparse` has no `normal()` accessor; the normal precision is
    // the public field checked here instead.
    #[test]
    fn normal() {
        assert_eq!(Sparse::new(3, 5).normal_precision, 3);
    }

    #[test]
    fn is_less_than() {
        assert!(Sparse::new(3, 5).is_less_than(Sparse::new(4, 5)));
        assert!(Sparse::new(4, 5).is_less_than(Sparse::new(4, 6)));
        assert!(Sparse::new(3, 5).is_less_than(Sparse::new(4, 6)));
        assert!(!Sparse::new(3, 5).is_less_than(Sparse::new(3, 5)));
        // Currently doesn't verify that precisions are incompatible.
        assert!(Sparse::new(2, 6).is_less_than(Sparse::new(3, 5)));
    }

    /// The `Object` / `null` comparisons of the Java test do not apply to a typed `PartialEq`.
    #[test]
    fn equals_to() {
        assert_eq!(Sparse::new(4, 5), Sparse::new(4, 5));
        assert_ne!(Sparse::new(4, 5), Sparse::new(3, 5));
        assert_ne!(Sparse::new(4, 5), Sparse::new(4, 6));
    }
}

mod state_test {
    use crate::proto::{put_varint, State, StateView, HYPERLOGLOG_PLUS_UNIQUE};

    /// A raw top-level field: number and either a varint or length-delimited payload.
    #[derive(Debug, PartialEq)]
    enum Field {
        Varint(u32, u64),
        Bytes(u32, Vec<u8>),
    }

    fn read_varint(buf: &[u8], pos: &mut usize) -> u64 {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = buf[*pos];
            *pos += 1;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    }

    /// Minimal protobuf scanner (varint and length-delimited only) for the serialize tests.
    fn fields(buf: &[u8]) -> Vec<Field> {
        let mut out = Vec::new();
        let mut pos = 0;
        while pos < buf.len() {
            let tag = read_varint(buf, &mut pos);
            let number = (tag >> 3) as u32;
            match tag & 7 {
                0 => out.push(Field::Varint(number, read_varint(buf, &mut pos))),
                2 => {
                    let len = read_varint(buf, &mut pos) as usize;
                    out.push(Field::Bytes(number, buf[pos..pos + len].to_vec()));
                    pos += len;
                }
                w => panic!("unexpected wire type {w}"),
            }
        }
        out
    }

    fn top_varint(bytes: &[u8], number: u32) -> Option<u64> {
        fields(bytes).into_iter().find_map(|f| match f {
            Field::Varint(n, v) if n == number => Some(v),
            _ => None,
        })
    }

    fn hll_fields(bytes: &[u8]) -> Vec<Field> {
        let nested = fields(bytes)
            .into_iter()
            .find_map(|f| match f {
                Field::Bytes(112, b) => Some(b),
                _ => None,
            })
            .expect("hll extension present");
        fields(&nested)
    }

    fn hll_varint(bytes: &[u8], number: u32) -> Option<u64> {
        hll_fields(bytes).into_iter().find_map(|f| match f {
            Field::Varint(n, v) if n == number => Some(v),
            _ => None,
        })
    }

    fn hll_bytes(bytes: &[u8], number: u32) -> Option<Vec<u8>> {
        hll_fields(bytes).into_iter().find_map(|f| match f {
            Field::Bytes(n, b) if n == number => Some(b),
            _ => None,
        })
    }

    /// One varint field (int32 values are sign-extended like protobuf `int32`).
    fn varint_field(number: u32, v: i64) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, u64::from(number << 3));
        put_varint(&mut out, v as u64);
        out
    }

    fn bytes_field(number: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, u64::from(number << 3 | 2));
        put_varint(&mut out, payload.len() as u64);
        out.extend_from_slice(payload);
        out
    }

    /// Java `build`: a default `AggregatorStateProto` wrapping the given HLL extension bytes.
    fn build(hll: &[u8]) -> Vec<u8> {
        let mut out = varint_field(1, i64::from(HYPERLOGLOG_PLUS_UNIQUE));
        out.extend(varint_field(2, 0));
        out.extend(bytes_field(112, hll));
        out
    }

    fn parse(bytes: &[u8]) -> State {
        State::parse(bytes).unwrap()
    }

    #[test]
    fn has_data() {
        let mut state = State {
            data: None,
            ..State::default()
        };
        assert!(!state.has_data());
        state.data = Some(Vec::new());
        assert!(!state.has_data());
        // Java's `allocate(5).position(5)` (no remaining bytes) is the same as empty here.
        state.data = Some(vec![0; 5]);
        assert!(state.has_data());
    }

    #[test]
    fn has_sparse_data() {
        let mut state = State {
            sparse_data: None,
            ..State::default()
        };
        assert!(!state.has_sparse_data());
        state.sparse_data = Some(Vec::new());
        assert!(!state.has_sparse_data());
        state.sparse_data = Some(vec![0; 5]);
        assert!(state.has_sparse_data());
    }

    #[test]
    fn parse_type() {
        // Required field should always be set; HLL++ is assumed when it is missing.
        assert_eq!(parse(&[]).r#type, HYPERLOGLOG_PLUS_UNIQUE);
        for t in [0, 1, 100, 111, 112, 113, 200] {
            assert_eq!(parse(&varint_field(1, t)).r#type, t as i32);
        }
    }

    #[test]
    fn parse_num_values() {
        assert_eq!(parse(&[]).num_values, 0);
        for n in [0, 42, i64::MIN, i64::MAX] {
            assert_eq!(parse(&varint_field(2, n)).num_values, n);
        }
    }

    #[test]
    fn parse_encoding_version() {
        // 1 is the default in the proto descriptor.
        assert_eq!(parse(&[]).encoding_version, 1);
        for v in [0, 42, i32::MIN, i32::MAX] {
            assert_eq!(parse(&varint_field(3, i64::from(v))).encoding_version, v);
        }
    }

    #[test]
    fn parse_value_type_unknown() {
        assert_eq!(parse(&[]).value_type, 0);
    }

    /// Rust keeps the value type as the raw enum number; covers the standard type numbers.
    #[test]
    fn parse_value_type_default_ops_type() {
        for v in 0..=20 {
            assert_eq!(parse(&varint_field(4, v)).value_type, v as i32);
        }
    }

    /// Custom value types are numbers above the standard range; the raw number round-trips.
    #[test]
    fn parse_value_type_custom_value_type() {
        for v in [1000, 1001, 1002] {
            assert_eq!(parse(&varint_field(4, v)).value_type, v as i32);
        }
    }

    #[test]
    fn parse_value_type_other_number() {
        assert_eq!(parse(&varint_field(4, 12345)).value_type, 12345);
    }

    #[test]
    fn parse_sparse_size() {
        assert_eq!(parse(&build(&[])).sparse_size, 0);
        for v in [0, 42, i32::MIN, i32::MAX] {
            assert_eq!(parse(&build(&varint_field(2, i64::from(v)))).sparse_size, v);
        }
    }

    #[test]
    fn parse_precision() {
        assert_eq!(parse(&build(&[])).precision, 0);
        for v in [0, 42, i32::MIN, i32::MAX] {
            assert_eq!(parse(&build(&varint_field(3, i64::from(v)))).precision, v);
        }
    }

    #[test]
    fn parse_sparse_precision() {
        assert_eq!(parse(&build(&[])).sparse_precision, 0);
        for v in [0, 42, i32::MIN, i32::MAX] {
            assert_eq!(
                parse(&build(&varint_field(4, i64::from(v)))).sparse_precision,
                v
            );
        }
    }

    #[test]
    fn parse_data() {
        assert_eq!(parse(&build(&[])).data, None);
        assert_eq!(parse(&build(&bytes_field(5, &[]))).data, Some(vec![]));
        assert_eq!(
            parse(&build(&bytes_field(5, &[1, 2, 3]))).data,
            Some(vec![1, 2, 3])
        );
    }

    /// Aliasing maps to `StateView`, which borrows `data` from the input bytes.
    #[test]
    fn parse_data_aliases_array() {
        let bytes = build(&bytes_field(5, &[1, 2, 3]));
        let view = StateView::parse(&bytes).unwrap();
        let data = view.data.unwrap();
        assert_eq!(data, &[1, 2, 3]);
        let range = bytes.as_ptr_range();
        assert!(range.contains(&data.as_ptr()));
    }

    /// The non-aliasing path maps to `State::parse`, which copies `data`.
    #[test]
    fn parse_data_parses_stream() {
        let bytes = build(&bytes_field(5, &[1, 2, 3]));
        assert_eq!(parse(&bytes).data, Some(vec![1, 2, 3]));
    }

    #[test]
    fn parse_sparse_data() {
        assert_eq!(parse(&build(&[])).sparse_data, None);
        assert_eq!(
            parse(&build(&bytes_field(6, &[]))).sparse_data,
            Some(vec![])
        );
        assert_eq!(
            parse(&build(&bytes_field(6, &[1, 2, 3]))).sparse_data,
            Some(vec![1, 2, 3])
        );
    }

    #[test]
    fn parse_sparse_data_aliases_array() {
        let bytes = build(&bytes_field(6, &[1, 2, 3]));
        let view = StateView::parse(&bytes).unwrap();
        let data = view.sparse_data.unwrap();
        assert_eq!(data, &[1, 2, 3]);
        assert!(bytes.as_ptr_range().contains(&data.as_ptr()));
    }

    #[test]
    fn parse_sparse_data_parses_stream() {
        let bytes = build(&bytes_field(6, &[1, 2, 3]));
        assert_eq!(parse(&bytes).sparse_data, Some(vec![1, 2, 3]));
    }

    #[test]
    fn parse_unknown_field() {
        let mut bytes = varint_field(2, 42);
        bytes.extend(bytes_field(999, b"foobar"));
        bytes.extend(varint_field(3, 43));
        let state = parse(&bytes);
        assert_eq!(state.num_values, 42);
        assert_eq!(state.encoding_version, 43);
    }

    #[test]
    fn serialize_type() {
        let mut state = State::default();
        assert_eq!(
            top_varint(&state.to_bytes(), 1),
            Some(HYPERLOGLOG_PLUS_UNIQUE as u64)
        );
        for t in [0, 1, 100, 112, 113, 200] {
            state.r#type = t;
            assert_eq!(top_varint(&state.to_bytes(), 1), Some(t as u64));
        }
    }

    #[test]
    fn serialize_num_values() {
        let mut state = State::default();
        assert_eq!(top_varint(&state.to_bytes(), 2), Some(0));
        state.num_values = 42;
        assert_eq!(top_varint(&state.to_bytes(), 2), Some(42));
    }

    #[test]
    fn serialize_encoding_version() {
        let mut state = State::default();
        assert_eq!(top_varint(&state.to_bytes(), 3), None);
        state.encoding_version = 2;
        assert_eq!(top_varint(&state.to_bytes(), 3), Some(2));
    }

    #[test]
    fn serialize_value_type() {
        let mut state = State::default();
        assert_eq!(top_varint(&state.to_bytes(), 4), None);
        for v in 0..=20 {
            state.value_type = v;
            let bytes = state.to_bytes();
            if v == 0 {
                assert_eq!(top_varint(&bytes, 4), None);
            }
            assert_eq!(parse(&bytes).value_type, v);
        }
    }

    #[test]
    fn serialize_sparse_size() {
        let mut state = State::default();
        assert_eq!(hll_varint(&state.to_bytes(), 2), None);
        state.sparse_size = 42;
        assert_eq!(hll_varint(&state.to_bytes(), 2), Some(42));
    }

    #[test]
    fn serialize_precision() {
        let mut state = State::default();
        assert_eq!(hll_varint(&state.to_bytes(), 3), None);
        state.precision = 42;
        assert_eq!(hll_varint(&state.to_bytes(), 3), Some(42));
    }

    #[test]
    fn serialize_sparse_precision() {
        let mut state = State::default();
        assert_eq!(hll_varint(&state.to_bytes(), 4), None);
        state.sparse_precision = 42;
        assert_eq!(hll_varint(&state.to_bytes(), 4), Some(42));
    }

    #[test]
    fn serialize_data() {
        let mut state = State::default();
        assert_eq!(hll_bytes(&state.to_bytes(), 5), None);
        state.data = Some(vec![1, 2, 3]);
        assert_eq!(hll_bytes(&state.to_bytes(), 5), Some(vec![1, 2, 3]));
    }

    #[test]
    fn serialize_sparse_data() {
        let mut state = State::default();
        assert_eq!(hll_bytes(&state.to_bytes(), 6), None);
        state.sparse_data = Some(vec![1, 2, 3]);
        assert_eq!(hll_bytes(&state.to_bytes(), 6), Some(vec![1, 2, 3]));
    }
}

/// Shared helpers for the representation tests. Rust has no separate representation types:
/// Java `repr.addSparseValue(s)(enc, values)` maps to merging a sparse sketch at `enc` that holds
/// `values` (which runs the same `add_sparse_values` code path), and `compact()` to
/// `serialize()`.
mod repr_util {
    use crate::encoding::encode_sparse;
    use crate::hll::{HyperLogLogPlusPlus, ENCODING_VERSION};
    use crate::proto::State;

    fn base(precision: i32, sparse_precision: i32) -> State {
        State {
            encoding_version: ENCODING_VERSION,
            precision,
            sparse_precision,
            ..State::default()
        }
    }

    /// Java `new SparseRepresentation(state)`.
    pub fn sparse(precision: i32, sparse_precision: i32) -> HyperLogLogPlusPlus {
        let h = HyperLogLogPlusPlus::from_state(base(precision, sparse_precision)).unwrap();
        assert!(h.is_sparse());
        h
    }

    /// Java `new NormalRepresentation(state)`. A Rust sketch with a sparse precision is only
    /// normal when it has registers, so they start allocated (all zero).
    pub fn normal(precision: i32, sparse_precision: i32) -> HyperLogLogPlusPlus {
        normal_with(precision, sparse_precision, vec![0; 1 << precision])
    }

    pub fn normal_with(
        precision: i32,
        sparse_precision: i32,
        data: Vec<u8>,
    ) -> HyperLogLogPlusPlus {
        let state = State {
            data: Some(data),
            ..base(precision, sparse_precision)
        };
        let h = HyperLogLogPlusPlus::from_state(state).unwrap();
        assert!(!h.is_sparse());
        h
    }

    /// A sparse sketch at `(p, sp)` holding exactly `values` (`None`: Java's `null` iterator).
    pub fn values(p: i32, sp: i32, values: Option<&[u32]>) -> HyperLogLogPlusPlus {
        let mut state = base(p, sp);
        if let Some(v) = values {
            state.sparse_data = Some(encode_sparse(v));
            state.sparse_size = v.len() as i32;
        }
        HyperLogLogPlusPlus::from_state(state).unwrap()
    }

    /// `compact()` then read back the serialized state.
    pub fn compact(h: &mut HyperLogLogPlusPlus) -> State {
        State::parse(&h.serialize()).unwrap()
    }

    pub fn sparse_values(state: &State) -> Vec<u32> {
        crate::encoding::decode_sparse(state.sparse_data.as_deref().unwrap_or(&[])).unwrap()
    }
}

mod sparse_representation_test {
    use super::repr_util::{compact, sparse, sparse_values, values};

    #[test]
    fn add_sparse_value_higher_precision() {
        let mut repr = sparse(10, 13);
        repr.merge(values(11, 15, Some(&[0b000000000011111])))
            .unwrap();
        let state = compact(&mut repr);
        assert_eq!(state.precision, 10);
        assert_eq!(state.sparse_precision, 13);
        assert_eq!(sparse_values(&state), vec![0b000000000111]);
    }

    #[test]
    fn add_sparse_value_lower_precision() {
        let mut repr = sparse(11, 15);
        let sparse_value = 0b0000000000001;
        repr.merge(values(10, 13, Some(&[sparse_value]))).unwrap();
        let state = compact(&mut repr);
        assert_eq!(state.precision, 10);
        assert_eq!(state.sparse_precision, 13);
        assert_eq!(sparse_values(&state)[0], sparse_value);
    }

    #[test]
    fn add_sparse_values_higher_precision() {
        let mut repr = sparse(10, 13);
        repr.merge(values(
            11,
            15,
            Some(&[0b000000000000001, 0b000000000011111]),
        ))
        .unwrap();
        let state = compact(&mut repr);
        assert_eq!(state.precision, 10);
        assert_eq!(state.sparse_precision, 13);
        assert_eq!(
            sparse_values(&state),
            vec![0b000000000111, 0b10000000000000010]
        );
    }

    #[test]
    fn add_sparse_values_higher_precision_null() {
        let mut repr = sparse(10, 13);
        repr.merge(values(11, 15, None)).unwrap();
        let state = compact(&mut repr);
        assert_eq!(state.precision, 10);
        assert_eq!(state.sparse_precision, 13);
    }

    #[test]
    fn add_sparse_values_lower_precision() {
        let mut repr = sparse(11, 15);
        let sparse_values_in = [0b0000000000001, 0b0000000001111];
        repr.merge(values(10, 13, Some(&sparse_values_in))).unwrap();
        let state = compact(&mut repr);
        assert_eq!(state.precision, 10);
        assert_eq!(state.sparse_precision, 13);
        assert_eq!(sparse_values(&state), sparse_values_in.to_vec());
    }

    #[test]
    fn add_sparse_values_lower_precision_null() {
        let mut repr = sparse(11, 15);
        repr.merge(values(10, 13, None)).unwrap();
        let state = compact(&mut repr);
        assert_eq!(state.precision, 10);
        assert_eq!(state.sparse_precision, 13);
    }
}

mod normal_representation_test {
    use super::repr_util::{compact, normal, normal_with, values};
    use crate::encoding::{Normal, Sparse};

    fn expected_downgraded(enc: Sparse, sparse_values: &[u32]) -> Vec<u8> {
        let mut expected = vec![0u8; 1 << 10];
        let normal = Normal { precision: 10 };
        for &v in sparse_values {
            expected[enc.decode_and_downgrade_normal_index(v, normal)] =
                enc.decode_and_downgrade_normal_rho_w(v, normal);
        }
        expected
    }

    #[test]
    fn add_sparse_value_downgrades_sparse_precision() {
        let mut repr = normal(10, 15);
        repr.merge(values(10, 13, Some(&[0b0000000000001])))
            .unwrap();
        assert_eq!(repr.sparse_precision(), 13);
    }

    #[test]
    fn add_sparse_value_higher_precision() {
        let mut repr = normal(10, 15);
        let v = 0b0000000000001;
        repr.merge(values(11, 13, Some(&[v]))).unwrap();
        assert_eq!(
            compact(&mut repr).data.unwrap(),
            expected_downgraded(Sparse::new(11, 13), &[v])
        );
    }

    #[test]
    fn add_sparse_value_lower_precision() {
        let mut repr = normal(11, 15);
        let v = 0b0000000000001;
        repr.merge(values(10, 13, Some(&[v]))).unwrap();
        assert_eq!(
            compact(&mut repr).data.unwrap(),
            expected_downgraded(Sparse::new(10, 13), &[v])
        );
    }

    #[test]
    fn add_sparse_values_downgrades_sparse_precision() {
        let mut repr = normal(10, 15);
        repr.merge(values(10, 13, Some(&[]))).unwrap();
        assert_eq!(repr.sparse_precision(), 13);
    }

    #[test]
    fn add_sparse_values_higher_precision() {
        let mut repr = normal(10, 15);
        let vs = [0b0000000000001, 0b00000000011111];
        repr.merge(values(11, 13, Some(&vs))).unwrap();
        assert_eq!(
            compact(&mut repr).data.unwrap(),
            expected_downgraded(Sparse::new(11, 13), &vs)
        );
    }

    #[test]
    fn add_sparse_values_lower_precision() {
        let mut repr = normal(11, 15);
        let enc = Sparse::new(10, 13);
        let vs = [0b0000000000001, 0b0000000001001];
        repr.merge(values(10, 13, Some(&vs))).unwrap();
        let mut expected = vec![0u8; 1 << 10];
        for &v in &vs {
            expected[enc.decode_normal_index(v)] = enc.decode_normal_rho_w(v);
        }
        assert_eq!(compact(&mut repr).data.unwrap(), expected);
    }

    /// Java expects `data == null`. A Rust sketch with a sparse precision is normal only when it
    /// has registers, so the representation under test starts with all-zero registers and keeps
    /// them; precisions behave as in Java.
    #[test]
    fn add_sparse_values_higher_precision_null_differs_from_java() {
        let mut repr = normal(10, 15);
        repr.merge(values(11, 13, None)).unwrap();
        let state = compact(&mut repr);
        assert_eq!(state.precision, 10);
        assert_eq!(state.sparse_precision, 13);
        assert_eq!(state.data, Some(vec![0u8; 1 << 10]));
    }

    #[test]
    fn add_sparse_values_lower_precision_null() {
        let mut repr = normal(11, 15);
        repr.merge(values(10, 13, None)).unwrap();
        let state = compact(&mut repr);
        assert_eq!(state.precision, 10);
        assert_eq!(state.sparse_precision, 13);
        assert_eq!(state.data, Some(vec![0u8; 1 << 10]));
    }

    #[test]
    fn merge_downgrades_sparse_precision() {
        let a = normal(10, 14);
        let mut b = normal(10, 15);
        b.merge(a).unwrap();
        assert_eq!(b.sparse_precision(), 14);
    }

    /// Java `NormalRepresentation.addHash` on fresh registers at precision `p`.
    fn registers(p: i32, hashes: &[u64]) -> Vec<u8> {
        let enc = Normal { precision: p };
        let mut data = vec![0u8; 1 << p];
        for &h in hashes {
            let i = enc.index(h);
            data[i] = data[i].max(enc.rho_w(h));
        }
        data
    }

    fn p10_hashes() -> [u64; 2] {
        [
            (0b0000000000u64 << 54) | (0b001u64 << 51), // rhoW = 3
            (0b0000000001u64 << 54) | (0b111u64 << 51), // rhoW = 1
        ]
    }

    fn p11_hashes() -> [u64; 2] {
        [
            (0b00000000000u64 << 53) | (0b111u64 << 50), // rhoW = 1
            (0b00000000010u64 << 53) | (0b001u64 << 50), // rhoW = 3
        ]
    }

    fn expected_merge() -> Vec<u8> {
        let mut expected = vec![0u8; 1 << 10];
        expected[0] = 3; // preserved
        expected[1] = 4; // updated (rhoW 3, 4 after downgrade)
        expected
    }

    #[test]
    fn merge_normal_with_higher_precision() {
        let mut target = normal_with(10, 15, registers(10, &p10_hashes()));
        let source = normal_with(11, 15, registers(11, &p11_hashes()));
        target.merge(source).unwrap();
        assert_eq!(compact(&mut target).data.unwrap(), expected_merge());
    }

    #[test]
    fn merge_normal_with_lower_precision() {
        let source = normal_with(10, 15, registers(10, &p10_hashes()));
        let mut target = normal_with(11, 15, registers(11, &p11_hashes()));
        target.merge(source).unwrap();
        let state = compact(&mut target);
        assert_eq!(state.data.unwrap(), expected_merge());
        assert_eq!(state.precision, 10);
    }
}
