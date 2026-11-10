//! The end-of-session summary over the synthetic drive feed
//! (`fixtures/drive.feed`, described in `tests/live.rs`): how it ended, its
//! stops and route, and each verdict with its device.

use std::io::Cursor;

use sensio_core::feed::EndReason;
use sensio_core::{EventKind, Live, Normalize, PersistenceRule, SUMMARY_SCHEMA, summarize};

const FEED: &str = include_str!("fixtures/drive.feed");

struct Sensio;

impl Normalize for Sensio {
    fn normalize(&self, source: &str, raw: &str) -> Result<Vec<EventKind>, String> {
        if source != "sensio" {
            return Err("no normalizer for this source".to_owned());
        }
        serde_json::from_str(raw)
            .map(|kind| vec![kind])
            .map_err(|_| "not an event".to_owned())
    }
}

fn summary(feed: &str, all: bool) -> sensio_core::Summary {
    summarize(
        Live::new(Sensio, PersistenceRule::default(), all),
        Cursor::new(feed),
    )
    .expect("the feed is summarized")
}

#[test]
fn the_drive_summarizes_to_its_stops_route_and_verdicts() {
    let s = summary(FEED, true);
    assert_eq!(s.schema, SUMMARY_SCHEMA);
    assert_eq!(s.session.end, EndReason::Stopped);
    // Four stops of 130 s; the 40 s light isn't one.
    let stops: Vec<f64> = s.session.stops.iter().map(|p| p.lat_deg).collect();
    assert_eq!(stops, [60.0, 60.05, 60.1, 60.15]);
    // A to D is 0.15 degree north, about 16.7 km, through the light.
    assert!(
        (16_500.0..=16_900.0).contains(&s.route.distance_m),
        "{}",
        s.route.distance_m
    );
    assert_eq!(s.route.fixes, 15);
    let flagged: Vec<(&str, usize)> = s
        .verdicts
        .iter()
        .map(|f| (f.verdict.device.as_str(), f.verdict.places.len()))
        .collect();
    assert_eq!(
        flagged,
        [("ble:aa:aa:aa:00:00:01", 4), ("bt:dd:dd:dd:00:00:06", 3)]
    );
    // Each verdict's device comes described, though it wasn't heard lately.
    assert!(s.verdicts.iter().all(|f| f.described.is_some()));
    // Every emitter of the session counts, not only the last minute's.
    assert_eq!((s.emitters.ble, s.emitters.bt_classic), (5, 1));
}

#[test]
fn a_cut_feed_says_so_and_a_withheld_line_stays_out() {
    let cut: String = FEED
        .lines()
        .filter(|l| !l.contains(r#""type":"end""#))
        .flat_map(|l| [l, "\n"])
        .collect();
    let s = summary(&cut, false);
    assert_eq!(s.session.end, EndReason::Cut);
    let tag = s
        .verdicts
        .iter()
        .find(|f| f.verdict.device.as_str() == "ble:aa:aa:aa:00:00:01")
        .expect("the tag is flagged");
    assert_eq!(tag.verdict.places.len(), 3);
}

#[test]
fn the_summary_lists_no_sighting() {
    let json = serde_json::to_string(&summary(FEED, true)).expect("serializes");
    for raw in ["rssi_dbm", "latest_dbm", "strongest_dbm", "\"recent\""] {
        assert!(!json.contains(raw), "{raw} in the summary");
    }
}
