//! The loop-closers: the .ics we hand out and the URL we ask people to click.
//!
//! Both are external contracts with silent failure modes - a calendar client that
//! rejects an unfolded line, a link that decodes to the wrong people. So both are
//! pinned here.

use tt_core::{
    build_ics, build_plain_text, parse_tz, score_window, AppState, IcsOptions, Person, Role,
    Weekend,
};

fn people() -> Vec<Person> {
    let mut a = Person::new(0, "You", "Bengaluru", "Asia/Kolkata");
    a.role = Role::Organizer;
    let mut b = Person::new(1, "Ana", "San Francisco", "America/Los_Angeles");
    b.work_start_min = 8 * 60;
    b.work_end_min = 16 * 60;
    let mut c = Person::new(2, "Mei", "Tokyo", "Asia/Tokyo");
    c.weekend = Weekend::SatSun;
    c.flex_min = 30;
    vec![a, b, c]
}

/// 2026-03-03T16:00Z = 21:30 IST, 08:00 PDT, 01:00 JST (next day).
const START: i64 = 1_772_553_600; // 2026-03-03T16:00:00Z

fn verdict() -> tt_core::Verdict {
    score_window(START, 90, "Asia/Kolkata", &people()).unwrap()
}

#[test]
fn ics_uses_utc_form_and_needs_no_vtimezone() {
    let ics = build_ics(&verdict(), &people(), &IcsOptions::default());
    assert!(ics.contains("DTSTART:20260303T160000Z"), "got: {ics}");
    assert!(ics.contains("DTEND:20260303T173000Z"));
    assert!(
        !ics.contains("BEGIN:VTIMEZONE"),
        "UTC form must not need a VTIMEZONE"
    );
    assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
    assert!(ics.trim_end().ends_with("END:VCALENDAR"));
}

#[test]
fn every_ics_line_is_crlf_and_folded_at_75_octets() {
    let opts = IcsOptions {
        summary: "Weekly cross-timezone sync between the platform and field teams".into(),
        description_extra: Some(
            "Agenda: 1. thing 2. other thing 3. a rather long item that goes on".into(),
        ),
        ..IcsOptions::default()
    };
    let ics = build_ics(&verdict(), &people(), &opts);
    for line in ics.split("\r\n") {
        assert!(
            line.len() <= 75,
            "unfolded line of {} bytes: {line:?}",
            line.len()
        );
    }
    assert!(
        ics.contains("\r\n "),
        "the long description must have been folded"
    );
    // Reassembling the fold must recover the original text.
    let joined = ics.replace("\r\n ", "");
    assert!(joined.contains("Weekly cross-timezone sync between the platform and field teams"));
}

#[test]
fn ics_text_is_escaped() {
    let opts = IcsOptions {
        summary: "Sync; plan, part 1".into(),
        ..IcsOptions::default()
    };
    let ics = build_ics(&verdict(), &people(), &opts);
    assert!(ics.contains("SUMMARY:Sync\\; plan\\, part 1"), "got: {ics}");
}

#[test]
fn attendee_local_times_land_in_the_description() {
    let opts = IcsOptions {
        link: Some("https://timetomeet.shantanugoel.com/#v=1".into()),
        ..IcsOptions::default()
    };
    let ics = build_ics(&verdict(), &people(), &opts);
    let joined = ics.replace("\r\n ", "");
    assert!(
        joined.contains("You (Bengaluru): Tue 3 Mar 21:30 +05:30"),
        "got: {joined}"
    );
    assert!(joined.contains("Ana (San Francisco): Tue 3 Mar 08:00 -08:00"));
    assert!(
        joined.contains("Mei (Tokyo): Wed 4 Mar 01:00 +09:00"),
        "Tokyo rolls to the next day"
    );
    assert!(joined.contains("timetomeet.shantanugoel.com/#v=1"));
}

#[test]
fn re_downloading_the_same_slot_yields_the_same_uid() {
    let a = build_ics(&verdict(), &people(), &IcsOptions::default());
    let b = build_ics(&verdict(), &people(), &IcsOptions::default());
    let uid = |s: &str| {
        s.lines()
            .find(|l| l.starts_with("UID:"))
            .unwrap()
            .to_string()
    };
    assert_eq!(
        uid(&a),
        uid(&b),
        "same slot must produce the same UID, or re-download duplicates the event"
    );
    // A different slot must produce a different one.
    let other = score_window(START + 3600, 90, "Asia/Kolkata", &people()).unwrap();
    let c = build_ics(&other, &people(), &IcsOptions::default());
    assert_ne!(uid(&a), uid(&c));
}

#[test]
fn plain_text_block_is_paste_ready() {
    let t = build_plain_text(
        &verdict(),
        parse_tz("Asia/Kolkata").unwrap(),
        Some("https://x/y"),
    );
    assert!(t.contains("Meeting: 21:30 (90 min)"));
    assert!(t.contains("Ana") && t.contains("08:00") && t.contains("San Francisco"));
    assert!(t.contains("https://x/y"));
    assert!(
        !t.contains("**"),
        "no markdown that breaks when pasted into Slack"
    );
}

// ---------------------------------------------------------------- URL state

#[test]
fn state_round_trips_through_the_url() {
    let st = AppState {
        anchor_date: "2026-03-03".into(),
        viewer_tz: "Asia/Kolkata".into(),
        duration_min: 90,
        start_utc: Some(START),
        people: people(),
        group_name: Some("Platform sync".into()),
    };
    let encoded = st.encode();
    assert!(encoded.starts_with("v=1&"), "versioned: {encoded}");
    let back = AppState::decode(&encoded);
    assert_eq!(back.anchor_date.as_str(), "2026-03-03");
    assert_eq!(back.viewer_tz.as_str(), "Asia/Kolkata");
    assert_eq!(back.duration_min, 90);
    assert_eq!(back.start_utc, Some(START));
    assert_eq!(back.people.len(), 3);
    assert_eq!(back.group_name.as_deref(), Some("Platform sync"));
    let ana = back.people.iter().find(|p| p.name == "Ana").unwrap();
    assert_eq!(
        (ana.work_start_min, ana.work_end_min, ana.flex_min),
        (480, 960, 60)
    );
    let mei = back.people.iter().find(|p| p.name == "Mei").unwrap();
    assert_eq!(mei.weekend, Weekend::SatSun);
    assert_eq!(mei.flex_min, 30);
    assert_eq!(back.people[0].role, Role::Organizer);
}

#[test]
fn names_with_url_hostile_characters_survive() {
    // Ampersands, hashes, percent signs, spaces, pipes and tildes are all plausible in a
    // name someone types, and all of them are structural characters in the encoding.
    for name in [
        "A & B",
        "100% sure",
        "#tag",
        "a|b",
        "a~b",
        "a=b",
        "Ünïcødé ✓",
        "a b c",
    ] {
        let mut p = Person::new(0, name, "city, with comma", "Asia/Kolkata");
        p.role = Role::Optional;
        let st = AppState {
            anchor_date: "2026-03-03".into(),
            viewer_tz: "UTC".into(),
            duration_min: 60,
            start_utc: None,
            people: vec![p],
            group_name: Some(format!("g {name}")),
        };
        let back = AppState::decode(&st.encode());
        assert_eq!(
            back.people.len(),
            1,
            "person with name {name:?} was dropped"
        );
        assert_eq!(
            back.people[0].name, name,
            "name round-trip failed for {name:?}"
        );
        assert_eq!(back.people[0].role, Role::Optional);
        assert_eq!(
            back.group_name.as_deref(),
            Some(format!("g {name}").as_str())
        );
    }
}

#[test]
fn a_corrupt_link_degrades_instead_of_taking_the_app_down() {
    // This is the sharing equivalent of the date-input bug: a hand-mangled link must
    // never be fatal.
    for junk in [
        "",
        "#",
        "v=1&d=not-a-date&z=Not/AZone&dur=99999&p=garbage",
        "v=1&p=~~~~~~~",
        "%00%00",
        "v=1&d=2026-13-45",
        "v=2&unknown=1",
        "v=1&p=x@y~",
        "v=1&dur=-5&s=notanumber",
    ] {
        let st = AppState::decode(junk);
        assert!(
            st.people.iter().all(|p| tt_core::parse_tz(&p.tz).is_some()),
            "junk link produced an unresolvable person: {junk:?}"
        );
        assert!(
            (15..=1440).contains(&st.duration_min),
            "duration escaped its bounds for {junk:?}"
        );
        assert!(
            st.anchor_date.is_empty()
                || chrono::NaiveDate::parse_from_str(&st.anchor_date, "%Y-%m-%d").is_ok()
        );
    }
}

#[test]
fn a_person_with_an_unknown_zone_is_dropped_not_trusted() {
    let st = AppState::decode(
        "v=1&d=2026-03-03&z=UTC&p=Bob~X~Not/AZone~0900-1700~60~r~ss|Ann~Y~UTC~0900-1700~60~r~ss",
    );
    assert_eq!(st.people.len(), 1, "only the resolvable person survives");
    assert_eq!(st.people[0].name, "Ann");
}

#[test]
fn an_oversized_link_is_capped_rather_than_exhausting_the_browser() {
    let mut q = String::from("v=1&d=2026-03-03&z=UTC&p=");
    for i in 0..500 {
        if i > 0 {
            q.push('|');
        }
        q.push_str(&format!("p{i}~c~UTC~0900-1700~60~r~ss"));
    }
    let st = AppState::decode(&q);
    assert!(
        st.people.len() <= 32,
        "capped at 32 people, got {}",
        st.people.len()
    );
}
