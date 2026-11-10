//! Aircraft events (ADR-0007): their validation, the judge reading none of
//! them, and their place in the picture.

use std::io::Cursor;

use sensio_core::describe::{Address, Band, EmitterKind};
use sensio_core::{
    DescribeParams, Event, EventKind, PersistenceRule, VendorTable, describe, judge, parse_jsonl,
};

const DRIVE: &str = include_str!("fixtures/drive.jsonl");
const T0: i64 = 1_800_000_000_000_000_000;

fn aircraft(fields: &str) -> String {
    format!(
        r#"{{"t_ns":{T0},"source":"readsb","receiver":"laptop","kind":"aircraft","address":"bbbb01"{fields}}}"#
    )
}

fn rejection(line: &str) -> String {
    parse_jsonl(Cursor::new(line)).unwrap_err().to_string()
}

#[test]
fn a_full_aircraft_event_parses_and_its_device_is_adsb() {
    let events = parse_jsonl(Cursor::new(aircraft(
        r#","heard_as":"adsb_icao","callsign":"TEST01","alt_baro_ft":3500,"alt_geom_ft":3650,"ground_speed_kt":120.4,"track_deg":271.2,"lat_deg":60.01,"lon_deg":10.02,"emitter_category":"A7","squawk":"1200","emergency":"general","signal_dbfs":-21.4"#,
    )))
    .expect("parses");
    assert_eq!(
        events[0].kind.device().map(|d| d.to_string()).as_deref(),
        Some("adsb:bbbb01")
    );
    let non_icao =
        parse_jsonl(Cursor::new(aircraft("").replace("bbbb01", "~CCCC03"))).expect("parses");
    assert_eq!(
        non_icao[0].kind.device().map(|d| d.to_string()).as_deref(),
        Some("adsb:~cccc03"),
        "a non-ICAO address keeps its mark"
    );
}

#[test]
fn a_malformed_aircraft_address_is_rejected_without_quoting_it() {
    for address in [
        "", "bbbb0", "bbbb011", "gggg01", "~~bbbb01", "aa aa01", "+aaa01",
    ] {
        let line = aircraft("").replace(r#""bbbb01""#, &format!(r#""{address}""#));
        assert_eq!(
            rejection(&line),
            "line 1: aircraft address isn't six hex digits, after a '~' for a non-ICAO one",
            "{address}"
        );
    }
}

#[test]
fn each_aircraft_field_out_of_range_is_rejected_with_its_reason() {
    for (fields, reason) in [
        (
            r#","heard_as":"ADS-B""#,
            "how the aircraft was heard isn't a label",
        ),
        (
            r#","callsign":"""#,
            "callsign must be 1 to 8 of A-Z, 0-9 and space",
        ),
        (
            r#","callsign":"TEST0001X""#,
            "callsign must be 1 to 8 of A-Z, 0-9 and space",
        ),
        (
            r#","callsign":"test01""#,
            "callsign must be 1 to 8 of A-Z, 0-9 and space",
        ),
        (
            r#","alt_baro_ft":130001"#,
            "aircraft altitude out of range: -2000 to 130000 feet",
        ),
        (
            r#","alt_geom_ft":-2001"#,
            "aircraft altitude out of range: -2000 to 130000 feet",
        ),
        (
            r#","ground_speed_kt":-1.0"#,
            "aircraft ground speed out of range: 0 to 5000 knots",
        ),
        (
            r#","ground_speed_kt":5000.5"#,
            "aircraft ground speed out of range: 0 to 5000 knots",
        ),
        (
            r#","track_deg":360.0"#,
            "aircraft track out of range: 0 to under 360 degrees",
        ),
        (
            r#","lat_deg":60.01"#,
            "an aircraft's latitude and longitude come together",
        ),
        (
            r#","lon_deg":10.02"#,
            "an aircraft's latitude and longitude come together",
        ),
        (
            r#","lat_deg":90.5,"lon_deg":10.02"#,
            "latitude out of range",
        ),
        (
            r#","lat_deg":60.01,"lon_deg":180.5"#,
            "longitude out of range",
        ),
        (
            r#","emitter_category":"E1""#,
            "emitter category isn't A0 to D7",
        ),
        (
            r#","emitter_category":"A8""#,
            "emitter category isn't A0 to D7",
        ),
        (
            r#","emitter_category":"a7""#,
            "emitter category isn't A0 to D7",
        ),
        (r#","squawk":"1280""#, "squawk isn't four octal digits"),
        (r#","squawk":"120""#, "squawk isn't four octal digits"),
        (
            r#","emergency":"none""#,
            "emergency isn't one a decoder names",
        ),
        (
            r#","emergency":"mayday""#,
            "emergency isn't one a decoder names",
        ),
        (
            r#","signal_dbfs":0.5"#,
            "aircraft signal out of range: -100 to 0 dBFS",
        ),
        (
            r#","signal_dbfs":-100.5"#,
            "aircraft signal out of range: -100 to 0 dBFS",
        ),
    ] {
        assert_eq!(
            rejection(&aircraft(fields)),
            format!("line 1: {reason}"),
            "{fields}"
        );
    }
}

#[test]
fn the_judge_reads_no_aircraft_even_one_at_every_stop() {
    let events = parse_jsonl(Cursor::new(DRIVE)).expect("the fixture parses");
    let rule = PersistenceRule::default();
    let before = judge(&events, &rule);
    assert!(!before.is_empty(), "the drive flags a device");
    // An aircraft heard wherever and whenever the flagged device is.
    let flagged = before[0].device.clone();
    let mut with_aircraft = events.clone();
    for event in &events {
        if event.kind.device().as_ref() == Some(&flagged) {
            with_aircraft.push(Event {
                t_ns: event.t_ns,
                source: "readsb".to_owned(),
                receiver: Some("laptop".to_owned()),
                kind: aircraft_kind(),
            });
        }
    }
    assert_eq!(judge(&with_aircraft, &rule), before);
}

fn aircraft_kind() -> EventKind {
    let events = parse_jsonl(Cursor::new(aircraft(""))).expect("parses");
    events[0].kind.clone()
}

#[test]
fn an_aircraft_in_the_picture_carries_its_announcements_distance_and_category() {
    let fix = format!(
        r#"{{"t_ns":{T0},"source":"android","kind":"gnss_fix","lat_deg":60.0,"lon_deg":10.0}}"#
    );
    let later = T0 + 1_000_000_000;
    let lines = [
        fix,
        aircraft(
            r#","callsign":"TEST01","alt_baro_ft":3500,"lat_deg":60.01,"lon_deg":10.0,"emitter_category":"A7","signal_dbfs":-21.4"#,
        ),
        // A later message without a position or callsign keeps the earlier ones.
        aircraft(r#","alt_baro_ft":3400,"squawk":"1200""#)
            .replace(&T0.to_string(), &later.to_string()),
    ];
    let events = parse_jsonl(Cursor::new(lines.join("\n"))).expect("parses");
    let picture = describe(
        &events,
        later,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    assert_eq!(picture.emitters.len(), 1);
    let e = &picture.emitters[0];
    assert_eq!(e.kind, EmitterKind::Aircraft);
    assert_eq!(
        (e.band, e.address, e.latest_dbm),
        (Band::Unknown, Address::Aircraft, None)
    );
    assert_eq!(e.vendor, None, "an aircraft address isn't a vendor block");
    assert_eq!(
        e.category.map(|c| (c.category, c.entry)),
        Some(("rotorcraft", "adsb-a7"))
    );
    let a = e.aircraft.as_ref().expect("an aircraft state");
    assert_eq!(a.callsign.as_deref(), Some("TEST01"));
    assert_eq!(
        (a.alt_baro_ft, a.squawk.as_deref()),
        (Some(3400), Some("1200"))
    );
    assert_eq!((a.position_ns, a.signal_dbfs), (Some(T0), Some(-21.4)));
    let distance = a.distance_m.expect("a distance");
    assert!((distance - 1111.95).abs() < 1.0, "{distance}");
    let bearing = a.bearing_deg.expect("a bearing");
    assert!(bearing.abs() < 0.01, "{bearing}");
    assert_eq!(picture.texture.counts.aircraft, 1);
}

#[test]
fn an_aircraft_without_a_fix_or_position_has_no_distance() {
    let events = parse_jsonl(Cursor::new(aircraft(
        r#","alt_baro_ft":3500,"emitter_category":"A0""#,
    )))
    .expect("parses");
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    let e = &picture.emitters[0];
    let a = e.aircraft.as_ref().expect("an aircraft state");
    assert_eq!((a.distance_m, a.bearing_deg), (None, None));
    assert_eq!(e.category, None, "category 0 says nothing");
}

#[test]
fn a_picture_without_aircraft_has_no_aircraft_count() {
    let events = parse_jsonl(Cursor::new(DRIVE)).expect("the fixture parses");
    let last = events.iter().map(|e| e.t_ns).max().expect("events");
    let picture = describe(
        &events,
        last,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    let text = serde_json::to_string(&picture.texture).expect("JSON");
    assert!(!text.contains("aircraft"), "{text}");
}
