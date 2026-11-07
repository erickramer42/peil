//! The `android` source's normalizer: the app's raw lines (synthetic).

use sensio_core::event::{ManufacturerData, ServiceData, WifiProtocol, WifiSecurity};
use sensio_core::{EventKind, Normalize};
use sensio_sources::Sources;

fn android(raw: &str) -> Result<Vec<EventKind>, String> {
    Sources.normalize("android", raw)
}

#[test]
fn a_ble_record_gives_its_manufacturer_data_service_data_and_uuids_without_the_name() {
    // Flags; a complete local name "ab"; 16-bit UUIDs 0xFEAA and 0x180F;
    // service data under 0xFCB2; manufacturer data for company 0x004C;
    // then the zero padding Android leaves.
    let record = concat!(
        "020106",
        "03096162",
        "0503aafe0f18",
        "0516b2fc0102",
        "07ff4c0012020000",
        "0000"
    );
    let raw = format!(r#"{{"t":"ble","addr":"aa:aa:aa:00:00:01","rssi":-61,"record":"{record}"}}"#);
    assert_eq!(
        android(&raw).expect("the line normalizes"),
        [EventKind::BleAdvert {
            addr: "aa:aa:aa:00:00:01".to_owned(),
            rssi_dbm: Some(-61),
            manufacturer_data: vec![ManufacturerData {
                company_id: 0x004c,
                data: "12020000".to_owned(),
            }],
            service_data: vec![ServiceData {
                uuid: "fcb2".to_owned(),
                data: "0102".to_owned(),
            }],
            service_uuids: vec!["feaa".to_owned(), "180f".to_owned()],
        }]
    );
}

#[test]
fn a_128_bit_uuid_is_written_in_its_long_form() {
    let record = "1107ffeeddccbbaa99887766554433221100";
    let raw = format!(r#"{{"t":"ble","addr":"aa:aa:aa:00:00:01","record":"{record}"}}"#);
    let kinds = android(&raw).expect("the line normalizes");
    let EventKind::BleAdvert { service_uuids, .. } = &kinds[0] else {
        panic!("not an advert: {kinds:?}");
    };
    assert_eq!(service_uuids, &["00112233-4455-6677-8899-aabbccddeeff"]);
}

#[test]
fn a_bad_ble_record_is_rejected() {
    for (record, why) in [
        ("0aff4c00", "longer than the record"),
        ("02f", "isn't hex"),
        ("zz", "isn't hex"),
    ] {
        let raw = format!(r#"{{"t":"ble","addr":"aa:aa:aa:00:00:01","record":"{record}"}}"#);
        let reason = android(&raw).expect_err(record);
        assert!(reason.contains(why), "{record}: {reason}");
    }
}

#[test]
fn a_wifi_scan_gives_one_access_point_per_result_with_its_security() {
    let raw = r#"{"t":"wifi","results":[
        {"bssid":"cc:cc:cc:00:00:01","ssid":"synthetic","freq":2412,"level":-50,"caps":"[WPA2-PSK-CCMP][RSN-PSK+SAE-CCMP][ESS]"},
        {"bssid":"cc:cc:cc:00:00:02","ssid":"","freq":5180,"level":-70,"caps":"[WPA2-EAP-CCMP][ESS]"},
        {"bssid":"cc:cc:cc:00:00:03","freq":99999,"level":-300,"caps":"[ESS]"}
    ]}"#
    .replace('\n', "");
    let kinds = android(&raw).expect("the line normalizes");
    let ap =
        |bssid: &str, ssid: Option<&str>, freq, level, protocol, enterprise| EventKind::WifiAp {
            bssid: bssid.to_owned(),
            ssid: ssid.map(str::to_owned),
            freq_mhz: freq,
            rssi_dbm: level,
            security: Some(WifiSecurity {
                protocol,
                enterprise,
            }),
        };
    assert_eq!(
        kinds,
        [
            ap(
                "cc:cc:cc:00:00:01",
                Some("synthetic"),
                Some(2412),
                Some(-50),
                WifiProtocol::Wpa3,
                false
            ),
            ap(
                "cc:cc:cc:00:00:02",
                None,
                Some(5180),
                Some(-70),
                WifiProtocol::Wpa2,
                true
            ),
            ap(
                "cc:cc:cc:00:00:03",
                None,
                None,
                None,
                WifiProtocol::Open,
                false
            ),
        ]
    );
}

#[test]
fn a_location_is_a_gnss_fix_and_an_unknown_type_is_rejected() {
    assert_eq!(
        android(r#"{"t":"loc","lat":60.01,"lon":10.02,"provider":"gps","acc":4.0}"#)
            .expect("the line normalizes"),
        [EventKind::GnssFix {
            lat_deg: 60.01,
            lon_deg: 10.02,
        }]
    );
    let reason = android(r#"{"t":"zz:zz:zz:00:00:09"}"#).expect_err("unknown type");
    assert!(!reason.contains("zz:zz:zz"), "{reason}");
}
