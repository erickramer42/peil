//! The `kismet` source's normalizer, over synthetic device records shaped as
//! Kismet 2026.10.R1 writes them.

use sensio_core::event::{ManufacturerData, WifiProtocol, WifiSecurity};
use sensio_core::{EventKind, Normalize};
use sensio_sources::Sources;
use sensio_sources::kismet::prune;
use serde_json::json;

/// The record normalized, after checking that its pruned form, as the laptop
/// sends it, normalizes the same.
fn kismet(record: &serde_json::Value) -> Result<Vec<EventKind>, String> {
    let whole = Sources.normalize("kismet", &record.to_string());
    let pruned = Sources.normalize("kismet", &prune(record).to_string());
    assert_eq!(
        whole.as_ref().ok(),
        pruned.as_ref().ok(),
        "pruning changed: {record}"
    );
    assert_eq!(whole.is_ok(), pruned.is_ok(), "pruning changed: {record}");
    whole
}

fn signal(last: i64) -> serde_json::Value {
    json!({
        "kismet.common.signal.type": "dbm",
        "kismet.common.signal.last_signal": last,
        "kismet.common.signal.min_signal": last - 5,
        "kismet.common.signal.max_signal": last + 5,
        "kismet.common.signal.signal_rrd": {"kismet.common.rrd.minute_vec": [0, 0, 0]}
    })
}

#[test]
fn an_access_point_gives_its_beaconed_name_channel_strength_and_security() {
    let record = json!({
        "kismet.device.base.macaddr": "CC:CC:CC:00:00:01",
        "kismet.device.base.phyname": "IEEE802.11",
        "kismet.device.base.type": "Wi-Fi AP",
        "kismet.device.base.channel": "6",
        "kismet.device.base.frequency": 2_437_000,
        "kismet.device.base.crypt": "WPA2",
        "kismet.device.base.signal": signal(-61),
        "dot11.device": {
            "dot11.device.last_bssid": "CC:CC:CC:00:00:01",
            "dot11.device.last_beaconed_ssid_record": {
                "dot11.advertisedssid.ssid": "example-hotspot",
                "dot11.advertisedssid.cloaked": 0,
                "dot11.advertisedssid.crypt_string": "WPA2-PSK WPA3-SAE",
                "dot11.advertisedssid.wps_device_name": "zz-wps-name"
            },
            "dot11.device.probed_ssid_map": [{"dot11.probedssid.ssid": "zz-probed-name"}]
        }
    });
    let kinds = kismet(&record).expect("the record normalizes");
    assert_eq!(
        kinds,
        [EventKind::WifiAp {
            bssid: "CC:CC:CC:00:00:01".to_owned(),
            ssid: Some("example-hotspot".to_owned()),
            freq_mhz: Some(2437),
            rssi_dbm: Some(-61),
            security: Some(WifiSecurity {
                protocol: WifiProtocol::Wpa3,
                enterprise: false,
            }),
        }]
    );
    let text = format!("{kinds:?}");
    assert!(!text.contains("zz-"), "a probe or WPS name leaked: {text}");
}

#[test]
fn a_pruned_record_keeps_no_probe_wps_name_or_signal_history() {
    let record = json!({
        "kismet.device.base.key": "4202770D00000000_0100CCCCCCCC",
        "kismet.device.base.macaddr": "CC:CC:CC:00:00:01",
        "kismet.device.base.phyname": "IEEE802.11",
        "kismet.device.base.type": "Wi-Fi AP",
        "kismet.device.base.last_time": 1_800_000_000,
        "kismet.device.base.signal": signal(-61),
        "dot11.device": {
            "dot11.device.last_beaconed_ssid_record": {
                "dot11.advertisedssid.ssid": "example-hotspot",
                "dot11.advertisedssid.wps_device_name": "zz-wps-name"
            },
            "dot11.device.probed_ssid_map": [{"dot11.probedssid.ssid": "zz-probed-name"}]
        }
    });
    let pruned = prune(&record);
    let text = pruned.to_string();
    assert!(
        !text.contains("zz-"),
        "a probe or WPS name was kept: {text}"
    );
    assert!(
        !text.contains("rrd") && !text.contains("min_signal"),
        "history kept: {text}"
    );
    assert_eq!(
        pruned["kismet.device.base.key"],
        record["kismet.device.base.key"]
    );
    assert_eq!(pruned["kismet.device.base.last_time"], 1_800_000_000);
    assert_eq!(kismet(&record).expect("normalizes").len(), 1);
}

#[test]
fn a_cloaked_network_keeps_no_name_and_an_unknown_strength_is_absent() {
    let record = json!({
        "kismet.device.base.macaddr": "cc:cc:cc:00:00:02",
        "kismet.device.base.phyname": "IEEE802.11",
        "kismet.device.base.type": "Wi-Fi AP",
        "kismet.device.base.frequency": 0,
        "kismet.device.base.signal": {"kismet.common.signal.type": "none", "kismet.common.signal.last_signal": 0},
        "dot11.device": {
            "dot11.device.last_beaconed_ssid_record": {
                "dot11.advertisedssid.ssid": "zz-hidden",
                "dot11.advertisedssid.cloaked": 1,
                "dot11.advertisedssid.crypt_string": ""
            }
        }
    });
    let kinds = kismet(&record).expect("the record normalizes");
    let EventKind::WifiAp {
        ssid,
        freq_mhz,
        rssi_dbm,
        security,
        ..
    } = &kinds[0]
    else {
        panic!("not an access point: {kinds:?}");
    };
    assert_eq!(
        (ssid, freq_mhz, rssi_dbm, security),
        (&None, &None, &None, &None)
    );
}

#[test]
fn a_station_gives_the_access_point_it_was_last_on() {
    let on = json!({
        "kismet.device.base.macaddr": "dd:dd:dd:00:00:01",
        "kismet.device.base.phyname": "IEEE802.11",
        "kismet.device.base.type": "Wi-Fi Client",
        "kismet.device.base.frequency": 5_180_000,
        "kismet.device.base.signal": signal(-70),
        "dot11.device": {"dot11.device.last_bssid": "cc:cc:cc:00:00:01"}
    });
    assert_eq!(
        kismet(&on).expect("the record normalizes"),
        [EventKind::WifiClient {
            addr: "dd:dd:dd:00:00:01".to_owned(),
            bssid: Some("cc:cc:cc:00:00:01".to_owned()),
            freq_mhz: Some(5180),
            rssi_dbm: Some(-70),
        }]
    );
    let alone = json!({
        "kismet.device.base.macaddr": "dd:dd:dd:00:00:02",
        "kismet.device.base.phyname": "IEEE802.11",
        "kismet.device.base.type": "Wi-Fi Device",
        "kismet.device.base.signal": 0,
        "dot11.device": {"dot11.device.last_bssid": "00:00:00:00:00:00"}
    });
    assert_eq!(
        kismet(&alone).expect("the record normalizes"),
        [EventKind::WifiClient {
            addr: "dd:dd:dd:00:00:02".to_owned(),
            bssid: None,
            freq_mhz: None,
            rssi_dbm: None,
        }]
    );
}

#[test]
fn a_station_whose_last_access_point_isnt_an_address_keeps_the_station() {
    for last in ["", "zz", "cc:cc:cc:00:00"] {
        let record = json!({
            "kismet.device.base.macaddr": "dd:dd:dd:00:00:03",
            "kismet.device.base.phyname": "IEEE802.11",
            "kismet.device.base.type": "Wi-Fi Client",
            "dot11.device": {"dot11.device.last_bssid": last}
        });
        let kinds = kismet(&record).expect("the record normalizes");
        assert!(
            matches!(&kinds[..], [EventKind::WifiClient { bssid: None, .. }]),
            "{last}: {kinds:?}"
        );
    }
}

#[test]
fn a_ble_device_gives_its_manufacturer_data_and_service_uuids() {
    let record = json!({
        "kismet.device.base.macaddr": "AA:AA:AA:00:00:01",
        "kismet.device.base.phyname": "Bluetooth",
        "kismet.device.base.type": "BTLE Tracker",
        "kismet.device.base.signal": signal(-58),
        "bluetooth.device": {
            "bluetooth.device.random_address_type": "non-resolvable",
            "bluetooth.device.manuf_company_id": "004c",
            "bluetooth.device.manuf_data": "12190000",
            "bluetooth.device.service_uuid_strs": ["FD6F", "not-a-uuid"],
            "bluetooth.device.service_data_bytes": {},
            "bluetooth.device.adv": {"bluetooth.company_name": "zz-company"}
        }
    });
    assert_eq!(
        kismet(&record).expect("the record normalizes"),
        [EventKind::BleAdvert {
            addr: "AA:AA:AA:00:00:01".to_owned(),
            rssi_dbm: Some(-58),
            manufacturer_data: vec![ManufacturerData {
                company_id: 0x004c,
                data: "12190000".to_owned(),
            }],
            service_data: Vec::new(),
            service_uuids: vec!["fd6f".to_owned()],
        }]
    );
}

#[test]
fn a_wds_access_point_is_an_access_point_with_or_without_a_beacon() {
    let beaconing = json!({
        "kismet.device.base.macaddr": "cc:cc:cc:00:00:0a",
        "kismet.device.base.phyname": "IEEE802.11",
        "kismet.device.base.type": "Wi-Fi WDS AP",
        "kismet.device.base.frequency": 5_180_000,
        "kismet.device.base.crypt": "WPA2",
        "kismet.device.base.signal": signal(-72),
        "dot11.device": {
            "dot11.device.last_bssid": "cc:cc:cc:00:00:0a",
            "dot11.device.last_beaconed_ssid_record": {
                "dot11.advertisedssid.ssid": "",
                "dot11.advertisedssid.cloaked": 1,
                "dot11.advertisedssid.crypt_string": "WPA2-PSK"
            }
        }
    });
    assert_eq!(
        kismet(&beaconing).expect("the record normalizes"),
        [EventKind::WifiAp {
            bssid: "cc:cc:cc:00:00:0a".to_owned(),
            ssid: None,
            freq_mhz: Some(5180),
            rssi_dbm: Some(-72),
            security: Some(WifiSecurity {
                protocol: WifiProtocol::Wpa2,
                enterprise: false,
            }),
        }]
    );
    // A receiver Kismet only saw addressed, with no signal of its own.
    let receiver = json!({
        "kismet.device.base.macaddr": "cc:cc:cc:00:00:0b",
        "kismet.device.base.phyname": "IEEE802.11",
        "kismet.device.base.type": "Wi-Fi WDS AP"
    });
    assert_eq!(
        kismet(&receiver).expect("the record normalizes"),
        [EventKind::WifiAp {
            bssid: "cc:cc:cc:00:00:0b".to_owned(),
            ssid: None,
            freq_mhz: None,
            rssi_dbm: None,
            security: None,
        }]
    );
}

#[test]
fn bridged_and_wds_devices_give_no_event_and_unknown_ones_are_rejected() {
    for kind in ["Wi-Fi Bridged", "Wi-Fi WDS"] {
        let behind = json!({
            "kismet.device.base.macaddr": "cc:cc:cc:00:00:09",
            "kismet.device.base.phyname": "IEEE802.11",
            "kismet.device.base.type": kind
        });
        assert_eq!(kismet(&behind), Ok(Vec::new()), "{kind}");
    }
    for (phy, kind, why) in [
        ("IEEE802.11", "Wi-Fi zz", "type not handled"),
        ("zz-phy", "zz", "PHY not handled"),
    ] {
        let record = json!({
            "kismet.device.base.macaddr": "cc:cc:cc:00:00:09",
            "kismet.device.base.phyname": phy,
            "kismet.device.base.type": kind
        });
        let reason = kismet(&record).expect_err(kind);
        assert!(reason.contains(why) && !reason.contains("zz"), "{reason}");
    }
    let reason = Sources
        .normalize("kismet", r#"{"kismet.device.base.phyname":"zz"}"#)
        .expect_err("no address");
    assert!(!reason.contains("zz"), "{reason}");
}

#[test]
fn a_classic_device_gives_its_class_of_device() {
    let record = json!({
        "kismet.device.base.macaddr": "EE:EE:EE:00:00:01",
        "kismet.device.base.phyname": "Bluetooth",
        "kismet.device.base.type": "BR/EDR",
        "kismet.device.base.signal": signal(-66),
        "bluetooth.device": {"bluetooth.device.class": 0x0020_0404, "bluetooth.device.major_class": "zz-audio"}
    });
    assert_eq!(
        kismet(&record).expect("the record normalizes"),
        [EventKind::BtClassic {
            addr: "EE:EE:EE:00:00:01".to_owned(),
            rssi_dbm: Some(-66),
            device_class: Some(0x0020_0404),
        }]
    );
}
