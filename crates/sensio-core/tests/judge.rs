//! The judge against a synthetic drive: golden verdicts, order independence,
//! and parsing that rejects bad input.
//!
//! The drive (`fixtures/drive.jsonl`) moves north about 1.1 km between fixes.
//! One BLE device and one TPMS sensor travel with it; others are seen once,
//! twice, or with no recent fix.
//!
//! The Wi-Fi drive (`fixtures/drive_wifi.jsonl`) takes the same route with two
//! receivers, the phone and the laptop: a hotspot both of them see stays with
//! it, a station on that hotspot stays for three places, and two others are
//! seen once.

use std::io::Cursor;

use proptest::prelude::*;
use sensio_core::{Event, PersistenceRule, judge, parse_jsonl};

const DRIVE: &str = include_str!("fixtures/drive.jsonl");
const WIFI_DRIVE: &str = include_str!("fixtures/drive_wifi.jsonl");

fn drive() -> Vec<Event> {
    parse_jsonl(Cursor::new(DRIVE)).expect("the fixture parses")
}

fn wifi_drive() -> Vec<Event> {
    parse_jsonl(Cursor::new(WIFI_DRIVE)).expect("the fixture parses")
}

/// The error for one line of input, as text.
fn rejection(line: &str) -> String {
    parse_jsonl(Cursor::new(line)).unwrap_err().to_string()
}

fn accepted(line: &str) -> bool {
    parse_jsonl(Cursor::new(line)).is_ok()
}

fn wifi_ap(fields: &str) -> String {
    format!(r#"{{"t_ns":1,"source":"android","kind":"wifi_ap",{fields}}}"#)
}

fn wifi_client(fields: &str) -> String {
    format!(r#"{{"t_ns":1,"source":"kismet","kind":"wifi_client",{fields}}}"#)
}

#[test]
fn drive_verdicts_match_golden() {
    let verdicts = judge(&drive(), &PersistenceRule::default());
    insta::assert_json_snapshot!(verdicts);
}

#[test]
fn stricter_rule_flags_fewer_devices() {
    let rule = PersistenceRule {
        min_places: 4,
        ..PersistenceRule::default()
    };
    let verdicts = judge(&drive(), &rule);
    let devices: Vec<&str> = verdicts.iter().map(|v| v.device.as_str()).collect();
    assert_eq!(devices, ["ble:aa:aa:aa:00:00:01"]);
}

#[test]
fn wifi_drive_verdicts_match_golden() {
    let verdicts = judge(&wifi_drive(), &PersistenceRule::default());
    insta::assert_json_snapshot!(verdicts);
}

#[test]
fn receivers_do_not_split_a_device() {
    // The hotspot is seen by the phone at three places and by the laptop at one.
    let verdicts = judge(&wifi_drive(), &PersistenceRule::default());
    let hotspot = verdicts
        .iter()
        .find(|v| v.device.as_str() == "wifi:ee:ee:ee:00:00:01")
        .expect("the hotspot is flagged");
    assert_eq!((hotspot.sightings, hotspot.places.len()), (4, 4));
}

#[test]
fn events_without_a_receiver_still_parse() {
    assert!(drive().iter().all(|e| e.receiver.is_none()));
    assert!(wifi_drive().iter().all(|e| e.receiver.is_some()));
}

proptest! {
    #[test]
    fn verdicts_do_not_depend_on_input_order(shuffled in Just(drive()).prop_shuffle()) {
        let rule = PersistenceRule::default();
        prop_assert_eq!(judge(&shuffled, &rule), judge(&drive(), &rule));
    }

    #[test]
    fn wifi_verdicts_do_not_depend_on_input_order(shuffled in Just(wifi_drive()).prop_shuffle()) {
        let rule = PersistenceRule::default();
        prop_assert_eq!(judge(&shuffled, &rule), judge(&wifi_drive(), &rule));
    }
}

#[test]
fn a_malformed_bssid_is_rejected_without_quoting_it() {
    for bssid in [
        "zz:ee:ee:00:00:01",
        "ee:ee:ee:00:00:1",
        "ee-ee-ee-00-00-01",
        "ee:ee:ee:00:00:011",
        "",
    ] {
        let err = rejection(&wifi_ap(&format!(r#""bssid":"{bssid}""#)));
        assert_eq!(
            err, "line 1: Wi-Fi BSSID isn't an address of the form xx:xx:xx:xx:xx:xx",
            "{bssid}"
        );
    }
    let station = wifi_client(r#""addr":"ff:ff:ff:00:00:02","bssid":"not-an-address""#);
    assert_eq!(
        rejection(&station),
        "line 1: Wi-Fi BSSID isn't an address of the form xx:xx:xx:xx:xx:xx"
    );
}

#[test]
fn a_malformed_ble_address_is_rejected_without_quoting_it() {
    for addr in [
        "",
        "zz:aa:aa:00:00:01",
        "aa:aa:aa:00:00",
        "aaaaaa000001",
        "aa:aa:aa:00:00:01:02",
    ] {
        let line = format!(r#"{{"t_ns":1,"source":"kismet","kind":"ble_advert","addr":"{addr}"}}"#);
        assert_eq!(
            rejection(&line),
            "line 1: BLE address isn't of the form xx:xx:xx:xx:xx:xx",
            "{addr}"
        );
    }
    let mixed_case =
        r#"{"t_ns":1,"source":"kismet","kind":"ble_advert","addr":"AA:aa:Aa:00:00:01"}"#;
    assert!(accepted(mixed_case));
}

#[test]
fn a_malformed_station_address_is_rejected() {
    let err = rejection(&wifi_client(r#""addr":"ff:ff:ff:00:00""#));
    assert_eq!(
        err,
        "line 1: Wi-Fi station address isn't of the form xx:xx:xx:xx:xx:xx"
    );
}

#[test]
fn a_wifi_frequency_outside_the_bands_is_rejected() {
    for mhz in [0, 2399, 2501, 4899, 5901, 5924, 7126] {
        let err = rejection(&wifi_ap(&format!(
            r#""bssid":"ee:ee:ee:00:00:01","freq_mhz":{mhz}"#
        )));
        assert_eq!(
            err, "line 1: Wi-Fi frequency outside the Wi-Fi bands",
            "{mhz}"
        );
    }
    for mhz in [2400, 2500, 4900, 5900, 5925, 7125] {
        assert!(
            accepted(&wifi_ap(&format!(
                r#""bssid":"ee:ee:ee:00:00:01","freq_mhz":{mhz}"#
            ))),
            "{mhz}"
        );
    }
}

#[test]
fn a_wifi_rssi_out_of_range_is_rejected() {
    for dbm in [-128, 21] {
        let err = rejection(&wifi_client(&format!(
            r#""addr":"ff:ff:ff:00:00:02","rssi_dbm":{dbm}"#
        )));
        assert_eq!(err, "line 1: Wi-Fi RSSI out of range", "{dbm}");
    }
    for dbm in [-127, 20] {
        assert!(
            accepted(&wifi_client(&format!(
                r#""addr":"ff:ff:ff:00:00:02","rssi_dbm":{dbm}"#
            ))),
            "{dbm}"
        );
    }
}

#[test]
fn an_ssid_must_be_1_to_32_bytes() {
    // 16 two-byte characters are 32 bytes; 17 are 34.
    let fits = "\u{e9}".repeat(16);
    let too_long = "\u{e9}".repeat(17);
    assert!(accepted(&wifi_ap(&format!(
        r#""bssid":"ee:ee:ee:00:00:01","ssid":"{fits}""#
    ))));
    for ssid in [too_long.as_str(), ""] {
        let err = rejection(&wifi_ap(&format!(
            r#""bssid":"ee:ee:ee:00:00:01","ssid":"{ssid}""#
        )));
        assert_eq!(err, "line 1: SSID must be 1 to 32 bytes");
        assert!(ssid.is_empty() || !err.contains(ssid), "{err}");
    }
}

#[test]
fn a_receiver_outside_its_characters_is_rejected() {
    for receiver in ["", "Phone", "rear pi", "phone/1", &"a".repeat(33)] {
        let line = format!(
            r#"{{"t_ns":1,"source":"kismet","receiver":"{receiver}","kind":"ble_advert","addr":"aa:aa:aa:00:00:01"}}"#
        );
        assert_eq!(
            rejection(&line),
            "line 1: receiver must be 1 to 32 characters of a-z, 0-9, '-', '_' or '.'",
            "{receiver}"
        );
    }
    let fine = r#"{"t_ns":1,"source":"kismet","receiver":"rear-pi_2.0","kind":"ble_advert","addr":"aa:aa:aa:00:00:01"}"#;
    assert!(accepted(fine));
}

#[test]
fn out_of_range_latitude_is_rejected_with_its_line() {
    let input = "\n{\"t_ns\":1,\"source\":\"gpsd\",\"kind\":\"gnss_fix\",\"lat_deg\":91.0,\"lon_deg\":0.0}\n";
    let err = parse_jsonl(Cursor::new(input)).unwrap_err();
    assert_eq!(err.to_string(), "line 2: latitude out of range");
}

#[test]
fn unknown_kind_is_rejected_without_quoting_it() {
    let input = "{\"t_ns\":1,\"source\":\"x\",\"kind\":\"zz_unknown_kind\"}";
    let err = parse_jsonl(Cursor::new(input)).unwrap_err().to_string();
    assert!(err.starts_with("line 1, column "), "{err}");
    assert!(
        err.contains("unknown variant (value left out), expected"),
        "{err}"
    );
    assert!(
        err.contains("`ble_advert`"),
        "the schema's kinds stay: {err}"
    );
    assert!(!err.contains("zz_unknown_kind"), "{err}");
}

#[test]
fn a_value_of_the_wrong_type_is_rejected_without_quoting_it() {
    // The value imitates serde's own wording, to try to fool the redaction.
    let input = r#"{"t_ns":1,"source":"kismet","kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":"zz:zz:zz, expected `zz` \"zz\""}"#;
    let err = parse_jsonl(Cursor::new(input)).unwrap_err().to_string();
    assert!(
        err.ends_with(": invalid type: string (value left out), expected i16"),
        "{err}"
    );
    assert!(!err.contains("zz"), "{err}");
}

#[test]
fn a_missing_field_is_named() {
    let input = "{\"t_ns\":1,\"source\":\"kismet\",\"kind\":\"ble_advert\"}";
    let err = parse_jsonl(Cursor::new(input)).unwrap_err().to_string();
    assert!(err.ends_with(": missing field `addr`"), "{err}");
}

#[test]
fn a_truncated_line_keeps_its_reason() {
    let input = "{\"t_ns\":1,";
    let err = parse_jsonl(Cursor::new(input)).unwrap_err().to_string();
    assert!(err.starts_with("line 1, column "), "{err}");
    assert!(err.contains("EOF while parsing"), "{err}");
}

#[test]
fn the_default_rule_is_valid() {
    assert_eq!(PersistenceRule::default().validate(), Ok(()));
}

#[test]
fn a_rule_needs_a_positive_finite_place_separation() {
    for place_separation_m in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let rule = PersistenceRule {
            place_separation_m,
            ..PersistenceRule::default()
        };
        assert_eq!(
            rule.validate(),
            Err("place_separation_m must be a finite number of meters above 0"),
            "{place_separation_m}"
        );
    }
}

#[test]
fn a_rule_needs_at_least_one_place() {
    let rule = PersistenceRule {
        min_places: 0,
        ..PersistenceRule::default()
    };
    assert_eq!(rule.validate(), Err("min_places must be at least 1"));
}

#[test]
fn a_rule_rejects_negative_durations() {
    let span = PersistenceRule {
        min_span_ns: -1,
        ..PersistenceRule::default()
    };
    assert_eq!(span.validate(), Err("min_span_ns must not be negative"));
    let age = PersistenceRule {
        max_fix_age_ns: -1,
        ..PersistenceRule::default()
    };
    assert_eq!(age.validate(), Err("max_fix_age_ns must not be negative"));
}
