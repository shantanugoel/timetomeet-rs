//! `tt` - the same engine as the web app, in the terminal.
//!
//! This exists partly because it is genuinely useful (planning an on-call handoff from
//! a shell, generating an .ics in a script) and mostly because it *forces the engine to
//! stay honest*: anything the CLI can do must be pure tt-core, with no browser in the
//! loop. If the CLI gets awkward, the engine boundary is wrong.
//!
//! Examples
//!   tt now Asia/Kolkata America/Los_Angeles Europe/London
//!   tt plan 'Mei@Asia/Tokyo, Ana@America/Los_Angeles, You@Asia/Kolkata' --dur 90
//!   tt plan '...' --date 2026-03-03 --slots 5 --grid
//!   tt ics  '...' --start 2026-03-03T16:00Z -o meeting.ics
//!   tt share '...' --dur 90

use std::io::Write;

use chrono_tz::Tz;
use tt_core::{offset_label, parse_tz, Person, Quality, Role};

const BARS: [char; 4] = ['\u{2591}', '\u{2592}', '\u{2593}', '\u{2588}']; // ░ ▒ ▓ █

fn usage() -> ! {
    eprintln!(
        "tt - plan a meeting across timezones\n\n\
         \x20 tt now [TZ ...]\n\
         \x20 tt plan 'Name@TZ[,Name@TZ]' [--date YYYY-MM-DD] [--dur MIN] [--slots N]\n\
         \x20         [--viewer TZ] [--grid] [--json] [--text] [--share]\n\
         \x20 tt ics  'Name@TZ,...' [--date ...] [--start YYYY-MM-DDTHH:MMZ] [--dur MIN] [-o f.ics]\n\
         \x20 tt search QUERY [--dataset app/public/tt-dataset.bin]\n\n\
         TZ is an IANA zone, e.g. Asia/Kolkata. --viewer defaults to $TZ, else UTC."
    );
    std::process::exit(2);
}

fn viewer_tz() -> Tz {
    std::env::var("TZ")
        .ok()
        .and_then(|s| parse_tz(&s))
        .unwrap_or_else(|| parse_tz("UTC").expect("UTC is always valid"))
}

/// Parse `Name@TZ` / `@TZ` / `TZ` comma-separated. Unknown zones are reported and skipped
/// rather than fatal - a typo should not lose you the other five people.
fn parse_people(spec: &str) -> (Vec<Person>, Vec<String>) {
    let mut out = Vec::new();
    let mut errs = Vec::new();
    for chunk in spec.split(',') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let (name, tzname) = match chunk.split_once('@') {
            Some((n, t)) => (n.trim(), t.trim()),
            None => ("", chunk),
        };
        if parse_tz(tzname).is_none() {
            errs.push(format!("skipped {chunk:?}: unknown timezone"));
            continue;
        }
        let mut p = Person::new(
            out.len() as u8,
            if name.is_empty() { tzname } else { name },
            tzname,
            tzname,
        );
        if out.is_empty() {
            p.role = Role::Organizer;
        }
        out.push(p);
    }
    (out, errs)
}

fn parse_start(s: &str) -> Option<i64> {
    // Accept YYYY-MM-DDTHH:MMZ or a bare unix timestamp.
    if let Ok(ts) = s.parse::<i64>() {
        return Some(ts);
    }
    let naive = chrono::NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%dT%H:%M")
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S")
        })
        .ok()?;
    Some(naive.and_utc().timestamp())
}

fn bar_for(q: Quality) -> char {
    BARS[q.rank() as usize]
}

fn print_grid(report: &tt_core::DayReport, viewer: Tz) {
    let Some(day) = report.days.first() else {
        return;
    };
    // Hour ruler in the viewer's zone.
    let mut ruler = String::from("      ");
    for h in 0..24 {
        let ts = day.grid_start + (h as i64) * 3600;
        let lm = tt_core::time::local_minutes(ts, viewer);
        let label = format!("{:02}", (lm / 60) % 24);
        ruler.push_str(&if h % 2 == 0 { label } else { "  ".to_string() });
    }
    println!("{ruler}");
    for row in &day.rows {
        if !row.ok {
            println!(
                "{:<14} {}",
                row.name,
                row.error.as_deref().unwrap_or_default()
            );
            continue;
        }
        let mut cells = String::new();
        for band in &row.bands {
            let w = (band.end_min - band.start_min) / 15;
            for _ in 0..w {
                cells.push(bar_for(band.quality));
            }
        }
        let sign = if row.dst_change { " \u{26a0} dst" } else { "" };
        println!(
            "{:<14} {:>6} {cells}{}",
            row.name.chars().take(12).collect::<String>(),
            offset_label(row.offset_min),
            sign
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }
    let cmd = args[0].as_str();
    let rest = &args[1..];

    let mut flag_map: Vec<(String, String)> = Vec::new();
    let mut pos: Vec<String> = Vec::new();
    let mut i = 0;
    let raw: Vec<String> = rest.to_vec();
    while i < raw.len() {
        let a = raw[i].clone();
        if a.starts_with('-') {
            const BOOLS: [&str; 4] = ["--grid", "--json", "--text", "--share"];
            if BOOLS.contains(&a.as_str()) {
                flag_map.push((a, String::new()));
                i += 1;
            } else {
                let v = raw.get(i + 1).cloned().unwrap_or_default();
                flag_map.push((a, v));
                i += 2;
            }
        } else {
            pos.push(a);
            i += 1;
        }
    }
    let opt = |k: &str| -> Option<String> {
        flag_map
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.clone())
    };
    let has = |k: &str| -> bool { flag_map.iter().any(|(a, _)| a == k) };

    match cmd {
        "now" => {
            let zones: Vec<String> = if pos.is_empty() {
                vec![viewer_tz().name().to_string()]
            } else {
                pos.clone()
            };
            let now = chrono::Utc::now().timestamp();
            for z in zones {
                match parse_tz(&z) {
                    Some(tz) => println!(
                        "{:<32} {} {} ({})",
                        z,
                        tt_core::time::fmt_date(now, tz),
                        tt_core::time::fmt_hhmm(now, tz),
                        offset_label(tt_core::time::offset_minutes(now, tz))
                    ),
                    None => eprintln!("{z}: unknown timezone"),
                }
            }
        }
        "plan" | "slots" => {
            let spec = pos.first().cloned().unwrap_or_default();
            if spec.is_empty() {
                eprintln!("need at least one person, e.g. 'You@Asia/Kolkata'");
                std::process::exit(2);
            }
            let (mut people, errs) = parse_people(&spec);
            for e in errs {
                eprintln!("warning: {e}");
            }
            if people.is_empty() {
                eprintln!("no valid people");
                std::process::exit(2);
            }
            let viewer = opt("--viewer")
                .and_then(|s| parse_tz(&s))
                .unwrap_or_else(viewer_tz);
            let dur: u16 = opt("--dur").and_then(|s| s.parse().ok()).unwrap_or(60);
            let slots: usize = opt("--slots").and_then(|s| s.parse().ok()).unwrap_or(5);
            let now = chrono::Utc::now().timestamp();
            let date = opt("--date").unwrap_or_else(|| {
                tt_core::local_date_of(now, viewer)
                    .format("%Y-%m-%d")
                    .to_string()
            });

            if has("--json") {
                let r = tt_core::build_day(&date, viewer.name(), &people, now, 1);
                println!("{}", serde_json::to_string_pretty(&r.unwrap()).unwrap());
                return;
            }

            let report = match tt_core::build_day(&date, viewer.name(), &people, now, 1) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            };
            println!(
                "{} \u{00b7} viewed from {} ({}) \u{00b7} {} min",
                report
                    .days
                    .first()
                    .map(|d| d.date_label.clone())
                    .unwrap_or_default(),
                viewer.name(),
                offset_label(report.viewer_offset_min),
                dur
            );
            if has("--grid") {
                print_grid(&report, viewer);
            }
            let best = tt_core::best_slots(&date, viewer.name(), &people, now, dur, slots, 24)
                .unwrap_or_else(|e| {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                });
            println!("\nbest {dur}-min slots");
            for (n, s) in best.iter().enumerate() {
                let start = tt_core::time::fmt_hhmm(s.start_utc, viewer);
                let end = tt_core::time::fmt_hhmm(s.start_utc + dur as i64 * 60, viewer);
                let per: Vec<String> = s
                    .persons
                    .iter()
                    .map(|p| {
                        format!(
                            "{} {}",
                            p.name.chars().take(6).collect::<String>(),
                            p.local_start
                        )
                    })
                    .collect();
                println!(
                    " {:>2}. {}-{}  score {:>3}   {}ok {}tight {}hard {}asleep\n      {}",
                    n + 1,
                    start,
                    end,
                    s.score,
                    s.counts.working,
                    s.counts.shoulder,
                    s.counts.off_hours,
                    s.counts.asleep,
                    per.join(" \u{00b7} ")
                );
            }
            if has("--text") {
                if let Some(first) = best.first() {
                    println!(
                        "\n{}",
                        tt_core::build_plain_text(
                            &tt_core::score_window(first.start_utc, dur, viewer.name(), &people)
                                .unwrap(),
                            viewer,
                            None,
                        )
                    );
                }
            }
            if has("--share") {
                if let Some(first) = best.first() {
                    let st = tt_core::AppState {
                        anchor_date: date.clone(),
                        viewer_tz: viewer.name().to_string(),
                        duration_min: dur,
                        start_utc: Some(first.start_utc),
                        people: people.clone(),
                        group_name: None,
                    };
                    println!("\nhttps://timetomeet.shantanugoel.com/#{}", st.encode());
                }
            }
            let _ = &mut people;
        }
        "ics" => {
            let spec = pos.first().cloned().unwrap_or_default();
            let (people, errs) = parse_people(&spec);
            for e in errs {
                eprintln!("warning: {e}");
            }
            let viewer = opt("--viewer")
                .and_then(|s| parse_tz(&s))
                .unwrap_or_else(viewer_tz);
            let dur: u16 = opt("--dur").and_then(|s| s.parse().ok()).unwrap_or(60);
            let now = chrono::Utc::now().timestamp();
            let start = match opt("--start").and_then(|s| parse_start(&s)) {
                Some(s) => s,
                None => {
                    eprintln!("--start YYYY-MM-DDTHH:MMZ is required for ics");
                    std::process::exit(2);
                }
            };
            let v = tt_core::score_window(start, dur, viewer.name(), &people).unwrap();
            let ics = tt_core::build_ics(
                &v,
                &people,
                &tt_core::IcsOptions {
                    summary: "Meeting".into(),
                    description_extra: None,
                    link: None,
                    include_local_times: true,
                    location: None,
                    now_utc: now,
                },
            );
            match opt("-o") {
                Some(path) if !path.is_empty() => {
                    let mut f = std::fs::File::create(&path).expect("cannot write output");
                    f.write_all(ics.as_bytes()).expect("write failed");
                    eprintln!("wrote {path}");
                }
                _ => print!("{ics}"),
            }
        }
        "search" => {
            let q = pos.join(" ");
            let path = opt("--dataset").unwrap_or_else(|| "app/public/tt-dataset.bin".into());
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => {
                    eprintln!("dataset not found at {path}\nbuild it: cargo run -p tt-dataset -- --in cities15000.txt --out {path}");
                    std::process::exit(1);
                }
            };
            let ds = match tt_core::Dataset::parse(&bytes) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("dataset error: {e}");
                    std::process::exit(1);
                }
            };
            let now = chrono::Utc::now().timestamp();
            for h in ds.search(&q, 10, now) {
                println!(
                    "{:<26} {:<3} {:<28} {:>6}  {} {}",
                    h.name, h.country, h.tz, h.local_time, h.local_date, h.tz
                );
            }
        }
        "version" | "-V" | "--version" => println!("tt {}", tt_core::VERSION),
        "help" | "-h" | "--help" => usage(),
        other => {
            eprintln!("unknown command {other:?}");
            usage();
        }
    }
}
