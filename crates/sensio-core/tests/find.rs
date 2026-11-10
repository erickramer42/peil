//! Find mode's bearing from a sweep (ADR-0004 sections 3 and 4), over
//! synthetic headings and BLE readings.

use std::io::Cursor;

use sensio_core::find::{FindParams, FindStatus, find};
use sensio_core::{DeviceId, Event, parse_jsonl};

const T0: i64 = 1_800_000_000_000_000_000;
const TARGET: &str = "aa:aa:aa:00:00:01";

fn events(lines: &[String]) -> Vec<Event> {
    parse_jsonl(Cursor::new(lines.join("\n"))).expect("synthetic events parse")
}

fn heading(t: i64, receiver: &str, az: f64, pitch: f64) -> String {
    format!(
        r#"{{"t_ns":{t},"source":"android","receiver":"{receiver}","kind":"heading","azimuth_deg":{az},"pitch_deg":{pitch},"roll_deg":0.0}}"#
    )
}

fn ble(t: i64, receiver: &str, addr: &str, rssi: f64) -> String {
    format!(
        r#"{{"t_ns":{t},"source":"android","receiver":"{receiver}","kind":"ble_advert","addr":"{addr}","rssi_dbm":{rssi:.0}}}"#
    )
}

fn target() -> DeviceId {
    events(&[ble(T0, "phone", TARGET, -60.0)])[0]
        .kind
        .device()
        .expect("a BLE advert is a sighting")
}

/// The signal from a source at `bearing`: -60 dBm facing it, -75 with the
/// body between, in whole dB.
fn strength(az: f64, bearing: f64) -> f64 {
    let off = (az - bearing).to_radians().cos();
    (-67.5 + 7.5 * off).round()
}

/// A sweep of `steps` of 7.5 degrees from 15 degrees, a heading every 250 ms and a
/// reading of the target 100 ms after each fourth one, at each sector's
/// center: 30 degrees a second.
fn sweep(steps: u32, bearing: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for k in 0..steps {
        let t = T0 + i64::from(k) * 250_000_000;
        let az = (7.5 * f64::from(k) + 15.0) % 360.0;
        lines.push(heading(t, "phone", az, -40.0));
        if k % 4 == 0 {
            lines.push(ble(t + 100_000_000, "phone", TARGET, strength(az, bearing)));
        }
    }
    lines
}

#[test]
fn a_full_turn_points_at_the_strongest_sector() {
    let answer = find(
        &events(&sweep(48, 105.0)),
        &target(),
        T0,
        &FindParams::default(),
    );
    assert_eq!(answer.status, FindStatus::Found);
    assert_eq!(answer.bearing_deg, Some(105.0));
    assert_eq!(answer.paired, 12);
    assert_eq!(answer.unpaired, 0);
    assert!(
        answer.contrast_db.is_some_and(|c| c >= 3.0),
        "{:?}",
        answer.contrast_db
    );
    assert_eq!(answer.sectors.len(), 12);
    assert!(answer.sectors.iter().all(|s| s.readings == 1));
}

#[test]
fn a_source_behind_north_is_found_across_the_wrap() {
    let answer = find(
        &events(&sweep(48, 345.0)),
        &target(),
        T0,
        &FindParams::default(),
    );
    assert_eq!(answer.bearing_deg, Some(345.0));
}

#[test]
fn a_quarter_turn_asks_for_more() {
    let answer = find(
        &events(&sweep(12, 105.0)),
        &target(),
        T0,
        &FindParams::default(),
    );
    assert_eq!(answer.status, FindStatus::Turn);
    assert_eq!(answer.bearing_deg, None);
    assert_eq!(answer.paired, 3);
}

#[test]
fn a_flat_signal_is_unclear() {
    let mut lines = sweep(48, 105.0);
    for line in &mut lines {
        if line.contains("ble_advert") {
            let start = line.find("\"rssi_dbm\":").expect("the line has a strength");
            line.replace_range(start.., "\"rssi_dbm\":-70}");
        }
    }
    let answer = find(&events(&lines), &target(), T0, &FindParams::default());
    assert_eq!(answer.status, FindStatus::Unclear);
    assert_eq!(answer.contrast_db, Some(0.0));
    assert_eq!(answer.bearing_deg, None);
}

#[test]
fn nothing_heard_is_no_readings() {
    let lines = vec![heading(T0, "phone", 10.0, -40.0)];
    let answer = find(&events(&lines), &target(), T0, &FindParams::default());
    assert_eq!(answer.status, FindStatus::NoReadings);
    assert_eq!(answer.latest_dbm, None);
    assert_eq!(answer.contrast_db, None);
}

#[test]
fn a_reading_with_a_stale_or_steep_heading_or_another_receivers_is_unpaired() {
    let lines = vec![
        heading(T0, "phone", 10.0, -40.0),
        // Over a second after the phone's heading.
        ble(T0 + 1_500_000_000, "phone", TARGET, -61.0),
        heading(T0 + 2_000_000_000, "phone", 20.0, -85.0),
        // The phone held upright: its azimuth isn't used.
        ble(T0 + 2_100_000_000, "phone", TARGET, -62.0),
        heading(T0 + 3_000_000_000, "phone", 30.0, -40.0),
        // The laptop has no heading of its own.
        ble(T0 + 3_100_000_000, "laptop", TARGET, -63.0),
    ];
    let answer = find(&events(&lines), &target(), T0, &FindParams::default());
    assert_eq!((answer.paired, answer.unpaired), (0, 3));
    assert_eq!(answer.status, FindStatus::NoReadings);
    assert_eq!(answer.latest_dbm, Some(-63));
    assert_eq!(answer.latest_ns, Some(T0 + 3_100_000_000));
}

#[test]
fn readings_before_the_sweep_and_other_devices_are_left_out() {
    let mut lines = sweep(48, 105.0);
    lines.push(ble(T0 - 1, "phone", TARGET, -40.0));
    lines.push(heading(T0 + 500_000_000, "phone", 1.0, -40.0));
    lines.push(ble(T0 + 600_000_000, "phone", "bb:bb:bb:00:00:02", -30.0));
    let answer = find(&events(&lines), &target(), T0, &FindParams::default());
    assert_eq!((answer.paired, answer.unpaired), (12, 0));
    assert_eq!(answer.bearing_deg, Some(105.0));
}

#[test]
fn a_tie_goes_to_the_first_sector_from_north() {
    let lines = vec![
        heading(T0, "phone", 100.0, -40.0),
        ble(T0 + 1, "phone", TARGET, -60.0),
        heading(T0 + 2, "phone", 280.0, -40.0),
        ble(T0 + 3, "phone", TARGET, -60.0),
    ];
    let params = FindParams {
        min_sectors: 2,
        min_contrast_db: 0.0,
        ..FindParams::default()
    };
    let answer = find(&events(&lines), &target(), T0, &params);
    assert_eq!(answer.status, FindStatus::Found);
    assert_eq!(answer.bearing_deg, Some(105.0));
}

#[test]
fn a_heading_at_the_same_instant_as_a_reading_pairs_with_it() {
    let lines = vec![
        ble(T0, "phone", TARGET, -60.0),
        heading(T0, "phone", 100.0, -40.0),
    ];
    let answer = find(&events(&lines), &target(), T0, &FindParams::default());
    assert_eq!(answer.paired, 1);
    assert_eq!(answer.sectors[3].readings, 1);
    assert_eq!(answer.sectors[3].median_dbm, Some(-60.0));
}

#[test]
fn the_answer_does_not_depend_on_input_order() {
    let mut lines = sweep(96, 200.0);
    let forward = find(&events(&lines), &target(), T0, &FindParams::default());
    lines.reverse();
    let backward = find(&events(&lines), &target(), T0, &FindParams::default());
    assert_eq!(forward, backward);
    assert_eq!(forward.bearing_deg, Some(195.0));
    // Two turns put two readings in each sector, so each median is a mean of two.
    assert!(forward.sectors.iter().all(|s| s.readings == 2));
}
