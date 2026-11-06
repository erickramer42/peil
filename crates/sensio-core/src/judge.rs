//! The deterministic judge: events and a versioned rule set in, verdicts out.
//!
//! Each verdict carries the evidence that produced it, so an alert can say why.
//! The judge orders events by time (GNSS fixes before sightings at the same
//! instant, then input order), so the verdicts don't depend on the order the
//! sources' events were merged in.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::event::{DeviceId, Event, EventKind, TimeNs};
use crate::geo;

/// The rule set's version. Bump it whenever a rule's logic or a default
/// changes, so recorded verdicts can be compared against the rules that made
/// them.
pub const RULE_SET_VERSION: u32 = 1;

const NS_PER_S: i64 = 1_000_000_000;

/// Flags a device seen at several distinct places over a span of time: one that
/// travels with the vehicle.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistenceRule {
    /// Distinct places a device must be seen at.
    pub min_places: usize,
    /// Time from its first sighting to its last, at least.
    pub min_span_ns: TimeNs,
    /// How far apart two sightings must be to count as different places.
    pub place_separation_m: f64,
    /// How old the latest GNSS fix may be for a sighting to take its position.
    pub max_fix_age_ns: TimeNs,
}

impl Default for PersistenceRule {
    fn default() -> Self {
        Self {
            min_places: 3,
            min_span_ns: 600 * NS_PER_S,
            place_separation_m: 500.0,
            max_fix_age_ns: 10 * NS_PER_S,
        }
    }
}

/// A place a device was seen at: its first sighting there.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Place {
    /// When the device was first seen there.
    pub first_seen_ns: TimeNs,
    /// Latitude of the fix that sighting took.
    pub lat_deg: f64,
    /// Longitude of the fix that sighting took.
    pub lon_deg: f64,
}

/// One device the rules flagged, with the evidence behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verdict {
    /// The device.
    pub device: DeviceId,
    /// The rule that fired.
    pub rule: &'static str,
    /// The rule set that made this verdict.
    pub rule_set_version: u32,
    /// Its first sighting.
    pub first_seen_ns: TimeNs,
    /// Its last sighting.
    pub last_seen_ns: TimeNs,
    /// All its sightings.
    pub sightings: usize,
    /// Sightings with no recent enough GNSS fix to place them.
    pub unlocated_sightings: usize,
    /// The distinct places it was seen at, in time order.
    pub places: Vec<Place>,
}

struct Track {
    first_seen_ns: TimeNs,
    last_seen_ns: TimeNs,
    sightings: usize,
    unlocated_sightings: usize,
    places: Vec<Place>,
}

/// Judges a session's events and returns its verdicts, ordered by device.
#[must_use]
pub fn judge(events: &[Event], rule: &PersistenceRule) -> Vec<Verdict> {
    let mut order: Vec<usize> = (0..events.len()).collect();
    order.sort_by_key(|&i| {
        let fix_first = u8::from(!matches!(events[i].kind, EventKind::GnssFix { .. }));
        (events[i].t_ns, fix_first, i)
    });

    let mut last_fix: Option<(TimeNs, f64, f64)> = None;
    let mut tracks: BTreeMap<DeviceId, Track> = BTreeMap::new();

    for event in order.into_iter().map(|i| &events[i]) {
        if let EventKind::GnssFix { lat_deg, lon_deg } = event.kind {
            last_fix = Some((event.t_ns, lat_deg, lon_deg));
            continue;
        }
        let Some(device) = event.kind.device() else {
            continue;
        };
        let t = event.t_ns;
        let track = tracks.entry(device).or_insert(Track {
            first_seen_ns: t,
            last_seen_ns: t,
            sightings: 0,
            unlocated_sightings: 0,
            places: Vec::new(),
        });
        track.last_seen_ns = t;
        track.sightings += 1;

        let position = last_fix
            .filter(|&(fix_t, _, _)| t.saturating_sub(fix_t) <= rule.max_fix_age_ns)
            .map(|(_, lat, lon)| (lat, lon));
        let Some((lat, lon)) = position else {
            track.unlocated_sightings += 1;
            continue;
        };
        let is_new_place = track
            .places
            .iter()
            .all(|p| geo::distance_m(p.lat_deg, p.lon_deg, lat, lon) >= rule.place_separation_m);
        if is_new_place {
            track.places.push(Place {
                first_seen_ns: t,
                lat_deg: lat,
                lon_deg: lon,
            });
        }
    }

    tracks
        .into_iter()
        .filter(|(_, track)| {
            track.places.len() >= rule.min_places
                && track.last_seen_ns.saturating_sub(track.first_seen_ns) >= rule.min_span_ns
        })
        .map(|(device, track)| Verdict {
            device,
            rule: "persistence.places",
            rule_set_version: RULE_SET_VERSION,
            first_seen_ns: track.first_seen_ns,
            last_seen_ns: track.last_seen_ns,
            sightings: track.sightings,
            unlocated_sightings: track.unlocated_sightings,
            places: track.places,
        })
        .collect()
}
