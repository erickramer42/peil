//! A radio's results over the last minute of arrivals, and whether a BLE
//! scan reading `on` gives too few of them (ADR-0005 as amended 2026-11-09).
//!
//! Android can turn a running BLE scan opportunistic after a timeout of its
//! own (5 minutes on the test phone, 30 in AOSP) without telling the app: the
//! radio still reads `on` while its results fall to what other apps' scans
//! bring. A full scan hears each device tens of times a minute, parked or
//! driving, so a minute with under 100 results and under 30 a device is a
//! scan short of full, or one with nothing to hear; coverage says which
//! minutes those were rather than guessing why.
//!
//! Counted by the lines' arrival on the boot clock, in whole seconds, so a
//! replay counts as live did.

use std::collections::{BTreeMap, VecDeque};

use crate::event::{DeviceId, EventKind, TimeNs};
use crate::feed::{RadioCoverage, RadioRate, RadioState};

const NS_PER_S: i64 = 1_000_000_000;
/// The window, in whole seconds of arrival.
const WINDOW_S: i64 = 60;
/// Under this many results in the window...
const LOW_RESULTS: u64 = 100;
/// ...and under this many a device, a BLE radio reading `on` is low.
const LOW_PER_DEVICE: u64 = 30;
/// The most devices the window holds; more count as this many.
const MAX_DEVICES: usize = 4096;

/// The radio an event kind comes from, as the app's `radio` lines name it.
pub(crate) fn radio_of(kind: &EventKind) -> Option<&'static str> {
    match kind {
        EventKind::BleAdvert { .. } => Some("ble"),
        EventKind::BtClassic { .. } => Some("bt_classic"),
        EventKind::WifiAp { .. } | EventKind::WifiClient { .. } => Some("wifi"),
        EventKind::GnssFix { .. } => Some("gnss"),
        EventKind::Heading { .. } | EventKind::Aircraft { .. } => None,
    }
}

/// Whether a radio gives events whose rate is counted.
pub(crate) fn counted(radio: &str) -> bool {
    matches!(radio, "ble" | "bt_classic" | "wifi" | "gnss")
}

/// One radio's results by second of arrival.
#[derive(Debug, Clone)]
pub(crate) struct Rate {
    /// Results by second, the window's seconds that had any, oldest first.
    seconds: VecDeque<(i64, u64)>,
    /// Each device's latest second in the window.
    devices: BTreeMap<DeviceId, i64>,
    /// The last whole second whose low state is counted.
    counted: i64,
    /// Time low, nanoseconds.
    low_ns: TimeNs,
}

impl Rate {
    /// A rate from the second of `boot_ns` on.
    pub(crate) fn new(boot_ns: TimeNs) -> Self {
        Self {
            seconds: VecDeque::new(),
            devices: BTreeMap::new(),
            counted: boot_ns.div_euclid(NS_PER_S) - 1,
            low_ns: 0,
        }
    }

    /// Counts each whole second before the one `boot_ns` falls in as low or
    /// not; `radio`'s state holds through them, since a state change counts
    /// up to itself first.
    pub(crate) fn advance(&mut self, boot_ns: TimeNs, radio: &RadioCoverage) {
        let last = boot_ns.div_euclid(NS_PER_S) - 1;
        let mut k = self.counted + 1;
        // Second by second while the window may still hold a result.
        let held = self.counted.saturating_add(WINDOW_S);
        while k <= last && k <= held {
            self.prune(k);
            let (results, devices) = self.counts(k);
            if low(radio, k, results, devices) {
                self.low_ns = self.low_ns.saturating_add(NS_PER_S);
            }
            k += 1;
        }
        if k <= last {
            // From here the window is empty: low wherever the radio reads
            // `on` through it.
            self.seconds.clear();
            self.devices.clear();
            if radio.radio == "ble" && radio.state == RadioState::On {
                let from = k.max(on_from(radio));
                if from <= last {
                    let seconds = last - from + 1;
                    self.low_ns = self.low_ns.saturating_add(seconds.saturating_mul(NS_PER_S));
                }
            }
        }
        self.counted = self.counted.max(last);
    }

    /// Adds a result arriving at `boot_ns`, after [`Rate::advance`] to it.
    pub(crate) fn record(&mut self, boot_ns: TimeNs, device: Option<DeviceId>) {
        // A line earlier than one already counted counts in the next second.
        let second = boot_ns.div_euclid(NS_PER_S).max(self.counted + 1);
        match self.seconds.back_mut() {
            Some((s, n)) if *s == second => *n += 1,
            _ => self.seconds.push_back((second, 1)),
        }
        if let Some(device) = device {
            if self.devices.len() < MAX_DEVICES || self.devices.contains_key(&device) {
                self.devices.insert(device, second);
            }
        }
    }

    /// The window ending with the second of `boot_ns`, and whether it is low.
    pub(crate) fn view(&self, boot_ns: TimeNs, radio: &RadioCoverage) -> (RadioRate, bool, TimeNs) {
        let k = boot_ns.div_euclid(NS_PER_S);
        let (results, devices) = self.counts(k);
        let rate = RadioRate { results, devices };
        (rate, low(radio, k, results, devices), self.low_ns)
    }

    /// Drops what is older than the window ending with second `k`.
    fn prune(&mut self, k: i64) {
        let oldest = k - WINDOW_S;
        while self.seconds.front().is_some_and(|&(s, _)| s <= oldest) {
            self.seconds.pop_front();
        }
        self.devices.retain(|_, s| *s > oldest);
    }

    /// Results and devices in the window ending with second `k`.
    fn counts(&self, k: i64) -> (u64, u64) {
        let oldest = k - WINDOW_S;
        let results = self
            .seconds
            .iter()
            .filter(|&&(s, _)| s > oldest && s <= k)
            .map(|&(_, n)| n)
            .sum();
        let devices = self
            .devices
            .values()
            .filter(|&&s| s > oldest && s <= k)
            .count() as u64;
        (results, devices)
    }
}

/// The first second whose window `radio` reads `on` through.
fn on_from(radio: &RadioCoverage) -> i64 {
    let since = radio.since_boot_ns.div_euclid(NS_PER_S)
        + i64::from(radio.since_boot_ns.rem_euclid(NS_PER_S) != 0);
    since.saturating_add(WINDOW_S - 1)
}

/// Whether the window ending with second `k` is low for `radio`.
fn low(radio: &RadioCoverage, k: i64, results: u64, devices: u64) -> bool {
    radio.radio == "ble"
        && radio.state == RadioState::On
        && k >= on_from(radio)
        && results < LOW_RESULTS
        && (results == 0 || results < LOW_PER_DEVICE.saturating_mul(devices))
}

#[cfg(test)]
mod tests {
    use super::{NS_PER_S, Rate};
    use crate::event::{DeviceId, EventKind};
    use crate::feed::{RadioCoverage, RadioState};

    fn ble(since_s: i64) -> RadioCoverage {
        RadioCoverage {
            radio: "ble".to_owned(),
            receiver: Some("phone".to_owned()),
            state: RadioState::On,
            since_boot_ns: since_s * NS_PER_S,
            filtered_ns: 0,
            lost_ns: 0,
            last_minute: None,
            low_rate: false,
            low_ns: 0,
        }
    }

    fn device(n: u64) -> Option<DeviceId> {
        let kind: EventKind = serde_json::from_str(&format!(
            r#"{{"kind":"ble_advert","addr":"aa:aa:aa:00:{:02x}:{:02x}"}}"#,
            n / 256 % 256,
            n % 256
        ))
        .expect("a BLE advert parses");
        kind.device()
    }

    /// Ten devices heard `per_device` times a second each from `from_s`
    /// for `seconds`, then the view at the end.
    fn run(rate: &mut Rate, radio: &RadioCoverage, from_s: i64, seconds: i64, per_s: u64) {
        for s in from_s..from_s + seconds {
            for n in 0..per_s {
                let at = s * NS_PER_S + i64::try_from(n).expect("small") * 1_000;
                rate.advance(at, radio);
                rate.record(at, device(n % 10));
            }
            rate.advance((s + 1) * NS_PER_S, radio);
        }
    }

    #[test]
    fn a_full_scan_is_not_low_and_a_quiet_one_is() {
        let radio = ble(0);
        let mut rate = Rate::new(0);
        // Ten devices, 2 results a second each: 1,200 a minute.
        run(&mut rate, &radio, 0, 120, 20);
        let (counts, low, low_ns) = rate.view(120 * NS_PER_S, &radio);
        assert!(!low && low_ns == 0, "{counts:?}");
        assert_eq!((counts.results, counts.devices), (1_180, 10));
        // Then one result every 4 s from the same devices: 15 a minute.
        for s in (120..240).step_by(4) {
            rate.advance(s * NS_PER_S, &radio);
            rate.record(s * NS_PER_S, device(u64::try_from(s).expect("small")));
        }
        rate.advance(240 * NS_PER_S, &radio);
        let (counts, low, low_ns) = rate.view(240 * NS_PER_S, &radio);
        assert!(low, "{counts:?}");
        assert!(low_ns >= 50 * NS_PER_S, "{low_ns}");
    }

    #[test]
    fn one_device_heard_fully_is_not_low() {
        let radio = ble(0);
        let mut rate = Rate::new(0);
        // One device, about 1.5 results a second: 90 a minute, as a full
        // scan hears a lone device.
        for s in 0..120 {
            for n in 0..=i64::from(s % 2 == 0) {
                rate.advance(s * NS_PER_S + n, &radio);
                rate.record(s * NS_PER_S + n, device(1));
            }
        }
        rate.advance(120 * NS_PER_S, &radio);
        let (counts, low, low_ns) = rate.view(120 * NS_PER_S, &radio);
        assert!(!low && low_ns == 0, "{counts:?} {low_ns}");
    }

    #[test]
    fn silence_through_a_gap_counts_as_low_once_the_window_has_passed() {
        let radio = ble(0);
        let mut rate = Rate::new(0);
        rate.advance(3_600 * NS_PER_S, &radio);
        // The first window the radio reads `on` through ends at second 59.
        assert_eq!(
            rate.view(3_600 * NS_PER_S, &radio).2,
            (3_600 - 59) * NS_PER_S
        );
    }

    #[test]
    fn a_radio_not_on_is_never_low() {
        let mut radio = ble(0);
        radio.state = RadioState::Filtered;
        let mut rate = Rate::new(0);
        rate.advance(600 * NS_PER_S, &radio);
        let (_, low, low_ns) = rate.view(600 * NS_PER_S, &radio);
        assert!(!low && low_ns == 0);
    }

    #[test]
    fn many_devices_keep_the_window_bounded() {
        let radio = ble(0);
        let mut rate = Rate::new(0);
        for n in 0..10_000u64 {
            rate.record(0, device(n));
        }
        assert_eq!(rate.devices.len(), super::MAX_DEVICES);
        rate.advance(NS_PER_S, &radio);
        assert_eq!(rate.view(NS_PER_S, &radio).0.results, 10_000);
    }
}
