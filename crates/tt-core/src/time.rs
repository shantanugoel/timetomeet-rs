//! Absolute-instant time math.
//!
//! The one rule that matters, and the one the old build broke: **we always start
//! from an absolute UTC instant and derive local wall clocks from it**, never the
//! other way round. `legacy/src/views.rs` anchored the whole grid on
//! `tz.from_local_date(date).and_hms(9,0,0)` - a local wall clock - which is how
//! DST bugs and `unwrap()` panics on ambiguous/nonexistent local times are born.
//!
//! Everything here takes and returns unix seconds.

use chrono::{Datelike, NaiveDate, NaiveDateTime, Offset, TimeZone, Timelike};
use chrono_tz::Tz;

use crate::model::{local_date_of, Grid, MINUTES_PER_DAY, SECONDS_PER_DAY};

/// UTC offset, in seconds, of `tz` at unix instant `ts`.
pub fn offset_seconds(ts: i64, tz: Tz) -> i32 {
    match naive_from_ts(ts) {
        Some(ndt) => tz.from_utc_datetime(&ndt).offset().fix().local_minus_utc(),
        None => 0,
    }
}

pub fn offset_minutes(ts: i64, tz: Tz) -> i32 {
    offset_seconds(ts, tz) / 60
}

pub fn naive_from_ts(ts: i64) -> Option<NaiveDateTime> {
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.naive_utc())
}

/// Local wall-clock minutes (0..1440) of unix instant `ts` in `tz`.
///
/// Note this is the time on a clock on the wall, which is what "is 3am bad?" needs;
/// it is *not* the same as seconds-since-local-midnight across a DST transition, and
/// nothing here pretends it is.
pub fn local_minutes(ts: i64, tz: Tz) -> u32 {
    let ndt = match naive_from_ts(ts) {
        Some(n) => n,
        None => return 0,
    };
    let local = tz.from_utc_datetime(&ndt);
    local.hour() * 60 + local.minute()
}

pub fn local_weekday(ts: i64, tz: Tz) -> chrono::Weekday {
    naive_from_ts(ts)
        .map(|n| tz.from_utc_datetime(&n).weekday())
        .unwrap_or(chrono::Weekday::Mon)
}

/// The first absolute instant whose local time in `tz` is at or after local midnight
/// of `date`.
///
/// Handles all three things that can happen to a local midnight:
/// * normal  -> that instant
/// * ambiguous (fall-back repeats it) -> the *earlier* of the two
/// * nonexistent (spring-forward skips it) -> the first instant that does exist
///
/// Anchored on local noon, which is never skipped or ambiguous in any real zone, then
/// scanned outward - so no `unwrap()` on a `LocalResult` anywhere.
pub fn local_midnight_utc(date: NaiveDate, tz: Tz) -> Option<i64> {
    let midnight = date.and_hms_opt(0, 0, 0)?;
    let noon = tz
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 12, 0, 0)
        .earliest()?;
    let anchor = noon.with_timezone(&chrono::Utc).timestamp() - 12 * 3600;
    // A DST gap is at most 2h, so the answer is within +-4h of noon-12h. Overshoot at
    // minute resolution, then refine to the second from the preceding minute.
    let mut u = anchor - 4 * 3600;
    let ceiling = anchor + 4 * 3600;
    while u < ceiling && local_naive(u, tz)? < midnight {
        u += 60;
    }
    let mut v = (u - 60).max(anchor - 4 * 3600);
    while v < u + 60 && local_naive(v, tz)? < midnight {
        v += 1;
    }
    Some(v)
}

fn local_naive(ts: i64, tz: Tz) -> Option<NaiveDateTime> {
    let ndt = naive_from_ts(ts)?;
    Some(tz.from_utc_datetime(&ndt).naive_local())
}

/// Build the shared 24h grid for `date` as seen from `viewer_tz`.
pub fn build_grid(date: NaiveDate, viewer_tz: Tz) -> Option<Grid> {
    let start_utc = local_midnight_utc(date, viewer_tz)?;
    Some(Grid {
        start_utc,
        viewer_tz,
        anchor_date: date,
    })
}

/// Snap a unix instant to the previous multiple of `SNAP_MIN` in the *viewer's* local
/// day, so the selection always lands on a visible gridline rather than drifting by
/// the zone's odd offset (:30 / :45 zones, and half-hour DST zones like Lord Howe).
pub fn snap_to_grid(ts: i64, grid_start: i64, snap_min: i32) -> i64 {
    let step = (snap_min as i64) * 60;
    if step <= 0 {
        return ts;
    }
    let delta = ts - grid_start;
    if delta >= 0 {
        grid_start + (delta / step) * step
    } else {
        grid_start - ((-delta + step - 1) / step) * step
    }
}

/// Minutes from grid start for an absolute instant (may be negative or >1440).
pub fn grid_minutes(ts: i64, grid_start: i64) -> i32 {
    (((ts - grid_start) / 60) as i32).rem_euclid(MINUTES_PER_DAY * 2)
}

pub fn grid_minutes_exact(ts: i64, grid_start: i64) -> i32 {
    ((ts - grid_start) / 60) as i32
}

/// Format a unix instant in a zone as `"%H:%M"`.
pub fn fmt_hhmm(ts: i64, tz: Tz) -> String {
    let m = local_minutes(ts, tz);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// Format a unix instant in a zone as `"Tue 3 Mar"`.
pub fn fmt_date(ts: i64, tz: Tz) -> String {
    local_date_of(ts, tz).format("%a %-d %b").to_string()
}

/// Does a DST transition happen for `tz` inside `[start, start + span)`?
pub fn dst_changes(start: i64, span: i64, tz: Tz) -> bool {
    offset_seconds(start, tz) != offset_seconds(start + span - 1, tz)
}

/// Shift the anchor date by N days *in the viewer's calendar*, not by N*86400 seconds.
/// Those differ on the two days a year a zone has a 23h or 25h day.
pub fn shift_date(date: NaiveDate, days: i64) -> NaiveDate {
    date.checked_add_signed(chrono::Duration::days(days))
        .unwrap_or(date)
}

/// UTC timestamp for use in ICS (`20260303T160000Z`).
pub fn fmt_ics_utc(ts: i64) -> String {
    naive_from_ts(ts)
        .map(|n| n.format("%Y%m%dT%H%M%SZ").to_string())
        .unwrap_or_default()
}

pub const SECONDS_DAY: i64 = SECONDS_PER_DAY;
