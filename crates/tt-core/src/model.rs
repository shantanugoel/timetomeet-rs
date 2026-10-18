//! Core data model. Everything here is pure data + fallible parsing; nothing panics
//! on bad input, because the old build's `unwrap()` on user input is exactly the bug
//! that killed the live site (see docs/REDESIGN.md §0.1).

use chrono::{NaiveDate, TimeZone};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

/// Sampling and snapping granularity in minutes. The grid, the bands and the slot
/// search all share this, so a band boundary can never disagree with a snap target.
pub const SNAP_MIN: i32 = 15;
pub const SAMPLES_PER_DAY: usize = 96;
pub const MINUTES_PER_DAY: i32 = 1440;
pub const SECONDS_PER_DAY: i64 = 86_400;

/// Parse an IANA zone name without panicking. Every caller must handle `None`.
pub fn parse_tz(name: &str) -> Option<Tz> {
    if name.trim().is_empty() {
        return None;
    }
    name.trim().parse::<Tz>().ok()
}

/// The local date at a unix instant in a zone.
pub fn local_date_of(ts: i64, tz: Tz) -> NaiveDate {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|u| tz.from_utc_datetime(&u.naive_utc()).date_naive())
        .unwrap_or_else(|| chrono::Utc::now().date_naive())
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// The person who owns the meeting.
    Organizer,
    #[default]
    Required,
    Optional,
    /// Genuinely skippable - should almost never drag the score.
    Peripheral,
}

impl Role {
    pub fn weight(self) -> f64 {
        match self {
            Role::Organizer | Role::Required => 1.0,
            Role::Optional => 0.3,
            Role::Peripheral => 0.15,
        }
    }
    pub fn parse(s: &str) -> Option<Role> {
        Some(match s.to_ascii_lowercase().as_str() {
            "o" | "organizer" => Role::Organizer,
            "r" | "required" => Role::Required,
            "opt" | "optional" => Role::Optional,
            "p" | "peripheral" => Role::Peripheral,
            _ => return None,
        })
    }
    pub fn code(self) -> &'static str {
        match self {
            Role::Organizer => "o",
            Role::Required => "r",
            Role::Optional => "opt",
            Role::Peripheral => "p",
        }
    }
}

/// Which days are off *where this person actually lives*. Almost no planning tool
/// models this, and it is the difference between suggesting 10:00 on a Friday in
/// Riyadh and suggesting it in Berlin.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub enum Weekend {
    #[default]
    SatSun,
    FriSat,
    FriSun,
    SunThu,
    None,
}

impl Weekend {
    pub fn contains(self, wd: chrono::Weekday) -> bool {
        use chrono::Weekday as W;
        match self {
            Weekend::SatSun => matches!(wd, W::Sat | W::Sun),
            Weekend::FriSat => matches!(wd, W::Fri | W::Sat),
            Weekend::FriSun => matches!(wd, W::Fri | W::Sun),
            Weekend::SunThu => matches!(wd, W::Sun | W::Thu),
            Weekend::None => false,
        }
    }
    pub fn parse(s: &str) -> Option<Weekend> {
        Some(match s.to_ascii_lowercase().as_str() {
            "ss" | "satsun" => Weekend::SatSun,
            "fs" | "frisat" => Weekend::FriSat,
            "fu" | "frisun" => Weekend::FriSun,
            "st" | "sunthu" => Weekend::SunThu,
            "n" | "none" => Weekend::None,
            _ => return None,
        })
    }
    pub fn code(self) -> &'static str {
        match self {
            Weekend::SatSun => "ss",
            Weekend::FriSat => "fs",
            Weekend::FriSun => "fu",
            Weekend::SunThu => "st",
            Weekend::None => "n",
        }
    }
}

/// Visual band. Derived from cost by threshold, so there is exactly one opinion about
/// how bad a moment is - the cost function - and the colours can never disagree with
/// the score.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Working,
    Shoulder,
    OffHours,
    Asleep,
}

impl Quality {
    pub const fn from_cost(c: f64) -> Quality {
        if c <= 0.01 {
            Quality::Working
        } else if c <= 0.35 {
            Quality::Shoulder
        } else if c <= 1.0 {
            Quality::OffHours
        } else {
            Quality::Asleep
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Quality::Working => "working",
            Quality::Shoulder => "shoulder",
            Quality::OffHours => "off hours",
            Quality::Asleep => "asleep",
        }
    }
    /// Ordered so the worst quality wins when summarising a whole window.
    pub const fn rank(self) -> u8 {
        match self {
            Quality::Working => 0,
            Quality::Shoulder => 1,
            Quality::OffHours => 2,
            Quality::Asleep => 3,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Person {
    pub id: u8,
    pub name: String,
    pub city: String,
    pub tz: String,
    /// Minutes from local midnight. 540 = 09:00.
    pub work_start_min: u16,
    pub work_end_min: u16,
    /// Declared tolerance outside the stated window; turns "off hours" into "shoulder".
    pub flex_min: u16,
    pub role: Role,
    pub weekend: Weekend,
    pub lat: f64,
    pub lon: f64,
}

impl Person {
    pub fn new(id: u8, name: &str, city: &str, tz: &str) -> Person {
        Person {
            id,
            name: name.to_string(),
            city: city.to_string(),
            tz: tz.to_string(),
            work_start_min: 540,
            work_end_min: 1080,
            flex_min: 60,
            role: Role::Required,
            weekend: Weekend::SatSun,
            lat: 0.0,
            lon: 0.0,
        }
    }
    pub fn parsed_tz(&self) -> Option<Tz> {
        parse_tz(&self.tz)
    }
    pub fn work_label(&self) -> String {
        format!("{}-{}", hhmm(self.work_start_min), hhmm(self.work_end_min))
    }
}

/// `540 -> "09:00"`.
pub fn hhmm(mins: u16) -> String {
    let m = mins % MINUTES_PER_DAY as u16;
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// `540 -> "9:00 AM"`, for people who live in 12-hour land.
pub fn hhmm12(mins: u16) -> String {
    let m = (mins % MINUTES_PER_DAY as u16) as i32;
    let h = m / 60;
    let mm = m % 60;
    let (h12, ap) = match h {
        0 => (12, "AM"),
        12..=23 => (h - 12, "PM"),
        h => (h, "AM"),
    };
    format!("{h12}:{mm:02} {ap}")
}

/// `+330` / `-0700` from a minute offset.
pub fn offset_label(min: i32) -> String {
    let sign = if min < 0 { '-' } else { '+' };
    let a = min.abs();
    format!("{sign}{:02}:{:02}", a / 60, a % 60)
}

#[derive(Serialize, Clone, Copy, Debug)]
pub struct Band {
    /// Minutes from grid start, half-open: [start_min, end_min).
    pub start_min: u16,
    pub end_min: u16,
    pub quality: Quality,
    /// True when this band's local date is a weekend day for this person. The view
    /// hatches it rather than inventing a fifth colour.
    pub weekend: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct MidnightMark {
    pub grid_min: u16,
    /// The local date that *begins* at this mark, e.g. "Wed 4 Mar".
    pub date_label: String,
}

/// One row of the grid: everything the view needs to paint a person, and nothing it
/// has to compute. Bands are a run-length list so the view can turn a whole row into
/// one CSS gradient without touching 96 nodes - which is what makes dragging free.
#[derive(Serialize, Clone, Debug)]
pub struct Row {
    pub person_id: u8,
    pub name: String,
    pub city: String,
    pub tz: String,
    pub ok: bool,
    pub error: Option<String>,
    pub bands: Vec<Band>,
    /// Local clock at each hour boundary of the grid, 25 entries, `"%H:%M"`.
    pub hour_labels: Vec<String>,
    /// Offset from UTC at grid start, in minutes.
    pub offset_min: i32,
    /// Local midnights inside the grid - the heavy line and the date labels.
    pub midnights: Vec<MidnightMark>,
    /// Where their "now" sits on this grid, in minutes; `None` if outside the window.
    pub now_min: Option<u16>,
    pub sunrise_min: Option<u16>,
    pub sunset_min: Option<u16>,
    /// A DST transition happens inside this grid for this person - flag it loudly,
    /// because it silently moves their clock relative to everyone else's.
    pub dst_change: bool,
    pub date_label: String,
    pub work_label: String,
    pub role: Role,
}

#[derive(Serialize, Clone, Copy, Debug, Default)]
pub struct Counts {
    pub working: u8,
    pub shoulder: u8,
    pub off_hours: u8,
    pub asleep: u8,
}

impl Counts {
    pub fn add(&mut self, q: Quality) {
        match q {
            Quality::Working => self.working += 1,
            Quality::Shoulder => self.shoulder += 1,
            Quality::OffHours => self.off_hours += 1,
            Quality::Asleep => self.asleep += 1,
        }
    }
}

/// Per-person outcome for one candidate window. `reason` is the product: the sentence
/// that tells you *who* is suffering and *by how much*.
#[derive(Serialize, Clone, Debug)]
pub struct PersonVerdict {
    pub person_id: u8,
    pub name: String,
    pub city: String,
    pub local_start: String,
    pub local_end: String,
    pub local_date: String,
    pub offset_min: i32,
    pub cost: f64,
    pub quality: Quality,
    pub crosses_midnight: bool,
    /// Minutes of the meeting outside their stated window (flex included).
    pub clipped_min: u16,
    pub reason: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Verdict {
    pub start_utc: i64,
    pub duration_min: u16,
    pub score: u8,
    pub label: String,
    pub total: f64,
    pub worst_off: f64,
    pub worst_name: Option<String>,
    pub equity: f64,
    pub coverage: f64,
    pub counts: Counts,
    pub persons: Vec<PersonVerdict>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Slot {
    pub start_utc: i64,
    pub duration_min: u16,
    pub score: u8,
    pub label: String,
    pub counts: Counts,
    pub worst_name: Option<String>,
    /// Date in the viewer's zone. The list can leave the anchor day (see `suggest`).
    pub date_label: String,
    pub is_anchor: bool,
    pub headline: String,
    pub persons: Vec<PersonVerdict>,
}

/// The shared 24h window, anchored at local midnight in the viewer's zone.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub start_utc: i64,
    pub viewer_tz: Tz,
    pub anchor_date: NaiveDate,
}

#[derive(Serialize, Clone, Debug)]
pub struct SearchHit {
    pub name: String,
    pub country: String,
    pub tz: String,
    pub lat: f64,
    pub lon: f64,
    pub population: u32,
    /// True for pure IANA zone entries ("Asia/Colombo") rather than cities.
    pub is_zone: bool,
    /// Offset at the search instant, in minutes, so the palette can show a live clock.
    pub offset_min: i32,
    pub local_time: String,
    pub local_date: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct AppInfo {
    pub version: &'static str,
    pub cities: usize,
    pub zones: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct SolarTimes {
    pub sunrise_min: Option<u16>,
    pub sunset_min: Option<u16>,
    /// Polar day or night - the view shows a marker instead of nothing.
    pub polar: bool,
}
