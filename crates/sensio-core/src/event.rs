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
    /// What produced it, for example `kismet`, `rtl_433`, `gpsd` or `android`.
    pub source: String,
    /// The radio that received it and where it sits (`phone`, `thinkpad`,
    /// `rear-pi`), from the user's configuration; `None` when unknown. It
    /// never splits a device: the same address from two receivers is one
    /// device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    /// What was seen.
    #[serde(flatten)]
    pub kind: EventKind,
}

/// What an event observed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    /// A BLE advertisement. The advertised device name is never kept here
    /// (it is often a person's name); it stays only in the raw line
    /// (ADR-0006 section 6).
    BleAdvert {
        /// The advertiser's address as the source reported it.
        addr: String,
        /// Received signal strength, if the source gave one.
        rssi_dbm: Option<i16>,
        /// The advert's manufacturer-specific data, in the advert's order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        manufacturer_data: Vec<ManufacturerData>,
        /// The advert's service data, in the advert's order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        service_data: Vec<ServiceData>,
        /// The service UUIDs the advert lists.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        service_uuids: Vec<String>,
    },
    /// A tire-pressure sensor's transmission.
    Tpms {
        /// The sensor's ID as the decoder reported it.
        sensor_id: String,
        /// Received signal strength, if the decoder gave one.
        rssi_dbm: Option<f32>,
    },
    /// A Wi-Fi access point, from its beacons or a scan result.
    WifiAp {
        /// The access point's address (BSSID).
        bssid: String,
        /// The network name it announces; `None` when hidden or not reported.
        ssid: Option<String>,
        /// The channel's center frequency, if the source gave it.
        freq_mhz: Option<u32>,
        /// Received signal strength, if the source gave one.
        rssi_dbm: Option<i16>,
        /// The security it announces, if the source reported it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        security: Option<WifiSecurity>,
    },
    /// A Wi-Fi station sending frames, seen in monitor mode. Nothing it sent
    /// is kept beyond its address and the network it's on (ADR-0001 section 5).
    WifiClient {
        /// The station's address.
        addr: String,
        /// The address (BSSID) of the network it's on, if any.
        bssid: Option<String>,
        /// The channel's center frequency, if the source gave it.
        freq_mhz: Option<u32>,
        /// Received signal strength, if the source gave one.
        rssi_dbm: Option<i16>,
    },
    /// A position fix from the GNSS receiver.
    GnssFix {
        /// Latitude, degrees north.
        lat_deg: f64,
        /// Longitude, degrees east.
        lon_deg: f64,
    },
}

/// One manufacturer-specific data entry of a BLE advert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManufacturerData {
    /// The Bluetooth SIG company identifier that opens the entry.
    pub company_id: u16,
    /// The bytes after the company identifier, as hex.
    pub data: String,
}

/// One service data entry of a BLE advert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceData {
    /// The service's UUID: 4 or 8 hex digits for a 16- or 32-bit UUID, or
    /// the 36-character form of a 128-bit one.
    pub uuid: String,
    /// The bytes after the UUID, as hex.
    pub data: String,
}

/// The security an access point announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiSecurity {
    /// The strongest protocol it offers.
    pub protocol: WifiProtocol,
    /// Whether it authenticates through 802.1X (enterprise) rather than a
    /// shared key.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enterprise: bool,
}

/// A Wi-Fi security protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiProtocol {
    /// No security.
    Open,
    /// Opportunistic Wireless Encryption: encrypted, unauthenticated.
    Owe,
    /// WEP.
    Wep,
    /// WPA.
    Wpa,
    /// WPA2.
    Wpa2,
    /// WPA3.
    Wpa3,
}

/// A device the judge tracks: one per transmitter identity a source reports.
///
/// The prefix names the radio, so a BLE address and a TPMS ID never collide.
/// Wi-Fi access points and stations share `wifi:`: a BSSID and a station's
/// address come from one address space, so one address is one radio in
/// either role.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DeviceId(String);

impl DeviceId {
    /// The ID's text form, for example `ble:aa:bb:cc:dd:ee:ff` or `wifi:aa:bb:cc:dd:ee:ff`.
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
            Self::WifiAp { bssid: addr, .. } | Self::WifiClient { addr, .. } => {
                Some(DeviceId(format!("wifi:{}", addr.to_ascii_lowercase())))
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
    ///
    /// The reason never quotes a value from the input: those are a
    /// stranger's (a device's address, say), and an error message is what
    /// gets pasted into an issue.
    #[error("line {line}, column {column}: {reason}")]
    Json {
        /// The 1-based line number.
        line: usize,
        /// The 1-based column where the parser stopped.
        column: usize,
        /// What's wrong, without the input's values.
        reason: String,
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
        let event: Event = serde_json::from_str(&text).map_err(|err| ParseError::Json {
            line: line_no,
            column: err.column(),
            reason: reason_without_values(&err),
        })?;
        event.validate().map_err(|reason| ParseError::Invalid {
            line: line_no,
            reason,
        })?;
        events.push(event);
    }
    Ok(events)
}

/// `serde_json`'s reason for rejecting a line, without any value it quoted.
///
/// Syntax and end-of-input errors are fixed text and pass through. Data
/// errors come from serde and can quote the input (`invalid type: string
/// "...", expected i16`, ``unknown variant `...`, expected one of ...``):
/// those keep their kind and the schema's "expected" part, and lose the
/// value. The "expected" part is found from the end, because serde writes it
/// after the value, and a value can itself contain ", expected ".
#[must_use]
pub fn reason_without_values(err: &serde_json::Error) -> String {
    let message = err.to_string();
    // `serde_json` appends " at line L column C"; the line and column are kept apart.
    let message = message
        .rsplit_once(" at line ")
        .map_or(message.as_str(), |(head, _)| head);
    if err.classify() != serde_json::error::Category::Data
        // These name one of the schema's own fields, never a value.
        || message.starts_with("missing field ")
        || message.starts_with("duplicate field ")
    {
        return message.to_owned();
    }
    let (head, expected) = message
        .rfind(", expected ")
        .map_or((message, ""), |i| message.split_at(i));
    match head.find(['"', '`']) {
        Some(quote) => format!("{} (value left out){expected}", head[..quote].trim_end()),
        None => format!("{head}{expected}"),
    }
}

/// Whether `s` is a 48-bit address written `xx:xx:xx:xx:xx:xx` in hex, either case.
#[must_use]
pub fn is_mac(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 17
        && bytes.iter().enumerate().all(|(i, &b)| {
            if i % 3 == 2 {
                b == b':'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// Whether `mhz` is a center frequency inside a Wi-Fi band (2.4, 5 or 6 GHz).
#[must_use]
pub fn is_wifi_freq(mhz: u32) -> bool {
    (2400..=2500).contains(&mhz) || (4900..=5900).contains(&mhz) || (5925..=7125).contains(&mhz)
}

/// The most entries of one kind (manufacturer data, service data, service
/// UUIDs) an advert carries: an extended advert holds 254 bytes, and every
/// entry takes at least a few.
pub const MAX_ADVERT_ENTRIES: usize = 32;

/// The most bytes one advert entry's data holds (ADR-0006 section 6).
pub const MAX_ADVERT_DATA_BYTES: usize = 255;

/// Whether `s` is hex for at most `max_bytes` bytes, either case.
#[must_use]
pub fn is_hex_bytes(s: &str, max_bytes: usize) -> bool {
    s.len() % 2 == 0 && s.len() <= 2 * max_bytes && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `s` is a Bluetooth UUID: 4 or 8 hex digits, or the 36-character
/// form of a 128-bit UUID (`8-4-4-4-12` hex digits), either case.
#[must_use]
pub fn is_uuid(s: &str) -> bool {
    let bytes = s.as_bytes();
    match bytes.len() {
        4 | 8 => bytes.iter().all(u8::is_ascii_hexdigit),
        36 => bytes.iter().enumerate().all(|(i, &b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        }),
        _ => false,
    }
}

/// The checks a BLE advert's optional contents share.
fn validate_advert(
    manufacturer_data: &[ManufacturerData],
    service_data: &[ServiceData],
    service_uuids: &[String],
) -> Result<(), &'static str> {
    if manufacturer_data.len() > MAX_ADVERT_ENTRIES
        || service_data.len() > MAX_ADVERT_ENTRIES
        || service_uuids.len() > MAX_ADVERT_ENTRIES
    {
        return Err("more than 32 BLE advert entries of one kind");
    }
    if manufacturer_data
        .iter()
        .any(|m| !is_hex_bytes(&m.data, MAX_ADVERT_DATA_BYTES))
    {
        return Err("BLE manufacturer data isn't hex of at most 255 bytes");
    }
    if service_data
        .iter()
        .any(|d| !is_uuid(&d.uuid) || !is_hex_bytes(&d.data, MAX_ADVERT_DATA_BYTES))
    {
        return Err("BLE service data isn't a UUID with hex of at most 255 bytes");
    }
    if service_uuids.iter().any(|u| !is_uuid(u)) {
        return Err("BLE service UUID isn't 4 or 8 hex digits or a 128-bit UUID");
    }
    Ok(())
}

/// A received signal strength a receiver can plausibly report, in dBm.
const RSSI_DBM: std::ops::RangeInclusive<i16> = -127..=20;

/// The checks a Wi-Fi sighting's optional radio fields share.
fn validate_wifi_radio(freq_mhz: Option<u32>, rssi_dbm: Option<i16>) -> Result<(), &'static str> {
    if freq_mhz.is_some_and(|f| !is_wifi_freq(f)) {
        return Err("Wi-Fi frequency outside the Wi-Fi bands");
    }
    if rssi_dbm.is_some_and(|r| !RSSI_DBM.contains(&r)) {
        return Err("Wi-Fi RSSI out of range");
    }
    Ok(())
}

/// Whether `s` is a name the user or a front end chose (a receiver, a source):
/// 1 to 32 characters of `a-z`, `0-9`, `-`, `_` or `.`.
#[must_use]
pub fn is_label(s: &str) -> bool {
    let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b);
    !s.is_empty() && s.len() <= 32 && s.bytes().all(allowed)
}

impl Event {
    /// Checks every value against its range.
    ///
    /// # Errors
    ///
    /// Returns what's wrong, without the value itself.
    pub fn validate(&self) -> Result<(), &'static str> {
        validate(self)
    }
}

fn validate(event: &Event) -> Result<(), &'static str> {
    if event.receiver.as_deref().is_some_and(|r| !is_label(r)) {
        return Err("receiver must be 1 to 32 characters of a-z, 0-9, '-', '_' or '.'");
    }
    match &event.kind {
        EventKind::GnssFix { lat_deg, lon_deg } => {
            if !lat_deg.is_finite() || !(-90.0..=90.0).contains(lat_deg) {
                return Err("latitude out of range");
            }
            if !lon_deg.is_finite() || !(-180.0..=180.0).contains(lon_deg) {
                return Err("longitude out of range");
            }
        }
        EventKind::BleAdvert {
            addr,
            rssi_dbm,
            manufacturer_data,
            service_data,
            service_uuids,
        } => {
            if !is_mac(addr) {
                return Err("BLE address isn't of the form xx:xx:xx:xx:xx:xx");
            }
            if rssi_dbm.is_some_and(|r| !RSSI_DBM.contains(&r)) {
                return Err("BLE RSSI out of range");
            }
            validate_advert(manufacturer_data, service_data, service_uuids)?;
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
        EventKind::WifiAp {
            bssid,
            ssid,
            freq_mhz,
            rssi_dbm,
            security: _,
        } => {
            if !is_mac(bssid) {
                return Err("Wi-Fi BSSID isn't an address of the form xx:xx:xx:xx:xx:xx");
            }
            // 802.11 allows up to 32 bytes; a hidden network has no name, never an empty one.
            if ssid.as_ref().is_some_and(|s| s.is_empty() || s.len() > 32) {
                return Err("SSID must be 1 to 32 bytes");
            }
            validate_wifi_radio(*freq_mhz, *rssi_dbm)?;
        }
        EventKind::WifiClient {
            addr,
            bssid,
            freq_mhz,
            rssi_dbm,
        } => {
            if !is_mac(addr) {
                return Err("Wi-Fi station address isn't of the form xx:xx:xx:xx:xx:xx");
            }
            if bssid.as_ref().is_some_and(|b| !is_mac(b)) {
                return Err("Wi-Fi BSSID isn't an address of the form xx:xx:xx:xx:xx:xx");
            }
            validate_wifi_radio(*freq_mhz, *rssi_dbm)?;
        }
    }
    Ok(())
}
