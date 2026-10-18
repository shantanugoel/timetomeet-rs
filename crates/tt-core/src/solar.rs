//! Sunrise / sunset via the standard Meeus approximation (the "Sunrise equation").
//!
//! This exists so a row can show *daylight*, not just declared working hours. It is
//! the difference between "18:00 is off-hours for Ana" and "18:00 in Berlin in March
//! is already dark", which is how people actually feel about a meeting.
//!
//! Returns absolute unix instants, so the caller decides how to project them onto a
//! grid; accuracy is well under a minute, far finer than the 15-minute grid needs.
//! Polar day/night return `None` rather than a fabricated time.

use chrono::{Datelike, NaiveDate};

use crate::model::SolarTimes;

/// Civil twilight angle for the upper limb of the sun with refraction.
const H0: f64 = -0.833_f64.to_radians();
const J2000: f64 = 2451545.0;
const UNIX_EPOCH_JD: f64 = 2440587.5;

fn rad(deg: f64) -> f64 {
    deg.to_radians()
}

fn mod360(x: f64) -> f64 {
    x.rem_euclid(360.0)
}

/// Julian date at 00:00 UT on `date`.
fn julian_day_midnight(date: NaiveDate) -> f64 {
    let y = date.year() as i64;
    let m = date.month() as i64;
    let d = date.day() as i64;
    let (yy, mm) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let a = yy / 100;
    let b = 2 - a + a / 4;
    (365.25 * (yy as f64 + 4716.0)).floor()
        + (30.6001 * (mm as f64 + 1.0)).floor()
        + d as f64
        + b as f64
        - 1524.5
}

fn jd_to_unix(jd: f64) -> i64 {
    ((jd - UNIX_EPOCH_JD) * 86_400.0).round() as i64
}

/// Absolute instants of sunrise and sunset nearest `date` at `lat`/`lon`.
pub fn sun_instants(date: NaiveDate, lat: f64, lon: f64) -> (Option<i64>, Option<i64>, bool) {
    if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90.0 {
        return (None, None, false);
    }
    let jd = julian_day_midnight(date);
    // Days since J2000 at 0h UT, rounded to the nearest day (the algorithm's n).
    let n = (jd - J2000 + 0.0008).round();
    // Longitude west-positive, per the conventional formulation.
    let lw = -lon;
    let j_star = n - lw / 360.0;

    let m = mod360(357.5291 + 0.98560028 * j_star);
    let c = 1.9148 * rad(m).sin() + 0.0200 * rad(2.0 * m).sin() + 0.0003 * rad(3.0 * m).sin();
    let ecliptic = mod360(m + c + 180.0 + 102.9372);
    let j_transit = J2000 + j_star + 0.0053 * rad(m).sin() - 0.0069 * rad(2.0 * ecliptic).sin();

    let eps = rad(23.4397 - 0.00000036 * n);
    let sin_dec = rad(ecliptic).sin() * eps.sin();
    let dec = sin_dec.asin();

    let lat_r = rad(lat);
    let cos_omega = (H0 - lat_r.sin() * dec.sin()) / (lat_r.cos() * dec.cos());
    if cos_omega > 1.0 || cos_omega < -1.0 {
        return (None, None, true); // polar day or polar night
    }
    let omega = cos_omega.acos().to_degrees();

    (
        Some(jd_to_unix(j_transit - omega / 360.0)),
        Some(jd_to_unix(j_transit + omega / 360.0)),
        false,
    )
}

/// Project sunrise/sunset onto a grid whose start is a unix instant.
pub fn sun_for_grid(grid_start: i64, date: NaiveDate, lat: f64, lon: f64) -> SolarTimes {
    let (rise, set, polar) = sun_instants(date, lat, lon);
    let to_grid = |ts: i64| -> Option<u16> {
        let gm = (ts - grid_start) / 60;
        (0..1440).contains(&gm).then_some(gm as u16)
    };
    SolarTimes {
        sunrise_min: rise.and_then(to_grid),
        sunset_min: set.and_then(to_grid),
        polar,
    }
}
