//! The `kismet` source: one Kismet device record per raw line, the nested
//! JSON that Kismet's REST API serves and its log keeps in the `devices`
//! table's `device` column (read from Kismet 2026.10.R1, 2026-11-07).
//!
//! Kismet runs on another computer and is read across a process boundary
//! (ADR-0001); the laptop's `peil` passes each updated record on as a line
//! (ADR-0004 section 6). A record says what Kismet knows about one device so
//! far; the event it gives is that device seen at the line's arrival, with
//! Kismet's last signal.
//!
//! - `IEEE802.11`, `Wi-Fi AP`: an access point, with the network it last
//!   beaconed (its name unless cloaked, and its crypt string as security).
//! - `Wi-Fi Client` and `Wi-Fi Device`: a station, with the access point it
//!   was last on, if any.
//! - `Bluetooth`, `BTLE` and `BTLE Tracker`: a BLE advert, with its
//!   manufacturer data (Kismet keeps the bytes after the company ID, as the
//!   event schema does), service data and service UUIDs.
//! - `Wi-Fi Bridged` (wired devices seen through an access point, not on the
//!   air) and `BR/EDR` (Bluetooth Classic, which the event schema has no kind
//!   for yet) give no event.
//!
//! Everything else in a record is left in the raw line: the probed network
//! names, which the core never keeps; WPS and vendor device names, which name
//! people's devices; signal histories; locations. A field of an unexpected
//! shape counts as absent rather than losing the record.

use sensio_core::EventKind;
use sensio_core::event::{
    MAX_ADVERT_DATA_BYTES, MAX_ADVERT_ENTRIES, ManufacturerData, ServiceData, is_hex_bytes, is_mac,
    is_uuid, is_wifi_freq, reason_without_values,
};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::wifi;

/// The feed's name for this source.
pub const SOURCE: &str = "kismet";

/// A field that reads as `None` when it's missing or of another shape.
fn lenient<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(
    d: D,
) -> Result<Option<T>, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(serde_json::from_value(value).ok())
}

#[derive(Deserialize)]
struct Device {
    #[serde(rename = "kismet.device.base.macaddr")]
    mac: String,
    #[serde(rename = "kismet.device.base.phyname")]
    phy: String,
    #[serde(rename = "kismet.device.base.type")]
    kind: String,
    #[serde(
        rename = "kismet.device.base.frequency",
        default,
        deserialize_with = "lenient"
    )]
    freq_khz: Option<u64>,
    #[serde(
        rename = "kismet.device.base.signal",
        default,
        deserialize_with = "lenient"
    )]
    signal: Option<Signal>,
    #[serde(
        rename = "kismet.device.base.crypt",
        default,
        deserialize_with = "lenient"
    )]
    crypt: Option<String>,
    #[serde(rename = "dot11.device", default, deserialize_with = "lenient")]
    dot11: Option<Dot11>,
    #[serde(rename = "bluetooth.device", default, deserialize_with = "lenient")]
    bluetooth: Option<Bluetooth>,
}

#[derive(Deserialize)]
struct Signal {
    #[serde(
        rename = "kismet.common.signal.type",
        default,
        deserialize_with = "lenient"
    )]
    kind: Option<String>,
    #[serde(
        rename = "kismet.common.signal.last_signal",
        default,
        deserialize_with = "lenient"
    )]
    last: Option<i64>,
}

#[derive(Deserialize)]
struct Dot11 {
    #[serde(
        rename = "dot11.device.last_bssid",
        default,
        deserialize_with = "lenient"
    )]
    last_bssid: Option<String>,
    #[serde(
        rename = "dot11.device.last_beaconed_ssid_record",
        default,
        deserialize_with = "lenient"
    )]
    beaconed: Option<AdvertisedSsid>,
}

#[derive(Deserialize)]
struct AdvertisedSsid {
    #[serde(
        rename = "dot11.advertisedssid.ssid",
        default,
        deserialize_with = "lenient"
    )]
    ssid: Option<String>,
    #[serde(
        rename = "dot11.advertisedssid.cloaked",
        default,
        deserialize_with = "lenient"
    )]
    cloaked: Option<i64>,
    #[serde(
        rename = "dot11.advertisedssid.crypt_string",
        default,
        deserialize_with = "lenient"
    )]
    crypt: Option<String>,
}

#[derive(Deserialize)]
struct Bluetooth {
    #[serde(
        rename = "bluetooth.device.manuf_company_id",
        default,
        deserialize_with = "lenient"
    )]
    company_id: Option<String>,
    #[serde(
        rename = "bluetooth.device.manuf_data",
        default,
        deserialize_with = "lenient"
    )]
    manuf_data: Option<String>,
    #[serde(
        rename = "bluetooth.device.service_uuid_strs",
        default,
        deserialize_with = "lenient"
    )]
    service_uuids: Option<Vec<Value>>,
    #[serde(
        rename = "bluetooth.device.service_data_bytes",
        default,
        deserialize_with = "lenient"
    )]
    service_data: Option<serde_json::Map<String, Value>>,
}

/// The event in one Kismet device record, or none for a device the event
/// schema doesn't cover.
///
/// # Errors
///
/// Returns why the record can't be used (not JSON, no address, a type or
/// PHY Kismet didn't have when this was written), never quoting its values.
pub fn normalize(raw: &str) -> Result<Vec<EventKind>, String> {
    let device: Device = serde_json::from_str(raw).map_err(|err| reason_without_values(&err))?;
    let rssi_dbm = device
        .signal
        .as_ref()
        .filter(|s| s.kind.as_deref() == Some("dbm"))
        .and_then(|s| wifi::dbm(s.last.filter(|&l| l != 0)));
    let freq_mhz = device
        .freq_khz
        .and_then(|khz| u32::try_from(khz.saturating_add(500) / 1000).ok())
        .filter(|&mhz| is_wifi_freq(mhz));
    match (device.phy.as_str(), device.kind.as_str()) {
        ("IEEE802.11", "Wi-Fi AP") => {
            let beaconed = device.dot11.and_then(|d| d.beaconed);
            let (ssid, crypt) = match beaconed {
                Some(b) => (
                    b.ssid.filter(|_| b.cloaked != Some(1)),
                    b.crypt.filter(|c| !c.is_empty()),
                ),
                None => (None, None),
            };
            let crypt = crypt.or(device.crypt.filter(|c| !c.is_empty()));
            Ok(vec![EventKind::WifiAp {
                bssid: device.mac,
                ssid: wifi::ssid(ssid),
                freq_mhz,
                rssi_dbm,
                security: crypt.as_deref().map(wifi::security),
            }])
        }
        ("IEEE802.11", "Wi-Fi Client" | "Wi-Fi Device") => {
            let bssid = device.dot11.and_then(|d| d.last_bssid).filter(|b| {
                is_mac(b) && b != "00:00:00:00:00:00" && !b.eq_ignore_ascii_case(&device.mac)
            });
            Ok(vec![EventKind::WifiClient {
                addr: device.mac,
                bssid,
                freq_mhz,
                rssi_dbm,
            }])
        }
        ("IEEE802.11", "Wi-Fi Bridged") | ("Bluetooth", "BR/EDR") => Ok(Vec::new()),
        ("Bluetooth", "BTLE" | "BTLE Tracker") => {
            let bt = device.bluetooth;
            let (manufacturer_data, service_data, service_uuids) =
                bt.map(advert).unwrap_or_default();
            Ok(vec![EventKind::BleAdvert {
                addr: device.mac,
                rssi_dbm,
                manufacturer_data,
                service_data,
                service_uuids,
            }])
        }
        ("IEEE802.11" | "Bluetooth", _) => Err("Kismet device type not handled".to_owned()),
        _ => Err("Kismet PHY not handled".to_owned()),
    }
}

type Advert = (Vec<ManufacturerData>, Vec<ServiceData>, Vec<String>);

/// A BLE device's advert contents, each entry kept only if the event schema
/// would accept it.
fn advert(bt: Bluetooth) -> Advert {
    let data_ok = |d: &str| is_hex_bytes(d, MAX_ADVERT_DATA_BYTES);
    let manufacturer = bt
        .company_id
        .filter(|id| id.len() == 4)
        .and_then(|id| u16::from_str_radix(&id, 16).ok())
        .map(|company_id| ManufacturerData {
            company_id,
            data: bt.manuf_data.unwrap_or_default().to_ascii_lowercase(),
        })
        .filter(|m| data_ok(&m.data))
        .into_iter()
        .collect();
    let service_data = bt
        .service_data
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(uuid, data)| match data {
            Value::String(data) if is_uuid(&uuid) && data_ok(&data) => Some(ServiceData {
                uuid: uuid.to_ascii_lowercase(),
                data: data.to_ascii_lowercase(),
            }),
            _ => None,
        })
        .take(MAX_ADVERT_ENTRIES)
        .collect();
    let service_uuids = bt
        .service_uuids
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| match v {
            Value::String(s) if is_uuid(&s) => Some(s.to_ascii_lowercase()),
            _ => None,
        })
        .take(MAX_ADVERT_ENTRIES)
        .collect();
    (manufacturer, service_data, service_uuids)
}
