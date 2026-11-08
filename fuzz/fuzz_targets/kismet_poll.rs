//! The Kismet poller's handling of an answer's body (`Poller::take_body`),
//! with no server: the input as a body, then as each record of a list.
//! Every record handed on is a JSON object that normalizes as the record
//! it was pruned from does, and the same body taken again hands on nothing,
//! since every device in it was handed on at its last time.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sensio_core::Normalize;
use sensio_sources::Sources;
use sensio_sources::kismet_poll::Poller;
use serde_json::Value;

fn check(body: &[u8]) {
    let mut poller = Poller::new("http://127.0.0.1:9", None, 10);
    let Ok(lines) = poller.take_body(body) else {
        return;
    };
    for line in &lines {
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|err| panic!("a record handed on isn't JSON ({err}): {line}"));
        assert!(value.is_object(), "a record handed on isn't an object: {line}");
    }
    let again = poller.take_body(body).expect("the same body parsed once");
    assert!(again.is_empty(), "the same body handed on {} records again", again.len());
}

fuzz_target!(|data: &[u8]| {
    check(data);
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The input as the one record of a list, so the pruning and the
    // deduplication see it whatever its shape.
    if let Ok(record) = serde_json::from_str::<Value>(text) {
        let body = Value::Array(vec![record.clone(), record.clone()]).to_string();
        check(body.as_bytes());
        if record.is_object() {
            let mut poller = Poller::new("http://127.0.0.1:9", None, 10);
            if let Ok(lines) = poller.take_body(format!("[{text}]").as_bytes())
                && let Some(pruned) = lines.first()
            {
                assert_eq!(
                    Sources.normalize("kismet", text).ok(),
                    Sources.normalize("kismet", pruned).ok(),
                    "the record handed on normalizes differently: {text}"
                );
            }
        }
    }
});
