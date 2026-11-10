//! Generators shared by the structured fuzz targets: sessions of events and
//! rule parameters drawn from `arbitrary` bytes.
//!
//! Values come mostly from small pools (a few devices, a grid of places,
//! steps of seconds) so the judge sees devices that repeat and places that
//! differ, with a raw escape hatch for any value at all.

use arbitrary::Arbitrary;
use sensio_core::event::{ManufacturerData, ServiceData, WifiProtocol, WifiSecurity};
use sensio_core::{EventKind, PersistenceRule, TimeNs};

const NS_PER_S: i64 = 1_000_000_000;

/// A time: a step forward from the session's cursor, or any value.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub enum Time {
    /// The cursor moves this many seconds forward, and the time is the cursor.
    Step(u8),
    /// Any time; the cursor stays.
    Raw(i64),
}

impl Time {
    /// The time this draws, moving `cursor` for a step.
    pub fn at(self, cursor: &mut TimeNs) -> TimeNs {
        match self {
            Self::Step(s) => {
                *cursor = cursor.saturating_add(i64::from(s) * NS_PER_S);
                *cursor
            }
            Self::Raw(t) => t,
        }
    }
}

/// A coordinate pair: a point on a grid of about 1 km, or any floats.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub enum Coord {
    /// Hundredths of a degree from 60 N, 10 E.
    Grid(i8, i8),
    /// Any values, out of range and not finite included.
    Raw(f64, f64),
}

impl Coord {
    fn lat_lon(self) -> (f64, f64) {
        match self {
            Self::Grid(a, b) => (60.0 + f64::from(a) / 100.0, 10.0 + f64::from(b) / 100.0),
            Self::Raw(a, b) => (a, b),
        }
    }
}

/// An address: one of a few synthetic ones, or any text.
#[derive(Arbitrary, Debug, Clone)]
pub enum Addr {
    /// `aa:aa:aa:00:00:0N`, in either case.
    Pool(u8, bool),
    /// Any text.
    Raw(String),
}

impl Addr {
    fn text(&self) -> String {
        match self {
            Self::Pool(n, upper) => {
                let addr = format!("aa:aa:aa:00:00:{:02x}", n % 4);
                if *upper {
                    addr.to_ascii_uppercase()
                } else {
                    addr
                }
            }
            Self::Raw(s) => s.clone(),
        }
    }
}

/// Bytes as hex, or any text in their place.
#[derive(Arbitrary, Debug, Clone)]
pub enum Hex {
    /// Bytes, written as hex.
    Bytes(Vec<u8>),
    /// Any text.
    Raw(String),
}

impl Hex {
    fn text(&self) -> String {
        match self {
            Self::Bytes(bytes) => bytes.iter().map(|b| format!("{b:02x}")).collect(),
            Self::Raw(s) => s.clone(),
        }
    }
}

/// A Bluetooth UUID: 16-bit, 128-bit, or any text.
#[derive(Arbitrary, Debug, Clone)]
pub enum Uuid {
    /// A 16-bit UUID.
    Short(u16),
    /// A 128-bit UUID.
    Long(u128),
    /// Any text.
    Raw(String),
}

impl Uuid {
    fn text(&self) -> String {
        match self {
            Self::Short(n) => format!("{n:04x}"),
            Self::Long(n) => {
                let h = format!("{n:032x}");
                format!(
                    "{}-{}-{}-{}-{}",
                    &h[..8],
                    &h[8..12],
                    &h[12..16],
                    &h[16..20],
                    &h[20..]
                )
            }
            Self::Raw(s) => s.clone(),
        }
    }
}

/// A BLE advert's contents: company IDs mostly from a few that name
/// tracker networks, so the describe step's formats get exercised.
#[derive(Arbitrary, Debug, Clone, Default)]
pub struct Advert {
    /// Manufacturer data: a company (an index into a pool, or any ID) and its bytes.
    pub manufacturer: Vec<(Result<u8, u16>, Hex)>,
    /// Service data.
    pub service_data: Vec<(Uuid, Hex)>,
    /// Listed service UUIDs.
    pub uuids: Vec<Uuid>,
}

/// Company IDs the pool draws from: Apple, Samsung, Google, and one other.
const COMPANIES: [u16; 4] = [0x004c, 0x0075, 0x00e0, 0x0059];

impl Advert {
    fn parts(&self) -> (Vec<ManufacturerData>, Vec<ServiceData>, Vec<String>) {
        let manufacturer = self
            .manufacturer
            .iter()
            .map(|(company, data)| ManufacturerData {
                company_id: match company {
                    Ok(i) => COMPANIES[usize::from(*i) % COMPANIES.len()],
                    Err(id) => *id,
                },
                data: data.text(),
            })
            .collect();
        let service_data = self
            .service_data
            .iter()
            .map(|(uuid, data)| ServiceData {
                uuid: uuid.text(),
                data: data.text(),
            })
            .collect();
        (
            manufacturer,
            service_data,
            self.uuids.iter().map(Uuid::text).collect(),
        )
    }
}

/// An access point's security, as an index into the protocols and a flag.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub struct Security(u8, bool);

impl Security {
    fn security(self) -> WifiSecurity {
        const PROTOCOLS: [WifiProtocol; 6] = [
            WifiProtocol::Open,
            WifiProtocol::Owe,
            WifiProtocol::Wep,
            WifiProtocol::Wpa,
            WifiProtocol::Wpa2,
            WifiProtocol::Wpa3,
        ];
        WifiSecurity {
            protocol: PROTOCOLS[usize::from(self.0) % PROTOCOLS.len()],
            enterprise: self.1,
        }
    }
}

/// An event kind, mostly well formed.
#[derive(Arbitrary, Debug, Clone)]
pub enum Kind {
    /// A GNSS fix, with a course or none.
    Fix(Coord, Option<f64>),
    /// The receiver's heading: azimuth, pitch, roll and accuracy, any values.
    Heading(f64, f64, f64, Option<f64>),
    /// A BLE advert.
    Ble(Addr, Option<i16>, Advert),
    /// A Wi-Fi access point.
    Ap(
        Addr,
        Option<String>,
        Option<u32>,
        Option<i16>,
        Option<Security>,
    ),
    /// A Wi-Fi station.
    Client(Addr, Option<Addr>, Option<u32>, Option<i16>),
    /// A Bluetooth Classic device.
    Classic(Addr, Option<i16>, Option<u32>),
}

impl Kind {
    /// The event kind it stands for.
    #[must_use]
    pub fn event_kind(&self) -> EventKind {
        match self {
            Self::Fix(coord, course_deg) => {
                let (lat_deg, lon_deg) = coord.lat_lon();
                EventKind::GnssFix {
                    lat_deg,
                    lon_deg,
                    course_deg: *course_deg,
                }
            }
            Self::Heading(azimuth_deg, pitch_deg, roll_deg, accuracy_deg) => EventKind::Heading {
                azimuth_deg: *azimuth_deg,
                pitch_deg: *pitch_deg,
                roll_deg: *roll_deg,
                accuracy_deg: *accuracy_deg,
            },
            Self::Ble(addr, rssi_dbm, advert) => {
                let (manufacturer_data, service_data, service_uuids) = advert.parts();
                EventKind::BleAdvert {
                    addr: addr.text(),
                    rssi_dbm: *rssi_dbm,
                    manufacturer_data,
                    service_data,
                    service_uuids,
                }
            }
            Self::Ap(bssid, ssid, freq_mhz, rssi_dbm, security) => EventKind::WifiAp {
                bssid: bssid.text(),
                ssid: ssid.clone(),
                freq_mhz: *freq_mhz,
                rssi_dbm: *rssi_dbm,
                security: security.map(Security::security),
            },
            Self::Client(addr, bssid, freq_mhz, rssi_dbm) => EventKind::WifiClient {
                addr: addr.text(),
                bssid: bssid.as_ref().map(Addr::text),
                freq_mhz: *freq_mhz,
                rssi_dbm: *rssi_dbm,
            },
            Self::Classic(addr, rssi_dbm, device_class) => EventKind::BtClassic {
                addr: addr.text(),
                rssi_dbm: *rssi_dbm,
                device_class: *device_class,
            },
        }
    }
}

/// Rule parameters: any values, so the rejected ones are drawn too.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub struct Rule {
    /// Distinct places, at least.
    pub min_places: u8,
    /// Seconds from first sighting to last, at least.
    pub min_span_s: i16,
    /// Meters between places.
    pub place_separation_m: f64,
    /// Seconds a fix stays usable.
    pub max_fix_age_s: i16,
    /// Meters a second over the movement window for moving.
    pub moving_mps: f64,
    /// Seconds in the movement window.
    pub moving_window_s: i16,
    /// Seconds a stop lasts before it is a place.
    pub min_stop_s: i16,
}

impl Rule {
    /// The rule it stands for, not yet validated.
    #[must_use]
    pub fn rule(self) -> PersistenceRule {
        PersistenceRule {
            min_places: usize::from(self.min_places),
            min_span_ns: i64::from(self.min_span_s) * NS_PER_S,
            place_separation_m: self.place_separation_m,
            max_fix_age_ns: i64::from(self.max_fix_age_s) * NS_PER_S,
            moving_mps: self.moving_mps,
            moving_window_ns: i64::from(self.moving_window_s) * NS_PER_S,
            min_stop_ns: i64::from(self.min_stop_s) * NS_PER_S,
        }
    }
}
