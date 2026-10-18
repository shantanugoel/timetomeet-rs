//! RFC 5545 output - the step that closes the loop.
//!
//! Details that actually matter in the wild:
//! * `DTSTART`/`DTEND` in UTC form (`...T160000Z`) needs no `VTIMEZONE` block, which
//!   sidesteps the whole class of client bugs around embedded timezone definitions.
//! * `UID` is derived from the slot, so re-downloading updates the event instead of
//!   creating a second one.
//! * Lines are folded at 75 octets with CRLF + space, and TEXT is escaped. Clients do
//!   reject unfolded or unescaped payloads, and the failure is silent.
//! * No `ORGANIZER`: this is a static site, we do not know who you are, and inventing
//!   an address is worse than omitting it.

use crate::model::offset_label;
use crate::model::{Person, Verdict};
use crate::time::{fmt_hhmm, fmt_ics_utc};

/// Escape a TEXT value per RFC 5545 §3.3.11.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Fold a content line at 75 octets (UTF-8 safe: we fold on char boundaries).
fn fold(line: &str) -> String {
    const LIMIT: usize = 73; // leave room for the leading space on continuations
    if line.len() <= 75 {
        return format!("{line}\r\n");
    }
    let mut out = String::with_capacity(line.len() + 16);
    let mut buf = String::new();
    let mut len = 0usize;
    let mut first = true;
    for ch in line.chars() {
        let bl = ch.len_utf8();
        if len + bl > if first { 75 } else { LIMIT } {
            out.push_str(&buf);
            out.push_str("\r\n");
            buf = String::new();
            len = 0;
            first = false;
            buf.push(' ');
            len += 1;
        }
        buf.push(ch);
        len += bl;
    }
    if !buf.is_empty() {
        out.push_str(&buf);
        out.push_str("\r\n");
    }
    out
}

/// Stable, slot-derived UID. Not cryptographic - just stable and collision-unlikely.
fn slot_uid(start_utc: i64, duration_min: u16, people: &[Person], summary: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    fn mix(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= *b as u64;
            *h = h.wrapping_mul(0x1000_0000_01b3);
        }
    }
    mix(&mut h, &start_utc.to_le_bytes());
    mix(&mut h, &duration_min.to_le_bytes());
    for p in people {
        mix(&mut h, p.tz.as_bytes());
        mix(&mut h, p.name.as_bytes());
    }
    mix(&mut h, summary.as_bytes());
    format!("{h:016x}@timetomeet.shantanugoel.com")
}

pub struct IcsOptions {
    pub summary: String,
    pub description_extra: Option<String>,
    pub link: Option<String>,
    pub include_local_times: bool,
    pub location: Option<String>,
    pub now_utc: i64,
}

impl Default for IcsOptions {
    fn default() -> Self {
        IcsOptions {
            summary: "Meeting".into(),
            description_extra: None,
            link: None,
            include_local_times: true,
            location: None,
            now_utc: 0,
        }
    }
}

/// Build a `text/calendar` payload for the window in `v`.
pub fn build_ics(v: &Verdict, people: &[Person], opts: &IcsOptions) -> String {
    let end_utc = v.start_utc + (v.duration_min as i64) * 60;
    let dtstamp = fmt_ics_utc(if opts.now_utc > 0 {
        opts.now_utc
    } else {
        v.start_utc
    });

    let mut desc = String::new();
    if opts.include_local_times {
        desc.push_str("Local times:\n");
        for pv in &v.persons {
            desc.push_str(&format!(
                "  {} ({}): {} {} {}\n",
                pv.name,
                pv.city,
                pv.local_date,
                pv.local_start,
                offset_label(pv.offset_min)
            ));
        }
        desc.push_str(&format!(
            "\nScore {} - {} comfortable, {} tight, {} hard, {} asleep\n",
            v.score, v.counts.working, v.counts.shoulder, v.counts.off_hours, v.counts.asleep
        ));
    }
    if let Some(extra) = &opts.description_extra {
        if !extra.trim().is_empty() {
            desc.push('\n');
            desc.push_str(extra.trim());
            desc.push('\n');
        }
    }
    if let Some(link) = &opts.link {
        desc.push_str("\nTimeslot link: ");
        desc.push_str(link);
    }

    let mut out = String::with_capacity(1024);
    out.push_str(&fold("BEGIN:VCALENDAR"));
    out.push_str(&fold("VERSION:2.0"));
    out.push_str(&fold(
        "PRODID:-//timetomeet.shantanugoel.com//Time to Meet//EN",
    ));
    out.push_str(&fold("CALSCALE:GREGORIAN"));
    out.push_str(&fold("METHOD:PUBLISH"));
    out.push_str(&fold("BEGIN:VEVENT"));
    out.push_str(&fold(&format!(
        "UID:{}",
        slot_uid(v.start_utc, v.duration_min, people, &opts.summary)
    )));
    out.push_str(&fold(&format!("DTSTAMP:{dtstamp}")));
    out.push_str(&fold(&format!("DTSTART:{}", fmt_ics_utc(v.start_utc))));
    out.push_str(&fold(&format!("DTEND:{}", fmt_ics_utc(end_utc))));
    out.push_str(&fold(&format!("SUMMARY:{}", escape_text(&opts.summary))));
    if let Some(loc) = &opts.location {
        if !loc.trim().is_empty() {
            out.push_str(&fold(&format!("LOCATION:{}", escape_text(loc))));
        }
    }
    if !desc.trim().is_empty() {
        out.push_str(&fold(&format!("DESCRIPTION:{}", escape_text(desc.trim()))));
    }
    out.push_str(&fold("SEQUENCE:0"));
    out.push_str(&fold("STATUS:CONFIRMED"));
    out.push_str(&fold("TRANSP:OPAQUE"));
    // End the event in the same zone it started, so a DST change inside the meeting
    // cannot make DTEND precede DTSTART in local terms.
    out.push_str(&fold("BEGIN:VALARM"));
    out.push_str(&fold("ACTION:DISPLAY"));
    out.push_str(&fold("DESCRIPTION:Starting soon"));
    out.push_str(&fold("TRIGGER:-PT10M"));
    out.push_str(&fold("END:VALARM"));
    out.push_str(&fold("END:VEVENT"));
    out.push_str(&fold("END:VCALENDAR"));
    out
}

/// The Slack/Teams/email block. Plain text, one line per person, no markdown that
/// breaks when pasted into a client.
pub fn build_plain_text(v: &Verdict, viewer_tz: chrono_tz::Tz, link: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Meeting: {} ({} min)\n",
        fmt_hhmm(v.start_utc, viewer_tz),
        v.duration_min
    ));
    for pv in &v.persons {
        let day = if pv.crosses_midnight {
            " → next day"
        } else {
            ""
        };
        out.push_str(&format!(
            "  {}  {} {}{} ({}) {}\n",
            pv.name,
            pv.local_date,
            pv.local_start,
            day,
            offset_label(pv.offset_min),
            pv.city
        ));
    }
    if let Some(l) = link {
        out.push_str(&format!("\n{l}"));
    }
    out
}
