//! readsb's aircraft list, read as a file (ADR-0007 section 1), and the
//! `readsb` normalizer (section 2), against a synthetic list.

use std::path::{Path, PathBuf};

use sensio_core::{Event, EventKind, Normalize};
use sensio_sources::Sources;
use sensio_sources::readsb_file::{MAX_FILE_BYTES, ReadError, Reader, STALE_NS};
use serde_json::Value;

/// The one event `raw` normalizes to, checked as the core checks it.
fn normalized(raw: &str) -> EventKind {
    let kinds = Sources.normalize("readsb", raw).expect("normalizes");
    assert_eq!(kinds.len(), 1, "{raw}");
    let event = Event {
        t_ns: CLOCK_NS,
        source: "readsb".to_owned(),
        receiver: Some("laptop".to_owned()),
        kind: kinds[0].clone(),
    };
    assert_eq!(event.validate(), Ok(()), "{raw}");
    kinds[0].clone()
}

#[test]
fn the_lists_aircraft_normalize_with_their_fields() {
    let mut reader = Reader::new(Path::new("unused"));
    let lines = reader.take_body(LIST.as_bytes(), CLOCK_NS).expect("a list");
    let EventKind::Aircraft {
        address,
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
    } = normalized(&lines[0])
    else {
        panic!("not an aircraft");
    };
    assert_eq!(address, "aaaa01");
    assert_eq!(heard_as.as_deref(), Some("adsb_icao"));
    assert_eq!(callsign.as_deref(), Some("TEST01"), "the padding goes");
    assert_eq!(
        (alt_baro_ft, alt_geom_ft, on_ground),
        (Some(3500), Some(3650), false)
    );
    assert_eq!((ground_speed_kt, track_deg), (Some(120.4), Some(271.2)));
    assert_eq!((lat_deg, lon_deg), (Some(60.01), Some(10.02)));
    assert_eq!(emitter_category.as_deref(), Some("A7"));
    assert_eq!(squawk.as_deref(), Some("1200"));
    assert_eq!(emergency, None, "none is no emergency");
    assert_eq!(signal_dbfs, Some(-21.4));

    // Mode S only: an altitude and no position.
    let EventKind::Aircraft {
        lat_deg,
        alt_baro_ft,
        heard_as,
        ..
    } = normalized(&lines[1])
    else {
        panic!("not an aircraft");
    };
    assert_eq!(
        (lat_deg, alt_baro_ft, heard_as.as_deref()),
        (None, Some(12000), Some("mode_s"))
    );

    // A non-ICAO track on the ground whose position is 5 s old.
    let EventKind::Aircraft {
        address,
        on_ground,
        alt_baro_ft,
        lat_deg,
        ..
    } = normalized(&lines[2])
    else {
        panic!("not an aircraft");
    };
    assert_eq!(
        (address.as_str(), on_ground, alt_baro_ft, lat_deg),
        ("~cccc03", true, None, Some(60.02))
    );
}

#[test]
fn a_field_of_the_wrong_shape_or_out_of_range_reads_as_absent() {
    let raw = r#"{"hex":"AAAA01","type":"ADS B","flight":"test 01!","alt_baro":200000,"alt_geom":"high",
        "gs":-3,"track":360,"lat":91,"lon":10.02,"category":"E1","squawk":"7800","emergency":"mayday","rssi":3}"#;
    let EventKind::Aircraft {
        address,
        heard_as,
        callsign,
        alt_baro_ft,
        alt_geom_ft,
        ground_speed_kt,
        track_deg,
        lat_deg,
        lon_deg,
        emitter_category,
        squawk,
        emergency,
        signal_dbfs,
        ..
    } = normalized(raw)
    else {
        panic!("not an aircraft");
    };
    assert_eq!(address, "aaaa01", "lowercased");
    assert_eq!(
        (
            heard_as,
            callsign,
            alt_baro_ft,
            alt_geom_ft,
            emitter_category,
            squawk,
            emergency
        ),
        (None, None, None, None, None, None, None)
    );
    assert_eq!(
        (ground_speed_kt, track_deg, lat_deg, lon_deg, signal_dbfs),
        (None, None, None, None, None)
    );
}

#[test]
fn a_position_older_than_ten_seconds_is_left_out() {
    let at = |seen: &str| {
        let raw = format!(r#"{{"hex":"aaaa01","lat":60.01,"lon":10.02,"seen_pos":{seen}}}"#);
        let EventKind::Aircraft { lat_deg, .. } = normalized(&raw) else {
            panic!("not an aircraft");
        };
        lat_deg
    };
    assert_eq!(at("10.0"), Some(60.01));
    assert_eq!(at("10.5"), None);
    assert_eq!(at("-1"), None);
    assert_eq!(at(r#""soon""#), None);
}

#[test]
fn what_has_no_usable_address_is_rejected_without_its_values() {
    for raw in [
        "",
        "[]",
        r#"{"flight":"TEST01"}"#,
        r#"{"hex":"aaaa0"}"#,
        r#"{"hex":"aaaa0g"}"#,
        r#"{"hex":7}"#,
    ] {
        let reason = Sources.normalize("readsb", raw).expect_err(raw);
        assert!(
            !reason.contains("TEST01") && !reason.contains("aaaa0"),
            "{reason}"
        );
    }
}

const LIST: &str = include_str!("fixtures/readsb-aircraft.json");
const CLOCK_NS: i64 = 1_800_000_000_000_000_000;

fn hexes(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            let o: Value = serde_json::from_str(l).expect("JSON");
            o["hex"].as_str().expect("a hex").to_owned()
        })
        .collect()
}

#[test]
fn each_aircraft_goes_out_as_its_own_object_with_the_lists_time() {
    let mut reader = Reader::new(Path::new("unused"));
    let lines = reader.take_body(LIST.as_bytes(), CLOCK_NS).expect("a list");
    assert_eq!(hexes(&lines), ["aaaa01", "bbbb02", "~cccc03"]);
    let first: Value = serde_json::from_str(&lines[0]).expect("JSON");
    assert_eq!(first["now"], 1_800_000_000.5);
    assert_eq!(first["flight"], "TEST01  ", "readsb's object as written");
    assert_eq!(first["rssi"], -21.4);
}

#[test]
fn only_aircraft_with_new_messages_go_out_again() {
    let mut reader = Reader::new(Path::new("unused"));
    reader.take_body(LIST.as_bytes(), CLOCK_NS).expect("a list");
    let next = LIST
        .replace("1800000000.5", "1800000001.5")
        .replace(r#""messages":12"#, r#""messages":13"#);
    let lines = reader
        .take_body(next.as_bytes(), CLOCK_NS + 1_000_000_000)
        .expect("a list");
    assert_eq!(hexes(&lines), ["bbbb02"]);

    // A count that went down is a restarted readsb, so the aircraft goes out.
    let restarted = next
        .replace("1800000001.5", "1800000002.5")
        .replace(r#""messages":40"#, r#""messages":2"#);
    let lines = reader
        .take_body(restarted.as_bytes(), CLOCK_NS + 2_000_000_000)
        .expect("a list");
    assert_eq!(hexes(&lines), ["aaaa01"]);
}

#[test]
fn a_list_whose_time_stands_still_reads_as_stopped_after_ten_seconds() {
    let mut reader = Reader::new(Path::new("unused"));
    reader.take_body(LIST.as_bytes(), CLOCK_NS).expect("a list");
    assert_eq!(
        reader.take_body(LIST.as_bytes(), CLOCK_NS + 1_000_000_000),
        Ok(Vec::new()),
        "read twice in its second"
    );
    assert_eq!(
        reader.take_body(LIST.as_bytes(), CLOCK_NS + STALE_NS + 1),
        Err(ReadError::Stale)
    );
}

#[test]
fn what_isnt_an_aircraft_list_is_rejected_without_its_values() {
    let mut reader = Reader::new(Path::new("unused"));
    for bad in [
        "",
        "[]",
        r#"{"aircraft":[]}"#,
        r#"{"now":"1800000000","aircraft":[]}"#,
        r#"{"now":1800000000.5}"#,
        r#"{"now":1800000000.5,"aircraft":{"hex":"aaaa01"}}"#,
    ] {
        match reader.take_body(bad.as_bytes(), CLOCK_NS) {
            Err(ReadError::NotAircraft(reason)) => {
                assert!(!reason.contains("aaaa01"), "{reason}");
            }
            other => panic!("{bad}: {other:?}"),
        }
    }
}

#[test]
fn entries_without_a_usable_address_are_skipped() {
    let mut reader = Reader::new(Path::new("unused"));
    let list = r#"{"now":1800000000.5,"aircraft":[
        1, {"type":"mode_s"}, {"hex":""}, {"hex":"aaaaaaaaaa01"}, {"hex":7}, {"hex":"aaaa01"},
        {"hex":"aaaa01","messages":9}
    ]}"#;
    let lines = reader.take_body(list.as_bytes(), CLOCK_NS).expect("a list");
    assert_eq!(
        hexes(&lines),
        ["aaaa01"],
        "an address listed twice is taken once"
    );
    let later = list.replace("1800000000.5", "1800000001.5");
    assert_eq!(
        reader.take_body(later.as_bytes(), CLOCK_NS + 1_000_000_000),
        Ok(Vec::new())
    );
}

#[test]
fn a_missing_file_is_absent_and_a_long_one_is_refused() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("readsb");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let missing = dir.join("missing.json");
    let _ = std::fs::remove_file(&missing);
    assert!(matches!(
        Reader::new(&missing).read(CLOCK_NS),
        Err(ReadError::Absent(_))
    ));

    let long = dir.join("long.json");
    let size = usize::try_from(MAX_FILE_BYTES).expect("fits") + 1;
    std::fs::write(&long, vec![b' '; size]).expect("write");
    assert!(matches!(
        Reader::new(&long).read(CLOCK_NS),
        Err(ReadError::NotAircraft(_))
    ));

    let good = dir.join("aircraft.json");
    std::fs::write(&good, LIST).expect("write");
    assert_eq!(Reader::new(&good).read(CLOCK_NS).map(|l| l.len()), Ok(3));
}
