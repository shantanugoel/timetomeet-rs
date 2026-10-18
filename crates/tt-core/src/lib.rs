//! # tt-core
//!
//! The timezone-overlap engine behind timetomeet.shantanugoel.com, the `tt` CLI, and
//! anything else that needs to answer "when can all of us talk?".
//!
//! Deliberately free of any browser or DOM binding: the whole engine runs under plain
//! `cargo test` on the host, which is how the DST behaviour is pinned down (§ see
//! `tests/dst.rs`). The wasm layer is a thin facade over the handful of functions
//! below.
//!
//! ## The two rules
//! 1. **Absolute instants in, wall clocks out.** Never anchor a computation on a local
//!    wall clock; derive it. That is what makes DST correct by construction instead of
//!    by special case.
//! 2. **Nothing panics on input.** Every parse is fallible and every caller handles
//!    `None`. The previous build `unwrap()`ed a date input and permanently killed the
//!    live site.

mod day;
mod ics;
pub mod model;
mod score;
pub mod search;
mod solar;
mod state;
pub mod time;

pub use day::{build_row, darkness_profile};
pub use ics::{build_ics, build_plain_text, IcsOptions};
pub use model::*;
pub use score::{cost_at, score_label, suggest, verdict};
pub use search::Dataset;
pub use solar::sun_instants;
pub use state::AppState;

use chrono::NaiveDate;
use chrono_tz::Tz;
use serde::Serialize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One 24h column of the grid. A multi-day span is a list of these, each with its own
/// grid start, ruler and earth shadow - the x-axis is "minutes of that day", so the view
/// must never assume one global grid_start.
#[derive(Serialize, Debug)]
pub struct DaySlice {
    pub grid_start: i64,
    pub date_label: String,
    pub rows: Vec<Row>,
    /// Fraction of positioned people in darkness at each hour boundary (25 values).
    pub darkness: Vec<f64>,
    /// Zones with a DST transition inside this slice - the view flags these loudly.
    pub dst_zones: Vec<String>,
    /// True when at least half the weighted team rests on this date. The grid is still
    /// correct, but the view should say so rather than imply a normal working day.
    pub rest_day: bool,
}

#[derive(Serialize, Debug)]
pub struct DayReport {
    pub anchor_date: String,
    pub viewer_tz: String,
    pub viewer_offset_min: i32,
    pub days: Vec<DaySlice>,
    pub errors: Vec<String>,
}

/// Parse `"YYYY-MM-DD"`, falling back to "today in `viewer_tz`".
pub fn parse_date_or_today(s: &str, viewer_tz: Tz, now: i64) -> NaiveDate {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .unwrap_or_else(|_| local_date_of(now, viewer_tz))
}

/// Everything the grid needs for one day. `span_days` supports the 48h/3-day views.
pub fn build_day(
    date_str: &str,
    viewer_tz_name: &str,
    people: &[Person],
    now: i64,
    span_days: i64,
) -> Result<DayReport, String> {
    let viewer_tz =
        parse_tz(viewer_tz_name).ok_or_else(|| format!("unknown viewer tz {viewer_tz_name:?}"))?;
    let date = parse_date_or_today(date_str, viewer_tz, now);
    let span_days = span_days.clamp(1, 7);

    let positioned: Vec<Person> = people
        .iter()
        .filter(|p| p.lat != 0.0 || p.lon != 0.0)
        .cloned()
        .collect();
    let mut days = Vec::with_capacity(span_days as usize);
    let mut errors = Vec::new();

    for d in 0..span_days {
        let day = time::shift_date(date, d);
        let g = time::build_grid(day, viewer_tz).ok_or("cannot build grid")?;
        let mut rows = Vec::with_capacity(people.len());
        let mut dst_zones = Vec::new();
        for p in people {
            let row = build_row(&g, p, now);
            if let Some(e) = &row.error {
                if d == 0 {
                    errors.push(e.clone());
                }
            }
            if row.dst_change && !dst_zones.contains(&row.tz) {
                dst_zones.push(row.tz.clone());
            }
            rows.push(row);
        }
        let noon = g.start_utc + 12 * 3600;
        let (mut off_w, mut tot_w) = (0.0f64, 0.0f64);
        for p in people {
            let w = p.role.weight();
            tot_w += w;
            if let Some(tz) = p.parsed_tz() {
                if p.weekend.contains(time::local_weekday(noon, tz)) {
                    off_w += w;
                }
            }
        }
        days.push(DaySlice {
            rest_day: tot_w > 0.0 && off_w / tot_w >= 0.5,
            grid_start: g.start_utc,
            date_label: time::fmt_date(g.start_utc, viewer_tz),
            darkness: darkness_profile(&g, &positioned),
            dst_zones,
            rows,
        });
    }

    Ok(DayReport {
        anchor_date: date.format("%Y-%m-%d").to_string(),
        viewer_tz: viewer_tz_name.to_string(),
        viewer_offset_min: days
            .first()
            .map(|d| time::offset_minutes(d.grid_start, viewer_tz))
            .unwrap_or(0),
        days,
        errors,
    })
}

/// Ranked candidate slots for a day.
pub fn best_slots(
    date_str: &str,
    viewer_tz_name: &str,
    people: &[Person],
    now: i64,
    duration_min: u16,
    k: usize,
    span_hours: i64,
) -> Result<Vec<Slot>, String> {
    let viewer_tz = parse_tz(viewer_tz_name).ok_or("unknown viewer tz")?;
    let date = parse_date_or_today(date_str, viewer_tz, now);
    let grid = time::build_grid(date, viewer_tz).ok_or("cannot build grid")?;
    Ok(suggest(
        &grid,
        people,
        duration_min.max(15),
        k.clamp(1, 24),
        span_hours.max(1),
        viewer_tz,
    ))
}

/// Score one specific window. `start_utc` is absolute, so the UI can drag freely.
pub fn score_window(
    start_utc: i64,
    duration_min: u16,
    viewer_tz_name: &str,
    people: &[Person],
) -> Result<Verdict, String> {
    // Validated even though the verdict itself is zone-independent: a shared link must
    // not silently render in the wrong frame.
    parse_tz(viewer_tz_name).ok_or("unknown viewer tz")?;
    Ok(verdict(start_utc, duration_min.max(15), people))
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn person(id: u8, name: &str, tz: &str) -> Person {
        Person::new(id, name, name, tz)
    }

    #[test]
    fn bad_tz_never_panics() {
        // The exact class of input that killed the live build.
        assert!(parse_tz("").is_none());
        assert!(parse_tz("Not/AZone").is_none());
        assert!(parse_tz("___").is_none());
        assert!(build_day(
            "2026-03-03",
            "Mars/Olympus",
            &[person(0, "x", "Nope/Nope")],
            0,
            1
        )
        .is_err());
        let r = build_day(
            "garbage-date",
            "Asia/Kolkata",
            &[person(0, "x", "Nope/Nope")],
            1_700_000_000,
            1,
        );
        assert!(r.is_ok(), "an unparseable date must fall back, not fail");
        assert_eq!(
            r.unwrap().errors.len(),
            1,
            "the bad tz is reported, not fatal"
        );
    }

    #[test]
    fn quality_thresholds_are_monotonic() {
        assert_eq!(Quality::from_cost(0.0), Quality::Working);
        assert_eq!(Quality::from_cost(0.30), Quality::Shoulder);
        assert_eq!(Quality::from_cost(0.99), Quality::OffHours);
        assert_eq!(Quality::from_cost(1.5), Quality::Asleep);
    }

    #[test]
    fn overnight_work_window_is_understood() {
        let mut p = person(0, "night nurse", "Europe/Berlin");
        p.work_start_min = 20 * 60;
        p.work_end_min = 4 * 60;
        let wd = chrono::Weekday::Tue;
        assert!(
            cost_at(&p, 22 * 60, wd) <= 0.01,
            "22:00 is inside 20:00-04:00"
        );
        assert!(
            cost_at(&p, 2 * 60, wd) <= 0.01,
            "02:00 is inside 20:00-04:00"
        );
        assert!(cost_at(&p, 12 * 60, wd) > 0.3, "midday is outside it");
    }

    #[test]
    fn midnight_is_worse_than_eleven_pm() {
        // Asymmetry of the night curve: the point of the tier table.
        let p = person(0, "x", "UTC");
        let wd = chrono::Weekday::Tue;
        let late = cost_at(&p, 23 * 60, wd);
        let deep = cost_at(&p, 3 * 60, wd);
        assert!(
            deep > late * 1.5,
            "03:00 ({deep}) should be far worse than 23:00 ({late})"
        );
    }

    #[test]
    fn weekend_penalty_applies_per_locale() {
        let mut fri = person(0, "dubai", "Asia/Dubai");
        fri.weekend = Weekend::FriSat;
        let mut ber = person(1, "berlin", "Europe/Berlin");
        ber.weekend = Weekend::SatSun;
        let friday = chrono::Weekday::Fri;
        let base_fri = cost_at(&fri, 10 * 60, friday);
        let base_ber = cost_at(&ber, 10 * 60, friday);
        assert!(
            base_fri > base_ber + 0.5,
            "Friday is a weekend day in Dubai, not Berlin"
        );
    }
}
