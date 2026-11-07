//! What the normalizers share about Wi-Fi.

use sensio_core::event::{WifiProtocol, WifiSecurity};

/// The strongest protocol a security string names, and whether it's
/// enterprise (802.1X, `EAP`). It reads Android's capabilities string
/// (`[WPA2-PSK-CCMP][RSN-PSK+SAE-CCMP][ESS]`) and Kismet's crypt string
/// alike. A WPA2/WPA3 transition network reads as WPA3.
pub fn security(s: &str) -> WifiSecurity {
    let s = s.to_ascii_uppercase();
    let protocol = if s.contains("SAE") || s.contains("WPA3") || s.contains("SUITE_B") {
        WifiProtocol::Wpa3
    } else if s.contains("OWE") {
        WifiProtocol::Owe
    } else if s.contains("RSN") || s.contains("WPA2") {
        WifiProtocol::Wpa2
    } else if s.contains("WPA") {
        WifiProtocol::Wpa
    } else if s.contains("WEP") {
        WifiProtocol::Wep
    } else {
        WifiProtocol::Open
    };
    WifiSecurity {
        protocol,
        enterprise: s.contains("EAP"),
    }
}

/// A network name the event schema keeps: 1 to 32 bytes.
pub fn ssid(s: Option<String>) -> Option<String> {
    s.filter(|s| (1..=32).contains(&s.len()))
}

/// A signal strength in the event schema's range, in dBm.
pub fn dbm<T: Into<i64> + Copy>(level: Option<T>) -> Option<i16> {
    level
        .map(Into::into)
        .filter(|l| (-127..=20).contains(l))
        .and_then(|l| i16::try_from(l).ok())
}
