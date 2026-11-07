//! The `sensio` source's normalizer and the choice of normalizer by source.

use sensio_core::{EventKind, Normalize};
use sensio_sources::Sources;

#[test]
fn a_sensio_line_is_one_event() {
    let kinds = Sources
        .normalize(
            "sensio",
            r#"{"kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":-60}"#,
        )
        .expect("the line normalizes");
    assert_eq!(
        kinds,
        [EventKind::BleAdvert {
            addr: "aa:aa:aa:00:00:01".to_owned(),
            rssi_dbm: Some(-60),
            manufacturer_data: Vec::new(),
            service_data: Vec::new(),
            service_uuids: Vec::new(),
        }]
    );
}

#[test]
fn a_bad_sensio_line_is_rejected_without_quoting_it() {
    for raw in [
        r#"{"kind":"zz:zz:zz:00:00:09"}"#,
        r#"{"kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":"zz:zz:zz:00:00:09"}"#,
    ] {
        let reason = Sources.normalize("sensio", raw).expect_err(raw);
        assert!(reason.contains("(value left out)"), "{reason}");
        assert!(!reason.contains("zz:zz:zz"), "{reason}");
    }
    let reason = Sources.normalize("sensio", "{").expect_err("torn JSON");
    assert_eq!(reason, "EOF while parsing an object");
}

#[test]
fn a_source_with_no_normalizer_is_rejected() {
    assert_eq!(
        Sources.normalize("kismet", "{}"),
        Err("no normalizer for this source".to_owned())
    );
}
