//! The laptop's side of `sensio.link/1`, on what the phone and the system
//! hand it: the input as a link line, as `/proc/net/route`'s text for
//! `tether_gateway`, and as a pairing's secret and a challenge's nonce for
//! `proof`. A line that parses writes back to a line that parses to the
//! same value; a proof comes only from a secret of hex digits, and is 64
//! lowercase hex digits.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sensio_sources::link::{LinkLine, Pairing, proof, tether_gateway};

fuzz_target!(|data: &[u8]| {
    if let Ok(line) = serde_json::from_slice::<LinkLine>(data) {
        let text = serde_json::to_string(&line).expect("a link line writes");
        let back: LinkLine = serde_json::from_str(&text)
            .unwrap_or_else(|err| panic!("a written link line doesn't parse ({err}): {text}"));
        assert_eq!(line, back, "a link line changed on its round trip: {text}");
    }

    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = tether_gateway(text);

    let (secret, nonce) = text.split_once('\n').unwrap_or((text, ""));
    let pairing = Pairing {
        receiver: "laptop".to_owned(),
        secret: secret.to_owned(),
    };
    if let Some(mac) = proof(&pairing, nonce) {
        assert!(
            secret.bytes().all(|b| b.is_ascii_hexdigit()),
            "a secret that isn't hex gave a proof: {secret:?}"
        );
        assert!(
            mac.len() == 64 && mac.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "a proof isn't 64 lowercase hex digits: {mac}"
        );
    }
});
