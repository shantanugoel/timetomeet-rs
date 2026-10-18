//! Versioned app-state codec for the URL hash.
//!
//! This is the whole sharing story: no backend, no accounts, nothing to delete. The
//! meeting *is* the URL. Because it is user-facing and hand-editable, it is versioned
//! and decodes defensively - a v2 link must never take the app down, and a corrupt
//! link must fall back to a sane default rather than panic (the old build's habit).

use crate::model::{Person, Role, Weekend};

pub const V: &str = "1";

#[derive(Clone, Debug)]
pub struct AppState {
    pub anchor_date: String, // YYYY-MM-DD, in the viewer's zone
    pub viewer_tz: String,
    pub duration_min: u16,
    pub start_utc: Option<i64>,
    pub people: Vec<Person>,
    pub group_name: Option<String>,
}

fn enc(s: &str, out: &mut String) {
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(*b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hi = (b[i + 1] as char).to_digit(16);
            let lo = (b[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// `HH:MM` or `HHMM` -> minutes.
fn parse_hhmm(s: &str) -> Option<u16> {
    let t = s.replace(':', "");
    if t.len() != 4 || !t.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let h: u16 = t[0..2].parse().ok()?;
    let m: u16 = t[2..4].parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

impl AppState {
    pub fn encode(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str(&format!("v={V}&d={}&z=", self.anchor_date));
        enc(&self.viewer_tz, &mut out);
        out.push_str(&format!("&dur={}", self.duration_min));
        if let Some(s) = self.start_utc {
            out.push_str(&format!("&s={s}"));
        }
        if let Some(g) = &self.group_name {
            if !g.is_empty() {
                out.push_str("&g=");
                enc(g, &mut out);
            }
        }
        if !self.people.is_empty() {
            out.push_str("&p=");
            for (i, p) in self.people.iter().enumerate() {
                if i > 0 {
                    out.push('|');
                }
                enc(&p.name, &mut out);
                out.push('~');
                enc(&p.city, &mut out);
                out.push('~');
                enc(&p.tz, &mut out);
                out.push('~');
                // HH:MM, not raw minutes: "0480" is not a time.
                out.push_str(&crate::model::hhmm(p.work_start_min));
                out.push('-');
                out.push_str(&crate::model::hhmm(p.work_end_min));
                out.push('~');
                out.push_str(&p.flex_min.to_string());
                out.push('~');
                out.push_str(p.role.code());
                out.push('~');
                out.push_str(p.weekend.code());
                if p.lat != 0.0 || p.lon != 0.0 {
                    out.push_str(&format!("~{:.2},{:.2}", p.lat, p.lon));
                }
            }
        }
        out
    }

    /// Never fails: unknown fields are ignored, malformed people are skipped.
    pub fn decode(raw: &str) -> AppState {
        let raw = raw.trim_start_matches('#');
        let mut st = AppState {
            anchor_date: String::new(),
            viewer_tz: String::new(),
            duration_min: 60,
            start_utc: None,
            people: Vec::new(),
            group_name: None,
        };
        for pair in raw.split('&') {
            let (k, v) = match pair.split_once('=') {
                Some(x) => x,
                None => continue,
            };
            match k {
                "d" => {
                    if parse_date(v).is_some() {
                        st.anchor_date = v.to_string();
                    }
                }
                "z" => {
                    let z = dec(v);
                    if crate::model::parse_tz(&z).is_some() {
                        st.viewer_tz = z;
                    }
                }
                "dur" => {
                    if let Ok(d) = v.parse::<u16>() {
                        if (15..=1440).contains(&d) {
                            st.duration_min = d;
                        }
                    }
                }
                "s" => st.start_utc = v.parse::<i64>().ok(),
                "g" => st.group_name = Some(dec(v)),
                "p" => {
                    for chunk in v.split('|') {
                        if let Some(p) = decode_person(chunk) {
                            if st.people.len() < 32 {
                                st.people.push(p);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        st
    }

    pub fn is_empty(&self) -> bool {
        self.people.is_empty() && self.anchor_date.is_empty()
    }
}

fn decode_person(chunk: &str) -> Option<Person> {
    let f: Vec<&str> = chunk.split('~').collect();
    if f.len() < 7 {
        return None;
    }
    let (name, city, tz) = (dec(f[0]), dec(f[1]), dec(f[2]));
    crate::model::parse_tz(&tz)?;
    let (mut ws, mut we) = (540u16, 1080u16);
    if let Some((a, b)) = f[3].split_once('-') {
        ws = parse_hhmm(a).unwrap_or(540);
        we = parse_hhmm(b).unwrap_or(1080);
    }
    let flex = f[4].parse::<u16>().unwrap_or(60).min(240);
    let role = Role::parse(f[5]).unwrap_or_default();
    let weekend = Weekend::parse(f[6]).unwrap_or_default();
    let mut p = Person::new(0, &name, &city, &tz);
    p.work_start_min = ws;
    p.work_end_min = we;
    p.flex_min = flex;
    p.role = role;
    p.weekend = weekend;
    if let Some(ll) = f.get(7) {
        if let Some((la, lo)) = ll.split_once(',') {
            p.lat = la.parse().unwrap_or(0.0);
            p.lon = lo.parse().unwrap_or(0.0);
        }
    }
    Some(p)
}
