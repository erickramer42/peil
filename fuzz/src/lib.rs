//! Generators shared by the structured fuzz targets: sessions of events and
//! rule parameters drawn from `arbitrary` bytes.
//!
//! Values come mostly from small pools (a few devices, a grid of places,
//! steps of seconds) so the judge sees devices that repeat and places that
//! differ, with a raw escape hatch for any value at all.

use arbitrary::Arbitrary;
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

/// An event kind, mostly well formed.
#[derive(Arbitrary, Debug, Clone)]
pub enum Kind {
    /// A GNSS fix.
    Fix(Coord),
    /// A BLE advert.
    Ble(Addr, Option<i16>),
    /// A TPMS transmission; the ID is one of a few, or any text.
    Tpms(Result<u8, String>, Option<f32>),
    /// A Wi-Fi access point.
    Ap(Addr, Option<String>, Option<u32>, Option<i16>),
    /// A Wi-Fi station.
    Client(Addr, Option<Addr>, Option<u32>, Option<i16>),
}

impl Kind {
    /// The event kind it stands for.
    #[must_use]
    pub fn event_kind(&self) -> EventKind {
        match self {
            Self::Fix(coord) => {
                let (lat_deg, lon_deg) = coord.lat_lon();
                EventKind::GnssFix { lat_deg, lon_deg }
            }
            Self::Ble(addr, rssi_dbm) => EventKind::BleAdvert {
                addr: addr.text(),
                rssi_dbm: *rssi_dbm,
            },
            Self::Tpms(id, rssi_dbm) => EventKind::Tpms {
                sensor_id: match id {
                    Ok(n) => format!("0a1b2c{:02x}", n % 4),
                    Err(s) => s.clone(),
                },
                rssi_dbm: *rssi_dbm,
            },
            Self::Ap(bssid, ssid, freq_mhz, rssi_dbm) => EventKind::WifiAp {
                bssid: bssid.text(),
                ssid: ssid.clone(),
                freq_mhz: *freq_mhz,
                rssi_dbm: *rssi_dbm,
            },
            Self::Client(addr, bssid, freq_mhz, rssi_dbm) => EventKind::WifiClient {
                addr: addr.text(),
                bssid: bssid.as_ref().map(Addr::text),
                freq_mhz: *freq_mhz,
                rssi_dbm: *rssi_dbm,
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
        }
    }
}
