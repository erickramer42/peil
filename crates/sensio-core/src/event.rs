//! The normalized event schema: one JSON object per line (JSONL).
//!
//! These are normalized events, derived from a recording's raw sources on
//! replay; they are never the recording itself (ADR-0002). Every field that
//! comes from a radio is untrusted, so parsing validates ranges and rejects
//! what it can't use instead of passing it on.

use std::fmt;
use std::io::BufRead;

use serde::{Deserialize, Serialize};

/// Nanoseconds since the Unix epoch (UTC), on the session clock.
pub type TimeNs = i64;

/// One normalized observation from a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// When the source saw it, on the session clock.
    pub t_ns: TimeNs,
    /// What produced it, for example `kismet`, `rtl_433` or `gpsd`.
    pub source: String,
    /// What was seen.
    #[serde(flatten)]
    pub kind: EventKind,
}

/// What an event observed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    /// A BLE advertisement.
    BleAdvert {
        /// The advertiser's address as the source reported it.
        addr: String,
        /// Received signal strength, if the source gave one.
        rssi_dbm: Option<i16>,
    },
    /// A tire-pressure sensor's transmission.
    Tpms {
        /// The sensor's ID as the decoder reported it.
        sensor_id: String,
        /// Received signal strength, if the decoder gave one.
        rssi_dbm: Option<f32>,
    },
    /// A position fix from the GNSS receiver.
    GnssFix {
        /// Latitude, degrees north.
        lat_deg: f64,
        /// Longitude, degrees east.
        lon_deg: f64,
    },
}

/// A device the judge tracks: one per transmitter identity a source reports.
///
/// The prefix names the radio, so a BLE address and a TPMS ID never collide.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DeviceId(String);

impl DeviceId {
    /// The ID's text form, for example `ble:aa:bb:cc:dd:ee:ff`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl EventKind {
    /// The device this event saw, or `None` for an event that isn't a sighting.
    #[must_use]
    pub fn device(&self) -> Option<DeviceId> {
        match self {
            Self::BleAdvert { addr, .. } => {
                Some(DeviceId(format!("ble:{}", addr.to_ascii_lowercase())))
            }
            Self::Tpms { sensor_id, .. } => {
                Some(DeviceId(format!("tpms:{}", sensor_id.to_ascii_lowercase())))
            }
            Self::GnssFix { .. } => None,
        }
    }
}

/// Why a line of normalized events was rejected.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The input couldn't be read.
    #[error("reading line {line}: {source}")]
    Io {
        /// The 1-based line number.
        line: usize,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The line isn't a valid event.
    #[error("line {line}: {source}")]
    Json {
        /// The 1-based line number.
        line: usize,
        /// The underlying error.
        source: serde_json::Error,
    },
    /// The line parsed but a value is out of range.
    #[error("line {line}: {reason}")]
    Invalid {
        /// The 1-based line number.
        line: usize,
        /// What's wrong with it.
        reason: &'static str,
    },
}

/// Reads normalized events, one per line; blank lines are skipped.
///
/// # Errors
///
/// Returns the first line that can't be read, isn't an event, or carries an
/// out-of-range value.
pub fn parse_jsonl(reader: impl BufRead) -> Result<Vec<Event>, ParseError> {
    let mut events = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line_no = index + 1;
        let text = line.map_err(|source| ParseError::Io {
            line: line_no,
            source,
        })?;
        if text.trim().is_empty() {
            continue;
        }
        let event: Event = serde_json::from_str(&text).map_err(|source| ParseError::Json {
            line: line_no,
            source,
        })?;
        validate(&event).map_err(|reason| ParseError::Invalid {
            line: line_no,
            reason,
        })?;
        events.push(event);
    }
    Ok(events)
}

fn validate(event: &Event) -> Result<(), &'static str> {
    match &event.kind {
        EventKind::GnssFix { lat_deg, lon_deg } => {
            if !lat_deg.is_finite() || !(-90.0..=90.0).contains(lat_deg) {
                return Err("latitude out of range");
            }
            if !lon_deg.is_finite() || !(-180.0..=180.0).contains(lon_deg) {
                return Err("longitude out of range");
            }
        }
        EventKind::BleAdvert { addr, .. } => {
            if addr.is_empty() {
                return Err("empty BLE address");
            }
        }
        EventKind::Tpms {
            sensor_id,
            rssi_dbm,
        } => {
            if sensor_id.is_empty() {
                return Err("empty TPMS sensor ID");
            }
            if rssi_dbm.is_some_and(|r| !r.is_finite()) {
                return Err("TPMS RSSI not finite");
            }
        }
    }
    Ok(())
}
