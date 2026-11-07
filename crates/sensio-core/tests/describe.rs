//! The describe layer over hand-written events: loud first, vendor blocks
//! and randomized addresses, the tracker formats, the recent window and
//! movement.

use std::io::Cursor;

use sensio_core::describe::{Address, Band, Movement, Tracker};
use sensio_core::{DescribeParams, Event, VendorTable, describe, parse_jsonl};

const T0: i64 = 1_800_000_000_000_000_000;
const S: i64 = 1_000_000_000;

fn events(lines: &[String]) -> Vec<Event> {
    parse_jsonl(Cursor::new(lines.join("\n"))).expect("the events parse")
}

fn fix(t: i64, lat: f64) -> String {
    format!(r#"{{"t_ns":{t},"source":"sensio","kind":"gnss_fix","lat_deg":{lat},"lon_deg":10.0}}"#)
}

fn ble(t: i64, addr: &str, rssi: i16, extra: &str) -> String {
    format!(
        r#"{{"t_ns":{t},"source":"sensio","receiver":"phone","kind":"ble_advert","addr":"{addr}","rssi_dbm":{rssi}{extra}}}"#
    )
}

fn ap(t: i64, bssid: &str, rssi: i16) -> String {
    format!(
        r#"{{"t_ns":{t},"source":"sensio","kind":"wifi_ap","bssid":"{bssid}","ssid":"example-net","rssi_dbm":{rssi},"security":{{"protocol":"wpa3"}}}}"#
    )
}

/// A synthetic registry: one MA-L block and a longer MA-S block inside it.
/// 0xCC has the locally administered bit (0x02) clear; 0xAA, 0xBB and 0xEE
/// have it set, so those read as randomized.
const VENDORS: &str = "Registry,Assignment,Organization Name,Organization Address\n\
MA-L,CCCCCC,\"Example Radio Co, Ltd\",\"1 Example Road\"\n\
MA-S,CCCCCC001,Example Small Block,\"\"\n";

#[test]
fn emitters_come_loud_first_with_their_bands() {
    let events = events(&[
        ble(T0, "aa:aa:aa:00:00:01", -90, ""),
        ble(T0, "bb:bb:bb:00:00:02", -50, ""),
        ap(T0, "ee:ee:ee:00:00:01", -70),
    ]);
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    let order: Vec<(&str, Band)> = picture
        .emitters
        .iter()
        .map(|e| (e.device.as_str(), e.band))
        .collect();
    assert_eq!(
        order,
        [
            ("ble:bb:bb:bb:00:00:02", Band::Strong),
            ("wifi:ee:ee:ee:00:00:01", Band::Medium),
            ("ble:aa:aa:aa:00:00:01", Band::Weak),
        ]
    );
}

#[test]
fn vendors_come_from_the_longest_block_and_randomized_addresses_have_none() {
    let vendors = VendorTable::parse(VENDORS.as_bytes()).expect("the registry parses");
    assert!(vendors.id().is_some_and(|id| id.starts_with("fnv1a64:")));
    let events = events(&[
        ap(T0, "cc:cc:cc:00:10:01", -60),
        ap(T0, "cc:cc:cc:12:00:01", -61),
        ap(T0, "ee:ee:ee:00:00:01", -62),
    ]);
    let picture = describe(&events, T0, &DescribeParams::default(), &vendors);
    let seen: Vec<(Address, Option<&str>)> = picture
        .emitters
        .iter()
        .map(|e| (e.address, e.vendor.as_deref()))
        .collect();
    assert_eq!(
        seen,
        [
            (Address::Global, Some("Example Small Block")),
            (Address::Global, Some("Example Radio Co, Ltd")),
            (Address::Randomized, None),
        ]
    );
}

#[test]
fn a_bad_registry_line_is_rejected_with_its_line() {
    let bad = "Registry,Assignment,Organization Name\nMA-L,EEEEE,Too Short\n";
    let err = VendorTable::parse(bad.as_bytes()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "line 2: not an MA-L, MA-M or MA-S assignment"
    );
    let unbalanced = "MA-L,EEEEEE,\"Unclosed\n";
    assert_eq!(
        VendorTable::parse(unbalanced.as_bytes())
            .unwrap_err()
            .to_string(),
        "line 1: unbalanced quotes"
    );
}

#[test]
fn the_tracker_formats_are_recognized() {
    let events = events(&[
        ble(
            T0,
            "aa:aa:aa:00:00:01",
            -60,
            r#","manufacturer_data":[{"company_id":76,"data":"1219aa"}]"#,
        ),
        ble(
            T0,
            "aa:aa:aa:00:00:02",
            -61,
            r#","service_data":[{"uuid":"FEAA","data":"40aabb"}]"#,
        ),
        ble(
            T0,
            "aa:aa:aa:00:00:03",
            -62,
            r#","service_data":[{"uuid":"0000fcb2-0000-1000-8000-00805f9b34fb","data":"0101"}]"#,
        ),
        ble(
            T0,
            "aa:aa:aa:00:00:04",
            -63,
            r#","manufacturer_data":[{"company_id":76,"data":"1005aa"}]"#,
        ),
    ]);
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    let trackers: Vec<Option<Tracker>> = picture.emitters.iter().map(|e| e.tracker).collect();
    assert_eq!(
        trackers,
        [
            Some(Tracker::AppleFindMy),
            Some(Tracker::GoogleFindHub),
            Some(Tracker::Dult),
            None
        ]
    );
    assert_eq!(picture.emitters[0].company_ids, [0x004c]);
    assert_eq!(
        picture.emitters[0].category.map(|c| (c.category, c.entry)),
        Some(("tracker", "apple-find-my"))
    );
}

#[test]
fn only_recent_emitters_are_listed_and_the_rest_are_counted() {
    let events = events(&[
        ble(T0, "aa:aa:aa:00:00:01", -60, ""),
        ble(T0 + 100 * S, "bb:bb:bb:00:00:02", -60, ""),
    ]);
    let picture = describe(
        &events,
        T0 + 100 * S,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    assert_eq!(picture.emitters.len(), 1);
    assert_eq!(picture.older, 1);
}

#[test]
fn movement_reads_moving_stopped_and_parked() {
    let params = DescribeParams::default();
    let none = VendorTable::default();
    // 0.01 degree of latitude is about 1.1 km: in 10 s, about 111 m/s.
    let moving = events(&[fix(T0, 60.0), fix(T0 + 10 * S, 60.01)]);
    assert_eq!(
        describe(&moving, T0 + 10 * S, &params, &none).movement,
        Movement::Moving
    );
    let still = events(&[fix(T0, 60.0), fix(T0 + 10 * S, 60.0)]);
    assert_eq!(
        describe(&still, T0 + 10 * S, &params, &none).movement,
        Movement::Stopped
    );
    assert_eq!(
        describe(&still, T0 + 400 * S, &params, &none).movement,
        Movement::Parked
    );
    assert_eq!(
        describe(&still[..1], T0, &params, &none).movement,
        Movement::Unknown
    );
}
