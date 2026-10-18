//! The scoring engine - the reason this is a planning tool and not a clock wall.
//!
//! One opinion, expressed once: how bad is this moment for a human being. Everything
//! else (band colours, the score, the ranking, the sentence about who is suffering)
//! is derived from `cost_at`, so they cannot disagree with each other.
//!
//! The editorial choice that matters most: **`worst_off` carries the largest weight.**
//! A slot that is comfortable for four people and 03:00 for one is a bad slot, and
//! average-based reasoning - which is what humans doing mental arithmetic fall back
//! to, and what most tools report - systematically misses exactly that.

use chrono::Weekday;

use crate::model::*;
use crate::time::*;

/// Generic human-preference curve over local wall-clock minutes, independent of any
/// person's declared window. Deeper night costs more, and asymmetrically: 04:00 is
/// worse than 23:00 by a lot more than the clock distance suggests.
fn tier_cost(mins: i32) -> f64 {
    let m = mins.rem_euclid(MINUTES_PER_DAY);
    let seg = |a: i32, b: i32, va: f64, vb: f64| -> f64 {
        let t = (m - a) as f64 / (b - a) as f64;
        va + (vb - va) * t.clamp(0.0, 1.0)
    };
    match m {
        0..300 => seg(0, 300, 1.30, 1.80),         // 00:00-05:00  severe
        300..420 => seg(300, 420, 1.00, 0.80),     // 05:00-07:00  very early
        420..540 => seg(420, 540, 0.70, 0.45),     // 07:00-09:00  early
        540..1080 => 0.30,                         // 09:00-18:00  daytime, outside their hours
        1080..1320 => seg(1080, 1320, 0.45, 0.70), // 18:00-22:00  evening
        _ => seg(1320, 1440, 0.80, 1.00),          // 22:00-24:00  late
    }
}

/// Is local minute `m` inside this person's working window? Handles overnight windows
/// (`20:00-04:00`) because night-shift and on-call people exist.
fn in_window(p: &Person, m: i32) -> bool {
    let s = p.work_start_min as i32;
    let e = p.work_end_min as i32;
    let m = m.rem_euclid(MINUTES_PER_DAY);
    if e > s {
        m >= s && m < e
    } else {
        m >= s || m < e
    }
}

/// Minutes before the window start (or after its end) that `m` falls, taking the short
/// way round the clock. 0 if inside. Correct for overnight windows (`20:00-04:00`),
/// which a naive `m - e` gets wrong by returning a negative distance.
fn outside_distance(p: &Person, m: i32) -> i32 {
    let s = p.work_start_min as i32;
    let e = p.work_end_min as i32;
    let m = m.rem_euclid(MINUTES_PER_DAY);
    if in_window(p, m) {
        return 0;
    }
    let after_end = (m - e).rem_euclid(MINUTES_PER_DAY);
    let before_start = (s - m).rem_euclid(MINUTES_PER_DAY);
    after_end.min(before_start)
}

/// Cost of one moment for one person. 0.0 = ideal, ~1.8 = the middle of their night.
pub fn cost_at(p: &Person, local_min: u32, weekday: Weekday) -> f64 {
    let m = local_min as i32;
    let base = if in_window(p, m) {
        0.0
    } else {
        let dist = outside_distance(p, m);
        let flex = p.flex_min.max(1) as i32;
        if dist <= flex {
            // Declared shoulder: still fine, ramping gently to the top of "shoulder".
            0.05 + 0.30 * (dist as f64 / flex as f64)
        } else {
            tier_cost(m) + 0.10 * (((dist - flex) as f64) / 120.0).min(1.0)
        }
    };
    let weekend = if p.weekend.contains(weekday) {
        0.60
    } else {
        0.0
    };
    (base + weekend).min(2.60)
}

/// Worst cost anywhere inside `[start, end)` for one person, plus how much of the
/// meeting falls outside their window.
fn person_window_cost(p: &Person, tz: chrono_tz::Tz, start: i64, end: i64) -> (f64, u16) {
    let step = (SNAP_MIN * 60) as i64;
    let mut worst = f64::MIN;
    let mut outside = 0u16;
    let mut ts = start;
    while ts < end {
        let lm = local_minutes(ts, tz);
        let wd = local_weekday(ts, tz);
        let c = cost_at(p, lm, wd);
        if c > worst {
            worst = c;
        }
        if !in_window(p, lm as i32) {
            outside += SNAP_MIN as u16;
        }
        ts += step;
    }
    if worst == f64::MIN {
        worst = 0.0;
    }
    (worst, outside.min(1440))
}

fn reason_for(p: &Person, v: &PersonVerdict) -> String {
    let mut parts: Vec<String> = Vec::new();
    match v.quality {
        Quality::Working => parts.push("inside working hours".into()),
        Quality::Shoulder => {
            let flex = p.flex_min;
            parts.push(format!(
                "{} outside their {}, within their {}m flex",
                fmt_minutes(v.clipped_min),
                p.work_label(),
                flex
            ));
        }
        Quality::OffHours => parts.push(format!(
            "{} outside their {}",
            fmt_minutes(v.clipped_min),
            p.work_label()
        )),
        Quality::Asleep => {
            let h = (local_minutes_of_str(&v.local_start) / 60) as i32;
            let what = if (0..5).contains(&h) {
                "the middle of their night"
            } else if !(7..22).contains(&h) {
                "way outside waking hours"
            } else {
                "outside their day"
            };
            parts.push(format!(
                "{} past their {} - {}",
                fmt_minutes(v.clipped_min),
                p.work_label(),
                what
            ));
        }
    }
    if v.crosses_midnight {
        parts.push("spans their midnight".into());
    }
    parts.join("; ")
}

fn local_minutes_of_str(hhmm: &str) -> u32 {
    let mut it = hhmm.split(':');
    let h: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let m: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    h * 60 + m
}

fn fmt_minutes(m: u16) -> String {
    if m == 0 {
        return "0 min".into();
    }
    let h = m / 60;
    let mm = m % 60;
    match (h, mm) {
        (0, mm) => format!("{mm} min"),
        (h, 0) => format!("{h}h"),
        (h, mm) => format!("{h}h {mm:02}m"),
    }
}

/// Evaluate one candidate window.
pub fn verdict(start_utc: i64, duration_min: u16, people: &[Person]) -> Verdict {
    let end_utc = start_utc + (duration_min as i64) * 60;
    let mut persons: Vec<PersonVerdict> = Vec::with_capacity(people.len());
    let mut counts = Counts::default();
    let mut total = 0.0f64;
    let mut wsum = 0.0f64;
    let mut worst_required = f64::MIN;
    let mut worst_name: Option<String> = None;
    let mut any_required = false;
    let mut covered = 0.0f64;

    for p in people {
        let tz = match p.parsed_tz() {
            Some(t) => t,
            None => continue,
        };
        let (cost, clipped) = person_window_cost(p, tz, start_utc, end_utc);
        let quality = Quality::from_cost(cost);
        counts.add(quality);
        let w = p.role.weight();
        wsum += w;
        total += w * cost;
        if cost <= 0.01 {
            covered += w;
        }
        if w >= 0.9 {
            any_required = true;
            if cost > worst_required {
                worst_required = cost;
                worst_name = Some(p.name.clone());
            }
        }
        let start_local = fmt_hhmm(start_utc, tz);
        let mut pv = PersonVerdict {
            person_id: p.id,
            name: p.name.clone(),
            city: p.city.clone(),
            local_start: start_local.clone(),
            local_end: fmt_hhmm(end_utc, tz),
            local_date: fmt_date(start_utc, tz),
            offset_min: offset_minutes(start_utc, tz),
            cost,
            quality,
            crosses_midnight: local_date_of(start_utc, tz) != local_date_of(end_utc, tz),
            clipped_min: clipped,
            reason: String::new(),
        };
        pv.reason = reason_for(p, &pv);
        persons.push(pv);
    }

    if !any_required {
        worst_required = persons.iter().map(|p| p.cost).fold(0.0f64, f64::max);
        worst_name = persons
            .iter()
            .max_by(|a, b| {
                a.cost
                    .partial_cmp(&b.cost)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|p| p.name.clone());
    }
    let worst_off = worst_required.max(0.0);

    // Equity: is the pain shared, or dumped on one person? Computed over everyone with
    // real weight, since a peripheral attendee's inconvenience should not move it.
    let heavy: Vec<f64> = persons
        .iter()
        .zip(people.iter())
        .filter(|(_, p)| p.role.weight() >= 0.9)
        .map(|(v, _)| v.cost)
        .collect();
    let equity = if heavy.len() < 2 {
        1.0
    } else {
        let mean = heavy.iter().sum::<f64>() / heavy.len() as f64;
        let var = heavy.iter().map(|c| (c - mean) * (c - mean)).sum::<f64>() / heavy.len() as f64;
        (1.0 - var.sqrt() / 1.8).clamp(0.0, 1.0)
    };

    let coverage = if wsum > 0.0 { covered / wsum } else { 0.0 };
    let total_norm = if wsum > 0.0 {
        total / (wsum * 1.8)
    } else {
        0.0
    };

    let score = 100.0
        * (0.45 * (1.0 - (worst_off / 1.8).clamp(0.0, 1.0))
            + 0.30 * (1.0 - total_norm.clamp(0.0, 1.0))
            + 0.15 * equity
            + 0.10 * coverage);

    let score = score.round().clamp(0.0, 100.0) as u8;
    Verdict {
        start_utc,
        duration_min,
        label: score_label(score).to_string(),
        score,
        total: (total * 100.0).round() / 100.0,
        worst_off: (worst_off * 100.0).round() / 100.0,
        worst_name,
        equity: (equity * 100.0).round() / 100.0,
        coverage: (coverage * 100.0).round() / 100.0,
        counts,
        persons,
    }
}

/// A word, not just a number. "58" makes people hunt for the scale; "workable" does not.
pub fn score_label(score: u8) -> &'static str {
    match score {
        80..=u8::MAX => "great",
        65..=79 => "good",
        50..=64 => "workable",
        35..=49 => "hard",
        _ => "brutal",
    }
}

/// Rank candidate starts and return the best `k`, spaced apart so the list is genuinely
/// different options rather than nine shades of the same hour.
pub fn suggest(
    grid: &Grid,
    people: &[Person],
    duration_min: u16,
    k: usize,
    span_hours: i64,
    viewer_tz: chrono_tz::Tz,
) -> Vec<Slot> {
    let step = (SNAP_MIN * 60) as i64;
    // If the anchor date is a rest day for most of the weighted team, every slot on it is
    // equally bad and the panel becomes noise. Widen the horizon so the list shows the
    // next real working day instead - labelled with its date, so nothing is hidden.
    let noon = grid.start_utc + 12 * 3600;
    let mut off_w = 0.0f64;
    let mut tot_w = 0.0f64;
    for p in people {
        let w = p.role.weight();
        tot_w += w;
        if let Some(tz) = p.parsed_tz() {
            if p.weekend.contains(local_weekday(noon, tz)) {
                off_w += w;
            }
        }
    }
    let rest_day = tot_w > 0.0 && off_w / tot_w >= 0.5;
    let span_hours = if rest_day {
        span_hours.max(72)
    } else {
        span_hours
    };
    let horizon = grid.start_utc + span_hours * 3600 - (duration_min as i64) * 60;
    let mut all: Vec<Verdict> = Vec::new();
    let mut ts = grid.start_utc;
    while ts <= horizon {
        all.push(verdict(ts, duration_min, people));
        ts += step;
    }
    all.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.counts.working.cmp(&a.counts.working))
            .then_with(|| a.start_utc.cmp(&b.start_utc))
    });

    let mut out: Vec<Slot> = Vec::with_capacity(k);
    for v in all {
        // Keep suggestions at least 45 min apart, or a whole duration apart, whichever
        // is larger - otherwise the top three are all "21:30-ish".
        let spacing = (duration_min as i64).max(45) * 60;
        if out
            .iter()
            .any(|s: &Slot| (s.start_utc - v.start_utc).abs() < spacing)
        {
            continue;
        }
        let headline = match v.worst_off {
            c if c <= 0.01 => "good for everyone".to_string(),
            _ => format!(
                "worst: {} at {}",
                v.worst_name.clone().unwrap_or_else(|| "someone".into()),
                v.persons
                    .iter()
                    .filter(|p| Some(p.name.as_str()) == v.worst_name.as_deref())
                    .map(|p| p.local_start.clone())
                    .next()
                    .unwrap_or_default()
            ),
        };
        out.push(Slot {
            start_utc: v.start_utc,
            duration_min: v.duration_min,
            score: v.score,
            label: score_label(v.score).to_string(),
            counts: v.counts,
            worst_name: v.worst_name.clone(),
            headline,
            date_label: fmt_date(v.start_utc, viewer_tz),
            is_anchor: fmt_date(v.start_utc, viewer_tz)
                == fmt_date(grid.start_utc + 12 * 3600, viewer_tz),
            persons: v.persons.clone(),
        });
        if out.len() >= k {
            break;
        }
    }
    out
}
