//! Builds `tt-dataset.bin` - the city/zone search index.
//!
//! Source: GeoNames `cities15000.txt` (tab-separated, public data, CC-BY 4.0 - the
//! attribution lives in the app footer) plus every zone in `chrono_tz::TIMEZONES` so
//! obscure zones stay findable by name.
//!
//! The output is a flat fixed-stride blob with a deduplicated string pool, designed to
//! be brotli-compressed and cached as its own asset. Nothing here is clever: a build
//! step that produces a 25-byte record is easier to trust than a runtime structure.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

const MAGIC: &[u8; 4] = b"TTD1";
const VERSION: u8 = 1;
const FLAG_ZONE: u8 = 1 << 0;
const FLAG_HAS_KEY: u8 = 1 << 1;

/// Common abbreviations, airport codes and colloquial names that no amount of fuzzy
/// string matching will connect to the official name. "blr" will never resemble
/// "Bengaluru"; the alias table is the honest fix.
const ALIASES: &[(&str, &str)] = &[
    ("blr", "Bengaluru"),
    ("bom", "Mumbai"),
    ("del", "Delhi"),
    ("hyd", "Hyderabad"),
    ("maa", "Chennai"),
    ("ccu", "Kolkata"),
    ("pnq", "Pune"),
    ("ixa", "Goa"),
    ("sfo", "San Francisco"),
    ("sjc", "San Jose"),
    ("lax", "Los Angeles"),
    ("sea", "Seattle"),
    ("nyc", "New York City"),
    (" NYC", "New York City"),
    ("bos", "Boston"),
    ("chi", "Chicago"),
    ("atl", "Atlanta"),
    ("aus", "Austin"),
    ("den", "Denver"),
    ("mia", "Miami"),
    ("yyz", "Toronto"),
    ("yvr", "Vancouver"),
    ("yul", "Montreal"),
    ("lhr", "London"),
    ("man", "Manchester"),
    ("dub", "Dublin"),
    ("cdg", "Paris"),
    ("ber", "Berlin"),
    ("mun", "Munich"),
    ("fra", "Frankfurt"),
    ("ams", "Amsterdam"),
    ("mad", "Madrid"),
    ("bcn", "Barcelona"),
    ("lis", "Lisbon"),
    ("zrh", "Zurich"),
    ("vie", "Vienna"),
    ("prg", "Prague"),
    ("waw", "Warsaw"),
    ("arn", "Stockholm"),
    ("osl", "Oslo"),
    ("cph", "Copenhagen"),
    ("hel", "Helsinki"),
    ("ist", "Istanbul"),
    ("ath", "Athens"),
    ("dxb", "Dubai"),
    ("auh", "Abu Dhabi"),
    ("rnl", "Riyadh"),
    ("ruh", "Riyadh"),
    ("doh", "Doha"),
    ("kwi", "Kuwait City"),
    ("amman", "Amman"),
    ("sin", "Singapore"),
    ("bkk", "Bangkok"),
    ("kul", "Kuala Lumpur"),
    ("mnl", "Manila"),
    ("cgk", "Jakarta"),
    ("sgn", "Ho Chi Minh City"),
    ("han", "Hanoi"),
    ("dad", "Da Nang"),
    ("hkg", "Hong Kong"),
    ("tpe", "Taipei"),
    ("icn", "Seoul"),
    ("gmp", "Seoul"),
    ("nrt", "Tokyo"),
    ("hnd", "Tokyo"),
    ("kix", "Osaka"),
    ("fuk", "Fukuoka"),
    ("syd", "Sydney"),
    ("mel", "Melbourne"),
    ("bne", "Brisbane"),
    ("per", "Perth"),
    ("akl", "Auckland"),
    ("jnb", "Johannesburg"),
    ("cpt", "Cape Town"),
    ("gru", "Sao Paulo"),
    ("cghe", "Sao Paulo"),
    ("gig", "Rio de Janeiro"),
    ("bue", "Buenos Aires"),
    ("scl", "Santiago"),
    ("bog", "Bogota"),
    ("lim", "Lima"),
    ("mex", "Mexico City"),
    ("mty", "Monterrey"),
    ("mow", "Moscow"),
    ("led", "Saint Petersburg"),
    ("kyiv", "Kiev"),
    ("pvg", "Shanghai"),
    ("pek", "Beijing"),
    ("can", "Guangzhou"),
    ("szx", "Shenzhen"),
    ("hgh", "Hangzhou"),
    ("chengdu", "Chengdu"),
];

#[derive(Clone)]
struct Entry {
    name: String,
    key: Option<String>,
    tz: String,
    lat: f64,
    lon: f64,
    pop: u32,
    cc: [u8; 2],
    zone: bool,
}

/// Must match `tt_core::search::normalize` exactly.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with(' ') {
            // Any separator or non-ASCII char collapses to a single space.
            out.push(' ');
        }
    }
    out.trim().to_string()
}

fn pool_intern(pool: &mut Vec<u8>, map: &mut HashMap<String, u32>, s: &str) -> u32 {
    if let Some(&o) = map.get(s) {
        return o;
    }
    let o = pool.len() as u32;
    pool.extend_from_slice(s.as_bytes());
    map.insert(s.to_string(), o);
    o
}

/// Optional zone.tab for coordinates of pure-zone entries.
fn load_zone_tab(path: Option<&str>) -> HashMap<String, (f64, f64)> {
    let mut m = HashMap::new();
    let Some(p) = path else { return m };
    let Ok(f) = std::fs::File::open(p) else {
        eprintln!("note: cannot open zone.tab at {p}; zone entries get 0,0");
        return m;
    };
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() < 3 {
            continue;
        }
        let coords: Vec<&str> = c[2].split('/').collect();
        if coords.len() < 2 {
            continue;
        }
        let (Some(la), Some(lo)) = (parse_coord(coords[0]), parse_coord(coords[1])) else {
            continue;
        };
        m.insert(c[2].to_string(), (la, lo));
    }
    m
}

/// zone.tab coords look like `+40n45` or `-2352-04637` (DDMM[.SS] with sign prefix).
fn parse_coord(s: &str) -> Option<f64> {
    let neg = s.starts_with('-');
    let t = s.trim_start_matches(['+', '-']);
    let digits: String = t
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.len() < 4 {
        return None;
    }
    let (dd, mm) = digits.split_at(digits.find('.').map(|i| i - 2).unwrap_or(2));
    let d: f64 = dd.parse().ok()?;
    let m: f64 = mm.trim_start_matches('.').parse().unwrap_or(0.0);
    let v = d + m / 60.0;
    Some(if neg { -v } else { v })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input: Option<String> = None;
    let mut out = PathBuf::from("app/public/tt-dataset.bin");
    let mut min_pop: u32 = 50_000;
    let mut zone_tab: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--in" => {
                input = args.get(i + 1).cloned();
                i += 2;
            }
            "--out" => {
                out = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
                i += 2;
            }
            "--min-pop" => {
                min_pop = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(50_000);
                i += 2;
            }
            "--zone-tab" => {
                zone_tab = args.get(i + 1).cloned();
                i += 2;
            }
            other => {
                eprintln!("warning: ignoring argument {other:?}");
                i += 1;
            }
        }
    }
    let input = input.ok_or(
        "usage: tt-dataset --in cities15000.txt [--out path] [--min-pop N] [--zone-tab path]",
    )?;

    let zone_coords = load_zone_tab(zone_tab.as_deref());

    // ---- cities -------------------------------------------------------------
    let file = std::fs::File::open(&input)?;
    let mut entries: Vec<Entry> = Vec::with_capacity(12_000);
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut rows = 0usize;
    let mut kept = 0usize;

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        rows += 1;
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() < 19 {
            continue;
        }
        let (name, ascii, lat, lon, cc, tz) = (c[1], c[2], c[4], c[5], c[8], c[17]);
        let pop: u32 = c[14].parse().unwrap_or(0);
        if pop < min_pop || name.is_empty() || tz.is_empty() {
            continue;
        }
        let (Ok(la), Ok(lo)) = (lat.parse::<f64>(), lon.parse::<f64>()) else {
            continue;
        };
        let key = normalize(ascii);
        if key.is_empty() {
            continue;
        }
        if !seen.insert((key.clone(), tz.to_string())) {
            continue; // same city, same zone - GeoNames has duplicates
        }
        if cc.len() != 2 {
            continue;
        }
        let mut cc2 = [0u8; 2];
        cc2.copy_from_slice(&cc.as_bytes()[..2]);
        entries.push(Entry {
            name: name.to_string(),
            key: if normalize(name) == key {
                None
            } else {
                Some(key)
            },
            tz: tz.to_string(),
            lat: la,
            lon: lo,
            pop,
            cc: cc2,
            zone: false,
        });
        kept += 1;
    }

    // ---- aliases ------------------------------------------------------------
    // Point an abbreviation at the largest city with a matching normalised name.
    let mut best: HashMap<String, usize> = HashMap::new();
    for (idx, e) in entries.iter().enumerate() {
        let k = normalize(&e.name);
        match best.get(&k) {
            Some(&cur) if entries[cur].pop >= e.pop => {}
            _ => {
                best.insert(k, idx);
            }
        }
    }
    let mut alias_added = 0usize;
    let mut alias_seen: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    for (alias, target) in ALIASES {
        let t = normalize(target);
        let Some(&idx) = best.get(&t) else { continue };
        let e = &entries[idx];
        let ak = normalize(alias);
        if ak.is_empty() || !alias_seen.insert((ak.clone(), e.tz.clone())) {
            continue;
        }
        entries.push(Entry {
            name: e.name.clone(),
            key: Some(ak),
            tz: e.tz.clone(),
            lat: e.lat,
            lon: e.lon,
            pop: e.pop,
            cc: e.cc,
            zone: false,
        });
        alias_added += 1;
    }

    // ---- every IANA zone, so "Asia/Colombo" is findable ---------------------
    let mut zone_added = 0usize;
    let mut zone_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for tz in chrono_tz::TZ_VARIANTS.iter() {
        let name = tz.name().to_string();
        if !zone_seen.insert(name.clone()) {
            continue;
        }
        let (la, lo) = zone_coords.get(&name).copied().unwrap_or((0.0, 0.0));
        entries.push(Entry {
            name: name.clone(),
            key: Some(normalize(&name)),
            tz: name,
            lat: la,
            lon: lo,
            pop: 0,
            cc: [0, 0],
            zone: true,
        });
        zone_added += 1;
    }

    // ---- emit ---------------------------------------------------------------
    let mut tz_offsets: Vec<u32> = Vec::new();
    let mut tz_pool: Vec<u8> = Vec::new();
    let mut tz_map: HashMap<String, u16> = HashMap::new();
    for e in &entries {
        if tz_map.contains_key(&e.tz) {
            continue;
        }
        tz_map.insert(e.tz.clone(), tz_offsets.len() as u16);
        tz_offsets.push(tz_pool.len() as u32);
        tz_pool.extend_from_slice(e.tz.as_bytes());
    }
    tz_offsets.push(tz_pool.len() as u32);

    let mut pool: Vec<u8> = Vec::new();
    let mut pool_map: HashMap<String, u32> = HashMap::new();
    // Sort cities before zones, then by population descending, so the blob's natural
    // order is also "most useful first".
    entries.sort_by(|a, b| {
        a.zone
            .cmp(&b.zone)
            .then_with(|| b.pop.cmp(&a.pop))
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut body: Vec<u8> = Vec::with_capacity(entries.len() * 25);
    for e in &entries {
        let name_off = pool_intern(&mut pool, &mut pool_map, &e.name);
        let name_len = e.name.len() as u16;
        let (key_off, key_len, mut flags) = match &e.key {
            Some(k) if normalize(&e.name) != *k => {
                let o = pool_intern(&mut pool, &mut pool_map, k);
                (o, k.len() as u16, FLAG_HAS_KEY)
            }
            _ => (0, 0, 0),
        };
        if e.zone {
            flags |= FLAG_ZONE;
        }
        let tz_idx = tz_map[&e.tz];
        let lat100 = (e.lat * 100.0).round().clamp(-9000.0, 9000.0) as i16;
        let lon100 = (e.lon * 100.0).round().clamp(-18000.0, 18000.0) as i16;

        body.extend_from_slice(&name_off.to_le_bytes());
        body.extend_from_slice(&name_len.to_le_bytes());
        body.extend_from_slice(&key_off.to_le_bytes());
        body.extend_from_slice(&key_len.to_le_bytes());
        body.extend_from_slice(&tz_idx.to_le_bytes());
        body.extend_from_slice(&lat100.to_le_bytes());
        body.extend_from_slice(&lon100.to_le_bytes());
        body.extend_from_slice(&e.pop.to_le_bytes());
        body.push(flags);
        body.extend_from_slice(&e.cc);
        debug_assert_eq!(body.len() % 25, 0);
    }

    let mut out_bytes: Vec<u8> = Vec::with_capacity(14 + body.len() + pool.len());
    out_bytes.extend_from_slice(MAGIC);
    out_bytes.push(VERSION);
    out_bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    out_bytes.extend_from_slice(&(pool.len() as u32).to_le_bytes());
    out_bytes.extend_from_slice(&(tz_offsets.len() as u16 - 1).to_le_bytes());
    out_bytes.extend_from_slice(&body);
    out_bytes.extend_from_slice(&(pool.len() as u32).to_le_bytes());
    out_bytes.extend_from_slice(&pool);
    for o in &tz_offsets {
        out_bytes.extend_from_slice(&o.to_le_bytes());
    }
    out_bytes.extend_from_slice(&tz_pool);

    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let mut f = std::fs::File::create(&out)?;
    f.write_all(&out_bytes)?;

    println!(
        "rows={rows} cities={kept} aliases={alias_added} zones={zone_added} records={} raw={} KiB -> {}",
        entries.len(),
        out_bytes.len() / 1024,
        out.display()
    );
    Ok(())
}
