//! The `kismet` source's normalizer, two ways: the input as a raw device
//! record, and the input's text in every field of a BLE record and an
//! access point's record under synthetic addresses. A record the normalizer
//! accepts must give events the core accepts: the normalizer never produces
//! what `Event::validate` rejects.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sensio_core::{Event, Normalize};
use sensio_sources::Sources;
use serde_json::json;

fn check(record: &serde_json::Value) {
    let raw = record.to_string();
    let Ok(kinds) = Sources.normalize("kismet", &raw) else {
        return;
    };
    for kind in kinds {
        let event = Event {
            t_ns: 1_800_000_000_000_000_000,
            source: "kismet".to_owned(),
            receiver: None,
            kind,
        };
        if let Err(reason) = event.validate() {
            panic!("the core rejects a normalized Kismet event ({reason}): {raw}");
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = Sources.normalize("kismet", text);

    let number = data.iter().fold(0i64, |n, &b| n.wrapping_mul(31).wrapping_add(i64::from(b)));
    let signal = json!({"kismet.common.signal.type": "dbm", "kismet.common.signal.last_signal": number % 200});
    check(&json!({
        "kismet.device.base.macaddr": "aa:aa:aa:00:00:01",
        "kismet.device.base.phyname": "Bluetooth",
        "kismet.device.base.type": "BTLE",
        "kismet.device.base.signal": signal,
        "bluetooth.device": {
            "bluetooth.device.manuf_company_id": text.get(..4).unwrap_or(text),
            "bluetooth.device.manuf_data": text,
            "bluetooth.device.service_uuid_strs": text.split(',').collect::<Vec<_>>(),
            "bluetooth.device.service_data_bytes": {text: text}
        }
    }));
    check(&json!({
        "kismet.device.base.macaddr": "cc:cc:cc:00:00:01",
        "kismet.device.base.phyname": "IEEE802.11",
        "kismet.device.base.type": "Wi-Fi AP",
        "kismet.device.base.frequency": number.unsigned_abs(),
        "kismet.device.base.crypt": text,
        "kismet.device.base.signal": signal,
        "dot11.device": {
            "dot11.device.last_bssid": text,
            "dot11.device.last_beaconed_ssid_record": {
                "dot11.advertisedssid.ssid": text,
                "dot11.advertisedssid.cloaked": number % 3,
                "dot11.advertisedssid.crypt_string": text
            }
        }
    }));
    check(&json!({
        "kismet.device.base.macaddr": "dd:dd:dd:00:00:01",
        "kismet.device.base.phyname": "IEEE802.11",
        "kismet.device.base.type": "Wi-Fi Client",
        "kismet.device.base.frequency": number,
        "dot11.device": {"dot11.device.last_bssid": text}
    }));
});
