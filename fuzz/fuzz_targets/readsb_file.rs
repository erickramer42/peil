//! readsb's aircraft list as the laptop reads it (`readsb_file::Reader`) and
//! the `readsb` normalizer: any text never panics, every line handed on is an
//! aircraft object with an address and the list's `now`, the same list read
//! again a second later hands on nothing new, and an event the normalizer
//! gives, from a line handed on or from the input itself, is one the core
//! accepts.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use sensio_core::{Event, Normalize};
use sensio_sources::Sources;
use sensio_sources::readsb_file::Reader;
use serde_json::Value;

fn accepted(raw: &str) {
    let Ok(kinds) = Sources.normalize("readsb", raw) else {
        return;
    };
    for kind in kinds {
        let event = Event {
            t_ns: CLOCK_NS,
            source: "readsb".to_owned(),
            receiver: Some("laptop".to_owned()),
            kind,
        };
        if let Err(reason) = event.validate() {
            panic!("the core rejects a normalized aircraft ({reason}): {raw}");
        }
    }
}

const CLOCK_NS: i64 = 1_800_000_000_000_000_000;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        accepted(text);
    }
    let mut reader = Reader::new(Path::new("unused"));
    let Ok(lines) = reader.take_body(data, CLOCK_NS) else {
        return;
    };
    for line in &lines {
        let Ok(Value::Object(object)) = serde_json::from_str::<Value>(line) else {
            panic!("a line that isn't an object: {line}");
        };
        assert!(object.get("hex").and_then(Value::as_str).is_some(), "{line}");
        assert!(object.get("now").and_then(Value::as_f64).is_some(), "{line}");
        accepted(line);
    }
    let again = reader.take_body(data, CLOCK_NS + 1_000_000_000);
    assert_eq!(again, Ok(Vec::new()), "the same list read twice");
});
