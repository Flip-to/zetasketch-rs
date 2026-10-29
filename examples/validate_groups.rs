//! Check `Accumulator` against BigQuery on your own data.
//!
//!     cargo run --release --example validate_groups -- <rows.tsv> <expected.tsv> [--trusted]
//!     cargo run --release --example validate_groups -- --self-check
//!
//! `rows.tsv`: one stored sketch per line, `group_key<TAB>base64(HLL_COUNT.INIT bytes)`.
//! `expected.tsv`: BigQuery's result per group,
//! `group_key<TAB>base64(HLL_COUNT.MERGE_PARTIAL)<TAB>HLL_COUNT.MERGE`.
//! Lines starting with `#` are ignored. See `docs/guides/real_data_validation.md` for the
//! export queries.
//!
//! Folds each group's rows in file order with `Accumulator`, compares the estimate with
//! `HLL_COUNT.MERGE` and the serialized bytes with `HLL_COUNT.MERGE_PARTIAL`, prints every
//! mismatch, and exits non-zero if there is one. `--self-check` generates synthetic data and
//! checks it against the reference fold, so this example stays runnable in CI.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::Instant;

use zetasketch::{Accumulator, HyperLogLogPlusPlus};

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut val, mut bits) = (0u32, 0);
    for c in s.trim().bytes() {
        if c == b'=' {
            break;
        }
        let d = B64
            .iter()
            .position(|&a| a == c)
            .ok_or_else(|| format!("invalid base64 character {:?}", c as char))?;
        val = (val << 6) | d as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((val >> bits) as u8);
        }
    }
    Ok(out)
}

fn b64_encode(b: &[u8]) -> String {
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                B64[((v >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// Group key -> sketches in input order.
type Groups = BTreeMap<String, Vec<Vec<u8>>>;
/// Group key -> (expected MERGE_PARTIAL bytes, expected MERGE estimate).
type Expected = BTreeMap<String, (Vec<u8>, i64)>;

fn lines(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').collect())
}

fn read_inputs(rows_path: &str, expected_path: &str) -> Result<(Groups, Expected), String> {
    let rows = std::fs::read_to_string(rows_path).map_err(|e| format!("{rows_path}: {e}"))?;
    let exp =
        std::fs::read_to_string(expected_path).map_err(|e| format!("{expected_path}: {e}"))?;
    let mut groups = Groups::new();
    for (i, f) in lines(&rows).enumerate() {
        let [key, sketch] = f[..] else {
            return Err(format!("{rows_path}:{}: expected 2 columns", i + 1));
        };
        groups
            .entry(key.to_string())
            .or_default()
            .push(b64_decode(sketch)?);
    }
    let mut expected = Expected::new();
    for (i, f) in lines(&exp).enumerate() {
        let [key, merged, est] = f[..] else {
            return Err(format!("{expected_path}:{}: expected 3 columns", i + 1));
        };
        let est = est
            .trim()
            .parse()
            .map_err(|e| format!("{expected_path}:{}: {e}", i + 1))?;
        expected.insert(key.to_string(), (b64_decode(merged)?, est));
    }
    Ok((groups, expected))
}

/// Synthetic stand-in for an export: expectations from the reference fold.
fn self_check_inputs() -> (Groups, Expected) {
    let mut seed = 0x5e1f_c4ec_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut groups = Groups::new();
    let mut expected = Expected::new();
    for g in 0..200 {
        let key = format!("group_{g:03}");
        let rows: Vec<Vec<u8>> = (0..=(next() % 40))
            .map(|_| {
                let mut s = HyperLogLogPlusPlus::for_longs(15).unwrap();
                let start = (next() % 1_000_000) as i64;
                for v in start..start + (next() % 3_000) as i64 {
                    s.add_long(v).unwrap();
                }
                s.serialize()
            })
            .collect();
        let mut reference = HyperLogLogPlusPlus::from_bytes(&rows[0]).unwrap();
        for r in &rows[1..] {
            reference.merge_bytes(r).unwrap();
        }
        let est = reference.estimate();
        expected.insert(key.clone(), (reference.serialize(), est));
        groups.insert(key, rows);
    }
    (groups, expected)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let trusted = args.iter().any(|a| a == "--trusted");
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let (groups, expected) = if args.iter().any(|a| a == "--self-check") {
        self_check_inputs()
    } else if let [rows, exp] = positional[..] {
        match read_inputs(rows, exp) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        eprintln!("usage: validate_groups <rows.tsv> <expected.tsv> [--trusted] | --self-check");
        return ExitCode::from(2);
    };

    let (mut ok, mut bad, mut sketches) = (0usize, 0usize, 0usize);
    let mut acc = Accumulator::new();
    let start = Instant::now();
    for (key, rows) in &groups {
        let Some((want_bytes, want_est)) = expected.get(key) else {
            println!("MISSING expectation for group {key}");
            bad += 1;
            continue;
        };
        acc.reset();
        for r in rows {
            let added = if trusted {
                acc.add_trusted_bytes(r)
            } else {
                acc.add_bytes(r)
            };
            if let Err(e) = added {
                println!("ERROR group {key}: {e}");
                bad += 1;
            }
        }
        sketches += rows.len();
        let est = acc.cardinality();
        let bytes = acc.serialize().unwrap_or_default();
        if est == *want_est && bytes == *want_bytes {
            ok += 1;
        } else {
            bad += 1;
            println!(
                "MISMATCH group {key}: estimate {est} vs MERGE {want_est}; bytes {} \
                 (ours {} vs MERGE_PARTIAL {})",
                if bytes == *want_bytes {
                    "equal"
                } else {
                    "differ"
                },
                b64_encode(&bytes).len(),
                b64_encode(want_bytes).len()
            );
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "{ok} groups match, {bad} mismatches; {sketches} sketches folded in {:.1} ms ({:.0} ns/sketch)",
        secs * 1e3,
        secs * 1e9 / sketches.max(1) as f64
    );
    if bad == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
