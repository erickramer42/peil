//! `read_bounded_line` with a small limit and a small buffer, so lines
//! cross the limit and the reader's refills. It must split the input at
//! each newline exactly as `split` does, giving a line over the limit as
//! `TooLong` and the last one without a newline as unterminated.

#![no_main]

use std::io::BufReader;

use libfuzzer_sys::fuzz_target;
use sensio_core::feed::{BoundedLine, read_bounded_line};

fuzz_target!(|data: &[u8]| {
    let [max, capacity, input @ ..] = data else {
        return;
    };
    let max = usize::from(*max % 64);
    let capacity = 1 + usize::from(*capacity % 16);

    let mut expected = Vec::new();
    if !input.is_empty() {
        let segments: Vec<&[u8]> = input.split(|&b| b == b'\n').collect();
        let terminated = input.ends_with(b"\n");
        let count = if terminated {
            segments.len() - 1
        } else {
            segments.len()
        };
        for (i, segment) in segments[..count].iter().enumerate() {
            expected.push(if segment.len() > max {
                BoundedLine::TooLong
            } else {
                BoundedLine::Text {
                    bytes: segment.to_vec(),
                    terminated: terminated || i + 1 < count,
                }
            });
        }
    }

    let mut reader = BufReader::with_capacity(capacity, input);
    let mut got = Vec::new();
    while let Some(line) = read_bounded_line(&mut reader, max).expect("a slice reads") {
        got.push(line);
        assert!(got.len() <= expected.len(), "more lines than the input has");
    }
    assert_eq!(got, expected);
});
