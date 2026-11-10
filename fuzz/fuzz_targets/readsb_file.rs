//! readsb's aircraft list as the laptop reads it (`readsb_file::Reader`):
//! any text never panics, every line handed on is an aircraft object with
//! an address and the list's `now`, and the same list read again a second
//! later hands on nothing new.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use sensio_sources::readsb_file::Reader;
use serde_json::Value;

const CLOCK_NS: i64 = 1_800_000_000_000_000_000;

fuzz_target!(|data: &[u8]| {
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
    }
    let again = reader.take_body(data, CLOCK_NS + 1_000_000_000);
    assert_eq!(again, Ok(Vec::new()), "the same list read twice");
});
