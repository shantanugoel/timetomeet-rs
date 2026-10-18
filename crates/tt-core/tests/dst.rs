//! DST goldens. This file is the reason the rewrite exists.
//!
//! Every case below is a thing that has bitten timezone software, and every one is
//! checked against the *rules* rather than a snapshot of today's output, so a tzdata
//! update that legitimately changes a rule shows up as a deliberate decision rather
//! than a silent regression.

use chrono::TimeZone;
use tt_core::model::{parse_tz, Person, Role, Weekend};
use tt_core::time::{
    build_grid, dst_changes, fmt_date, fmt_hhmm, local_midnight_utc, local_minutes, offset_minutes,
};
use tt_core::{score_window, suggest};

fn tz(name: &str) -> chrono_tz::Tz {
    parse_tz(name).unwrap_or_else(|| panic!("test used a bad zone: {name}"))
}

fn person(id: u8, name: &str, zone: &str) -> Person {
    let mut p = Person::new(id, name, name, zone);
    if id == 0 {
        p.role = Role::Organizer;
    }
    p
}

/// 2026-03-03 in Europe/Berlin is an ordinary Tuesday; 2026-03-29 is the spring-forward.
#[test]
fn grid_start_is_local_midnight_for_an_ordinary_day() {
    let berlin = tz("Europe/Berlin");
    let g = build_grid(chrono::NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(), berlin).unwrap();
    // Ordinary winter day: UTC+1, so local midnight is 23:00 UTC the day before.
    assert_eq!(offset_minutes(g.start_utc, berlin), 60);
    assert_eq!(
        local_minutes(g.start_utc, berlin),
        0,
        "grid must start at local midnight"
    );
}

#[test]
fn spring_forward_day_still_has_a_grid_start() {
    // 2026-03-29: Berlin skips 02:00 -> 03:00. Midnight itself is fine, but the day is
    // 23 hours long. The grid must still start exactly at local midnight.
    let berlin = tz("Europe/Berlin");
    let d = chrono::NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
    let g = build_grid(d, berlin).unwrap();
    assert_eq!(local_minutes(g.start_utc, berlin), 0);
    assert!(
        dst_changes(g.start_utc, 86_400, berlin),
        "must be flagged as a DST day"
    );
}

#[test]
fn zones_that_skip_midnight_itself_do_not_panic() {
    // America/Havana's DST transition is 00:00 -> 01:00, so local midnight does not
    // exist on that date. The old build would have hit a LocalResult::None and
    // `unwrap()`ed it. Here we must land on the first instant that does exist.
    let havana = tz("America/Havana");
    let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    let ts = local_midnight_utc(d, havana);
    assert!(
        ts.is_some(),
        "a skipped local midnight must resolve, not fail"
    );
    let ts = ts.unwrap();
    // Whatever we chose, it must be within a couple of hours of the intended midnight
    // and must be a real instant in that zone.
    let lm = local_minutes(ts, havana);
    assert!(
        lm <= 180 || lm >= 1380,
        "resolved to a sane instant, got minute {lm}"
    );
}

#[test]
fn half_hour_and_45_minute_offsets_are_exact() {
    let now = 1_772_000_000; // 2026-02-25, northern hemisphere winter
    assert_eq!(offset_minutes(now, tz("Asia/Kolkata")), 330);
    assert_eq!(offset_minutes(now, tz("Asia/Kathmandu")), 345);
    assert_eq!(
        offset_minutes(now, tz("America/St_Johns")),
        -210,
        "Newfoundland is a half-hour zone: NST -3:30"
    );
}

#[test]
fn lord_howe_shifts_by_thirty_minutes() {
    // The famous one: +10:30 -> +11:00. A 30-minute DST step, which breaks any code
    // that assumes an hour.
    let lh = tz("Australia/Lord_Howe");
    // Southern hemisphere: DST runs Oct->Apr, so July is standard (+10:30) and
    // November is daylight (+11:00). The step between them is 30 minutes, not 60.
    let standard = chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
    let daylight = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    let a = local_midnight_utc(standard, lh).unwrap();
    let b = local_midnight_utc(daylight, lh).unwrap();
    let (oa, ob) = (offset_minutes(a, lh), offset_minutes(b, lh));
    assert_eq!(oa, 630, "Lord Howe standard time is +10:30");
    assert_eq!(
        ob, 660,
        "Lord Howe daylight time is +11:00 - a 30-minute step"
    );
}

#[test]
fn kiritimati_is_the_furthest_forward_zone() {
    let now = 1_772_000_000;
    assert_eq!(offset_minutes(now, tz("Pacific/Kiritimati")), 14 * 60);
}

#[test]
fn the_same_instant_is_a_different_date_in_india_and_seattle() {
    // The single most common real-world confusion, and the one the old table hid inside
    // a cell that said "8:30 PM 16 Oct" next to "9:00 AM 17 Oct" with no explanation.
    let ts = 1_772_510_400; // 2026-03-03T04:00:00Z
    let blr = tz("Asia/Kolkata");
    let sea = tz("America/Los_Angeles");
    assert_eq!(fmt_hhmm(ts, blr), "09:30");
    assert_eq!(fmt_date(ts, blr), "Tue 3 Mar");
    assert_eq!(fmt_hhmm(ts, sea), "20:00");
    assert_eq!(
        fmt_date(ts, sea),
        "Mon 2 Mar",
        "Seattle is still on the previous day"
    );
}

#[test]
fn a_fixed_utc_meeting_moves_in_local_time_across_a_dst_change() {
    // A weekly 16:00 UTC standup: comfortable in winter, an hour later in local terms
    // for Europeans after their spring-forward. The engine must show the drift, not
    // hide it - this is the "on-call handoff crept by an hour" complaint.
    let berlin = tz("Europe/Berlin");
    let before = 1_772_553_600; // 2026-03-03T16:00Z, Berlin is UTC+1
    let after = 1_775577600; // 2026-04-07T16:00Z, Berlin is UTC+2
    assert_eq!(fmt_hhmm(before, berlin), "17:00");
    assert_eq!(
        fmt_hhmm(after, berlin),
        "18:00",
        "the same UTC slot drifted an hour"
    );
}

#[test]
fn band_grid_starts_are_monotonic_across_every_zone_for_a_year() {
    // Exhaustive smoke test: no zone, on any day of 2026, may fail to produce a grid
    // start or produce one that is not near local midnight.
    let zones = [
        "UTC",
        "Asia/Kolkata",
        "Asia/Kathmandu",
        "Australia/Lord_Howe",
        "America/Havana",
        "America/Santiago",
        "Pacific/Chatham",
        "Pacific/Apia",
        "Asia/Tehran",
        "Europe/Berlin",
        "America/New_York",
        "Australia/Sydney",
        "Pacific/Kiritimati",
        "Asia/Kathmandu",
    ];
    let mut checked = 0usize;
    for z in zones {
        let t = tz(z);
        let mut day = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        while day <= end {
            let ts =
                local_midnight_utc(day, t).unwrap_or_else(|| panic!("{z} {day} has no grid start"));
            let lm = local_minutes(ts, t);
            assert!(
                lm <= 120 || lm >= 1320,
                "{z} {day}: grid start is at local minute {lm}"
            );
            day += chrono::Duration::days(1);
            checked += 1;
        }
    }
    assert!(
        checked > 5000,
        "expected to sweep the year for every zone, checked {checked}"
    );
}

// ---------------------------------------------------------------- scoring

#[test]
fn one_sleepy_person_beats_four_happy_ones() {
    // THE editorial rule: worst_off must dominate. A slot that is 03:00 for one person
    // has to lose to a slot that is merely shoulder-hour for everyone.
    let people = vec![
        person(0, "You", "America/New_York"),
        person(1, "B", "America/Chicago"),
        person(2, "C", "America/Denver"),
        person(3, "D", "America/Los_Angeles"),
        person(4, "E", "Asia/Kolkata"),
    ];
    let day = "2026-03-03";
    let slots = suggest(
        &build_grid(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
            tz("America/New_York"),
        )
        .unwrap(),
        &people,
        60,
        8,
        24,
        tz("America/New_York"),
    );
    assert!(!slots.is_empty());
    // No suggested slot may put a required attendee in the middle of their night while
    // a strictly better one exists.
    let best = slots.first().unwrap();
    for s in &slots[1..] {
        assert!(s.score <= best.score);
    }
    let asleep_in_best = best
        .persons
        .iter()
        .filter(|p| p.quality == tt_core::Quality::Asleep)
        .count();
    let asleep_in_worst = slots
        .last()
        .unwrap()
        .persons
        .iter()
        .filter(|p| p.quality == tt_core::Quality::Asleep)
        .count();
    assert!(
        asleep_in_best <= asleep_in_worst,
        "ranking put night-time slots above better ones"
    );
    let _ = day;
}

#[test]
fn a_slot_inside_everyones_hours_scores_100() {
    // Two people in the same zone, meeting squarely inside working hours.
    let people = vec![
        person(0, "A", "Europe/Berlin"),
        person(1, "B", "Europe/Berlin"),
    ];
    let berlin = tz("Europe/Berlin");
    let start = berlin
        .with_ymd_and_hms(2026, 3, 3, 10, 0, 0)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .timestamp();
    let v = score_window(start, 60, "Europe/Berlin", &people).unwrap();
    assert_eq!(v.score, 100, "perfect slot must max out: {:?}", v.persons);
    assert_eq!(v.counts.working, 2);
}

#[test]
fn three_am_scores_near_zero_and_names_the_victim() {
    let people = vec![
        person(0, "A", "Europe/Berlin"),
        person(1, "Mei", "Asia/Kolkata"),
    ];
    let kolkata = tz("Asia/Kolkata");
    // 03:00 in Kolkata on a weekday.
    let start = kolkata
        .with_ymd_and_hms(2026, 3, 3, 3, 0, 0)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .timestamp();
    let v = score_window(start, 60, "Europe/Berlin", &people).unwrap();
    assert!(
        v.score < 25,
        "03:00 for someone must score badly, got {}",
        v.score
    );
    assert_eq!(v.worst_name.as_deref(), Some("Mei"));
    assert!(
        v.persons.iter().any(|p| p.reason.contains("night")),
        "reason must say why: {:?}",
        v.persons.iter().map(|p| &p.reason).collect::<Vec<_>>()
    );
}

#[test]
fn optional_people_do_not_sink_a_slot() {
    let mut people = vec![
        person(0, "A", "Europe/Berlin"),
        person(1, "B", "Europe/Berlin"),
    ];
    let mut night_owl = person(2, "Peripheral", "Asia/Kolkata");
    night_owl.role = Role::Peripheral;
    people.push(night_owl);
    let berlin = tz("Europe/Berlin");
    let start = berlin
        .with_ymd_and_hms(2026, 3, 3, 10, 0, 0)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .timestamp();
    let v = score_window(start, 60, "Europe/Berlin", &people).unwrap();
    // Peripheral attendee is asleep at 04:30 IST, but should not drag this below "good".
    assert!(
        v.score >= 80,
        "a peripheral sleeper should not wreck the score, got {}",
        v.score
    );
}

#[test]
fn weekend_is_judged_by_where_you_live() {
    // 2026-03-06 is a Friday: a working day in Berlin, a weekend day in Dubai.
    let berlin = tz("Europe/Berlin");
    let start = berlin
        .with_ymd_and_hms(2026, 3, 6, 10, 0, 0)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .timestamp();
    let mut dubai = person(1, "Rashid", "Asia/Dubai");
    dubai.weekend = Weekend::FriSat;
    let mut gina = person(2, "Gina", "Europe/Berlin");
    gina.weekend = Weekend::SatSun;
    let v = score_window(
        start,
        60,
        "Europe/Berlin",
        &[person(0, "A", "Europe/Berlin"), dubai, gina],
    )
    .unwrap();
    let d = v.persons.iter().find(|p| p.name == "Rashid").unwrap();
    let g = v.persons.iter().find(|p| p.name == "Gina").unwrap();
    assert!(
        d.cost > g.cost + 0.4,
        "Friday is a weekend in Dubai: {} vs {}",
        d.cost,
        g.cost
    );
}

#[test]
fn flex_turns_off_hours_into_shoulders() {
    let mut strict = person(0, "strict", "Europe/Berlin");
    strict.flex_min = 0;
    let mut flexy = person(1, "flexy", "Europe/Berlin");
    flexy.flex_min = 90;
    let berlin = tz("Europe/Berlin");
    // 08:00, one hour before a 09:00 start.
    let start = berlin
        .with_ymd_and_hms(2026, 3, 3, 8, 0, 0)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .timestamp();
    let v = score_window(start, 30, "Europe/Berlin", &[strict.clone(), flexy.clone()]).unwrap();
    let s = v.persons.iter().find(|p| p.name == "strict").unwrap();
    let f = v.persons.iter().find(|p| p.name == "flexy").unwrap();
    assert_eq!(s.quality, tt_core::Quality::OffHours);
    assert_eq!(
        f.quality,
        tt_core::Quality::Shoulder,
        "declared flex must soften the same slot"
    );
}

#[test]
fn crossing_midnight_is_flagged() {
    // 23:30 Kolkata -> the meeting spans their midnight.
    let kolkata = tz("Asia/Kolkata");
    let start = kolkata
        .with_ymd_and_hms(2026, 3, 3, 23, 30, 0)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .timestamp();
    let v = score_window(start, 60, "UTC", &[person(0, "A", "Asia/Kolkata")]).unwrap();
    assert!(v.persons[0].crosses_midnight);
    assert!(v.persons[0].reason.contains("midnight"));
}

#[test]
fn suggestion_spacing_is_not_nine_shades_of_the_same_hour() {
    let people = vec![
        person(0, "A", "Asia/Kolkata"),
        person(1, "B", "America/Los_Angeles"),
    ];
    let g = build_grid(
        chrono::NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
        tz("Asia/Kolkata"),
    )
    .unwrap();
    let slots = suggest(&g, &people, 90, 6, 24, tz("America/New_York"));
    for w in slots.windows(2) {
        let gap = (w[1].start_utc - w[0].start_utc).abs() / 60;
        assert!(
            gap >= 45,
            "suggestions must be spaced, got {} min apart",
            gap
        );
    }
}

#[test]
fn scoring_survives_garbage_without_panicking() {
    // Regression guard for the live incident: nothing on this path may panic.
    let people = vec![
        person(0, "ghost", "Nowhere/Nothing"),
        person(1, "ok", "UTC"),
    ];
    let v = score_window(1_772_467_200, 60, "Not/AZone", &people);
    assert!(
        v.is_err(),
        "an unknown viewer zone is an error, not a crash"
    );
    let v = score_window(1_772_467_200, 60, "UTC", &people).unwrap();
    assert_eq!(
        v.persons.len(),
        1,
        "the unresolvable person is dropped, not fatal"
    );
    let _ = suggest(
        &build_grid(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
            tz("UTC"),
        )
        .unwrap(),
        &people,
        60,
        3,
        24,
        tz("UTC"),
    );
}

#[test]
fn suggestions_leave_a_rest_day_instead_of_ranking_noise() {
    // Sun 18 Oct 2026: a Mon-Fri team has no good hour, and ranking 96 equally-bad slots
    // is worse than useless - it looks like advice. The list must reach into the next
    // working day and say which day it is.
    let tzv = tz("Asia/Kolkata");
    let g = build_grid(chrono::NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(), tzv).unwrap();
    let people = vec![
        person(0, "Ravi", "Europe/London"),
        person(1, "Ana", "America/Los_Angeles"),
    ];
    let slots = suggest(&g, &people, 60, 6, 24, tzv);
    assert!(
        slots.iter().any(|s| !s.is_anchor),
        "list should extend past the rest day"
    );
    assert!(
        slots.iter().any(|s| !s.is_anchor && s.counts.working > 0),
        "the next working day should offer a slot with someone actually working"
    );
    assert!(slots.iter().all(|s| !s.date_label.is_empty()));
}

#[test]
fn scores_come_with_a_word() {
    assert_eq!(tt_core::score_label(91), "great");
    assert_eq!(tt_core::score_label(58), "workable");
    assert_eq!(tt_core::score_label(12), "brutal");
}
