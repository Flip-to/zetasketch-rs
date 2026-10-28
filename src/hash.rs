//! `Fingerprint2011`, the 64-bit hash ZetaSketch (through Guava `Hashing.fingerprint2011()`)
//! applies to every value before it enters a sketch. Ported from Guava `Fingerprint2011`
//! (Apache-2.0).
//!
//! Integers are hashed as their little-endian bytes (4 for `i32`, 8 for `i64`), strings as UTF-8.

const K0: u64 = 0xa5b8_5c5e_198e_d849;
const K1: u64 = 0x8d58_ac26_afe1_2e47;
const K2: u64 = 0xc47b_6e9e_3a97_0ed3;
const K3: u64 = 0xc6a4_a793_5bd1_e995;

#[inline]
fn load64(b: &[u8], at: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(buf)
}

#[inline]
fn load64_safely(b: &[u8], at: usize, len: usize) -> u64 {
    let mut result = 0u64;
    for i in 0..len.min(8) {
        result |= u64::from(b[at + i]) << (i * 8);
    }
    result
}

#[inline]
fn shift_mix(v: u64) -> u64 {
    v ^ (v >> 47)
}

#[inline]
fn hash128_to_64(high: u64, low: u64) -> u64 {
    let mut a = (low ^ high).wrapping_mul(K3);
    a ^= a >> 47;
    let mut b = (high ^ a).wrapping_mul(K3);
    b ^= b >> 47;
    b.wrapping_mul(K3)
}

#[inline]
fn weak_hash_len32_with_seeds(b: &[u8], at: usize, mut seed_a: u64, mut seed_b: u64) -> (u64, u64) {
    let part1 = load64(b, at);
    let part2 = load64(b, at + 8);
    let part3 = load64(b, at + 16);
    let part4 = load64(b, at + 24);
    seed_a = seed_a.wrapping_add(part1);
    seed_b = seed_b
        .wrapping_add(seed_a)
        .wrapping_add(part4)
        .rotate_right(51);
    let c = seed_a;
    seed_a = seed_a.wrapping_add(part2).wrapping_add(part3);
    seed_b = seed_b.wrapping_add(seed_a.rotate_right(23));
    (seed_a.wrapping_add(part4), seed_b.wrapping_add(c))
}

fn murmur_hash64_with_seed(b: &[u8], seed: u64) -> u64 {
    let len = b.len();
    let aligned = len & !7;
    let rest = len & 7;
    let mut hash = seed ^ (len as u64).wrapping_mul(K3);
    let mut i = 0;
    while i < aligned {
        let data = shift_mix(load64(b, i).wrapping_mul(K3)).wrapping_mul(K3);
        hash ^= data;
        hash = hash.wrapping_mul(K3);
        i += 8;
    }
    if rest != 0 {
        hash ^= load64_safely(b, aligned, rest);
        hash = hash.wrapping_mul(K3);
    }
    hash = shift_mix(hash).wrapping_mul(K3);
    shift_mix(hash)
}

fn hash_length33_to64(b: &[u8]) -> u64 {
    let len = b.len();
    let mut z = load64(b, 24);
    let mut a = load64(b, 0).wrapping_add(
        (len as u64)
            .wrapping_add(load64(b, len - 16))
            .wrapping_mul(K0),
    );
    let mut bb = a.wrapping_add(z).rotate_right(52);
    let mut c = a.rotate_right(37);
    a = a.wrapping_add(load64(b, 8));
    c = c.wrapping_add(a.rotate_right(7));
    a = a.wrapping_add(load64(b, 16));
    let vf = a.wrapping_add(z);
    let vs = bb.wrapping_add(a.rotate_right(31)).wrapping_add(c);
    a = load64(b, 16).wrapping_add(load64(b, len - 32));
    z = load64(b, len - 8);
    bb = a.wrapping_add(z).rotate_right(52);
    c = a.rotate_right(37);
    a = a.wrapping_add(load64(b, len - 24));
    c = c.wrapping_add(a.rotate_right(7));
    a = a.wrapping_add(load64(b, len - 16));
    let wf = a.wrapping_add(z);
    let ws = bb.wrapping_add(a.rotate_right(31)).wrapping_add(c);
    let r = shift_mix(
        vf.wrapping_add(ws)
            .wrapping_mul(K2)
            .wrapping_add(wf.wrapping_add(vs).wrapping_mul(K0)),
    );
    shift_mix(r.wrapping_mul(K0).wrapping_add(vs)).wrapping_mul(K2)
}

fn full_fingerprint(b: &[u8]) -> u64 {
    let total = b.len();
    let mut x = load64(b, 0);
    let mut y = load64(b, total - 16) ^ K1;
    let mut z = load64(b, total - 56) ^ K0;
    let mut v = weak_hash_len32_with_seeds(b, total - 64, total as u64, y);
    let mut w = weak_hash_len32_with_seeds(b, total - 32, (total as u64).wrapping_mul(K1), K0);
    z = z.wrapping_add(shift_mix(v.1).wrapping_mul(K1));
    x = z.wrapping_add(x).rotate_right(39).wrapping_mul(K1);
    y = y.rotate_right(33).wrapping_mul(K1);
    let mut offset = 0usize;
    let mut len = (total - 1) & !63;
    loop {
        x = x
            .wrapping_add(y)
            .wrapping_add(v.0)
            .wrapping_add(load64(b, offset + 16))
            .rotate_right(37)
            .wrapping_mul(K1);
        y = y
            .wrapping_add(v.1)
            .wrapping_add(load64(b, offset + 48))
            .rotate_right(42)
            .wrapping_mul(K1);
        x ^= w.1;
        y ^= v.0;
        z = (z ^ w.0).rotate_right(33);
        v = weak_hash_len32_with_seeds(b, offset, v.1.wrapping_mul(K1), x.wrapping_add(w.0));
        w = weak_hash_len32_with_seeds(b, offset + 32, z.wrapping_add(w.1), y);
        std::mem::swap(&mut z, &mut x);
        offset += 64;
        len -= 64;
        if len == 0 {
            break;
        }
    }
    hash128_to_64(
        hash128_to_64(v.0, w.0)
            .wrapping_add(shift_mix(y).wrapping_mul(K1))
            .wrapping_add(z),
        hash128_to_64(v.1, w.1).wrapping_add(x),
    )
}

/// `Fingerprint2011` of `bytes`, equal to Guava's
/// `Hashing.fingerprint2011().hashBytes(bytes).asLong()` reinterpreted as `u64`.
#[must_use]
pub fn fingerprint2011(bytes: &[u8]) -> u64 {
    let len = bytes.len();
    let mut result = if len <= 32 {
        murmur_hash64_with_seed(bytes, K0 ^ K1 ^ K2)
    } else if len <= 64 {
        hash_length33_to64(bytes)
    } else {
        full_fingerprint(bytes)
    };
    let u = if len >= 8 { load64(bytes, 0) } else { K0 };
    let v = if len >= 9 { load64(bytes, len - 8) } else { K0 };
    result = hash128_to_64(result.wrapping_add(v), u);
    if result == 0 || result == 1 {
        result.wrapping_add(!1)
    } else {
        result
    }
}
