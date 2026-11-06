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
fn unknown_kind_is_rejected() {
    let input = "{\"t_ns\":1,\"source\":\"x\",\"kind\":\"cell_tower\"}";
    let err = parse_jsonl(Cursor::new(input)).unwrap_err();
    assert!(err.to_string().starts_with("line 1: "), "{err}");
}
