//! The end-of-session summary: what a session's verdicts were and the
//! evidence behind them, with its coverage, its stops and its route, and
//! whether it was stopped or cut. Built from a whole feed by
//! [`summarize`](crate::live::summarize); it lists no raw sighting.
//!
//! Like the verdict stream, it is a pure function of the feed, so the same
//! feed always gives the same summary.

use serde::Serialize;

use crate::describe::{Address, Category, Description, EmitterKind, KindCounts, Tracker};
use crate::event::{TimeNs, WifiSecurity};
use crate::feed::{Coverage, EndReason};
use crate::geo::distance_m;
use crate::judge::Verdict;

/// The summary's schema, named in it.
pub const SUMMARY_SCHEMA: &str = "sensio.summary/1";

/// Route points are kept at least this far apart, so a parked receiver's
/// GNSS jitter neither draws a scribble nor adds distance.
const ROUTE_SPACING_M: f64 = 25.0;
/// The most route points kept; past it the distance still counts.
const MAX_ROUTE_POINTS: usize = 100_000;

/// A session's summary.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    /// [`SUMMARY_SCHEMA`].
    pub schema: &'static str,
    /// The core's version.
    pub core: &'static str,
    /// The rule set the verdicts come from.
    pub rule_set_version: u32,
    /// The describe layer's version.
    pub describe_version: u32,
    /// The vendor registry's identity, if one was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendors: Option<String>,
    /// The session: how it ended, its times and its stops.
    pub session: Session,
    /// What it covered, as the stream's final coverage line says.
    pub coverage: Coverage,
    /// The emitters it heard, by kind.
    pub emitters: KindCounts,
    /// Its verdicts, each with the device's description.
    pub verdicts: Vec<Flagged>,
    /// Its GNSS track.
    pub route: Route,
}

/// How a session ran.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Session {
    /// Whether it was stopped or cut.
    pub end: EndReason,
    /// Its first event, on the session clock.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_ns: Option<TimeNs>,
    /// Its last event, on the session clock.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_ns: Option<TimeNs>,
    /// How long a stop lasts before it is a place (`PersistenceRule::min_stop_ns`).
    pub min_stop_ns: TimeNs,
    /// The stops long enough to be places, in time order.
    pub stops: Vec<StopSpan>,
}

/// One stop of the vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StopSpan {
    /// When it began.
    pub from_ns: TimeNs,
    /// Its latest fix.
    pub to_ns: TimeNs,
    /// Latitude of the fix it began at.
    pub lat_deg: f64,
    /// Longitude of the fix it began at.
    pub lon_deg: f64,
}

/// A verdict and what the describe layer knows about its device.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Flagged {
    /// The verdict, with its evidence.
    #[serde(flatten)]
    pub verdict: Verdict,
    /// The device, by lookup; `None` for one the describe layer doesn't
    /// list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub described: Option<Device>,
}

/// A device's description, without its strength or its recent sightings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Device {
    /// What kind of emitter it is.
    pub kind: EmitterKind,
    /// What its address says.
    pub address: Address,
    /// Its vendor block's organization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// Its hardware category.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    /// The tracker network its advert names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracker: Option<Tracker>,
    /// An access point's security.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<WifiSecurity>,
    /// An access point's network name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    /// The receiver that reported its address as its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub own: Option<String>,
    /// The receivers that heard it.
    pub receivers: Vec<String>,
}

impl From<&Description> for Device {
    fn from(d: &Description) -> Self {
        Self {
            kind: d.kind,
            address: d.address,
            vendor: d.vendor.clone(),
            category: d.category,
            tracker: d.tracker,
            security: d.security,
            ssid: d.ssid.clone(),
            own: d.own.clone(),
            receivers: d.receivers.clone(),
        }
    }
}

/// A session's GNSS track.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Route {
    /// Fixes read.
    pub fixes: u64,
    /// Distance along the kept points, meters.
    pub distance_m: f64,
    /// The track, a point at least 25 m from the one before, in time order.
    pub points: Vec<RoutePoint>,
    /// Whether points past the most kept were left out.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// One point of the route.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RoutePoint {
    /// Its fix's time.
    pub t_ns: TimeNs,
    /// Latitude.
    pub lat_deg: f64,
    /// Longitude.
    pub lon_deg: f64,
}

/// The route as fixes come, in the judge's order.
#[derive(Debug, Clone, Default)]
pub(crate) struct RouteFold {
    route: Route,
    /// The last point kept, or past the cap the last that would have been.
    last: Option<RoutePoint>,
}

impl RouteFold {
    pub(crate) fn observe(&mut self, t_ns: TimeNs, lat_deg: f64, lon_deg: f64) {
        self.route.fixes += 1;
        let point = RoutePoint {
            t_ns,
            lat_deg,
            lon_deg,
        };
        if let Some(last) = self.last {
            let d = distance_m(last.lat_deg, last.lon_deg, lat_deg, lon_deg);
            if d < ROUTE_SPACING_M {
                return;
            }
            self.route.distance_m += d;
        }
        self.last = Some(point);
        if self.route.points.len() < MAX_ROUTE_POINTS {
            self.route.points.push(point);
        } else {
            self.route.truncated = true;
        }
    }

    pub(crate) fn route(&self) -> &Route {
        &self.route
    }
}

#[cfg(test)]
mod tests {
    use super::RouteFold;

    #[test]
    fn jitter_adds_no_point_and_no_distance() {
        let mut fold = RouteFold::default();
        let m = 1.0 / 111_195.0;
        for i in 0..600 {
            let jitter = if i % 2 == 0 { 3.0 } else { -3.0 };
            fold.observe(i, 60.0 + jitter * m, 10.0);
        }
        assert_eq!(fold.route().points.len(), 1);
        assert!(fold.route().distance_m.abs() < f64::EPSILON);
        // Then 1 km north in 10 m steps: a point every 30 m, 990 m of them.
        for i in 0..100 {
            fold.observe(
                600 + i,
                60.0 + f64::from(i32::try_from(i).expect("small")) * 10.0 * m,
                10.0,
            );
        }
        let route = fold.route();
        assert_eq!(route.fixes, 700);
        assert!(
            (960.0..=1_000.0).contains(&route.distance_m),
            "{}",
            route.distance_m
        );
    }
}
