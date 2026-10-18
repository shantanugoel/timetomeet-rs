//! Compiled city/zone index and search.
//!
//! Replaces the old design, which `include_str!`'d a 690 KB CSV and then inserted all
//! 24,568 rows into a trie on the main thread *before first paint*. Here the index is
//! built at compile time by `tt-dataset` into a flat fixed-stride blob, fetched as its
//! own cacheable stream, and searched by a linear scan over typed arrays: n is ~12k, so
//! a scan is sub-millisecond and needs no runtime structure construction at all.

use chrono_tz::Tz;

use crate::model::SearchHit;
use crate::time::{fmt_date, fmt_hhmm, offset_minutes};

pub const MAGIC: &[u8; 4] = b"TTD1";
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 15;
/// Fixed stride: name(4+2) key(4+2) tz(2) lat(2) lon(2) pop(4) flags(1) cc(2) = 25 bytes.
pub const RECORD_LEN: usize = 25;

const FLAG_ZONE: u8 = 1 << 0;
const FLAG_HAS_KEY: u8 = 1 << 1;

#[derive(Clone, Copy)]
struct Record {
    name_off: u32,
    name_len: u16,
    key_off: u32,
    key_len: u16,
    tz_idx: u16,
    lat_100: i16,
    lon_100: i16,
    population: u32,
    flags: u8,
    /// ISO-3166 alpha-2, ASCII, zero-padded for pure zone entries.
    cc: [u8; 2],
}

pub struct Dataset {
    records: Vec<Record>,
    pool: Vec<u8>,
    tz_pool: Vec<u8>,
    tz_offsets: Vec<u32>,
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Dataset {
    pub fn parse(bytes: &[u8]) -> Result<Dataset, String> {
        if bytes.len() < HEADER_LEN {
            return Err("dataset too short".into());
        }
        if &bytes[0..4] != MAGIC {
            return Err("bad dataset magic (rebuild the dataset)".into());
        }
        if bytes[4] != VERSION {
            return Err(format!("unsupported dataset version {}", bytes[4]));
        }
        let count = u32le(bytes, 5) as usize;
        let pool_len = u32le(bytes, 9) as usize;
        let tz_count = u16le(bytes, 13) as usize;

        let need = HEADER_LEN + count * RECORD_LEN + 4 + pool_len + (tz_count + 1) * 4;
        if bytes.len() < need {
            return Err("dataset truncated".into());
        }

        let mut records = Vec::with_capacity(count);
        let mut o = HEADER_LEN;
        for _ in 0..count {
            let r = bytes[o..o + RECORD_LEN].to_vec();
            records.push(Record {
                name_off: u32le(&r, 0),
                name_len: u16le(&r, 4),
                key_off: u32le(&r, 6),
                key_len: u16le(&r, 10),
                tz_idx: u16le(&r, 12),
                lat_100: i16::from_le_bytes([r[14], r[15]]),
                lon_100: i16::from_le_bytes([r[16], r[17]]),
                population: u32le(&r, 18),
                flags: r[22],
                cc: [r[23], r[24]],
            });
            o += RECORD_LEN;
        }
        let pool_len_actual = u32le(bytes, o) as usize;
        o += 4;
        if o + pool_len_actual > bytes.len() {
            return Err("dataset string pool truncated".into());
        }
        let pool = bytes[o..o + pool_len_actual].to_vec();
        o += pool_len_actual;

        let mut tz_offsets = Vec::with_capacity(tz_count + 1);
        for _ in 0..=tz_count {
            if o + 4 > bytes.len() {
                return Err("dataset tz table truncated".into());
            }
            tz_offsets.push(u32le(bytes, o));
            o += 4;
        }
        let tz_pool = bytes[o..].to_vec();
        // The final offset is the end of the last zone name; if the pool is shorter than
        // that, the blob was truncated. Without this a byte-losing fetch silently yields a
        // mangled zone name and a person who cannot be placed.
        if tz_offsets.last().copied().unwrap_or(0) as usize > tz_pool.len() {
            return Err("dataset tz pool truncated".into());
        }

        Ok(Dataset {
            records,
            pool,
            tz_pool,
            tz_offsets,
        })
    }

    fn str(&self, off: u32, len: u16) -> &str {
        let (o, l) = (off as usize, len as usize);
        if o + l > self.pool.len() {
            return "";
        }
        std::str::from_utf8(&self.pool[o..o + l]).unwrap_or("")
    }

    pub fn tz_name(&self, idx: u16) -> &str {
        let i = idx as usize;
        if i + 1 >= self.tz_offsets.len() {
            return "";
        }
        let (a, b) = (self.tz_offsets[i] as usize, self.tz_offsets[i + 1] as usize);
        if b > self.tz_pool.len() || a > b {
            return "";
        }
        std::str::from_utf8(&self.tz_pool[a..b]).unwrap_or("")
    }

    pub fn city_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.flags & FLAG_ZONE == 0)
            .count()
    }
    pub fn zone_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.flags & FLAG_ZONE != 0)
            .count()
    }

    fn key_of(&self, r: &Record) -> &str {
        if r.flags & FLAG_HAS_KEY != 0 {
            self.str(r.key_off, r.key_len)
        } else {
            self.str(r.name_off, r.name_len)
        }
    }

    /// Prefix / word-boundary / substring / subsequence search, ranked by match quality
    /// then population. `query` is matched case-insensitively against the ASCII key.
    pub fn search(&self, query: &str, k: usize, at_utc: i64) -> Vec<SearchHit> {
        let q = normalize(query);
        if q.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(i64, usize)> = Vec::with_capacity(64);
        for (i, r) in self.records.iter().enumerate() {
            let key = self.key_of(r);
            let Some(base) = match_score(key, &q) else {
                continue;
            };
            // Population on a log scale so Mumbai outranks Mumbai-adjacent-village but a
            // perfect match on a small city still beats a fuzzy match on a megacity.
            let pop_bonus = (r.population as f64 + 1.0).log10() * 25.0;
            let zone_bonus = if r.flags & FLAG_ZONE != 0 { 40 } else { 0 };
            scored.push(((base + pop_bonus as i64 + zone_bonus), i));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        // An alias and its city are two records for one place, and showing "Bengaluru"
        // twice in a nine-row palette both wastes a slot and looks broken. Dedupe by the
        // place, not the record, and keep filling until k *distinct* places are returned.
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        scored
            .into_iter()
            .filter_map(|(_, i)| {
                let r = &self.records[i];
                let tz_name = self.tz_name(r.tz_idx);
                let tz: Tz = tz_name.parse().ok()?;
                Some(SearchHit {
                    name: self.str(r.name_off, r.name_len).to_string(),
                    country: cc_string(r.cc),
                    tz: tz_name.to_string(),
                    lat: r.lat_100 as f64 / 100.0,
                    lon: r.lon_100 as f64 / 100.0,
                    population: r.population,
                    is_zone: r.flags & FLAG_ZONE != 0,
                    offset_min: offset_minutes(at_utc, tz),
                    local_time: fmt_hhmm(at_utc, tz),
                    local_date: fmt_date(at_utc, tz),
                })
            })
            .filter(|h| seen.insert((h.name.clone(), h.tz.clone())))
            .take(k)
            .collect()
    }
}

fn cc_string(cc: [u8; 2]) -> String {
    let bytes: Vec<u8> = cc.into_iter().filter(|b| b.is_ascii_alphabetic()).collect();
    String::from_utf8(bytes).unwrap_or_default()
}

/// Lowercase, strip diacritics-ish punctuation, collapse spaces. Keys are built with
/// the same function, so the two sides can never drift.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if c == ' ' || c == '-' || c == '_' || c == '.' || c == '\'' || c == '/' {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
        } else {
            // Non-ASCII: fold to space so "München" still matches "munchen" poorly but
            // never produces a phantom token; the dataset stores an ASCII key too.
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
        }
    }
    out.trim().to_string()
}

/// Higher is better; `None` means no match at all.
fn match_score(key: &str, q: &str) -> Option<i64> {
    let k = normalize(key);
    if k.is_empty() {
        return None;
    }
    if k == q {
        return Some(1000);
    }
    if k.starts_with(q) {
        return Some(820 - k.len() as i64);
    }
    // Word-boundary prefix: "new york" matches "New York City" strongly.
    if k.contains(&format!(" {q}")) {
        return Some(720 - k.len() as i64);
    }
    if let Some(pos) = k.find(q) {
        return Some(600 - pos as i64 - (k.len() as i64) / 4);
    }
    // Subsequence catches typos and abbreviations typed in order: "blr" won't hit
    // "bengaluru", but "bengauru" will, and aliases cover the abbreviations.
    let mut it = k.chars();
    let mut used = 0usize;
    let mut span = 0usize;
    for need in q.chars() {
        let mut found = false;
        for have in it.by_ref() {
            span += 1;
            if have == need {
                used += 1;
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
    }
    if used == q.chars().count() {
        // Only accept a subsequence if it is reasonably tight, so "par" does not match
        // "Saint Petersburg".
        if span <= q.len() * 4 + 6 {
            return Some(300 - span as i64);
        }
    }
    None
}
