#![allow(dead_code)]

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut val, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let Some(d) = ALPHABET.iter().position(|&a| a == c) else {
            break;
        };
        val = (val << 6) | d as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((val >> bits) as u8);
        }
    }
    out
}

pub fn b64_encode(b: &[u8]) -> String {
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = chunk.len();
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= n {
                out.push(ALPHABET[((v >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn rows(tsv: &str) -> impl Iterator<Item = Vec<&str>> {
    tsv.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').collect())
}
