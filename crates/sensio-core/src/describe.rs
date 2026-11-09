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
use crate::geo::distance_m;
use crate::judge::order_key;

/// The version of the describe layer's code and its tables in code (the
/// advert formats); it follows `RULE_SET_VERSION`'s rules.
pub const DESCRIBE_VERSION: u32 = 2;

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
    /// At or above this speed between fixes, the vehicle is moving, in m/s.
    pub moving_mps: f64,
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
    /// A tire-pressure sensor.
    Tpms,
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
    /// No strength on a scale the bands use (none reported, or TPMS's
    /// relative scale).
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
    /// Not an address (a TPMS sensor ID).
    NotAnAddress,
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
    /// Fewer than two fixes.
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
    /// Tire-pressure sensors.
    pub tpms: u64,
}

impl KindCounts {
    fn add(&mut self, kind: EmitterKind) {
        let count = match kind {
            EmitterKind::AccessPoint => &mut self.access_point,
            EmitterKind::Station => &mut self.station,
            EmitterKind::Ble => &mut self.ble,
            EmitterKind::BtClassic => &mut self.bt_classic,
            EmitterKind::Tpms => &mut self.tpms,
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
/// section 5).
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

impl VendorTable {
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
fn sighting(kind: &EventKind) -> Option<(EmitterKind, Option<f32>, Option<&String>)> {
    match kind {
        EventKind::WifiAp {
            bssid, rssi_dbm, ..
        } => Some((
            EmitterKind::AccessPoint,
            rssi_dbm.map(f32::from),
            Some(bssid),
        )),
        EventKind::WifiClient { addr, rssi_dbm, .. } => {
            Some((EmitterKind::Station, rssi_dbm.map(f32::from), Some(addr)))
        }
        EventKind::BleAdvert { addr, rssi_dbm, .. } => {
            Some((EmitterKind::Ble, rssi_dbm.map(f32::from), Some(addr)))
        }
        EventKind::BtClassic { addr, rssi_dbm, .. } => {
            Some((EmitterKind::BtClassic, rssi_dbm.map(f32::from), Some(addr)))
        }
        EventKind::Tpms { rssi_dbm, .. } => Some((EmitterKind::Tpms, *rssi_dbm, None)),
        EventKind::GnssFix { .. } => None,
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
    addr: Option<String>,
    company_ids: Vec<u16>,
    tracker: Option<(Tracker, Category)>,
    security: Option<WifiSecurity>,
    device_class: Option<u32>,
}

impl Track {
    fn new(kind: EmitterKind, t: TimeNs, addr: Option<&String>) -> Self {
        Self {
            kind,
            ssid: None,
            receivers: Vec::new(),
            first_seen_ns: t,
            last_seen_ns: t,
            sightings: 0,
            latest_dbm: None,
            addr: addr.map(|a| a.to_ascii_lowercase()),
            company_ids: Vec::new(),
            tracker: None,
            security: None,
            device_class: None,
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

/// Whether the vehicle moved between consecutive fixes, folded in the
/// judge's order.
#[derive(Debug, Clone, Copy, Default)]
struct MovementFold {
    prev: Option<(TimeNs, f64, f64)>,
    moving: Option<bool>,
    stopped_since: Option<TimeNs>,
}

impl MovementFold {
    /// Adds the next fix; a pair of fixes ending after `now_ns` doesn't count,
    /// and without a `now_ns` none does.
    fn observe(
        &mut self,
        fix: (TimeNs, f64, f64),
        now_ns: Option<TimeNs>,
        params: &DescribeParams,
    ) {
        let (t1, lat1, lon1) = fix;
        if let Some((t0, lat0, lon0)) = self.prev {
            if t1 > t0 && now_ns.is_none_or(|now| t1 <= now) {
                #[allow(clippy::cast_precision_loss)]
                let seconds = (t1 - t0) as f64 / 1e9;
                let fast = distance_m(lat0, lon0, lat1, lon1) / seconds >= params.moving_mps;
                if fast {
                    self.stopped_since = None;
                } else if self.stopped_since.is_none() {
                    self.stopped_since = Some(t0);
                }
                self.moving = Some(fast);
            }
        }
        self.prev = Some(fix);
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
    /// must be at or after a fix read here: a pair of fixes counts toward
    /// movement whatever the answer's time.
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
        let mut movement = self.movement;
        let mut touched: BTreeMap<DeviceId, Track> = BTreeMap::new();
        for event in tail {
            if let EventKind::GnssFix { lat_deg, lon_deg } = event.kind {
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
    if let EventKind::GnssFix { lat_deg, lon_deg } = event.kind {
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
    params: &DescribeParams,
    vendors: &VendorTable,
) -> Description {
    let recent = recent.unwrap_or_default();
    let band = match (track.kind, recent.strongest_dbm) {
        (EmitterKind::Tpms, _) | (_, None) => Band::Unknown,
        (_, Some(d)) if d >= f32::from(params.strong_dbm) => Band::Strong,
        (_, Some(d)) if d >= f32::from(params.weak_dbm) => Band::Medium,
        (_, Some(_)) => Band::Weak,
    };
    let address = match (track.kind, &track.addr) {
        (EmitterKind::Ble, _) => Address::Ble,
        (_, None) => Address::NotAnAddress,
        (_, Some(a))
            if u8::from_str_radix(a.get(..2).unwrap_or("00"), 16).is_ok_and(|b| b & 0x02 != 0) =>
        {
            Address::Randomized
        }
        (_, Some(_)) => Address::Global,
    };
    let vendor = match (address, &track.addr) {
        (Address::Global, Some(a)) => vendors.lookup(a).map(str::to_owned),
        _ => None,
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
            None => track.device_class.and_then(classic_category),
        },
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
