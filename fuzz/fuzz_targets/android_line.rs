//! The `android` source's normalizer, two ways: the input as a raw line, and
//! the input's bytes as a BLE advert's record under a synthetic address. A
//! record the normalizer accepts must give an event the core accepts: the
//! normalizer never produces what `Event::validate` rejects.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sensio_core::{Event, Normalize};
use sensio_sources::Sources;

fuzz_target!(|data: &[u8]| {
    if let Ok(raw) = std::str::from_utf8(data) {
        let _ = Sources.normalize("android", raw);
    }

    let record: String = data.iter().map(|b| format!("{b:02x}")).collect();
    let raw = format!(r#"{{"t":"ble","addr":"aa:aa:aa:00:00:01","rssi":-60,"record":"{record}"}}"#);
    let Ok(kinds) = Sources.normalize("android", &raw) else {
        return;
    };
    for kind in kinds {
        let event = Event {
            t_ns: 1_800_000_000_000_000_000,
            source: "android".to_owned(),
            receiver: None,
            kind,
        };
        if let Err(reason) = event.validate() {
            panic!("the core rejects a normalized advert ({reason}): {raw}");
        }
    }
});
