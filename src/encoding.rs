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
    pub(crate) fn is_rho_encoded(self, v: u32) -> bool {
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

    /// True when `v` decodes to a register inside `2^p` with a `rhoW` the dense form can hold.
    pub fn is_valid(self, v: u32) -> bool {
        if !self.is_rho_encoded(v) {
            return v >> self.sparse_precision == 0;
        }
        let rest = v ^ self.rho_encoded_flag;
        rest < self.rho_encoded_flag
            && (rest >> RHOW_BITS) >> self.normal_precision == 0
            && i32::from(self.decode_normal_rho_w(v)) <= 65 - self.normal_precision
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

/// Streaming decoder for a difference/varint-encoded sparse stream (Java `DifferenceDecoder`),
/// reading straight from the serialized bytes. Yields plain values and stops at the first
/// corrupt one; check [`error`](Self::error) afterwards.
pub(crate) struct SparseIter<'a> {
    bytes: &'a [u8],
    pos: usize,
    last: u32,
    error: Option<&'static str>,
}

impl<'a> SparseIter<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        SparseIter {
            bytes,
            pos: 0,
            last: 0,
            error: None,
        }
    }

    /// The decode error that ended iteration, if any.
    pub fn error(&self) -> Result<()> {
        self.error
            .map_or(Ok(()), |m| Err(Error::CorruptSparseData(m)))
    }

    #[cold]
    fn fail(&mut self, msg: &'static str) -> Option<u32> {
        self.pos = self.bytes.len();
        self.error = Some(msg);
        None
    }
}

impl Iterator for SparseIter<'_> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<u32> {
        let bytes = self.bytes;
        if self.pos >= bytes.len() {
            return None;
        }
        // Fast path: one-byte delta, the common case for dense-ish sparse streams.
        let b = bytes[self.pos];
        let delta = if b & 0x80 == 0 {
            self.pos += 1;
            u32::from(b)
        } else {
            let mut delta: u32 = 0;
            let mut shift = 0;
            loop {
                let Some(&b) = bytes.get(self.pos) else {
                    return self.fail("truncated varint");
                };
                self.pos += 1;
                if shift == 28 && b & 0xf0 != 0 {
                    return self.fail("varint too long");
                }
                delta |= u32::from(b & 0x7f) << shift;
                if b & 0x80 == 0 {
                    break delta;
                }
                shift += 7;
            }
        };
        self.last = self.last.wrapping_add(delta);
        if self.last > i32::MAX as u32 {
            return self.fail("negative value");
        }
        Some(self.last)
    }
}

/// Max-merges dense `source` registers at a higher precision into `target` registers at a lower
/// one, with the same result as applying `Normal::downgrade_index` / `downgrade_rho_w` to every
/// source register (Java `mergeNormalDataMaybeDowngrading`), but per block instead of per
/// register.
///
/// Lowering by `d` bits maps each block of `2^d` source registers to one target register:
/// - offset 0 maps to `r + d` (or 0 when `r == 0`);
/// - a non-zero register at offset `o > 0` maps to `d - floor(log2 o)`, independent of its
///   value.
///
/// So the block's value is `max(first + d, d - floor(log2(first non-zero offset)))`. That only
/// needs a scan for the first non-zero byte, which is cheap.
pub(crate) fn downgrade_registers_max_into(
    source: &[u8],
    source_p: i32,
    target: &mut [u8],
    target_p: i32,
) {
    debug_assert!(source_p > target_p);
    debug_assert_eq!(source.len(), 1 << source_p);
    debug_assert_eq!(target.len(), 1 << target_p);
    let d = (source_p - target_p) as u32;
    let block = 1usize << d;
    for (t, chunk) in target.iter_mut().zip(source.chunks_exact(block)) {
        let first = chunk[0];
        let mut best = if first == 0 { 0 } else { first + d as u8 };
        if let Some(off) = first_nonzero_after_first(chunk) {
            let rho = (d - (off as u32).ilog2()) as u8;
            best = best.max(rho);
        }
        if *t < best {
            *t = best;
        }
    }
}

/// Offset of the first non-zero byte in `chunk[1..]` (as an index into `chunk`), scanning eight
/// bytes per step with `u64` words when the block is at least 8 bytes long.
#[inline]
fn first_nonzero_after_first(chunk: &[u8]) -> Option<usize> {
    if chunk.len() < 8 {
        return chunk[1..].iter().position(|&r| r != 0).map(|o| o + 1);
    }
    for (w, word) in chunk.chunks_exact(8).enumerate() {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(word);
        let mut x = u64::from_le_bytes(bytes);
        if w == 0 {
            x &= !0xff; // skip offset 0
        }
        if x != 0 {
            return Some(w * 8 + (x.trailing_zeros() / 8) as usize);
        }
    }
    None
}

/// Decodes and range-checks a whole sparse stream into `out` in one pass. On error `out` holds
/// a partial prefix and must be discarded.
pub(crate) fn decode_validated_into(bytes: &[u8], enc: Sparse, out: &mut Vec<u32>) -> Result<()> {
    out.clear();
    out.reserve(bytes.len());
    let n = bytes.len();
    let mut pos = 0;
    let mut last = 0u32;
    while pos < n {
        let mut b = u32::from(bytes[pos]);
        pos += 1;
        let mut delta = b & 0x7f;
        if b & 0x80 != 0 {
            let mut shift = 7;
            loop {
                if pos >= n {
                    return Err(Error::CorruptSparseData("truncated varint"));
                }
                b = u32::from(bytes[pos]);
                pos += 1;
                if shift == 28 && b & 0xf0 != 0 {
                    return Err(Error::CorruptSparseData("varint too long"));
                }
                delta |= (b & 0x7f) << shift;
                if b & 0x80 == 0 {
                    break;
                }
                shift += 7;
            }
        }
        last = last.wrapping_add(delta);
        if last > i32::MAX as u32 || !enc.is_valid(last) {
            return Err(Error::CorruptSparseData("value out of range"));
        }
        out.push(last);
    }
    Ok(())
}

/// Decodes a whole sparse stream into a vector.
pub(crate) fn decode_sparse(bytes: &[u8]) -> Result<Vec<u32>> {
    let mut it = SparseIter::new(bytes);
    let out: Vec<u32> = it.by_ref().collect();
    it.error()?;
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

/// One pass of [`merge_sorted`] then [`Sparse::dedupe`] into `out`. Returns the
/// difference-encoded byte length of the result (what Java checks against the normalization
/// threshold). Equal to the three-pass version by construction:
///
/// - a non-rho-encoded value only collapses with exact duplicates;
/// - a rho-encoded group keeps its last (largest) value;
/// - the two kinds never share a sparse index, because rho-encoded indices have zero low bits.
pub(crate) fn merge_dedupe_into(enc: Sparse, a: &[u32], b: &[u32], out: &mut Vec<u32>) -> usize {
    out.clear();
    out.reserve(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    let mut pending: Option<u32> = None;
    let mut bytes = 0usize;
    let mut last = 0u32;
    while i < a.len() || j < b.len() {
        let v = if j < b.len() && (i >= a.len() || b[j] < a[i]) {
            j += 1;
            b[j - 1]
        } else {
            i += 1;
            a[i - 1]
        };
        match pending {
            Some(p) if enc.is_rho_encoded(p) => {
                if enc.decode_sparse_index(v) == enc.decode_sparse_index(p) {
                    pending = Some(v);
                    continue;
                }
            }
            Some(p) if v == p => continue,
            _ => {}
        }
        if let Some(p) = pending {
            bytes += crate::proto::varint_len(p - last);
            last = p;
            out.push(p);
        }
        pending = Some(v);
    }
    if let Some(p) = pending {
        bytes += crate::proto::varint_len(p - last);
        out.push(p);
    }
    bytes
}

/// Inserts the ascending values `incoming` into `union`, which must already be deduped,
/// with the same result as [`merge_dedupe_into`]. `bytes` is the difference-encoded length of
/// `union` and is kept up to date by adjusting only the gaps next to each changed value.
/// Costs O(k log n) comparisons plus one `memmove` per new value, against O(n + k) for a full
/// merge, so it wins when `incoming` is small next to `union`.
pub(crate) fn insert_dedupe(
    enc: Sparse,
    union: &mut Vec<u32>,
    incoming: &[u32],
    bytes: &mut usize,
) {
    #[inline]
    fn gap(prev: Option<u32>, cur: u32) -> usize {
        crate::proto::varint_len(cur - prev.unwrap_or(0))
    }
    for &v in incoming {
        // A rho-encoded group shares every bit above the 6-bit rhoW', so it is the range
        // [v & !63, v | 63]. A plain sparse index only collapses with an exact duplicate.
        let (lo, encoded) = if enc.is_rho_encoded(v) {
            (v & !63, true)
        } else {
            (v, false)
        };
        let pos = union.partition_point(|&x| x < lo);
        let prev = pos.checked_sub(1).map(|i| union[i]);
        if let Some(&e) = union.get(pos) {
            let same = if encoded { e & !63 == lo } else { e == v };
            if same {
                if encoded && v > e {
                    let next = union.get(pos + 1).copied();
                    *bytes -= gap(prev, e) + next.map_or(0, |n| gap(Some(e), n));
                    *bytes += gap(prev, v) + next.map_or(0, |n| gap(Some(v), n));
                    union[pos] = v;
                }
                continue;
            }
        }
        let next = union.get(pos).copied();
        if let Some(n) = next {
            *bytes -= gap(prev, n);
            *bytes += gap(Some(v), n);
        }
        *bytes += gap(prev, v);
        union.insert(pos, v);
    }
}

/// Merges two ascending sequences (Java `MergedIntIterator`; ties take `a` first).
pub(crate) fn merge_sorted(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::new();
    merge_sorted_into(a, b, &mut out);
    out
}

/// [`merge_sorted`] into a reused buffer.
pub(crate) fn merge_sorted_into(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.clear();
    out.reserve(a.len() + b.len());
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_pass(enc: Sparse, a: &[u32], b: &[u32]) -> (Vec<u32>, usize) {
        let merged = enc.dedupe(&merge_sorted(a, b));
        let bytes = encode_sparse(&merged).len();
        (merged, bytes)
    }

    #[test]
    fn fused_merge_dedupe_matches_three_pass() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for (p, sp) in [(10, 15), (12, 17), (15, 20), (15, 15), (4, 25)] {
            let enc = Sparse::new(p, sp);
            for _ in 0..300 {
                let (na, nb) = (next() % 400, next() % 400);
                let mut a: Vec<u32> = (0..na).map(|_| enc.encode_hash(next())).collect();
                let mut b: Vec<u32> = (0..nb).map(|_| enc.encode_hash(next())).collect();
                a.sort_unstable();
                b.sort_unstable();
                let mut out = Vec::new();
                let bytes = merge_dedupe_into(enc, &a, &b, &mut out);
                let expected = three_pass(enc, &a, &b);
                assert_eq!((out, bytes), expected, "p={p} sp={sp}");

                // In-place insertion into the deduped `a` must agree too.
                let mut union = enc.dedupe(&a);
                let mut union_bytes = encode_sparse(&union).len();
                insert_dedupe(enc, &mut union, &b, &mut union_bytes);
                assert_eq!((union, union_bytes), expected, "insert p={p} sp={sp}");
            }
        }
    }
}

#[cfg(test)]
mod downgrade_block_tests {
    use super::*;

    fn per_register(source: &[u8], sp: i32, target: &mut [u8], tp: i32) {
        let (from, to) = (Normal { precision: sp }, Normal { precision: tp });
        for (i, &r) in source.iter().enumerate() {
            let ni = from.downgrade_index(i, to);
            let nr = from.downgrade_rho_w(i, r, to);
            if target[ni] < nr {
                target[ni] = nr;
            }
        }
    }

    #[test]
    fn block_downgrade_matches_per_register() {
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for sp in 4..=16 {
            for tp in 4..sp {
                for density in [0u64, 2, 10, 50, 100] {
                    let source: Vec<u8> = (0..1usize << sp)
                        .map(|_| {
                            if next() % 100 < density {
                                (1 + next() % (65 - sp as u64)) as u8
                            } else {
                                0
                            }
                        })
                        .collect();
                    let start: Vec<u8> = (0..1usize << tp).map(|_| (next() % 4) as u8).collect();
                    let (mut a, mut b) = (start.clone(), start);
                    downgrade_registers_max_into(&source, sp, &mut a, tp);
                    per_register(&source, sp, &mut b, tp);
                    assert_eq!(a, b, "sp={sp} tp={tp} density={density}");
                }
            }
        }
    }
}
