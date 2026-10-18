//! The entire wasm boundary.
//!
//! Deliberately tiny and deliberately JSON at the edges: six pure functions in, plain
//! data out, no DOM, no callbacks into JS, no `Rc<RefCell<App>>` living in the module.
//! The view layer owns the document and the pointer events; this owns the time math and
//! the optimizer. That split is what keeps the payload small, keeps the engine testable
//! with `cargo test` on the host, and keeps 60 fps dragging possible - the view paints
//! each row as one CSS gradient, so dragging never touches band DOM at all.
//!
//! Every entry point returns a JSON string, including on error
//! (`{"error":"..."}`), so a bad argument degrades into a visible message instead of a
//! trap. That is the lesson from the live incident in docs/REDESIGN.md §0.1.

use std::cell::RefCell;

use tt_core::{AppState, Dataset, IcsOptions, Person};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

thread_local! {
    static DATASET: RefCell<Option<Dataset>> = const { RefCell::new(None) };
}

fn err<T: std::fmt::Display>(e: T) -> String {
    format!("{{\"error\":{:?}}}", e.to_string())
}

fn parse_people(json: &str) -> Result<Vec<Person>, String> {
    let mut people: Vec<Person> =
        serde_json::from_str(json).map_err(|e| format!("bad people json: {e}"))?;
    if people.len() > 32 {
        people.truncate(32);
    }
    for (i, p) in people.iter_mut().enumerate() {
        p.id = i as u8;
    }
    Ok(people)
}

fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(err)
}

/// Load the compiled city index. Returns `{"cities":N,"zones":M}` or `{"error":...}`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn init_dataset(bytes: &[u8]) -> String {
    match Dataset::parse(bytes) {
        Ok(d) => {
            let info = format!(
                "{{\"cities\":{},\"zones\":{}}}",
                d.city_count(),
                d.zone_count()
            );
            DATASET.with(|c| *c.borrow_mut() = Some(d));
            info
        }
        Err(e) => err(e),
    }
}

/// One day (or several) of grid rows: bands, local rulers, midnights, solar, DST flags.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn build_day(
    date: &str,
    viewer_tz: &str,
    people_json: &str,
    now: i64,
    span_days: i32,
) -> String {
    let people = match parse_people(people_json) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    match tt_core::build_day(date, viewer_tz, &people, now, span_days as i64) {
        Ok(r) => to_json(&r),
        Err(e) => err(e),
    }
}

/// Score one window. Called on every pointermove while dragging, so it stays cheap.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn score_window(
    start_utc: i64,
    duration_min: u16,
    viewer_tz: &str,
    people_json: &str,
) -> String {
    let people = match parse_people(people_json) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    match tt_core::score_window(start_utc, duration_min, viewer_tz, &people) {
        Ok(v) => to_json(&v),
        Err(e) => err(e),
    }
}

/// Ranked candidate slots.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn suggest_slots(
    date: &str,
    viewer_tz: &str,
    people_json: &str,
    now: i64,
    duration_min: u16,
    k: u32,
    span_hours: i32,
) -> String {
    let people = match parse_people(people_json) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    match tt_core::best_slots(
        date,
        viewer_tz,
        &people,
        now,
        duration_min,
        k as usize,
        span_hours as i64,
    ) {
        Ok(s) => to_json(&s),
        Err(e) => err(e),
    }
}

/// City / zone search with live offsets. Empty result if the dataset is not loaded yet.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn search_cities(query: &str, k: u32, now: i64) -> String {
    let loaded = DATASET.with(|c| c.borrow().is_some());
    if !loaded {
        return "[]".to_string();
    }
    DATASET.with(|c| {
        let b = c.borrow();
        let d = match b.as_ref() {
            Some(d) => d,
            None => return "[]".to_string(),
        };
        to_json(&d.search(query, (k as usize).clamp(1, 20), now))
    })
}

/// RFC 5545 payload as a string.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn render_ics(
    start_utc: i64,
    duration_min: u16,
    viewer_tz: &str,
    people_json: &str,
    opts_json: &str,
) -> String {
    let people = match parse_people(people_json) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    #[derive(serde::Deserialize, Default)]
    struct Opts {
        #[serde(default)]
        summary: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        link: Option<String>,
        #[serde(default)]
        location: Option<String>,
        #[serde(default = "default_true")]
        local_times: bool,
        #[serde(default)]
        now_utc: i64,
    }
    fn default_true() -> bool {
        true
    }
    let o: Opts = serde_json::from_str(opts_json).unwrap_or_default();
    let v = tt_core::score_window(start_utc, duration_min, viewer_tz, &people);
    let v = match v {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let summary = if o.summary.trim().is_empty() {
        "Meeting"
    } else {
        o.summary.as_str()
    };
    tt_core::build_ics(
        &v,
        &people,
        &IcsOptions {
            summary: summary.to_string(),
            description_extra: o.description,
            link: o.link,
            include_local_times: o.local_times,
            location: o.location,
            now_utc: o.now_utc,
        },
    )
}

/// Slack/Teams/email plain text.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn render_text(
    start_utc: i64,
    duration_min: u16,
    viewer_tz: &str,
    people_json: &str,
    link: &str,
) -> String {
    let people = match parse_people(people_json) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    let tz = match tt_core::parse_tz(viewer_tz) {
        Some(t) => t,
        None => return "unknown timezone".into(),
    };
    let v = match tt_core::score_window(start_utc, duration_min, viewer_tz, &people) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let link = if link.is_empty() { None } else { Some(link) };
    tt_core::build_plain_text(&v, tz, link)
}

/// URL hash from app state (JSON of `{anchorDate,viewerTz,durationMin,startUtc,people,groupName}`).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn encode_state(state_json: &str) -> String {
    #[derive(serde::Deserialize)]
    struct In {
        #[serde(default, rename = "anchorDate")]
        anchor_date: String,
        #[serde(default, rename = "viewerTz")]
        viewer_tz: String,
        #[serde(default, rename = "durationMin")]
        duration_min: u16,
        #[serde(default, rename = "startUtc")]
        start_utc: Option<i64>,
        #[serde(default)]
        people: Vec<Person>,
        #[serde(default)]
        group: Option<String>,
    }
    let i: In = match serde_json::from_str(state_json) {
        Ok(i) => i,
        Err(e) => return err(e),
    };
    let mut people = i.people;
    for (n, p) in people.iter_mut().enumerate() {
        p.id = n as u8;
    }
    let st = AppState {
        anchor_date: i.anchor_date,
        viewer_tz: i.viewer_tz,
        duration_min: if i.duration_min == 0 {
            60
        } else {
            i.duration_min
        },
        start_utc: i.start_utc,
        people,
        group_name: i.group,
    };
    st.encode()
}

/// URL hash -> state JSON. Never fails; unknown or malformed fields are dropped.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn decode_state(hash: &str) -> String {
    let st = AppState::decode(hash);
    #[derive(serde::Serialize)]
    struct Out {
        #[serde(rename = "anchorDate")]
        anchor_date: String,
        #[serde(rename = "viewerTz")]
        viewer_tz: String,
        #[serde(rename = "durationMin")]
        duration_min: u16,
        #[serde(rename = "startUtc")]
        start_utc: Option<i64>,
        people: Vec<Person>,
        group: Option<String>,
        #[serde(rename = "isEmpty")]
        is_empty: bool,
    }
    let empty = st.is_empty();
    to_json(&Out {
        anchor_date: st.anchor_date,
        viewer_tz: st.viewer_tz,
        duration_min: st.duration_min,
        start_utc: st.start_utc,
        people: st.people,
        group: st.group_name,
        is_empty: empty,
    })
}

/// Non-wasm build of this crate is used by the test harness; these are no-ops there.
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
fn _unused(_js: js_sys::JsString) {}
