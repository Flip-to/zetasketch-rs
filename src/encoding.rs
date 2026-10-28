//! Register and sparse-value encodings, ported from ZetaSketch Java
//! `internal/hllplus/Encoding.java`.
//!
//! Shifts use `wrapping_shl`, which masks the shift amount exactly like Java's `<<` on `int` and
//! `long`. Several reference code paths depend on that masking at the precision edges.

use crate::error::{Error, Result};

const RHOW_BITS: i32 = 6;
const RHOW_MASK: u32 = (1 << RHOW_BITS) - 1;

/// Number of leading zeros of the bits after the index, plus one (`rhoW` in the HLL++ paper).
#[inline]
pub(crate) fn compute_rho_w(value: u64, bits: i32) -> u8 {
    let w = value.wrapping_shl((64 - bits) as u32);
    if w == 0 {
        (bits + 1) as u8
    } else {
        (w.leading_zeros() + 1) as u8
    }
}

#[inline]
fn downgrade_rho_w(index: u32, rho_w: u8, source_p: i32, target_p: i32) -> u8 {
    if source_p == target_p {
        return rho_w;
    }
    let suffix = index.wrapping_shl((32 - source_p + target_p) as u32);
    if suffix == 0 {
        (i32::from(rho_w) + source_p - target_p) as u8
    } else {
        (1 + suffix.leading_zeros()) as u8
    }
}

/// Dense ("normal") encoding at precision `p`: `2^p` one-byte registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Normal {
    pub precision: i32,
}

impl Normal {
    #[inline]
    pub fn index(self, hash: u64) -> usize {
        (hash >> (64 - self.precision)) as usize
    }

    #[inline]
    pub fn rho_w(self, hash: u64) -> u8 {
        compute_rho_w(hash, 64 - self.precision)
    }

    #[inline]
    pub fn downgrade_index(self, index: usize, target: Normal) -> usize {
        index >> (self.precision - target.precision)
    }

    #[inline]
    pub fn downgrade_rho_w(self, index: usize, rho_w: u8, target: Normal) -> u8 {
        // 0 means "unset register" and must stay 0.
        if rho_w == 0 {
            return 0;
        }
        downgrade_rho_w(index as u32, rho_w, self.precision, target.precision)
    }
}

/// Sparse encoding: each value is either a sparse index (when its low `sp - p` bits are not all
/// zero, the normal `rhoW` is implied by them) or a flagged `normal index | rhoW'` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sparse {
    pub normal_precision: i32,
    pub sparse_precision: i32,
    rho_encoded_flag: u32,
}

impl Sparse {
    pub fn new(normal_precision: i32, sparse_precision: i32) -> Sparse {
        let flag_bit = sparse_precision.max(normal_precision + RHOW_BITS);
        Sparse {
            normal_precision,
            sparse_precision,
            rho_encoded_flag: 1u32.wrapping_shl(flag_bit as u32),
        }
    }

    pub fn assert_compatible(self, other: Sparse) -> Result<()> {
        if (self.normal_precision <= other.normal_precision
            && self.sparse_precision <= other.sparse_precision)
            || (self.normal_precision >= other.normal_precision
                && self.sparse_precision >= other.sparse_precision)
        {
            return Ok(());
        }
        Err(Error::IncompatiblePrecision {
            this: (self.normal_precision, self.sparse_precision),
            other: (other.normal_precision, other.sparse_precision),
        })
    }

    /// Java `isLessThan`: true when either precision of `self` is lower than `other`'s.
    pub fn is_less_than(self, other: Sparse) -> bool {
        self.normal_precision < other.normal_precision
            || self.sparse_precision < other.sparse_precision
    }

    #[inline]
    pub fn encode_hash(self, hash: u64) -> u32 {
        let sparse_index = (hash >> (64 - self.sparse_precision)) as u32;
        let sparse_rho_w = compute_rho_w(hash, 64 - self.sparse_precision);
        self.encode(sparse_index, sparse_rho_w)
    }

    #[inline]
    pub fn encode(self, sparse_index: u32, sparse_rho_w: u8) -> u32 {
        let mask = 1u32
            .wrapping_shl((self.sparse_precision - self.normal_precision) as u32)
            .wrapping_sub(1);
        if sparse_index & mask != 0 {
            return sparse_index;
        }
        let normal_index = sparse_index >> (self.sparse_precision - self.normal_precision);
        self.rho_encoded_flag | normal_index << RHOW_BITS | u32::from(sparse_rho_w)
    }

    #[inline]
    fn is_rho_encoded(self, v: u32) -> bool {
        v & self.rho_encoded_flag != 0
    }

    #[inline]
    pub fn decode_sparse_index(self, v: u32) -> u32 {
        if !self.is_rho_encoded(v) {
            return v;
        }
        ((v ^ self.rho_encoded_flag) >> RHOW_BITS)
            << (self.sparse_precision - self.normal_precision)
    }

    #[inline]
    fn decode_sparse_rho_w_if_present(self, v: u32) -> u8 {
        if !self.is_rho_encoded(v) {
            return 0;
        }
        (v & RHOW_MASK) as u8
    }

    #[inline]
    pub fn decode_normal_index(self, v: u32) -> usize {
        if !self.is_rho_encoded(v) {
            return (v >> (self.sparse_precision - self.normal_precision)) as usize;
        }
        ((v ^ self.rho_encoded_flag) >> RHOW_BITS) as usize
    }

    #[inline]
    pub fn decode_normal_rho_w(self, v: u32) -> u8 {
        if !self.is_rho_encoded(v) {
            return compute_rho_w(u64::from(v), self.sparse_precision - self.normal_precision);
        }
        ((v & RHOW_MASK) as i32 + self.sparse_precision - self.normal_precision) as u8
    }

    #[inline]
    pub fn decode_and_downgrade_normal_index(self, v: u32, target: Normal) -> usize {
        self.decode_normal_index(v) >> (self.normal_precision - target.precision)
    }

    #[inline]
    pub fn decode_and_downgrade_normal_rho_w(self, v: u32, target: Normal) -> u8 {
        downgrade_rho_w(
            self.decode_normal_index(v) as u32,
            self.decode_normal_rho_w(v),
            self.normal_precision,
            target.precision,
        )
    }

    /// Re-encodes a value of this encoding in the (lower) `target` encoding.
    pub fn downgrade(self, v: u32, target: Sparse) -> u32 {
        let old_index = self.decode_sparse_index(v);
        let old_rho_w = self.decode_sparse_rho_w_if_present(v);
        let new_index = old_index >> (self.sparse_precision - target.sparse_precision);
        let new_rho_w = downgrade_rho_w(
            old_index,
            old_rho_w,
            self.sparse_precision,
            target.sparse_precision,
        );
        target.encode(new_index, new_rho_w)
    }

    /// Collapses a sorted stream so that each sparse index appears once, keeping the largest
    /// `rhoW'` for rho-encoded values (Java `Encoding.Sparse.dedupe`).
    pub fn dedupe(self, sorted: &[u32]) -> Vec<u32> {
        let mut out = Vec::with_capacity(sorted.len());
        let mut i = 0;
        while i < sorted.len() {
            let next = sorted[i];
            if !self.is_rho_encoded(next) {
                i += 1;
                while i < sorted.len() && sorted[i] == next {
                    i += 1;
                }
                out.push(next);
                continue;
            }
            let sparse_index = self.decode_sparse_index(next);
            let mut max = next;
            i += 1;
            while i < sorted.len() && self.decode_sparse_index(sorted[i]) == sparse_index {
                max = sorted[i];
                i += 1;
            }
            out.push(max);
        }
        out
    }
}

/// Decodes a difference/varint-encoded sparse stream (Java `DifferenceDecoder`).
pub(crate) fn decode_sparse(bytes: &[u8]) -> Result<Vec<u32>> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut pos = 0;
    let mut last: u32 = 0;
    while pos < bytes.len() {
        let mut delta: u32 = 0;
        let mut shift = 0;
        loop {
            let b = *bytes
                .get(pos)
                .ok_or(Error::CorruptSparseData("truncated varint"))?;
            pos += 1;
            if shift == 28 && b & 0xf0 != 0 {
                return Err(Error::CorruptSparseData("varint too long"));
            }
            delta |= u32::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        last = last.wrapping_add(delta);
        if last > i32::MAX as u32 {
            return Err(Error::CorruptSparseData("negative value"));
        }
        out.push(last);
    }
    Ok(out)
}

/// Encodes ascending non-negative values as differences (Java `DifferenceEncoder`).
pub(crate) fn encode_sparse(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 2);
    let mut last = 0u32;
    for &v in values {
        debug_assert!(v >= last, "sparse values must be ascending");
        crate::proto::put_varint(&mut out, u64::from(v - last));
        last = v;
    }
    out
}

/// Merges two ascending sequences (Java `MergedIntIterator`; ties take `a` first).
pub(crate) fn merge_sorted(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if j < b.len() && (i >= a.len() || b[j] < a[i]) {
            out.push(b[j]);
            j += 1;
        } else {
            out.push(a[i]);
            i += 1;
        }
    }
    out
}
