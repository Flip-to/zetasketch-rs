//! Driver for the Java ZetaSketch differential oracle (`oracle/Oracle.java`).
//!
//!     cargo run --release --example oracle_driver -- <seed> <scenarios> <ops.txt> <rust.txt>
//!
//! Generates random scenarios (INT64 and STRING sketches; values with negatives, extremes,
//! unicode and every Fingerprint2011 length branch; precisions 10..=16; adds interleaved with
//! merges of earlier emitted sketches), runs them through this crate, and writes the ops plus
//! this crate's output in the oracle's format. CI runs the same ops through Java and diffs.
//!
//! Merges only ever bring in sketches of equal or *higher* precision than the current one, or
//! lower it once; a lower -> higher -> lower chain would hit the known Java downgrade bug
//! (`docs/tech_debt/sparse_downgrade_buffer_encoding.md`), which this crate fixes on purpose.

use std::fmt::Write as _;
use std::process::ExitCode;

use zetasketch::HyperLogLogPlusPlus;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(b: &[u8]) -> String {
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

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn size(&mut self, max: u64) -> u64 {
        let bits = 64 - u64::from(max.leading_zeros());
        let b = self.below(bits + 1);
        self.below(1 << b).min(max)
    }
}

fn random_string(rng: &mut Rng) -> String {
    const ALPHABET: [char; 8] = ['a', 'Z', '0', ' ', '\u{fc}', '\u{5b57}', '\u{1f642}', '\t'];
    let len = rng.below(120) as usize; // crosses the 32 / 64 byte hash branches
    (0..len).map(|_| ALPHABET[rng.below(8) as usize]).collect()
}

fn hex(s: &str) -> String {
    if s.is_empty() {
        return "-".into();
    }
    let mut out = String::with_capacity(2 * s.len());
    for b in s.bytes() {
        write!(out, "{b:02x}").unwrap();
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [seed, scenarios, ops_path, rust_path] = &args[..] else {
        eprintln!("usage: oracle_driver <seed> <scenarios> <ops.txt> <rust.txt>");
        return ExitCode::from(2);
    };
    let mut rng = Rng(seed.parse::<u64>().unwrap_or(1) | 1);
    let scenarios: usize = scenarios.parse().unwrap_or(100);
    let (mut ops, mut expected) = (String::new(), String::new());
    // Emitted sketches available to later merges: (bytes, normal precision, is_strings).
    let mut emitted: Vec<(Vec<u8>, i32, bool)> = Vec::new();

    for _ in 0..scenarios {
        let strings = rng.below(3) == 0;
        let p = 10 + rng.below(7) as i32;
        let sp = (p + 5).min(25);
        writeln!(ops, "S {p} {sp} {}", if strings { 'S' } else { 'L' }).unwrap();
        let mut s = if strings {
            HyperLogLogPlusPlus::for_strings(p)
        } else {
            HyperLogLogPlusPlus::for_longs(p)
        }
        .unwrap();
        let mut current_p = p;
        let mut lowered = false;
        for _ in 0..=rng.below(3) {
            // Maybe merge an earlier sketch of the same kind first, while the add-buffer is
            // empty (fresh sketch, or right after an emit, which compacts): equal or higher
            // precision, or a single lowering. Java mishandles a lowering merge into a sparse
            // sketch with unflushed adds, the bug this crate fixes.
            let candidates: Vec<usize> = (0..emitted.len())
                .filter(|&i| {
                    let (_, ep, es) = &emitted[i];
                    *es == strings && (*ep >= current_p || !lowered)
                })
                .collect();
            if !candidates.is_empty() && rng.below(2) == 0 {
                let pick = candidates[rng.below(candidates.len() as u64) as usize];
                let (bytes, ep, _) = &emitted[pick];
                writeln!(ops, "M {}", b64(bytes)).unwrap();
                s.merge_bytes(bytes).unwrap();
                if *ep < current_p {
                    current_p = *ep;
                    lowered = true;
                }
            }
            // A batch of adds.
            for _ in 0..rng.size(6_000) {
                if strings {
                    let v = random_string(&mut rng);
                    writeln!(ops, "A {}", hex(&v)).unwrap();
                    s.add_str(&v).unwrap();
                } else {
                    let v = match rng.below(10) {
                        0 => [i64::MIN, i64::MAX, 0, -1, 1][rng.below(5) as usize],
                        1..=3 => -(rng.next() as i64).abs(),
                        _ => rng.below(5_000_000) as i64,
                    };
                    writeln!(ops, "A {v}").unwrap();
                    s.add_long(v).unwrap();
                }
            }
            writeln!(ops, "E").unwrap();
            let bytes = s.serialize();
            writeln!(expected, "{} {}", b64(&bytes), s.estimate()).unwrap();
            emitted.push((bytes, current_p, strings));
            if emitted.len() > 200 {
                emitted.remove(0);
            }
        }
    }
    if let Err(e) = std::fs::write(ops_path, ops).and_then(|()| std::fs::write(rust_path, expected))
    {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}
