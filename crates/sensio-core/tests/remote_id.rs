//! Drone Remote ID in BLE adverts (ADR-0007 section 6), from synthetic
//! messages laid out as Open Drone ID lays them out: service data under
//! 0xFFFA, the application code 0x0D, a counter, then 25-byte messages,
//! little-endian.

use std::io::Cursor;

use sensio_core::{DescribeParams, VendorTable, describe, parse_jsonl};

const T0: i64 = 1_800_000_000_000_000_000;

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// A message of `kind` (the header's high nibble), protocol 2, with `fill`
/// writing its fields.
fn message(kind: u8, fill: impl FnOnce(&mut [u8; 25])) -> [u8; 25] {
    let mut m = [0u8; 25];
    m[0] = (kind << 4) | 2;
    fill(&mut m);
    m
}

fn degrees(m: &mut [u8; 25], at: usize, deg: f64) {
    #[allow(clippy::cast_possible_truncation)]
    let raw = (deg * 1e7).round() as i32;
    m[at..at + 4].copy_from_slice(&raw.to_le_bytes());
}

fn meters(m: &mut [u8; 25], at: usize, m_above: f64) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let raw = ((m_above + 1000.0) * 2.0).round() as u16;
    m[at..at + 2].copy_from_slice(&raw.to_le_bytes());
}

fn basic_id() -> [u8; 25] {
    message(0, |m| {
        m[1] = 0x12; // A serial number; a helicopter or multirotor.
        m[2..12].copy_from_slice(b"TEST000001");
    })
}

fn location() -> [u8; 25] {
    message(1, |m| {
        m[1] = 0x20; // Airborne, no speed multiplier, direction under 180.
        m[2] = 90;
        m[3] = 20; // 5 m/s.
        m[4] = 4; // 2 m/s up.
        degrees(m, 5, 60.01);
        degrees(m, 9, 10.0);
        meters(m, 15, 120.0);
        meters(m, 17, 60.0);
    })
}

fn system() -> [u8; 25] {
    message(4, |m| {
        m[1] = 0x01; // The operator's live GNSS position.
        degrees(m, 2, 59.99);
        degrees(m, 6, 10.0);
    })
}

fn operator_id() -> [u8; 25] {
    message(5, |m| m[2..9].copy_from_slice(b"OPTEST1"))
}

fn advert(t: i64, data: &[u8]) -> String {
    format!(
        r#"{{"t_ns":{t},"source":"android","receiver":"phone","kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":-70,"service_data":[{{"uuid":"fffa","data":"{}"}}]}}"#,
        hex(data)
    )
}

fn wrapped(counter: u8, body: &[u8]) -> Vec<u8> {
    let mut data = vec![0x0d, counter];
    data.extend_from_slice(body);
    data
}

fn fix() -> String {
    format!(r#"{{"t_ns":{T0},"source":"android","kind":"gnss_fix","lat_deg":60.0,"lon_deg":10.0}}"#)
}

fn picture_of(lines: &[String], now: i64) -> sensio_core::Picture {
    let events = parse_jsonl(Cursor::new(lines.join("\n"))).expect("parses");
    describe(
        &events,
        now,
        &DescribeParams::default(),
        &VendorTable::default(),
    )
}

#[test]
fn a_drones_messages_across_adverts_give_it_and_its_operator_from_the_vehicle() {
    let lines = [
        fix(),
        advert(T0, &wrapped(1, &basic_id())),
        advert(T0 + 1, &wrapped(2, &location())),
        advert(T0 + 2, &wrapped(3, &system())),
        advert(T0 + 3, &wrapped(4, &operator_id())),
    ];
    let picture = picture_of(&lines, T0 + 3);
    let e = &picture.emitters[0];
    assert_eq!(
        e.category.map(|c| (c.category, c.entry)),
        Some(("drone", "remote-id"))
    );
    let d = e.drone.as_ref().expect("a drone");
    assert_eq!(
        (d.id_type, d.ua_type, d.uas_id.as_deref()),
        (Some(1), Some(2), Some("TEST000001"))
    );
    assert_eq!(d.status, Some(2));
    assert_eq!(
        (d.direction_deg, d.speed_mps, d.vertical_speed_mps),
        (Some(90.0), Some(5.0), Some(2.0))
    );
    assert_eq!(
        (d.alt_geo_m, d.height_m, d.alt_baro_m),
        (Some(120.0), Some(60.0), None)
    );
    assert_eq!(d.position_ns, Some(T0 + 1));
    let near =
        |a: Option<f64>, b: f64, tolerance: f64| a.is_some_and(|a| (a - b).abs() < tolerance);
    assert!(
        near(d.lat_deg, 60.01, 1e-6) && near(d.lon_deg, 10.0, 1e-6),
        "{d:?}"
    );
    assert!(near(d.distance_m, 1111.95, 1.0), "{:?}", d.distance_m);
    assert!(near(d.bearing_deg, 0.0, 0.01), "{:?}", d.bearing_deg);
    assert_eq!(d.operator_location_type, Some(1));
    assert!(
        near(d.operator_distance_m, 1111.95, 1.0),
        "{:?}",
        d.operator_distance_m
    );
    assert!(
        near(d.operator_bearing_deg, 180.0, 0.01),
        "{:?}",
        d.operator_bearing_deg
    );
    assert_eq!(d.operator_id.as_deref(), Some("OPTEST1"));
}

#[test]
fn a_message_pack_gives_each_message_it_holds() {
    let mut pack = vec![0xf2, 25, 2];
    pack.extend_from_slice(&basic_id());
    pack.extend_from_slice(&location());
    let picture = picture_of(&[fix(), advert(T0, &wrapped(1, &pack))], T0);
    let d = picture.emitters[0].drone.as_ref().expect("a drone");
    assert_eq!(d.uas_id.as_deref(), Some("TEST000001"));
    assert!(d.lat_deg.is_some());
}

#[test]
fn encoded_unknowns_read_as_absent() {
    let unknown = message(1, |m| {
        m[1] = 0x03; // Status 0, the speed multiplier and the east-west bit.
        m[2] = 181; // With the east-west bit, 361: the encoding's unknown direction.
        m[3] = 255; // Speed unknown.
        m[4] = 126; // Vertical speed unknown (63 m/s).
        // Position 0, 0 and altitudes 0 (-1000 m): unknown.
    });
    let picture = picture_of(&[advert(T0, &wrapped(1, &unknown))], T0);
    let d = picture.emitters[0].drone.as_ref().expect("a drone");
    assert_eq!(d.status, Some(0));
    assert_eq!(
        (
            d.direction_deg,
            d.speed_mps,
            d.vertical_speed_mps,
            d.lat_deg,
            d.alt_geo_m,
            d.height_m
        ),
        (None, None, None, None, None, None)
    );
}

#[test]
fn service_data_that_isnt_remote_id_makes_no_drone() {
    let lines = [
        advert(T0, &[0x0c, 1]),
        advert(T0, &wrapped(1, &basic_id()[..24]))
            .replace("aa:aa:aa:00:00:01", "aa:aa:aa:00:00:02"),
        advert(T0, &wrapped(1, &basic_id()))
            .replace(r#""uuid":"fffa""#, r#""uuid":"fffb""#)
            .replace("aa:aa:aa:00:00:01", "aa:aa:aa:00:00:03"),
    ];
    let picture = picture_of(&lines, T0);
    assert_eq!(picture.emitters.len(), 3);
    assert!(
        picture
            .emitters
            .iter()
            .all(|e| e.drone.is_none() && e.category.is_none())
    );
}
