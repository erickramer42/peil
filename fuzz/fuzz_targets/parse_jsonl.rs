//! `parse_jsonl` over any bytes. Beyond not panicking: what it accepts, the
//! core writes and reads back unchanged; a rejection names a line the input
//! has, and a data error never quotes a value.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sensio_core::{ParseError, parse_jsonl};

fuzz_target!(|data: &[u8]| {
    match parse_jsonl(data) {
        Ok(events) => {
            let mut text = Vec::new();
            for event in &events {
                serde_json::to_writer(&mut text, event).expect("an event serializes");
                text.push(b'\n');
            }
            let again = parse_jsonl(&text[..]).expect("what the core writes, it reads");
            assert_eq!(again, events, "a round trip changed the events");
        }
        Err(err) => {
            let lines = data.split(|&b| b == b'\n').count();
            let line = match &err {
                ParseError::Io { line, .. }
                | ParseError::Json { line, .. }
                | ParseError::Invalid { line, .. } => *line,
            };
            assert!((1..=lines).contains(&line), "line {line} of {lines}: {err}");
            if let ParseError::Json { reason, .. } = &err {
                // Values are quoted with '"'; the schema's own names with '`'.
                assert!(!reason.contains('"'), "a value leaked: {reason}");
            }
        }
    }
});
