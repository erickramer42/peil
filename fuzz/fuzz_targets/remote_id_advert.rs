//! Drone Remote ID (ADR-0007 section 6): the input as a BLE advert's service
//! data under 0xFFFA, with and without the application code in front, and
//! as a message pack, through `describe`. Any bytes never panic, and every
//! distance, bearing, speed and position the picture gives is finite and in
//! its range.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sensio_core::{DescribeParams, Event, VendorTable, describe, parse_jsonl};

const T0: i64 = 1_800_000_000_000_000_000;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn check(data: &[u8]) {
    // The core bounds service data at 255 bytes.
    let data = &data[..data.len().min(255)];
    let lines = [
        format!(r#"{{"t_ns":{T0},"source":"android","kind":"gnss_fix","lat_deg":60.0,"lon_deg":10.0}}"#),
        format!(
            r#"{{"t_ns":{T0},"source":"android","kind":"ble_advert","addr":"aa:aa:aa:00:00:01","service_data":[{{"uuid":"fffa","data":"{}"}}]}}"#,
            hex(data)
        ),
    ];
    let Ok(events): Result<Vec<Event>, _> = parse_jsonl(lines.join("\n").as_bytes()) else {
        return;
    };
    let picture = describe(&events, T0, &DescribeParams::default(), &VendorTable::default());
    for e in &picture.emitters {
        let Some(d) = &e.drone else {
            continue;
        };
        for v in [d.distance_m, d.operator_distance_m, d.speed_mps, d.height_m, d.alt_geo_m].into_iter().flatten() {
            assert!(v.is_finite() && v >= -1000.0, "{d:?}");
        }
        for b in [d.bearing_deg, d.operator_bearing_deg, d.direction_deg].into_iter().flatten() {
            assert!((0.0..360.0).contains(&b), "{d:?}");
        }
        for lat in [d.lat_deg, d.operator_lat_deg].into_iter().flatten() {
            assert!((-90.0..=90.0).contains(&lat), "{d:?}");
        }
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    let mut wrapped = vec![0x0d, 0];
    wrapped.extend_from_slice(data);
    check(&wrapped);
    let mut pack = vec![0x0d, 0, 0xf2, 25, data.first().map_or(0, |n| n % 10)];
    pack.extend_from_slice(data);
    check(&pack);
});
