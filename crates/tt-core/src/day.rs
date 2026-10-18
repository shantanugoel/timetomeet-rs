//! Grid construction: turn (grid, person) into the run-length bands the view paints.
//!
//! The grid is sampled at `SNAP_MIN` resolution rather than solved analytically. That
//! is a deliberate trade: the cost function is piecewise and depends on local
//! wall-clock time, weekday and per-person windows, so solving for exact breakpoints
//! would be a lot of subtle code for a benefit finer than the grid can even draw.
//! Sampling at the same granularity the UI snaps to means a band boundary can never
//! disagree with a snap target, and DST is handled for free because every sample is
//! derived from an absolute instant.

use crate::model::*;
use crate::solar::sun_for_grid;
use crate::time::*;

/// Sample interval in seconds.
const STEP: i64 = (SNAP_MIN * 60) as i64;

pub fn build_row(grid: &Grid, p: &Person, now: i64) -> Row {
    let tz = match p.parsed_tz() {
        Some(t) => t,
        None => {
            return Row {
                person_id: p.id,
                name: p.name.clone(),
                city: p.city.clone(),
                tz: p.tz.clone(),
                ok: false,
                error: Some(format!("unknown timezone {:?}", p.tz)),
                bands: vec![],
                hour_labels: vec![],
                offset_min: 0,
                midnights: vec![],
                now_min: None,
                sunrise_min: None,
                sunset_min: None,
                dst_change: false,
                date_label: String::new(),
                work_label: p.work_label(),
                role: p.role,
            };
        }
    };

    // Run-length encode the samples. The band's end_min is extended on every sample that
    // keeps the same quality - getting this wrong silently leaves holes in the row, which
    // is exactly what the CLI dogfood caught.
    let mut bands: Vec<Band> = Vec::with_capacity(8);
    for i in 0..SAMPLES_PER_DAY {
        let ts = grid.start_utc + (i as i64) * STEP;
        let q = Quality::from_cost(crate::score::cost_at(
            p,
            local_minutes(ts, tz),
            local_weekday(ts, tz),
        ));
        let wknd = p.weekend.contains(local_weekday(ts, tz));
        let end = ((i as i32 + 1) * SNAP_MIN) as u16;
        match bands.last_mut() {
            Some(b) if b.quality == q && b.weekend == wknd => b.end_min = end,
            _ => bands.push(Band {
                start_min: (i as i32 * SNAP_MIN) as u16,
                end_min: end,
                quality: q,
                weekend: wknd,
            }),
        }
    }
    debug_assert_eq!(
        bands
            .iter()
            .map(|b| (b.end_min - b.start_min) as i32)
            .sum::<i32>(),
        MINUTES_PER_DAY,
        "bands must tile the whole grid for {}",
        p.name
    );

    let hour_labels: Vec<String> = (0..=24)
        .map(|h| fmt_hhmm(grid.start_utc + (h as i64) * 3600, tz))
        .collect();

    // Their local midnights inside this grid: the heavy line and the "Mon 2 -> Tue 3"
    // labels. Date confusion, not offset confusion, is the commonest real-world error.
    let mut midnights = Vec::new();
    let d0 = local_date_of(grid.start_utc, tz);
    for offset in 0..3i64 {
        let d = shift_date(d0, offset);
        if let Some(ts) = local_midnight_utc(d, tz) {
            let gm = (ts - grid.start_utc) / 60;
            if gm >= 0 && gm < MINUTES_PER_DAY as i64 {
                midnights.push(MidnightMark {
                    grid_min: gm as u16,
                    date_label: d.format("%a %-d %b").to_string(),
                });
            }
        }
    }

    let now_min = if now >= grid.start_utc && now < grid.start_utc + SECONDS_PER_DAY {
        Some(((now - grid.start_utc) / 60) as u16)
    } else {
        None
    };

    let has_pos = p.lat != 0.0 || p.lon != 0.0;
    let sun = if has_pos {
        sun_for_grid(grid.start_utc, d0, p.lat, p.lon)
    } else {
        SolarTimes {
            sunrise_min: None,
            sunset_min: None,
            polar: false,
        }
    };

    Row {
        person_id: p.id,
        name: p.name.clone(),
        city: p.city.clone(),
        tz: p.tz.clone(),
        ok: true,
        error: None,
        bands,
        hour_labels,
        offset_min: offset_minutes(grid.start_utc, tz),
        midnights,
        now_min,
        sunrise_min: sun.sunrise_min,
        sunset_min: sun.sunset_min,
        dst_change: dst_changes(grid.start_utc, SECONDS_PER_DAY, tz),
        date_label: fmt_date(grid.start_utc, tz),
        work_label: p.work_label(),
        role: p.role,
    }
}

/// The earth shadow: for each hour of the grid, the fraction of selected people who
/// are in darkness. Cheap, and it is the one visual that makes the whole tool legible.
pub fn darkness_profile(grid: &Grid, people: &[Person]) -> Vec<f64> {
    (0..=24)
        .map(|h| {
            let ts = grid.start_utc + (h as i64) * 3600;
            let mut dark = 0usize;
            let mut total = 0usize;
            for p in people {
                let tz = match p.parsed_tz() {
                    Some(t) => t,
                    None => continue,
                };
                if p.lat == 0.0 && p.lon == 0.0 {
                    continue;
                }
                total += 1;
                let d = local_date_of(ts, tz);
                let (rise, set, polar) = crate::solar::sun_instants(d, p.lat, p.lon);
                let is_dark = match (rise, set) {
                    (Some(r), Some(s)) => ts < r || ts >= s,
                    (None, None) => polar && local_minutes(ts, tz) < 720, // rough: night half of polar night
                    _ => false,
                };
                if is_dark {
                    dark += 1;
                }
            }
            if total == 0 {
                0.0
            } else {
                dark as f64 / total as f64
            }
        })
        .collect()
}
