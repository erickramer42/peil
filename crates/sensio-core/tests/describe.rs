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
        ble(
            T0,
            "aa:aa:aa:00:00:05",
            -64,
            r#","service_data":[{"uuid":"FEED","data":"0102"}]"#,
        ),
        ble(
            T0,
            "aa:aa:aa:00:00:06",
            -65,
            r#","service_data":[{"uuid":"0000feec-0000-1000-8000-00805f9b34fb","data":"01"}]"#,
        ),
        ble(
            T0,
            "aa:aa:aa:00:00:07",
            -66,
            r#","service_data":[{"uuid":"fd84","data":"01"}]"#,
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
            None,
            Some(Tracker::Tile),
            Some(Tracker::Tile),
            Some(Tracker::Tile),
        ]
    );
    assert_eq!(
        picture.emitters[4].category.map(|c| (c.category, c.entry)),
        Some(("tracker", "tile"))
    );
    assert_eq!(picture.emitters[0].company_ids, [0x004c]);
    // Status 0xaa: bits 4 and 5 are 2, another maker's Find My accessory.
    assert_eq!(
        picture.emitters[0].category.map(|c| (c.category, c.entry)),
        Some(("tracker", "apple-find-my-accessory"))
    );
}

#[test]
fn a_find_my_adverts_status_byte_names_the_device() {
    // Status bytes with bits 4 and 5 set to 1, 2, 3 and 0, other bits set
    // around them; then an advert too short to carry a status byte.
    let apple = |n: u8, data: &str| {
        ble(
            T0,
            &format!("aa:aa:aa:00:00:{n:02x}"),
            -60 - i16::from(n),
            &format!(r#","manufacturer_data":[{{"company_id":76,"data":"{data}"}}]"#),
        )
    };
    let events = events(&[
        apple(1, "1219d0aa"),
        apple(2, "1202e5"),
        apple(3, "1219f0"),
        apple(4, "1219cf"),
        apple(5, "1219"),
    ]);
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    assert!(
        picture
            .emitters
            .iter()
            .all(|e| e.tracker == Some(Tracker::AppleFindMy))
    );
    let categories: Vec<Option<(&str, &str)>> = picture
        .emitters
        .iter()
        .map(|e| e.category.map(|c| (c.category, c.entry)))
        .collect();
    assert_eq!(
        categories,
        [
            Some(("tracker", "apple-find-my-airtag")),
            Some(("tracker", "apple-find-my-accessory")),
            Some(("audio", "apple-find-my-airpods")),
            Some(("apple-device", "apple-find-my-apple-device")),
            Some(("tracker", "apple-find-my")),
        ]
    );
}

#[test]
fn a_smarttag_is_fd5a_service_data_whose_first_byte_masks_to_0x10() {
    // The low three bits are the tag's state and pass the mask; 0x18 and 0x08
    // set the bits next to it, and empty service data isn't one.
    let samsung = |n: u8, data: &str| {
        ble(
            T0,
            &format!("aa:aa:aa:00:00:{n:02x}"),
            -60 - i16::from(n),
            &format!(r#","service_data":[{{"uuid":"FD5A","data":"{data}"}}]"#),
        )
    };
    let events = events(&[
        samsung(1, "10aabb"),
        samsung(2, "13"),
        samsung(3, "17aa"),
        samsung(4, "18aabb"),
        samsung(5, "08aabb"),
        samsung(6, ""),
    ]);
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    let trackers: Vec<Option<Tracker>> = picture.emitters.iter().map(|e| e.tracker).collect();
    let tag = Some(Tracker::SmartThingsFind);
    assert_eq!(trackers, [tag, tag, tag, None, None, None]);
    assert_eq!(
        picture.emitters[0].category.map(|c| (c.category, c.entry)),
        Some(("tracker", "samsung-smarttag"))
    );
}

#[test]
fn flock_network_names_match_the_firmwares_forms_and_nothing_near_them() {
    let names = [
        "Flock-a1b2c3",
        "FLOCK-A1B2C3",
        "Flock",
        "Flock Camera net.",
        "FS Ext Battery",
        // Not hex, one digit short, one over, the wrong case, a name that
        // merely contains the word, and a common word left out of the table.
        "Flock-Family",
        "Flock-a1b2c",
        "Flock-a1b2c3d",
        "flock-a1b2c3",
        "My Flock",
        "Penguin",
    ];
    let lines: Vec<String> = names
        .iter()
        .zip(1u8..)
        .map(|(name, n)| {
            format!(
                r#"{{"t_ns":{T0},"source":"sensio","kind":"wifi_ap","bssid":"cc:cc:cc:00:00:{n:02x}","ssid":"{name}","rssi_dbm":{}}}"#,
                -40 - i16::from(n)
            )
        })
        .collect();
    let picture = describe(
        &events(&lines),
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    let entries: Vec<Option<(&str, &str)>> = picture
        .emitters
        .iter()
        .map(|e| e.category.map(|c| (c.category, c.entry)))
        .collect();
    assert_eq!(
        entries,
        [
            Some(("flock-class", "flock-softap")),
            Some(("flock-class", "flock-softap")),
            Some(("flock-class", "flock-ssid")),
            Some(("flock-class", "flock-camera-net")),
            Some(("flock-class", "flock-ext-battery-ap")),
            None,
            None,
            None,
            None,
            None,
            None,
        ]
    );
}

#[test]
fn flocks_address_block_marks_wifi_emitters_only() {
    // Built from parts, since these prefixes are real and the rest is made up.
    let block = "b4:1e:52";
    let next_block = "b4:1e:53";
    let station = |addr: &str, rssi: i16| {
        format!(
            r#"{{"t_ns":{T0},"source":"sensio","kind":"wifi_client","addr":"{addr}","rssi_dbm":{rssi}}}"#
        )
    };
    let events = events(&[
        ap(T0, &format!("{block}:00:00:01"), -41),
        station(&format!("{block}:00:00:02"), -42),
        ap(T0, &format!("{next_block}:00:00:03"), -43),
        ble(T0, &format!("{block}:00:00:04"), -44, ""),
    ]);
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    let entries: Vec<Option<&str>> = picture
        .emitters
        .iter()
        .map(|e| e.category.map(|c| c.entry))
        .collect();
    assert_eq!(entries, [Some("flock-oui"), Some("flock-oui"), None, None]);
}

#[test]
fn the_battery_packs_company_id_names_flock_class_unless_a_tracker_format_does() {
    let with = |n: u8, manufacturer: &str| {
        ble(
            T0,
            &format!("aa:aa:aa:00:00:{n:02x}"),
            -40 - i16::from(n),
            &format!(r#","manufacturer_data":[{manufacturer}]"#),
        )
    };
    let events = events(&[
        with(1, r#"{"company_id":2504,"data":"aabb"}"#),
        with(2, r#"{"company_id":2505,"data":"aabb"}"#),
        with(
            3,
            r#"{"company_id":2504,"data":"aabb"},{"company_id":76,"data":"1219d0"}"#,
        ),
    ]);
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    let entries: Vec<Option<(&str, &str)>> = picture
        .emitters
        .iter()
        .map(|e| e.category.map(|c| (c.category, c.entry)))
        .collect();
    assert_eq!(
        entries,
        [
            Some(("flock-class", "flock-battery-company-id")),
            None,
            Some(("tracker", "apple-find-my-airtag")),
        ]
    );
}

#[test]
fn service_data_under_other_uuids_names_no_tracker() {
    // 0xFEEE is next to Tile's 0xFEED; 0xFD00 and 0xFE2C belong to other
    // companies; a 128-bit UUID off the Bluetooth base isn't a short one; a
    // Tile UUID in manufacturer data's place isn't service data.
    let events = events(&[
        ble(
            T0,
            "aa:aa:aa:00:00:01",
            -60,
            r#","service_data":[{"uuid":"feee","data":"01"}]"#,
        ),
        ble(
            T0,
            "aa:aa:aa:00:00:02",
            -61,
            r#","service_data":[{"uuid":"fd00","data":"01"},{"uuid":"fe2c","data":"01"}]"#,
        ),
        ble(
            T0,
            "aa:aa:aa:00:00:03",
            -62,
            r#","service_data":[{"uuid":"0000feed-0000-1000-8000-00805f9b34fc","data":"01"}]"#,
        ),
        ble(
            T0,
            "aa:aa:aa:00:00:04",
            -63,
            r#","manufacturer_data":[{"company_id":65261,"data":"01"}]"#,
        ),
    ]);
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    assert_eq!(picture.emitters.len(), 4);
    assert!(picture.emitters.iter().all(|e| e.tracker.is_none()));
    assert!(picture.emitters.iter().all(|e| e.category.is_none()));
}

fn classic(t: i64, addr: &str, rssi: i16, class: u32) -> String {
    format!(
        r#"{{"t_ns":{t},"source":"sensio","kind":"bt_classic","addr":"{addr}","rssi_dbm":{rssi},"device_class":{class}}}"#
    )
}

#[test]
fn a_classic_devices_category_comes_from_its_class_of_device() {
    // Service bits set where a real device would set them; they don't count.
    let services = 0x24_0000;
    let events = events(&[
        classic(T0, "cc:cc:cc:00:00:01", -50, services | (4 << 8) | (8 << 2)),
        classic(T0, "cc:cc:cc:00:00:02", -51, services | (4 << 8) | (2 << 2)),
        classic(T0, "cc:cc:cc:00:00:03", -52, (2 << 8) | (3 << 2)),
        classic(T0, "cc:cc:cc:00:00:04", -53, (6 << 8) | (1 << 5) | (1 << 7)),
        classic(T0, "cc:cc:cc:00:00:05", -54, (6 << 8) | (1 << 7)),
        classic(T0, "cc:cc:cc:00:00:06", -55, services | (4 << 8) | (3 << 2)),
        classic(T0, "cc:cc:cc:00:00:07", -56, 0x1f << 8),
        classic(T0, "cc:cc:cc:00:00:08", -57, services),
    ]);
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    let categories: Vec<Option<(&str, &str)>> = picture
        .emitters
        .iter()
        .map(|e| e.category.map(|c| (c.category, c.entry)))
        .collect();
    assert_eq!(
        categories,
        [
            Some(("car-audio", "cod-av-car-audio")),
            Some(("hands-free", "cod-av-hands-free")),
            Some(("phone", "cod-phone")),
            Some(("camera-class", "cod-imaging-camera")),
            Some(("imaging", "cod-imaging")),
            Some(("audio-video", "cod-audio-video")),
            None,
            None,
        ],
        "uncategorized and miscellaneous classes say nothing"
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
fn the_texture_counts_the_recent_emitters_and_names_the_newest() {
    let events = events(&[
        ble(T0, "aa:aa:aa:00:00:01", -60, ""),
        ble(T0 + 100 * S, "aa:aa:aa:00:00:01", -60, ""),
        ble(T0 + 90 * S, "bb:bb:bb:00:00:02", -60, ""),
        ap(T0 + 95 * S, "ee:ee:ee:00:00:01", -70),
        ap(T0 + 95 * S, "cc:cc:cc:00:00:01", -75),
    ]);
    let params = DescribeParams {
        newest: 2,
        ..DescribeParams::default()
    };
    let texture = describe(&events, T0 + 100 * S, &params, &VendorTable::default()).texture;
    assert_eq!((texture.counts.ble, texture.counts.access_point), (2, 2));
    assert_eq!(texture.randomized_wifi, 1, "0xEE is randomized, 0xCC isn't");
    assert_eq!(
        (texture.new.ble, texture.new.access_point),
        (1, 2),
        "the first BLE device was first seen before the window"
    );
    let newest: Vec<&str> = texture
        .newest
        .iter()
        .map(sensio_core::DeviceId::as_str)
        .collect();
    assert_eq!(newest, ["wifi:cc:cc:cc:00:00:01", "wifi:ee:ee:ee:00:00:01"]);
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
