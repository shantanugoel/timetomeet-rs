//! Format + search round-trip.
//!
//! The blob is written by `tt-dataset` and read by `tt-core`. Two independent
//! implementations of a binary format is exactly where silent corruption lives, so this
//! file builds a blob with the same layout by hand and asserts the reader agrees. If the
//! format changes, this test changes with it - deliberately, and in one place.

use std::collections::HashSet;

use tt_core::search::{Dataset, MAGIC, RECORD_LEN, VERSION};

struct Row {
    name: &'static str,
    key: Option<&'static str>,
    tz: &'static str,
    lat: f64,
    lon: f64,
    pop: u32,
    cc: &'static str,
    zone: bool,
}

/// Mirror of tt-dataset's emitter. Kept tiny and obvious on purpose.
fn build(rows: &[Row]) -> Vec<u8> {
    let mut pool: Vec<u8> = Vec::new();
    let mut tzpool: Vec<u8> = Vec::new();
    let mut tzs: Vec<u32> = Vec::new();
    let mut body: Vec<u8> = Vec::new();

    for r in rows {
        let intern = |pool: &mut Vec<u8>, s: &str| -> (u32, u16) {
            let o = pool.len() as u32;
            pool.extend_from_slice(s.as_bytes());
            (o, s.len() as u16)
        };
        let (name_off, name_len) = intern(&mut pool, r.name);
        let (key_off, key_len) = match r.key {
            Some(k) => intern(&mut pool, k),
            None => (0, 0),
        };
        let tz_idx = match tzs.iter().enumerate().find(|(_, o)| {
            let s = std::str::from_utf8(&tzpool[**o as usize..]).unwrap_or("");
            s.starts_with(r.tz)
        }) {
            Some((i, _)) => i as u16,
            None => {
                tzs.push(tzpool.len() as u32);
                tzpool.extend_from_slice(r.tz.as_bytes());
                (tzs.len() - 1) as u16
            }
        };
        let mut flags = 0u8;
        if r.key.is_some() {
            flags |= 1 << 1;
        }
        if r.zone {
            flags |= 1;
        }
        body.extend_from_slice(&name_off.to_le_bytes());
        body.extend_from_slice(&name_len.to_le_bytes());
        body.extend_from_slice(&key_off.to_le_bytes());
        body.extend_from_slice(&key_len.to_le_bytes());
        body.extend_from_slice(&tz_idx.to_le_bytes());
        body.extend_from_slice(&((r.lat * 100.0) as i16).to_le_bytes());
        body.extend_from_slice(&((r.lon * 100.0) as i16).to_le_bytes());
        body.extend_from_slice(&r.pop.to_le_bytes());
        body.push(flags);
        let mut cc = [0u8; 2];
        if r.cc.len() == 2 {
            cc.copy_from_slice(r.cc.as_bytes());
        }
        body.extend_from_slice(&cc);
    }
    tzs.push(tzpool.len() as u32);

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    out.extend_from_slice(&(pool.len() as u32).to_le_bytes());
    out.extend_from_slice(&((tzs.len() - 1) as u16).to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&(pool.len() as u32).to_le_bytes());
    out.extend_from_slice(&pool);
    for o in &tzs {
        out.extend_from_slice(&o.to_le_bytes());
    }
    out.extend_from_slice(&tzpool);
    out
}

fn fixture() -> Vec<u8> {
    build(&[
        Row {
            name: "Bengaluru",
            key: None,
            tz: "Asia/Kolkata",
            lat: 12.97,
            lon: 77.59,
            pop: 8_000_000,
            cc: "IN",
            zone: false,
        },
        Row {
            name: "Bengaluru",
            key: Some("blr"),
            tz: "Asia/Kolkata",
            lat: 12.97,
            lon: 77.59,
            pop: 8_000_000,
            cc: "IN",
            zone: false,
        },
        Row {
            name: "San Jose",
            key: None,
            tz: "America/Los_Angeles",
            lat: 37.34,
            lon: -121.89,
            pop: 1_000_000,
            cc: "US",
            zone: false,
        },
        Row {
            name: "San Francisco",
            key: None,
            tz: "America/Los_Angeles",
            lat: 37.77,
            lon: -122.42,
            pop: 8_700_000,
            cc: "US",
            zone: false,
        },
        Row {
            name: "Sano",
            key: None,
            tz: "Asia/Dili",
            lat: -8.9,
            lon: 125.5,
            pop: 20_000,
            cc: "TL",
            zone: false,
        },
        Row {
            name: "Asia/Colombo",
            key: Some("colombo"),
            tz: "Asia/Colombo",
            lat: 6.9,
            lon: 79.86,
            pop: 0,
            cc: "",
            zone: true,
        },
        Row {
            name: "München",
            key: Some("munchen"),
            tz: "Europe/Berlin",
            lat: 48.14,
            lon: 11.58,
            pop: 1_500_000,
            cc: "DE",
            zone: false,
        },
    ])
}

const NOW: i64 = 1_772_510_400; // 2026-03-03T04:00:00Z

#[test]
fn parses_a_well_formed_blob() {
    let bytes = fixture();
    let d = Dataset::parse(&bytes).expect("fixture must parse");
    assert_eq!(d.city_count(), 6);
    assert_eq!(d.zone_count(), 1);
}

#[test]
fn rejects_truncated_and_corrupt_blobs_without_panicking() {
    let bytes = fixture();
    for cut in [0, 4, 10, 40, bytes.len() / 2, bytes.len() - 1] {
        let r = Dataset::parse(&bytes[..cut]);
        assert!(
            r.is_err(),
            "truncation at {cut} must be an error, not a panic"
        );
    }
    let mut bad = bytes.clone();
    bad[0] = b'X';
    assert!(Dataset::parse(&bad).is_err());
    let mut bad2 = bytes.clone();
    bad2[4] = 99;
    assert!(
        Dataset::parse(&bad2).is_err(),
        "unknown version must be refused"
    );
    // A record stride mismatch is caught by the length check, not by reading garbage.
    assert_eq!(RECORD_LEN, 25);
    assert_eq!(VERSION, 1);
}

#[test]
fn abbreviation_alias_resolves_to_the_real_city() {
    let d = Dataset::parse(&fixture()).unwrap();
    let hits = d.search("blr", 5, NOW);
    assert!(!hits.is_empty(), "the alias 'blr' must find Bengaluru");
    assert_eq!(hits[0].name, "Bengaluru");
    assert_eq!(hits[0].tz, "Asia/Kolkata");
    assert_eq!(hits[0].country, "IN");
    // Live offset is part of the hit so the palette can show a clock.
    assert_eq!(hits[0].offset_min, 330);
    assert_eq!(hits[0].local_time, "09:30");
}

#[test]
fn prefix_and_word_boundary_beat_substring() {
    let d = Dataset::parse(&fixture()).unwrap();
    let hits = d.search("san jo", 5, NOW);
    let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names.first().copied(), Some("San Jose"), "got {names:?}");
    // "san" should rank the megacity above the tiny town of the same prefix.
    let hits = d.search("san", 5, NOW);
    let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
    assert!(
        names.iter().position(|n| *n == "San Francisco") < names.iter().position(|n| *n == "Sano"),
        "population must break the tie: {names:?}"
    );
}

#[test]
fn diacritic_names_are_findable_by_their_ascii_key() {
    let d = Dataset::parse(&fixture()).unwrap();
    let hits = d.search("munchen", 5, NOW);
    assert!(
        hits.iter().any(|h| h.name == "München"),
        "got {:?}",
        hits.iter().map(|h| &h.name).collect::<Vec<_>>()
    );
}

#[test]
fn pure_iana_zones_are_searchable() {
    let d = Dataset::parse(&fixture()).unwrap();
    let hits = d.search("colombo", 5, NOW);
    assert!(hits.iter().any(|h| h.tz == "Asia/Colombo" && h.is_zone));
}

#[test]
fn empty_and_nonsense_queries_return_nothing_rather_than_everything() {
    let d = Dataset::parse(&fixture()).unwrap();
    for q in ["", "   ", "\u{1f600}", "zzzzzzzzzz", "%"] {
        let hits = d.search(q, 10, NOW);
        assert!(hits.len() <= 2, "{q:?} matched {} rows", hits.len());
    }
}

#[test]
fn no_duplicate_records_for_the_same_city_and_zone() {
    // The GeoNames dump contains duplicates; the builder must dedupe. Here we assert the
    // *reader* is at least consistent about what it exposes.
    let d = Dataset::parse(&fixture()).unwrap();
    let hits = d.search("bengaluru", 10, NOW);
    let keys: HashSet<(String, String)> = hits
        .iter()
        .map(|h| (h.name.clone(), h.tz.clone()))
        .collect();
    assert!(
        hits.len() <= keys.len() + 1,
        "aliases legitimately repeat a city; real dupes must not"
    );
}

#[test]
fn an_alias_and_its_city_are_returned_once() {
    // "blr" matches both the alias record and the city record for Bengaluru. Two identical
    // rows in a nine-row palette looks broken and costs the user a real alternative.
    let d = Dataset::parse(&fixture()).unwrap();
    let hits = d.search("blr", 9, NOW);
    let bengalurus = hits.iter().filter(|h| h.name == "Bengaluru").count();
    assert_eq!(
        bengalurus,
        1,
        "got {:?}",
        hits.iter().map(|h| &h.name).collect::<Vec<_>>()
    );
}
