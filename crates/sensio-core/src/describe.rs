//! The describe layer (ADR-0006): one description per emitter, by lookup and
//! measurement, the loud ones first.
//!
//! A description says what an emitter is as far as a lookup can say (its
//! kind, its vendor block, the tracker network its advert's format names,
//! its security) and how loud it is; it never says whether it matters, which
//! is the judge's business. Like the judge, it is a pure function of the
//! events, its parameters and its tables, so a replay describes the same way.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::Serialize;

use crate::event::{DeviceId, Event, EventKind, ServiceData, TimeNs, WifiSecurity};
use crate::feed::OwnRadio;
use crate::judge::order_key;
use crate::movement::{Fix, Window};

/// The version of the describe layer's code and its tables in code (the
/// advert formats); it follows `RULE_SET_VERSION`'s rules.
pub const DESCRIBE_VERSION: u32 = 7;

const NS_PER_S: i64 = 1_000_000_000;

/// The describe layer's parameters.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DescribeParams {
    /// At or above this, a Wi-Fi or BLE emitter is strong, in dBm.
    pub strong_dbm: i16,
    /// Below this, it is weak; in between, medium.
    pub weak_dbm: i16,
    /// How far back from the answer's time an emitter counts as recent.
    pub recent_ns: TimeNs,
    /// At or above this speed over the movement window, the vehicle is
    /// moving, in m/s.
    pub moving_mps: f64,
    /// The movement window: speed is measured from the newest fix at least
    /// this much older than the latest, so a parked receiver's jitter
    /// between fixes a second apart averages out.
    pub moving_window_ns: TimeNs,
    /// Stopped this long, it is parked.
    pub parked_after_ns: TimeNs,
    /// How many of the newest emitters the texture names.
    pub newest: usize,
}

impl Default for DescribeParams {
    fn default() -> Self {
        Self {
            strong_dbm: -60,
            weak_dbm: -80,
            recent_ns: 60 * NS_PER_S,
            moving_mps: 2.0,
            moving_window_ns: 10 * NS_PER_S,
            parked_after_ns: 300 * NS_PER_S,
            newest: 5,
        }
    }
}

impl DescribeParams {
    /// Checks the parameters.
    ///
    /// # Errors
    ///
    /// Returns the first one out of range.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.weak_dbm >= self.strong_dbm {
            return Err("weak_dbm must be below strong_dbm");
        }
        if self.recent_ns <= 0 {
            return Err("recent_ns must be above 0");
        }
        if !self.moving_mps.is_finite() || self.moving_mps <= 0.0 {
            return Err("moving_mps must be a finite speed above 0");
        }
        if self.moving_window_ns <= 0 {
            return Err("moving_window_ns must be above 0");
        }
        if self.parked_after_ns < 0 {
            return Err("parked_after_ns must not be negative");
        }
        Ok(())
    }
}

/// What kind of emitter it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmitterKind {
    /// A Wi-Fi access point.
    AccessPoint,
    /// A Wi-Fi station.
    Station,
    /// A BLE device.
    Ble,
    /// A Bluetooth Classic device.
    BtClassic,
    /// An aircraft, from its 1090 MHz broadcasts (ADR-0007).
    Aircraft,
}

/// A signal-strength band; never a distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    /// Strong.
    Strong,
    /// Medium.
    Medium,
    /// Weak.
    Weak,
    /// No strength on a scale the bands use (none reported, or an aircraft's
    /// dBFS).
    Unknown,
}

/// What an emitter's ID says about its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Address {
    /// A Wi-Fi address with the locally administered bit clear: a vendor block.
    Global,
    /// A Wi-Fi address with the locally administered bit set: no vendor.
    Randomized,
    /// A BLE address, whose type (public or random) the events don't carry,
    /// so it isn't looked up.
    Ble,
    /// An aircraft's 24-bit address, which no vendor registry covers.
    Aircraft,
}

/// The tracker network an advert's format names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tracker {
    /// Apple's Find My network (offline finding).
    AppleFindMy,
    /// Google's Find Hub network.
    GoogleFindHub,
    /// An accessory following the DULT draft, whatever its network.
    Dult,
    /// Tile's network.
    Tile,
    /// Samsung's `SmartThings` Find network.
    #[serde(rename = "smartthings_find")]
    SmartThingsFind,
}

/// A hardware category, with the table entry that matched (ADR-0006 section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Category {
    /// The category.
    pub category: &'static str,
    /// The table entry that matched.
    pub entry: &'static str,
}

/// Sightings by one receiver within the recent window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReceiverSightings {
    /// The receiver, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    /// Its sightings.
    pub sightings: u64,
}

/// One emitter's description.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Description {
    /// The emitter.
    pub device: DeviceId,
    /// Its kind.
    pub kind: EmitterKind,
    /// An access point's latest announced name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    /// The receivers that ever saw it, ordered.
    pub receivers: Vec<String>,
    /// Its first sighting.
    pub first_seen_ns: TimeNs,
    /// Its last sighting.
    pub last_seen_ns: TimeNs,
    /// Its sightings.
    pub sightings: u64,
    /// Its latest reported strength.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_dbm: Option<f32>,
    /// Its strongest reported strength within the recent window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strongest_dbm: Option<f32>,
    /// The band of its strongest recent strength.
    pub band: Band,
    /// Sightings within the recent window, per receiver.
    pub recent: Vec<ReceiverSightings>,
    /// What its address says.
    pub address: Address,
    /// The registry's organization for its vendor block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// The company IDs in its adverts' manufacturer data, ordered.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub company_ids: Vec<u16>,
    /// The tracker network its latest advert's format names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracker: Option<Tracker>,
    /// An access point's latest announced security.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<WifiSecurity>,
    /// Its hardware category.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    /// The receiver that reported its address as its own ([`OwnAddresses`]):
    /// a label for the address, not an identity, since anything sending
    /// that address carries it too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub own: Option<String>,
    /// What an aircraft last announced, and where it is from the vehicle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aircraft: Option<AircraftState>,
}

/// An aircraft's latest announcements (ADR-0007 sections 2 and 3): each
/// field is the latest one it sent, so a field it stopped sending keeps its
/// last value, except `on_ground` and `emergency`, which are the latest
/// event's.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AircraftState {
    /// How the decoder last heard it (`adsb_icao`, `mode_s`, `mlat` and the rest).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heard_as: Option<String>,
    /// Its callsign.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    /// Barometric altitude, feet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_baro_ft: Option<i32>,
    /// Whether it last announced being on the ground.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub on_ground: bool,
    /// Geometric altitude, feet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_geom_ft: Option<i32>,
    /// Ground speed, knots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_speed_kt: Option<f64>,
    /// Track over the ground, degrees from true north.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_deg: Option<f64>,
    /// Its latest position, degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat_deg: Option<f64>,
    /// Its latest position, degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lon_deg: Option<f64>,
    /// When it reported that position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_ns: Option<TimeNs>,
    /// Great-circle distance from the vehicle's latest fix to that position,
    /// meters, when both are known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_m: Option<f64>,
    /// Bearing from the vehicle's latest fix to that position, degrees
    /// clockwise from true north, when both are known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearing_deg: Option<f64>,
    /// Its emitter category code, `A0` to `D7`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emitter_category: Option<String>,
    /// Its Mode A code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub squawk: Option<String>,
    /// The emergency it last announced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emergency: Option<String>,
    /// Its latest signal strength, dBFS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_dbfs: Option<f64>,
}

impl AircraftState {
    /// Takes the next aircraft event's fields, in the judge's order.
    fn observe(&mut self, t_ns: TimeNs, kind: &EventKind) {
        let EventKind::Aircraft {
            heard_as,
            callsign,
            alt_baro_ft,
            on_ground,
            alt_geom_ft,
            ground_speed_kt,
            track_deg,
            lat_deg,
            lon_deg,
            emitter_category,
            squawk,
            emergency,
            signal_dbfs,
            address: _,
        } = kind
        else {
            return;
        };
        let keep = |field: &mut Option<String>, new: &Option<String>| {
            if new.is_some() {
                field.clone_from(new);
            }
        };
        keep(&mut self.heard_as, heard_as);
        keep(&mut self.callsign, callsign);
        keep(&mut self.emitter_category, emitter_category);
        keep(&mut self.squawk, squawk);
        self.alt_baro_ft = alt_baro_ft.or(self.alt_baro_ft);
        self.alt_geom_ft = alt_geom_ft.or(self.alt_geom_ft);
        self.ground_speed_kt = ground_speed_kt.or(self.ground_speed_kt);
        self.track_deg = track_deg.or(self.track_deg);
        self.signal_dbfs = signal_dbfs.or(self.signal_dbfs);
        if let (Some(lat), Some(lon)) = (lat_deg, lon_deg) {
            (self.lat_deg, self.lon_deg, self.position_ns) = (Some(*lat), Some(*lon), Some(t_ns));
        }
        self.on_ground = *on_ground;
        self.emergency.clone_from(emergency);
    }

    /// This state with its distance and bearing from `fix`, where both
    /// positions are known.
    fn seen_from(&self, fix: Option<Fix>) -> Self {
        let mut out = self.clone();
        if let (Some((_, from_lat, from_lon)), Some(lat), Some(lon)) =
            (fix, self.lat_deg, self.lon_deg)
        {
            out.distance_m = Some(crate::geo::distance_m(from_lat, from_lon, lat, lon));
            out.bearing_deg = Some(crate::geo::bearing_deg(from_lat, from_lon, lat, lon));
        }
        out
    }
}

/// The addresses receivers reported as their own, by the device IDs they
/// name: a Wi-Fi address as `wifi:`, a Bluetooth one as `bt:` and `ble:`
/// (ADR-0005 and ADR-0006 as amended 2026-11-09). They label descriptions
/// and change no verdict.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnAddresses {
    /// Device ID to the receivers reporting it, in order.
    by_device: BTreeMap<String, Vec<String>>,
}

impl OwnAddresses {
    /// Replaces `receiver`'s addresses with `addresses`, each a radio and an
    /// address already checked as `xx:xx:xx:xx:xx:xx`.
    pub fn set(&mut self, receiver: &str, addresses: &[(OwnRadio, String)]) {
        for receivers in self.by_device.values_mut() {
            receivers.retain(|r| r != receiver);
        }
        self.by_device.retain(|_, receivers| !receivers.is_empty());
        for (radio, address) in addresses {
            let address = address.to_ascii_lowercase();
            let prefixes: &[&str] = match radio {
                OwnRadio::Wifi => &["wifi"],
                OwnRadio::Bluetooth => &["bt", "ble"],
            };
            for prefix in prefixes {
                let receivers = self
                    .by_device
                    .entry(format!("{prefix}:{address}"))
                    .or_default();
                if !receivers.iter().any(|r| r == receiver) {
                    receivers.push(receiver.to_owned());
                    receivers.sort();
                }
            }
        }
    }

    /// Labels each emitter in `picture` whose device a receiver reported,
    /// with the first such receiver by name.
    pub fn label(&self, picture: &mut Picture) {
        for emitter in &mut picture.emitters {
            emitter.own = self
                .by_device
                .get(emitter.device.as_str())
                .and_then(|receivers| receivers.first().cloned());
        }
    }
}

/// Whether the vehicle is moving, from the GNSS track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Movement {
    /// Moving.
    Moving,
    /// Stopped, not yet for long.
    Stopped,
    /// Stopped for at least `parked_after_ns`.
    Parked,
    /// No fix yet a movement window before the latest.
    Unknown,
}

/// The emitter picture at one moment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Picture {
    /// Whether the vehicle is moving.
    pub movement: Movement,
    /// The emitters seen within the recent window, loud first.
    pub emitters: Vec<Description>,
    /// Emitters seen before the recent window, not listed.
    pub older: u64,
    /// How busy the air is, over the listed emitters.
    pub texture: Texture,
}

/// Emitters by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct KindCounts {
    /// Wi-Fi access points.
    pub access_point: u64,
    /// Wi-Fi stations.
    pub station: u64,
    /// BLE devices, one per address, so a device that rotates its address
    /// counts once per address.
    pub ble: u64,
    /// Bluetooth Classic devices.
    pub bt_classic: u64,
    /// Aircraft; left out of the line when there are none.
    #[serde(skip_serializing_if = "is_zero")]
    pub aircraft: u64,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if takes a reference.
fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl KindCounts {
    fn add(&mut self, kind: EmitterKind) {
        let count = match kind {
            EmitterKind::AccessPoint => &mut self.access_point,
            EmitterKind::Station => &mut self.station,
            EmitterKind::Ble => &mut self.ble,
            EmitterKind::BtClassic => &mut self.bt_classic,
            EmitterKind::Aircraft => &mut self.aircraft,
        };
        *count += 1;
    }
}

/// The ambient texture: counts over the emitters seen within the recent
/// window, and the newest of them. It needs no identity linking.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Texture {
    /// The emitters seen within the recent window, by kind.
    pub counts: KindCounts,
    /// The Wi-Fi emitters among them whose address is randomized.
    pub randomized_wifi: u64,
    /// The emitters among them first seen within the window, by kind.
    pub new: KindCounts,
    /// The newest emitters by first sighting, latest first, then by device;
    /// at most `DescribeParams::newest` of them.
    pub newest: Vec<DeviceId>,
}

fn texture(emitters: &[Description], recent_from: TimeNs, newest: usize) -> Texture {
    let mut texture = Texture::default();
    for e in emitters {
        texture.counts.add(e.kind);
        if e.address == Address::Randomized {
            texture.randomized_wifi += 1;
        }
        if e.first_seen_ns >= recent_from {
            texture.new.add(e.kind);
        }
    }
    let mut by_age: Vec<&Description> = emitters.iter().collect();
    by_age.sort_by(|a, b| {
        b.first_seen_ns
            .cmp(&a.first_seen_ns)
            .then_with(|| a.device.cmp(&b.device))
    });
    texture.newest = by_age
        .into_iter()
        .take(newest)
        .map(|e| e.device.clone())
        .collect();
    texture
}

/// The tracker formats (ADR-0006 section 4), each with its source and the date
/// it was read.
///
/// - `apple-find-my`: manufacturer data with Apple's company ID 0x004C whose
///   first byte is 0x12, "offline finding"; `OpenHaystack`'s firmware,
///   github.com/seemoo-lab/openhaystack, `Firmware/Microbit_v1/offline-finding/main.c`
///   (read 2026-11-07). Its third byte is a status byte whose bits 4 and 5
///   name the device: 1 an `AirTag` (`apple-find-my-airtag`), 2 another
///   maker's Find My accessory (`apple-find-my-accessory`), both trackers; 3
///   `AirPods` (`apple-find-my-airpods`, audio); 0 another Apple device
///   (`apple-find-my-apple-device`); without a third byte, the entry is
///   `apple-find-my`. `AirGuard`'s `DeviceManager.kt`, `calculateDeviceType`,
///   with each type's `statusByteDeviceType` under `types/`,
///   github.com/seemoo-lab/AirGuard, `app/src/main/java/de/seemoo/at_tracking_detection/database/models/device/`
///   (read 2026-11-08).
/// - `google-find-hub`: service data under UUID 0xFEAA with frame type 0x40, or
///   0x41 in unwanted-tracking-protection mode; Google's Find Hub Network
///   accessory specification, developers.google.com/nearby/fast-pair/specifications/extensions/fmdn
///   (read 2026-11-07).
/// - `dult`: service data under UUID 0xFCB2; draft-ietf-dult-accessory-protocol-01,
///   section 3.6, an Internet-Draft (work in progress, read 2026-11-07).
/// - `tile`: service data under UUID 0xFEED, 0xFEEC or 0xFD84, each assigned
///   to Tile, Inc.; the Bluetooth SIG's assigned numbers, `member_uuids.yaml`,
///   bitbucket.org/bluetooth-SIG/public/raw/main/assigned_numbers/uuids/member_uuids.yaml
///   (read 2026-11-08).
/// - `samsung-smarttag`: service data under UUID 0xFD5A whose first byte,
///   masked with 0xF8, is 0x10, a `SmartTag` on Samsung's `SmartThings` Find
///   network (the advert alone doesn't tell the `SmartTag` models apart);
///   `AirGuard`'s scan filter in `SamsungTracker.kt`, github.com/seemoo-lab/AirGuard,
///   `app/src/main/java/de/seemoo/at_tracking_detection/database/models/device/types/`
///   (read 2026-11-08).
fn tracker(
    manufacturer: &[crate::event::ManufacturerData],
    service: &[ServiceData],
) -> Option<(Tracker, Category)> {
    let found = |tracker, category, entry| Some((tracker, Category { category, entry }));
    let apple = manufacturer.iter().find(|m| {
        m.company_id == 0x004c
            && m.data
                .get(..2)
                .is_some_and(|t| t.eq_ignore_ascii_case("12"))
    });
    if let Some(m) = apple {
        let status = m
            .data
            .get(4..6)
            .and_then(|s| u8::from_str_radix(s, 16).ok());
        let (category, entry) = match status.map(|s| (s >> 4) & 0x03) {
            Some(1) => ("tracker", "apple-find-my-airtag"),
            Some(2) => ("tracker", "apple-find-my-accessory"),
            Some(3) => ("audio", "apple-find-my-airpods"),
            Some(_) => ("apple-device", "apple-find-my-apple-device"),
            None => ("tracker", "apple-find-my"),
        };
        return found(Tracker::AppleFindMy, category, entry);
    }
    for entry in service {
        match short_uuid(&entry.uuid).as_deref() {
            Some("feaa") if entry.data.get(..2).is_some_and(|t| t == "40" || t == "41") => {
                return found(Tracker::GoogleFindHub, "tracker", "google-find-hub");
            }
            Some("fcb2") => return found(Tracker::Dult, "tracker", "dult"),
            Some("feed" | "feec" | "fd84") => return found(Tracker::Tile, "tracker", "tile"),
            Some("fd5a")
                if entry
                    .data
                    .get(..2)
                    .and_then(|b| u8::from_str_radix(b, 16).ok())
                    .is_some_and(|b| b & 0xf8 == 0x10) =>
            {
                return found(Tracker::SmartThingsFind, "tracker", "samsung-smarttag");
            }
            _ => {}
        }
    }
    None
}

/// A Bluetooth Classic device's category from its Class of Device, or none
/// when the class says nothing (Miscellaneous, Uncategorized, or a major
/// class without an entry).
///
/// Source: the Bluetooth SIG's assigned numbers, `class_of_device.yaml`,
/// bitbucket.org/bluetooth-SIG/public/raw/main/assigned_numbers/core/class_of_device.yaml
/// (read 2026-11-07). Its service classes are bits 13 to 23, so the major
/// device class (0 to 31) is bits 8 to 12 and the minor class bits 2 to 7;
/// Imaging's minor field is a set of bits (4 Display, 5 Camera, 6 Scanner,
/// 7 Printer). The entries, by major class (minor class):
///
/// - Audio/Video (4): Car Audio (8) is `car-audio`; Hands-free Device (2) is
///   `hands-free`; Video Camera (12) and Camcorder (13) are `camera-class`;
///   Wearable Headset (1), Microphone (4), Loudspeaker (5), Headphones (6),
///   Portable Audio (7), `HiFi` Audio (10) and Hearing Aid (19) are `audio`;
///   Set-top box (9), VCR (11), Video Monitor (14), Video Display and
///   Loudspeaker (15) and Video Conferencing (16) are `video`; Gaming/Toy
///   (18) is `toy`; Glasses (20) is `wearable`; any other minor class is
///   `audio-video`.
/// - Imaging (6): with the Camera bit, `camera-class`; otherwise `imaging`.
/// - Computer (1) `computer`, Phone (2) `phone`, LAN/Network Access Point
///   (3) `network-access-point`, Peripheral (5) `peripheral`, Wearable (7)
///   `wearable`, Toy (8) `toy`, Health (9) `health`, whatever the minor class.
fn classic_category(class: u32) -> Option<Category> {
    let major = (class >> 8) & 0x1f;
    let minor = (class >> 2) & 0x3f;
    let (category, entry) = match (major, minor) {
        (4, 8) => ("car-audio", "cod-av-car-audio"),
        (4, 2) => ("hands-free", "cod-av-hands-free"),
        (4, 12) => ("camera-class", "cod-av-video-camera"),
        (4, 13) => ("camera-class", "cod-av-camcorder"),
        (4, 1 | 4 | 5 | 6 | 7 | 10 | 19) => ("audio", "cod-av-audio"),
        (4, 9 | 11 | 14 | 15 | 16) => ("video", "cod-av-video"),
        (4, 18) => ("toy", "cod-av-gaming-toy"),
        (4, 20) => ("wearable", "cod-av-glasses"),
        (4, _) => ("audio-video", "cod-audio-video"),
        (6, _) if class & (1 << 5) != 0 => ("camera-class", "cod-imaging-camera"),
        (6, _) => ("imaging", "cod-imaging"),
        (1, _) => ("computer", "cod-computer"),
        (2, _) => ("phone", "cod-phone"),
        (3, _) => ("network-access-point", "cod-network-access-point"),
        (5, _) => ("peripheral", "cod-peripheral"),
        (7, _) => ("wearable", "cod-wearable"),
        (8, _) => ("toy", "cod-toy"),
        (9, _) => ("health", "cod-health"),
        _ => return None,
    };
    Some(Category { category, entry })
}

/// Flock Safety's equipment, as far as what an emitter announces can say
/// (ADR-0006 section 4). The category, `flock-class`, means consistent with
/// that equipment and no more: anyone can name a network this way, so the
/// entry that matched is the evidence.
///
/// - `flock-softap`: an access point named `Flock-` and six hex digits, the
///   end of the camera's Wi-Fi address; `flock-ssid`: one named exactly
///   `Flock`; `flock-ext-battery-ap`: one named exactly `FS Ext Battery`.
///   `datasets/firmware_derived_signatures.md`, github.com/simeononsecurity/flock-you-esp32
///   (read 2026-11-08).
/// - `flock-softap` also takes the `FLOCK-` spelling; `flock-camera-net`: an
///   access point named exactly `Flock Camera net.`, whose addresses are
///   randomized. That repository's `README.md`, "SSID Pattern Matching"
///   (read 2026-11-08).
/// - `flock-oui`: a Wi-Fi address in the block b4:1e:52, registered to Flock
///   Safety; the `manuf` list of IEEE assignments, wireshark.org/download/automated/data/manuf.gz
///   (read 2026-11-08). The other blocks seen on this equipment belong to
///   component vendors and say nothing alone, so they aren't entries.
/// - `flock-battery-company-id`: manufacturer data under company ID 0x09C8,
///   which the firmware list above gives for the battery pack's adverts. The
///   Bluetooth SIG's `company_identifiers.yaml` assigns it to XUNTONG, not
///   Flock Safety, so another XUNTONG product matches too (read 2026-11-08).
///
/// Other published signatures rest on what the events don't carry: advertised
/// device names (ADR-0006 section 6) and probe requests (ADR-0001 section 5).
fn flock_category(track: &Track) -> Option<Category> {
    let named = track.ssid.as_deref().and_then(|ssid| match ssid {
        "Flock" => Some("flock-ssid"),
        "Flock Camera net." => Some("flock-camera-net"),
        "FS Ext Battery" => Some("flock-ext-battery-ap"),
        _ => is_flock_softap(ssid).then_some("flock-softap"),
    });
    let block = matches!(track.kind, EmitterKind::AccessPoint | EmitterKind::Station)
        && track.addr.starts_with("b4:1e:52:");
    let company = track.company_ids.binary_search(&0x09c8).is_ok();
    let entry = named
        .or(block.then_some("flock-oui"))
        .or(company.then_some("flock-battery-company-id"))?;
    Some(Category {
        category: "flock-class",
        entry,
    })
}

/// Axon Enterprise's equipment, by the address block its BLE adverts come
/// from (ADR-0006 section 4). The category, `axon-class`, names the maker and
/// not the kind: the block holds Axon's body cameras, conducted energy weapons,
/// holster sensors and vehicle units alike, and the address can't tell them
/// apart.
///
/// - `axon-oui`: a BLE advert whose address is in the block 00:25:df. The
///   IEEE's MA-L list assigns it to Axon Enterprise, Inc. (the bundled
///   snapshot, `data/ieee-vendors.csv`, read 2026-11-09). github.com/judcrandall/lookout.py,
///   `README.md` and `lookout.py`, matches it on BLE advertising addresses and
///   names "an Axon bodycam, taser, holster or Axon Signal Vehicle";
///   github.com/PoliceDetector/PoliceDetector, `police.py`, matches it the
///   same way (both read 2026-11-09).
///
/// Equipment that rotates its BLE address doesn't match: Axon said in 2023
/// that it would rotate them ("How hackers are using Bluetooth to track police
/// activity", engadget.com, 2023-10-09, read 2026-11-09). The events carry no
/// address type, so a random non-resolvable address that happens to start
/// 00:25:df matches too.
fn axon_category(track: &Track) -> Option<Category> {
    let block = matches!(track.kind, EmitterKind::Ble)
        && track
            .addr
            .get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("00:25:df:"));
    block.then_some(Category {
        category: "axon-class",
        entry: "axon-oui",
    })
}

/// The kind an aircraft announces in its ADS-B identification message, from
/// its emitter category code (ADR-0007 section 4). The entry names the code.
///
/// - The categories: "The 1090 Megahertz Riddle" (Junzi Sun, second
///   edition), "Aircraft Identification and Category", Table 1.1,
///   mode-s.org/1090mhz/content/ads-b/2-identification.html (read
///   2026-11-09). Category 0 of each set says nothing, set D and the codes
///   it lists as reserved aren't entries.
/// - The codes: readsb prints the type code's set as a letter, A for type
///   code 4, B for 3, C for 2, D for 1, then the category (`mode_s.c`,
///   `category = ((0x0E - metype) << 4) | mesub`, and `json_out.c`, `%02X`;
///   github.com/wiedehopf/readsb, read 2026-11-09).
fn adsb_category(code: &str) -> Option<Category> {
    let (category, entry) = match code {
        "A1" => ("light-aircraft", "adsb-a1"),
        "A2" => ("medium-aircraft", "adsb-a2"),
        "A3" => ("medium-aircraft", "adsb-a3"),
        "A4" => ("high-vortex-aircraft", "adsb-a4"),
        "A5" => ("heavy-aircraft", "adsb-a5"),
        "A6" => ("high-performance-aircraft", "adsb-a6"),
        "A7" => ("rotorcraft", "adsb-a7"),
        "B1" => ("glider", "adsb-b1"),
        "B2" => ("lighter-than-air", "adsb-b2"),
        "B3" => ("parachutist", "adsb-b3"),
        "B4" => ("ultralight", "adsb-b4"),
        "B6" => ("unmanned-aircraft", "adsb-b6"),
        "B7" => ("space-vehicle", "adsb-b7"),
        "C1" => ("surface-emergency-vehicle", "adsb-c1"),
        "C2" => ("surface-service-vehicle", "adsb-c2"),
        "C3" => ("ground-obstruction", "adsb-c3"),
        _ => return None,
    };
    Some(Category { category, entry })
}

/// Whether a network's name is the camera's setup hotspot.
fn is_flock_softap(ssid: &str) -> bool {
    ssid.strip_prefix("Flock-")
        .or_else(|| ssid.strip_prefix("FLOCK-"))
        .is_some_and(|tail| tail.len() == 6 && tail.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A 16-bit UUID's four hex digits, lowercase, from its short form or its
/// 128-bit form on the Bluetooth base UUID.
fn short_uuid(uuid: &str) -> Option<String> {
    let lower = uuid.to_ascii_lowercase();
    match lower.len() {
        4 => Some(lower),
        36 if lower.starts_with("0000") && lower.ends_with("-0000-1000-8000-00805f9b34fb") => {
            Some(lower[4..8].to_owned())
        }
        _ => None,
    }
}

/// A vendor registry, from the IEEE's MA-L, MA-M and MA-S lists (ADR-0006
/// section 5): the bundled snapshot ([`VendorTable::bundled`]) or a supplied
/// file ([`VendorTable::parse`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VendorTable {
    /// By prefix length in bits (36, 28, 24) and the prefix's value.
    by_prefix: BTreeMap<(u8, u64), String>,
    id: Option<String>,
}

/// Why a vendor registry couldn't be read.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {reason}")]
pub struct VendorError {
    /// The 1-based line.
    pub line: usize,
    /// What's wrong.
    pub reason: &'static str,
}

/// The longest organization name kept.
const MAX_ORGANIZATION: usize = 256;

/// The IEEE's three public listings as of the date in
/// `data/ieee-vendors.sources.txt`, cut to registry, assignment and
/// organization name by `packaging/vendors.py` (ADR-0006 section 5 as
/// amended). A block the IEEE lists under several organizations names them
/// all, joined with " / ".
const BUNDLED_VENDORS: &[u8] = include_bytes!("../data/ieee-vendors.csv");

impl VendorTable {
    /// The registry snapshot built into the core, so descriptions carry
    /// vendor names without a file; `--vendors` supplies a newer or other
    /// list instead.
    ///
    /// # Errors
    ///
    /// Returns the first line of the snapshot that isn't a registry row,
    /// which a test holds never happens.
    pub fn bundled() -> Result<Self, VendorError> {
        Self::parse(BUNDLED_VENDORS)
    }

    /// How many assignments the registry holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_prefix.len()
    }

    /// Whether the registry holds no assignment at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_prefix.is_empty()
    }

    /// Reads the IEEE's CSV lists (`Registry,Assignment,Organization
    /// Name,Organization Address`), one or several concatenated; header lines
    /// and blank lines are skipped.
    ///
    /// # Errors
    ///
    /// Returns the first line that isn't a registry row.
    pub fn parse(bytes: &[u8]) -> Result<Self, VendorError> {
        let text = std::str::from_utf8(bytes).map_err(|_| VendorError {
            line: 1,
            reason: "not UTF-8",
        })?;
        let mut by_prefix = BTreeMap::new();
        for (index, line) in text.lines().enumerate() {
            let err = |reason| VendorError {
                line: index + 1,
                reason,
            };
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() || line.starts_with("Registry,") {
                continue;
            }
            let fields = csv_fields(line).ok_or_else(|| err("unbalanced quotes"))?;
            let [registry, assignment, organization, ..] = fields.as_slice() else {
                return Err(err("fewer than three fields"));
            };
            let bits = match (registry.as_str(), assignment.len()) {
                ("MA-L", 6) => 24,
                ("MA-M", 7) => 28,
                ("MA-S", 9) => 36,
                _ => return Err(err("not an MA-L, MA-M or MA-S assignment")),
            };
            let prefix =
                u64::from_str_radix(assignment, 16).map_err(|_| err("assignment isn't hex"))?;
            let organization = organization.trim();
            if organization.is_empty() || organization.len() > MAX_ORGANIZATION {
                return Err(err("organization name empty or longer than 256 bytes"));
            }
            by_prefix.insert((bits, prefix), organization.to_owned());
        }
        Ok(Self {
            by_prefix,
            id: Some(format!("fnv1a64:{:016x}", fnv1a64(bytes))),
        })
    }

    /// The registry's identity: a hash of its contents; `None` without one.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// The organization for an address `xx:xx:xx:xx:xx:xx`, longest prefix first.
    #[must_use]
    pub fn lookup(&self, addr: &str) -> Option<&str> {
        let hex: String = addr.chars().filter(char::is_ascii_hexdigit).collect();
        let value = u64::from_str_radix(&hex, 16).ok()?;
        [36u8, 28, 24]
            .into_iter()
            .find_map(|bits| self.by_prefix.get(&(bits, value >> (48 - u32::from(bits)))))
            .map(String::as_str)
    }
}

/// The fields of one CSV line, with `"` quoting and `""` escapes; `None` for
/// unbalanced quotes.
fn csv_fields(line: &str) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => fields.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    if quoted {
        return None;
    }
    fields.push(field);
    Some(fields)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// What a sighting says before it's folded: the emitter's kind, its
/// strength, and its address; `None` for a fix or an event without a device.
fn sighting(kind: &EventKind) -> Option<(EmitterKind, Option<f32>, &String)> {
    match kind {
        EventKind::WifiAp {
            bssid, rssi_dbm, ..
        } => Some((EmitterKind::AccessPoint, rssi_dbm.map(f32::from), bssid)),
        EventKind::WifiClient { addr, rssi_dbm, .. } => {
            Some((EmitterKind::Station, rssi_dbm.map(f32::from), addr))
        }
        EventKind::BleAdvert { addr, rssi_dbm, .. } => {
            Some((EmitterKind::Ble, rssi_dbm.map(f32::from), addr))
        }
        EventKind::BtClassic { addr, rssi_dbm, .. } => {
            Some((EmitterKind::BtClassic, rssi_dbm.map(f32::from), addr))
        }
        // dBFS is never placed on the dBm scale (ADR-0007 section 2).
        EventKind::Aircraft { address, .. } => Some((EmitterKind::Aircraft, None, address)),
        EventKind::GnssFix { .. } | EventKind::Heading { .. } => None,
    }
}

/// One emitter's state over its sightings in the judge's order; what depends
/// on the answer's time (the recent window) is counted apart ([`Recent`]).
#[derive(Debug, Clone)]
struct Track {
    kind: EmitterKind,
    ssid: Option<String>,
    receivers: Vec<String>,
    first_seen_ns: TimeNs,
    last_seen_ns: TimeNs,
    sightings: u64,
    latest_dbm: Option<f32>,
    addr: String,
    company_ids: Vec<u16>,
    tracker: Option<(Tracker, Category)>,
    security: Option<WifiSecurity>,
    device_class: Option<u32>,
    aircraft: Option<AircraftState>,
}

impl Track {
    fn new(kind: EmitterKind, t: TimeNs, addr: &str) -> Self {
        Self {
            kind,
            ssid: None,
            receivers: Vec::new(),
            first_seen_ns: t,
            last_seen_ns: t,
            sightings: 0,
            latest_dbm: None,
            addr: addr.to_ascii_lowercase(),
            company_ids: Vec::new(),
            tracker: None,
            security: None,
            device_class: None,
            aircraft: None,
        }
    }

    /// Adds the next sighting in the judge's order.
    fn observe(&mut self, event: &Event, kind: EmitterKind, dbm: Option<f32>) {
        // One address is one radio in either Wi-Fi role; an access point is
        // the role worth naming.
        if kind == EmitterKind::AccessPoint {
            self.kind = kind;
        }
        self.last_seen_ns = event.t_ns;
        self.sightings += 1;
        if let Some(r) = &event.receiver {
            if let Err(at) = self.receivers.binary_search(r) {
                self.receivers.insert(at, r.clone());
            }
        }
        if dbm.is_some() {
            self.latest_dbm = dbm;
        }
        match &event.kind {
            EventKind::WifiAp { ssid, security, .. } => {
                self.ssid.clone_from(ssid);
                if security.is_some() {
                    self.security = *security;
                }
            }
            EventKind::BleAdvert {
                manufacturer_data,
                service_data,
                ..
            } => {
                for m in manufacturer_data {
                    if let Err(at) = self.company_ids.binary_search(&m.company_id) {
                        self.company_ids.insert(at, m.company_id);
                    }
                }
                if let Some(found) = tracker(manufacturer_data, service_data) {
                    self.tracker = Some(found);
                }
            }
            EventKind::BtClassic {
                device_class: Some(class),
                ..
            } => self.device_class = Some(*class),
            EventKind::Aircraft { .. } => self
                .aircraft
                .get_or_insert_with(AircraftState::default)
                .observe(event.t_ns, &event.kind),
            _ => {}
        }
    }
}

/// One emitter's sightings within the recent window.
#[derive(Debug, Default)]
struct Recent {
    by_receiver: BTreeMap<Option<String>, u64>,
    strongest_dbm: Option<f32>,
}

/// Whether the vehicle moved over the movement window, folded in the judge's
/// order.
#[derive(Debug, Clone, Default)]
struct MovementFold {
    window: Window,
    moving: Option<bool>,
    stopped_since: Option<TimeNs>,
    /// The latest fix that counted, for distances and bearings from the vehicle.
    latest: Option<Fix>,
}

impl MovementFold {
    /// Adds the next fix; a fix after `now_ns` doesn't count, and without a
    /// `now_ns` every fix does.
    fn observe(&mut self, fix: Fix, now_ns: Option<TimeNs>, params: &DescribeParams) {
        if now_ns.is_some_and(|now| fix.0 > now) {
            return;
        }
        self.latest = Some(fix);
        let Some((fast, (t0, _, _))) =
            self.window
                .observe(fix, params.moving_window_ns, params.moving_mps)
        else {
            return;
        };
        if fast {
            self.stopped_since = None;
        } else if self.stopped_since.is_none() {
            self.stopped_since = Some(t0);
        }
        self.moving = Some(fast);
    }

    fn movement(&self, now_ns: TimeNs, params: &DescribeParams) -> Movement {
        match (self.moving, self.stopped_since) {
            (None, _) => Movement::Unknown,
            (Some(true), _) => Movement::Moving,
            (Some(false), Some(since))
                if now_ns.saturating_sub(since) >= params.parked_after_ns =>
            {
                Movement::Parked
            }
            (Some(false), _) => Movement::Stopped,
        }
    }
}

/// The describe layer's state over events read in the judge's order; the
/// live judge folds a session into it as the session goes.
#[derive(Debug, Clone, Default)]
pub(crate) struct DescribeFold {
    tracks: BTreeMap<DeviceId, Track>,
    movement: MovementFold,
}

impl DescribeFold {
    /// Reads the next event in the judge's order. Every later answer's time
    /// must be at or after a fix read here: a fix counts toward movement
    /// whatever the answer's time.
    pub(crate) fn observe(&mut self, event: &Event, params: &DescribeParams) {
        observe(&mut self.tracks, &mut self.movement, event, None, params);
    }

    /// The emitter picture at `now_ns` over what was read and then `tail`,
    /// which follows it in the judge's order; `recent` holds at least every
    /// event of either within the recent window, in any order, and may hold
    /// older ones. The fold itself is unchanged; only the emitters `tail`
    /// touches are copied.
    pub(crate) fn picture_with<'a>(
        &self,
        tail: impl IntoIterator<Item = &'a Event>,
        recent: impl IntoIterator<Item = &'a Event>,
        now_ns: TimeNs,
        params: &DescribeParams,
        vendors: &VendorTable,
    ) -> Picture {
        let mut movement = self.movement.clone();
        let mut touched: BTreeMap<DeviceId, Track> = BTreeMap::new();
        for event in tail {
            if let EventKind::GnssFix {
                lat_deg, lon_deg, ..
            } = event.kind
            {
                movement.observe((event.t_ns, lat_deg, lon_deg), Some(now_ns), params);
                continue;
            }
            let Some((kind, dbm, addr)) = sighting(&event.kind) else {
                continue;
            };
            let Some(device) = event.kind.device() else {
                continue;
            };
            let track = match touched.entry(device) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let base = self.tracks.get(entry.key()).cloned();
                    entry.insert(base.unwrap_or_else(|| Track::new(kind, event.t_ns, addr)))
                }
            };
            track.observe(event, kind, dbm);
        }

        let recent_from = now_ns.saturating_sub(params.recent_ns);
        let mut windows: BTreeMap<DeviceId, Recent> = BTreeMap::new();
        for event in recent {
            if event.t_ns < recent_from {
                continue;
            }
            let (Some((_, dbm, _)), Some(device)) = (sighting(&event.kind), event.kind.device())
            else {
                continue;
            };
            let window = windows.entry(device).or_default();
            *window
                .by_receiver
                .entry(event.receiver.clone())
                .or_default() += 1;
            if let Some(d) = dbm {
                window.strongest_dbm = Some(window.strongest_dbm.map_or(d, |s| s.max(d)));
            }
        }

        let mut stale = 0;
        let mut emitters: Vec<Description> = self
            .tracks
            .iter()
            .filter(|(device, _)| !touched.contains_key(*device))
            .chain(touched.iter())
            .filter_map(|(device, track)| {
                if track.last_seen_ns < recent_from {
                    stale += 1;
                    return None;
                }
                Some(description(
                    device,
                    track,
                    windows.remove(device),
                    movement.latest,
                    params,
                    vendors,
                ))
            })
            .collect();
        emitters.sort_by(loud_first);
        Picture {
            movement: movement.movement(now_ns, params),
            texture: texture(&emitters, recent_from, params.newest),
            emitters,
            older: stale,
        }
    }
}

/// Reads one event into `tracks` and `movement`.
fn observe(
    tracks: &mut BTreeMap<DeviceId, Track>,
    movement: &mut MovementFold,
    event: &Event,
    now_ns: Option<TimeNs>,
    params: &DescribeParams,
) {
    if let EventKind::GnssFix {
        lat_deg, lon_deg, ..
    } = event.kind
    {
        movement.observe((event.t_ns, lat_deg, lon_deg), now_ns, params);
        return;
    }
    let (Some((kind, dbm, addr)), Some(device)) = (sighting(&event.kind), event.kind.device())
    else {
        return;
    };
    tracks
        .entry(device)
        .or_insert_with(|| Track::new(kind, event.t_ns, addr))
        .observe(event, kind, dbm);
}

/// The emitter picture at `now_ns`, over `events`.
#[must_use]
pub fn describe(
    events: &[Event],
    now_ns: TimeNs,
    params: &DescribeParams,
    vendors: &VendorTable,
) -> Picture {
    let mut order: Vec<usize> = (0..events.len()).collect();
    order.sort_by_key(|&i| order_key(&events[i], i));
    let mut fold = DescribeFold::default();
    for i in order {
        observe(
            &mut fold.tracks,
            &mut fold.movement,
            &events[i],
            Some(now_ns),
            params,
        );
    }
    fold.picture_with([], events, now_ns, params, vendors)
}

fn description(
    device: &DeviceId,
    track: &Track,
    recent: Option<Recent>,
    fix: Option<Fix>,
    params: &DescribeParams,
    vendors: &VendorTable,
) -> Description {
    let recent = recent.unwrap_or_default();
    let band = match (track.kind, recent.strongest_dbm) {
        (EmitterKind::Aircraft, _) | (_, None) => Band::Unknown,
        (_, Some(d)) if d >= f32::from(params.strong_dbm) => Band::Strong,
        (_, Some(d)) if d >= f32::from(params.weak_dbm) => Band::Medium,
        (_, Some(_)) => Band::Weak,
    };
    let address = match track.kind {
        EmitterKind::Ble => Address::Ble,
        EmitterKind::Aircraft => Address::Aircraft,
        EmitterKind::AccessPoint | EmitterKind::Station | EmitterKind::BtClassic
            if u8::from_str_radix(track.addr.get(..2).unwrap_or("00"), 16)
                .is_ok_and(|b| b & 0x02 != 0) =>
        {
            Address::Randomized
        }
        EmitterKind::AccessPoint | EmitterKind::Station | EmitterKind::BtClassic => Address::Global,
    };
    let vendor = match address {
        Address::Global => vendors.lookup(&track.addr).map(str::to_owned),
        Address::Randomized | Address::Ble | Address::Aircraft => None,
    };
    Description {
        device: device.clone(),
        kind: track.kind,
        ssid: track.ssid.clone(),
        receivers: track.receivers.clone(),
        first_seen_ns: track.first_seen_ns,
        last_seen_ns: track.last_seen_ns,
        sightings: track.sightings,
        latest_dbm: track.latest_dbm,
        strongest_dbm: recent.strongest_dbm,
        band,
        recent: recent
            .by_receiver
            .into_iter()
            .map(|(receiver, sightings)| ReceiverSightings {
                receiver,
                sightings,
            })
            .collect(),
        address,
        vendor,
        company_ids: track.company_ids.clone(),
        tracker: track.tracker.map(|(t, _)| t),
        security: track.security,
        category: match track.tracker {
            Some((_, category)) => Some(category),
            None => flock_category(track)
                .or_else(|| axon_category(track))
                .or_else(|| track.device_class.and_then(classic_category))
                .or_else(|| {
                    let state = track.aircraft.as_ref()?;
                    adsb_category(state.emitter_category.as_deref()?)
                }),
        },
        own: None,
        aircraft: track.aircraft.as_ref().map(|a| a.seen_from(fix)),
    }
}

/// Loud first: by band, then the strongest recent strength, then the device.
fn loud_first(a: &Description, b: &Description) -> Ordering {
    a.band
        .cmp(&b.band)
        .then_with(|| {
            let strength = |d: &Description| d.strongest_dbm.unwrap_or(f32::NEG_INFINITY);
            strength(b).total_cmp(&strength(a))
        })
        .then_with(|| a.device.cmp(&b.device))
}
