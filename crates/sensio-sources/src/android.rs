//! The `android` source: the app's raw lines (ADR-0004), what Android's own
//! APIs reported, one JSON object per line with its type in `t`.
//!
//! This module is the contract between the app and the core. The app writes
//! what the platform gave it and decides nothing; everything below is parsed
//! here, so the app and a replay of its saved feed read the same way.
//!
//! - `wifi`: one Wi-Fi scan's results (`WifiManager.getScanResults`), in
//!   `results`, each `{"bssid", "ssid", "freq", "level", "caps"}`:
//!   `ScanResult`'s BSSID, SSID (empty when hidden), frequency in MHz, level in
//!   dBm and capabilities string (`[WPA2-PSK-CCMP][ESS]`).
//! - `ble`: one BLE scan result: `addr` (`BluetoothDevice.getAddress`),
//!   `rssi`, and `record`, the advert's bytes (`ScanRecord.getBytes`) as hex.
//!   The record is parsed here into manufacturer data, service data and
//!   service UUIDs; its local name is left in the raw line only (ADR-0006
//!   section 6).
//! - `bt`: one Bluetooth Classic device found by discovery, which the user
//!   turns on (it transmits inquiries): `addr`, `rssi` and `class`, the
//!   `BluetoothClass` it announced. Its name stays in the raw line only.
//! - `loc`: one location fix: `lat`, `lon` in degrees, and `bearing`, the
//!   direction of travel (`Location.getBearing`) when Android reported one.
//!   Other fields the app writes (`provider`, `acc`, `speed`) stay in the raw
//!   line.
//! - `rot`: the phone's orientation from the rotation-vector sensor
//!   (`SensorManager.getOrientation`): `az` (azimuth from magnetic north),
//!   `pitch` and `roll` in degrees, and `acc`, the sensor's heading accuracy
//!   estimate in degrees, when it gave one.
//!
//! Any field may be absent where Android didn't report it; a line of another
//! type is rejected. Times come from the feed line's arrival, never from the
//! raw line (ADR-0005 section 3).

use sensio_core::EventKind;
use sensio_core::event::{
    MAX_ADVERT_ENTRIES, ManufacturerData, ServiceData, is_wifi_freq, reason_without_values,
};
use serde::Deserialize;

use crate::wifi;

/// The feed's name for this source.
pub const SOURCE: &str = "android";

/// The longest advert record accepted, in bytes: an extended advert's data
/// can reach 1650.
const MAX_RECORD_BYTES: usize = 2048;

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Line {
    Wifi {
        #[serde(default)]
        results: Vec<WifiResult>,
    },
    Ble {
        addr: String,
        rssi: Option<i16>,
        #[serde(default)]
        record: String,
    },
    Bt {
        addr: String,
        rssi: Option<i16>,
        class: Option<u32>,
    },
    Loc {
        lat: f64,
        lon: f64,
        bearing: Option<f64>,
    },
    Rot {
        az: f64,
        pitch: f64,
        roll: f64,
        acc: Option<f64>,
    },
}

#[derive(Deserialize)]
struct WifiResult {
    bssid: String,
    ssid: Option<String>,
    freq: Option<u32>,
    level: Option<i16>,
    caps: Option<String>,
}

/// The events in one raw line of the app's.
///
/// # Errors
///
/// Returns why the line can't be used, never quoting its values.
pub fn normalize(raw: &str) -> Result<Vec<EventKind>, String> {
    let line: Line = serde_json::from_str(raw).map_err(|err| reason_without_values(&err))?;
    match line {
        Line::Wifi { results } => Ok(results.into_iter().map(wifi).collect()),
        Line::Ble { addr, rssi, record } => {
            let bytes = hex_bytes(&record)
                .ok_or_else(|| "BLE record isn't hex of at most 2048 bytes".to_owned())?;
            let (manufacturer_data, service_data, service_uuids) = parse_record(&bytes)?;
            Ok(vec![EventKind::BleAdvert {
                addr,
                // Android reports 127 when it has no strength for a result.
                rssi_dbm: wifi::dbm(rssi),
                manufacturer_data,
                service_data,
                service_uuids,
            }])
        }
        Line::Bt { addr, rssi, class } => Ok(vec![EventKind::BtClassic {
            addr,
            rssi_dbm: wifi::dbm(rssi),
            device_class: class.filter(|&c| c <= 0x00ff_ffff),
        }]),
        Line::Loc { lat, lon, bearing } => Ok(vec![EventKind::GnssFix {
            lat_deg: lat,
            lon_deg: lon,
            course_deg: bearing,
        }]),
        Line::Rot {
            az,
            pitch,
            roll,
            acc,
        } => Ok(vec![EventKind::Heading {
            azimuth_deg: az,
            pitch_deg: pitch,
            roll_deg: roll,
            accuracy_deg: acc,
        }]),
    }
}

/// One access point. Fields the event schema would reject (a name over 32
/// bytes, a frequency outside the Wi-Fi bands, a strength out of range) are
/// dropped rather than losing the access point; its address is checked by
/// the core.
fn wifi(result: WifiResult) -> EventKind {
    EventKind::WifiAp {
        bssid: result.bssid,
        ssid: wifi::ssid(result.ssid),
        freq_mhz: result.freq.filter(|&f| is_wifi_freq(f)),
        rssi_dbm: wifi::dbm(result.level),
        security: result.caps.as_deref().map(wifi::security),
    }
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || s.len() > 2 * MAX_RECORD_BYTES {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// A UUID written as the event schema does, from its little-endian bytes in an
/// advert: 4 or 8 hex digits, or the 36-character form of a 128-bit UUID.
fn uuid(le: &[u8]) -> String {
    let be: Vec<u8> = le.iter().rev().copied().collect();
    let h = hex(&be);
    if be.len() == 16 {
        format!(
            "{}-{}-{}-{}-{}",
            &h[..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..]
        )
    } else {
        h
    }
}

type Parsed = (Vec<ManufacturerData>, Vec<ServiceData>, Vec<String>);

/// An advert's AD structures (length, type, data), from the Bluetooth Core
/// Specification Supplement's data types: 0x02-0x07 service UUID lists
/// (16-, 32- and 128-bit), 0x16, 0x20 and 0x21 service data (16-, 32- and
/// 128-bit UUIDs), 0xFF manufacturer-specific data. A length of 0 ends the
/// record (the padding Android leaves). Other types, the local name among
/// them, are skipped. Past [`MAX_ADVERT_ENTRIES`] of one kind, the rest are
/// dropped.
fn parse_record(record: &[u8]) -> Result<Parsed, String> {
    let mut manufacturer = Vec::new();
    let mut service_data = Vec::new();
    let mut uuids = Vec::new();
    let mut rest = record;
    while let Some((&len, tail)) = rest.split_first() {
        if len == 0 {
            break;
        }
        let len = usize::from(len);
        if tail.len() < len {
            return Err("BLE record has an AD structure longer than the record".to_owned());
        }
        let (ad, after) = tail.split_at(len);
        rest = after;
        let (kind, data) = (ad[0], &ad[1..]);
        match kind {
            0x02 | 0x03 => uuids.extend(data.chunks_exact(2).map(uuid)),
            0x04 | 0x05 => uuids.extend(data.chunks_exact(4).map(uuid)),
            0x06 | 0x07 => uuids.extend(data.chunks_exact(16).map(uuid)),
            0x16 | 0x20 | 0x21 => {
                let width = match kind {
                    0x16 => 2,
                    0x20 => 4,
                    _ => 16,
                };
                if data.len() >= width {
                    service_data.push(ServiceData {
                        uuid: uuid(&data[..width]),
                        data: hex(&data[width..]),
                    });
                }
            }
            0xff if data.len() >= 2 => manufacturer.push(ManufacturerData {
                company_id: u16::from_le_bytes([data[0], data[1]]),
                data: hex(&data[2..]),
            }),
            _ => {}
        }
    }
    manufacturer.truncate(MAX_ADVERT_ENTRIES);
    service_data.truncate(MAX_ADVERT_ENTRIES);
    uuids.truncate(MAX_ADVERT_ENTRIES);
    Ok((manufacturer, service_data, uuids))
}
