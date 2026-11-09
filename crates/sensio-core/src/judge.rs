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
use crate::movement::{Fix, Window};

/// The rule set's version. Bump it whenever a rule's logic or a default
/// changes, so recorded verdicts can be compared against the rules that made
/// them.
pub const RULE_SET_VERSION: u32 = 2;

const NS_PER_S: i64 = 1_000_000_000;

/// Flags a device heard during the vehicle's stops at several distinct
/// places over a span of time: one that stays with the vehicle across stops.
///
/// A stop is a stretch where the vehicle moves slower than `moving_mps` over
/// the movement window, and it is a place once it has lasted `min_stop_ns`,
/// so a traffic light rarely counts and passing along a road never does. A
/// sighting counts toward a stop while the judge reads the vehicle stopped
/// there, with a fix no older than `max_fix_age_ns`.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistenceRule {
    /// Distinct places a device must be heard at.
    pub min_places: usize,
    /// Time from its first sighting to its last, at least.
    pub min_span_ns: TimeNs,
    /// How far apart two stops must be to count as different places.
    pub place_separation_m: f64,
    /// How old the latest GNSS fix may be for a sighting to be placed.
    pub max_fix_age_ns: TimeNs,
    /// At or above this speed over the movement window, the vehicle is
    /// moving, in m/s.
    pub moving_mps: f64,
    /// The movement window: speed is measured from the newest fix at least
    /// this much older than the latest.
    pub moving_window_ns: TimeNs,
    /// How long the vehicle must stay stopped for the stop to be a place.
    pub min_stop_ns: TimeNs,
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
        if !self.moving_mps.is_finite() || self.moving_mps <= 0.0 {
            return Err("moving_mps must be a finite speed above 0");
        }
        if self.moving_window_ns <= 0 {
            return Err("moving_window_ns must be above 0");
        }
        if self.min_stop_ns < 0 {
            return Err("min_stop_ns must not be negative");
        }
        Ok(())
    }
}

impl Default for PersistenceRule {
    fn default() -> Self {
        Self {
            min_places: 2,
            min_span_ns: 600 * NS_PER_S,
            place_separation_m: 500.0,
            max_fix_age_ns: 10 * NS_PER_S,
            moving_mps: 2.0,
            moving_window_ns: 10 * NS_PER_S,
            min_stop_ns: 120 * NS_PER_S,
        }
    }
}

/// A place a device was heard at: a stop of the vehicle.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Place {
    /// When the device was first heard during the stop.
    pub first_seen_ns: TimeNs,
    /// Latitude of the fix the stop began at.
    pub lat_deg: f64,
    /// Longitude of the fix the stop began at.
    pub lon_deg: f64,
    /// When the stop began: the start of the first window that read it slow.
    pub stopped_from_ns: TimeNs,
    /// The stop's latest fix, so far.
    pub stopped_to_ns: TimeNs,
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
    /// The distinct places it was heard at, in time order.
    pub places: Vec<Place>,
}

/// One stop of the vehicle, from the fix its first slow window began at to
/// its latest fix.
#[derive(Debug, Clone, Copy)]
struct Stop {
    from_ns: TimeNs,
    to_ns: TimeNs,
    lat_deg: f64,
    lon_deg: f64,
}

/// The vehicle's track as the judge reads it: its latest fix and its stops.
#[derive(Debug, Clone, Default)]
struct Vehicle {
    last_fix: Option<Fix>,
    window: Window,
    stops: Vec<Stop>,
    /// Whether the latest window read slow, so the last stop goes on.
    stopped: bool,
}

impl Vehicle {
    fn observe(&mut self, fix: Fix, rule: &PersistenceRule) {
        self.last_fix = Some(fix);
        match self
            .window
            .observe(fix, rule.moving_window_ns, rule.moving_mps)
        {
            Some((false, (from_ns, lat_deg, lon_deg))) => match self.stops.last_mut() {
                Some(stop) if self.stopped => stop.to_ns = fix.0,
                _ => {
                    self.stops.push(Stop {
                        from_ns,
                        to_ns: fix.0,
                        lat_deg,
                        lon_deg,
                    });
                    self.stopped = true;
                }
            },
            Some((true, _)) => self.stopped = false,
            None => {}
        }
    }

    /// Whether a sighting at `t` has a fix recent enough to place it.
    fn located(&self, t: TimeNs, rule: &PersistenceRule) -> bool {
        self.last_fix
            .is_some_and(|(fix_t, _, _)| t.saturating_sub(fix_t) <= rule.max_fix_age_ns)
    }

    /// The stop a placed sighting is heard during, if the vehicle reads
    /// stopped.
    fn stop(&self) -> Option<usize> {
        self.stopped
            .then(|| self.stops.len().checked_sub(1))
            .flatten()
    }
}

#[derive(Debug, Clone)]
struct Track {
    first_seen_ns: TimeNs,
    last_seen_ns: TimeNs,
    sightings: usize,
    unlocated_sightings: usize,
    /// The stops it was heard during, each with its first sighting there.
    stops: Vec<(usize, TimeNs)>,
}

impl Track {
    fn new(t: TimeNs) -> Self {
        Self {
            first_seen_ns: t,
            last_seen_ns: t,
            sightings: 0,
            unlocated_sightings: 0,
            stops: Vec::new(),
        }
    }

    fn observe(&mut self, t: TimeNs, vehicle: &Vehicle, rule: &PersistenceRule) {
        self.last_seen_ns = t;
        self.sightings += 1;
        if !vehicle.located(t, rule) {
            self.unlocated_sightings += 1;
            return;
        }
        if let Some(stop) = vehicle.stop() {
            if self.stops.last().is_none_or(|&(last, _)| last != stop) {
                self.stops.push((stop, t));
            }
        }
    }

    /// Its places: the stops it was heard during that lasted `min_stop_ns`,
    /// each `place_separation_m` from every earlier one.
    fn places(&self, stops: &[Stop], rule: &PersistenceRule) -> Vec<Place> {
        let mut places: Vec<Place> = Vec::new();
        for &(i, first_seen_ns) in &self.stops {
            let Some(stop) = stops.get(i) else {
                continue;
            };
            if stop.to_ns.saturating_sub(stop.from_ns) < rule.min_stop_ns {
                continue;
            }
            let distinct = places.iter().all(|p| {
                geo::distance_m(p.lat_deg, p.lon_deg, stop.lat_deg, stop.lon_deg)
                    >= rule.place_separation_m
            });
            if distinct {
                places.push(Place {
                    first_seen_ns,
                    lat_deg: stop.lat_deg,
                    lon_deg: stop.lon_deg,
                    stopped_from_ns: stop.from_ns,
                    stopped_to_ns: stop.to_ns,
                });
            }
        }
        places
    }

    fn verdict(
        &self,
        device: &DeviceId,
        stops: &[Stop],
        rule: &PersistenceRule,
    ) -> Option<Verdict> {
        if self.last_seen_ns.saturating_sub(self.first_seen_ns) < rule.min_span_ns {
            return None;
        }
        let places = self.places(stops, rule);
        (places.len() >= rule.min_places).then(|| Verdict {
            device: device.clone(),
            rule: "persistence.places",
            rule_set_version: RULE_SET_VERSION,
            first_seen_ns: self.first_seen_ns,
            last_seen_ns: self.last_seen_ns,
            sightings: self.sightings,
            unlocated_sightings: self.unlocated_sightings,
            places,
        })
    }
}

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
    vehicle: Vehicle,
    tracks: BTreeMap<DeviceId, Track>,
}

impl JudgeFold {
    /// Reads the next event in the judge's order.
    pub(crate) fn observe(&mut self, event: &Event, rule: &PersistenceRule) {
        if let EventKind::GnssFix { lat_deg, lon_deg } = event.kind {
            self.vehicle.observe((event.t_ns, lat_deg, lon_deg), rule);
            return;
        }
        let Some(device) = event.kind.device() else {
            return;
        };
        let t = event.t_ns;
        self.tracks
            .entry(device)
            .or_insert_with(|| Track::new(t))
            .observe(t, &self.vehicle, rule);
    }

    /// The verdicts over what was read and then `tail`, which follows it in
    /// the judge's order, ordered by device; the fold itself is unchanged.
    /// Only the devices `tail` touches are copied.
    pub(crate) fn verdicts_with<'a>(
        &self,
        tail: impl IntoIterator<Item = &'a Event>,
        rule: &PersistenceRule,
    ) -> Vec<Verdict> {
        let mut vehicle = self.vehicle.clone();
        let mut touched: BTreeMap<DeviceId, Track> = BTreeMap::new();
        for event in tail {
            if let EventKind::GnssFix { lat_deg, lon_deg } = event.kind {
                vehicle.observe((event.t_ns, lat_deg, lon_deg), rule);
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
            track.observe(t, &vehicle, rule);
        }
        let mut verdicts: Vec<Verdict> = self
            .tracks
            .iter()
            .filter(|(device, _)| !touched.contains_key(*device))
            .chain(touched.iter())
            .filter_map(|(device, track)| track.verdict(device, &vehicle.stops, rule))
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
