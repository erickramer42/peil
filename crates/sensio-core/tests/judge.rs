//! The judge against a synthetic drive: golden verdicts, order independence,
//! and parsing that rejects bad input.
//!
//! The drive (`fixtures/drive.jsonl`) moves north about 1.1 km between fixes.
//! One BLE device and one TPMS sensor travel with it; others are seen once,
//! twice, or with no recent fix.

use std::io::Cursor;

use proptest::prelude::*;
use sensio_core::{Event, PersistenceRule, judge, parse_jsonl};

const DRIVE: &str = include_str!("fixtures/drive.jsonl");

fn drive() -> Vec<Event> {
    parse_jsonl(Cursor::new(DRIVE)).expect("the fixture parses")
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

proptest! {
    #[test]
    fn verdicts_do_not_depend_on_input_order(shuffled in Just(drive()).prop_shuffle()) {
        let rule = PersistenceRule::default();
        prop_assert_eq!(judge(&shuffled, &rule), judge(&drive(), &rule));
    }
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
