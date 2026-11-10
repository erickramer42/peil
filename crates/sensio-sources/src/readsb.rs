//! The `readsb` source (ADR-0007): one aircraft as readsb describes it, the
//! object from its aircraft list with the list's `now` added
//! ([`crate::readsb_file`]), field names from readsb's `README-json.md`
//! (github.com/wiedehopf/readsb, read 2026-11-09).
//!
//! Every field is lenient: a wrong shape or a value out of range reads as
//! absent, so what this gives always passes the core's validation; only an
//! object without a usable address is rejected. The event's time is the
//! line's arrival (ADR-0002 section 3): readsb's `now` and `seen` stay in the
//! raw line.

use sensio_core::EventKind;
use serde_json::{Map, Value};

/// The feed's name for this source.
pub const SOURCE: &str = "readsb";

/// A position older than this, seconds before readsb's `now`, isn't the
/// aircraft's position at the line's arrival, so it's left out.
pub const MAX_POSITION_AGE_S: f64 = 10.0;

/// The emergencies readsb names, besides `none`.
const EMERGENCIES: [&str; 7] = [
    "general",
    "lifeguard",
    "minfuel",
    "nordo",
    "unlawful",
    "downed",
    "reserved",
];

/// The aircraft event one readsb object gives.
///
/// # Errors
///
/// A reason, quoting none of the text, when it isn't a JSON object with an
/// address of six hex digits (after a `~` for a non-ICAO one).
pub fn normalize(raw: &str) -> Result<Vec<EventKind>, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|err| sensio_core::event::reason_without_values(&err))?;
    let object = value
        .as_object()
        .ok_or_else(|| "an aircraft isn't a JSON object".to_owned())?;
    let address = object
        .get("hex")
        .and_then(Value::as_str)
        .filter(|h| {
            let hex = h.strip_prefix('~').unwrap_or(h);
            hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())
        })
        .ok_or_else(|| "no aircraft address of six hex digits".to_owned())?
        .to_ascii_lowercase();
    let text = |key: &str| object.get(key).and_then(Value::as_str);
    let number = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite())
    };
    let feet = |key: &str| number(key).and_then(whole_feet);
    let on_ground = text("alt_baro") == Some("ground");
    let (lat_deg, lon_deg) = position(object);
    Ok(vec![EventKind::Aircraft {
        address,
        heard_as: text("type")
            .filter(|t| sensio_core::event::is_label(t))
            .map(str::to_owned),
        callsign: text("flight")
            .map(str::trim)
            .filter(|c| is_callsign(c))
            .map(str::to_owned),
        alt_baro_ft: feet("alt_baro"),
        on_ground,
        alt_geom_ft: feet("alt_geom"),
        ground_speed_kt: number("gs").filter(|s| (0.0..=5000.0).contains(s)),
        track_deg: number("track").filter(|t| (0.0..360.0).contains(t)),
        lat_deg,
        lon_deg,
        emitter_category: text("category")
            .filter(|c| is_category(c))
            .map(str::to_owned),
        squawk: text("squawk")
            .filter(|s| s.len() == 4 && s.bytes().all(|b| (b'0'..=b'7').contains(&b)))
            .map(str::to_owned),
        emergency: text("emergency")
            .filter(|e| EMERGENCIES.contains(e))
            .map(str::to_owned),
        signal_dbfs: number("rssi").filter(|r| (-100.0..=0.0).contains(r)),
    }])
}

/// The aircraft's position, when both coordinates are in range and it's no
/// older than [`MAX_POSITION_AGE_S`].
fn position(object: &Map<String, Value>) -> (Option<f64>, Option<f64>) {
    let number = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite())
    };
    let fresh = object
        .get("seen_pos")
        .map_or(Some(true), |s| {
            s.as_f64().map(|s| (0.0..=MAX_POSITION_AGE_S).contains(&s))
        })
        .unwrap_or(false);
    match (number("lat"), number("lon")) {
        (Some(lat), Some(lon))
            if fresh && (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) =>
        {
            (Some(lat), Some(lon))
        }
        _ => (None, None),
    }
}

/// An altitude in whole feet, when it's -2000 to 130000.
#[allow(clippy::cast_possible_truncation)] // In range, so it fits an i32 exactly.
fn whole_feet(feet: f64) -> Option<i32> {
    (-2000.0..=130_000.0)
        .contains(&feet)
        .then(|| feet.round() as i32)
}

/// A callsign the core keeps: 1 to 8 of A-Z, 0-9 and space.
fn is_callsign(c: &str) -> bool {
    !c.is_empty()
        && c.len() <= 8
        && c.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' ')
}

/// An emitter category code as readsb prints it, `A0` to `D7`.
fn is_category(c: &str) -> bool {
    let b = c.as_bytes();
    b.len() == 2 && (b'A'..=b'D').contains(&b[0]) && (b'0'..=b'7').contains(&b[1])
}
