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

impl PersistenceRule {
    /// Checks that the parameters describe a rule the judge can apply.
    ///
    /// A separation of zero or less would make every located sighting a new
    /// place (and the places scan quadratic); a NaN separation would make no
    /// second place possible; zero places would let a verdict carry no place
    /// as evidence.
    ///
    /// # Errors
    ///
    /// Returns the first parameter out of range, and why.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.min_places == 0 {
            return Err("min_places must be at least 1");
        }
        if self.min_span_ns < 0 {
            return Err("min_span_ns must not be negative");
        }
        if !self.place_separation_m.is_finite() || self.place_separation_m <= 0.0 {
            return Err("place_separation_m must be a finite number of meters above 0");
        }
        if self.max_fix_age_ns < 0 {
            return Err("max_fix_age_ns must not be negative");
        }
        Ok(())
    }
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

#[derive(Debug, Clone)]
struct Track {
    first_seen_ns: TimeNs,
    last_seen_ns: TimeNs,
    sightings: usize,
    unlocated_sightings: usize,
    places: Vec<Place>,
}

impl Track {
    fn new(t: TimeNs) -> Self {
        Self {
            first_seen_ns: t,
            last_seen_ns: t,
            sightings: 0,
            unlocated_sightings: 0,
            places: Vec::new(),
        }
    }

    fn observe(&mut self, t: TimeNs, last_fix: Option<Fix>, rule: &PersistenceRule) {
        self.last_seen_ns = t;
        self.sightings += 1;
        let position = last_fix
            .filter(|&(fix_t, _, _)| t.saturating_sub(fix_t) <= rule.max_fix_age_ns)
            .map(|(_, lat, lon)| (lat, lon));
        let Some((lat, lon)) = position else {
            self.unlocated_sightings += 1;
            return;
        };
        let is_new_place = self
            .places
            .iter()
            .all(|p| geo::distance_m(p.lat_deg, p.lon_deg, lat, lon) >= rule.place_separation_m);
        if is_new_place {
            self.places.push(Place {
                first_seen_ns: t,
                lat_deg: lat,
                lon_deg: lon,
            });
        }
    }

    fn verdict(&self, device: &DeviceId, rule: &PersistenceRule) -> Option<Verdict> {
        let fires = self.places.len() >= rule.min_places
            && self.last_seen_ns.saturating_sub(self.first_seen_ns) >= rule.min_span_ns;
        fires.then(|| Verdict {
            device: device.clone(),
            rule: "persistence.places",
            rule_set_version: RULE_SET_VERSION,
            first_seen_ns: self.first_seen_ns,
            last_seen_ns: self.last_seen_ns,
            sightings: self.sightings,
            unlocated_sightings: self.unlocated_sightings,
            places: self.places.clone(),
        })
    }
}

/// A fix's time, latitude and longitude.
type Fix = (TimeNs, f64, f64);

/// The order the judge reads events in: by time, a GNSS fix before a
/// sighting at the same instant, then by input order.
pub(crate) fn order_key(event: &Event, input: usize) -> (TimeNs, u8, usize) {
    let fix_first = u8::from(!matches!(event.kind, EventKind::GnssFix { .. }));
    (event.t_ns, fix_first, input)
}

/// The judge's state over events read in its order ([`order_key`]); the
/// live judge folds a session into it as the session goes.
#[derive(Debug, Clone, Default)]
pub(crate) struct JudgeFold {
    last_fix: Option<Fix>,
    tracks: BTreeMap<DeviceId, Track>,
}

impl JudgeFold {
    /// Reads the next event in the judge's order.
    pub(crate) fn observe(&mut self, event: &Event, rule: &PersistenceRule) {
        if let EventKind::GnssFix { lat_deg, lon_deg } = event.kind {
            self.last_fix = Some((event.t_ns, lat_deg, lon_deg));
            return;
        }
        let Some(device) = event.kind.device() else {
            return;
        };
        let t = event.t_ns;
        self.tracks
            .entry(device)
            .or_insert_with(|| Track::new(t))
            .observe(t, self.last_fix, rule);
    }

    /// The verdicts over what was read and then `tail`, which follows it in
    /// the judge's order, ordered by device; the fold itself is unchanged.
    /// Only the devices `tail` touches are copied.
    pub(crate) fn verdicts_with<'a>(
        &self,
        tail: impl IntoIterator<Item = &'a Event>,
        rule: &PersistenceRule,
    ) -> Vec<Verdict> {
        let mut last_fix = self.last_fix;
        let mut touched: BTreeMap<DeviceId, Track> = BTreeMap::new();
        for event in tail {
            if let EventKind::GnssFix { lat_deg, lon_deg } = event.kind {
                last_fix = Some((event.t_ns, lat_deg, lon_deg));
                continue;
            }
            let Some(device) = event.kind.device() else {
                continue;
            };
            let t = event.t_ns;
            let track = match touched.entry(device) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let base = self.tracks.get(entry.key()).cloned();
                    entry.insert(base.unwrap_or_else(|| Track::new(t)))
                }
            };
            track.observe(t, last_fix, rule);
        }
        let mut verdicts: Vec<Verdict> = self
            .tracks
            .iter()
            .filter(|(device, _)| !touched.contains_key(*device))
            .chain(touched.iter())
            .filter_map(|(device, track)| track.verdict(device, rule))
            .collect();
        verdicts.sort_by(|a, b| a.device.cmp(&b.device));
        verdicts
    }
}

/// Judges a session's events and returns its verdicts, ordered by device.
///
/// The rule should pass [`PersistenceRule::validate`]; front ends check it
/// before calling the judge.
#[must_use]
pub fn judge(events: &[Event], rule: &PersistenceRule) -> Vec<Verdict> {
    let mut order: Vec<usize> = (0..events.len()).collect();
    order.sort_by_key(|&i| order_key(&events[i], i));
    let mut fold = JudgeFold::default();
    for i in order {
        fold.observe(&events[i], rule);
    }
    fold.verdicts_with([], rule)
}
