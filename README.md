# Peil

_*Peil* is Norwegian for a bearing; *radiopeiling* is radio direction finding._

What's transmitting around a vehicle or a building, and is any of it unusual or staying with you? Peil describes every emitter nearby, the loud ones first, and judges them with versioned, deterministic rules.

It reads what established sensors already see (an Android phone's own scans and Kismet for Wi-Fi and Bluetooth today; rtl_433 for tire-pressure sensors and gpsd for position next) and puts it on one clock. It keeps metadata only (addresses, adverts, signal strength), never anyone's communications. It's an early scaffold.

## Engineering highlights

- **A process boundary to the sensor tier:** Kismet and rtl_433 are GPL programs that Peil reads but never links; CI's `deny` job keeps every crate on the allowlist ([ADR-0001](docs/adr/0001-scope-and-sensor-boundary.md)).
- **A deterministic judge:** a pure function of the events and the rule set's version, each verdict with its evidence; `drive_verdicts_match_golden` and `verdicts_do_not_depend_on_input_order` hold it.
- **Recordings that replay to the same bytes:** the app saves each source's raw lines, and replaying a saved feed gives what the screen showed ([ADR-0002](docs/adr/0002-recordings-and-session-clock.md), [ADR-0005](docs/adr/0005-live-feed-and-verdict-stream.md)).
- **Every radio field is hostile input:** validated, rejected without quoting the value, `unsafe` forbidden, and fuzz targets run in CI.
- **One core everywhere:** the same Rust core runs on Linux and, built for Android, as a process inside the app, spoken to in JSON lines ([ADR-0004](docs/adr/0004-android-app.md)).

## What it does

- **See what's transmitting:** every emitter, loudest first: its kind, signal band (strength, never distance), vendor, tracker network and Wi-Fi security ([ADR-0006](docs/adr/0006-emitter-picture-and-describe-layer.md)).
  - `peil describe` prints it; the app draws it as a ring and a list.
  - The ambient texture: emitters of each kind in the last minute, the new ones, randomized Wi-Fi.
- **Judge whether a device stays with you:** `peil judge` runs the first rule, `persistence.places`, a device heard during the vehicle's stops at distinct places over time.
- **Run live, replay later:** `peil live` reads the feed the app writes and answers with the picture, coverage and verdicts; a saved feed replays to the same bytes.
- **The phone app:** Android 12 or later; it reads the phone's Wi-Fi, BLE and GNSS and keeps each session's raw lines on the phone.
  - Tap a dot for its details and the emitters closest to it in signal strength; mark your own devices to hide them.
  - Coverage: each radio's state, its time lost or filtered, and what isn't watched.
  - Settings, themes and retention are in the [guide](docs/guide.md).
- **Add a laptop's Kismet:** `peil link` sends Kismet's devices to the app over USB tethering, Wi-Fi stations included, after pairing once with the app's code ([guide](docs/guide.md#the-laptop-link)).
- **Install a release:** each release ships `peil` for Linux on x86_64 and aarch64 and the signed APK; [CHANGELOG.md](CHANGELOG.md) has the changes.

## Status

An early scaffold: its tests and CI run on synthetic input only. Not built yet: find mode, the HUD, hardware categories other than the three built (trackers, Class of Device and Flock Safety's equipment), and Kismet's event stream and packets. Nor are the rtl_433 and gpsd adapters, linking a BLE device across address rotations, fusing sources into one device, or the bridge to vehicle data. Each [ADR](docs/adr/README.md)'s Status line says what's built.

## How it fits together

```
sensor tier (separate programs)          Peil
-------------------------------          ----------------------------------------------
Kismet   REST, events, pcapng  --.
rtl_433  JSON lines            --+-->  sources -> recording (raw, as received)
gpsd     JSON                  --'                   |
                                                     v  replay
                                            normalized events -> evidence -> verdicts
                                                                               |
                                                                      CLI, HUD, alerts
```

Peil talks to the sensor tier only across a process boundary and links none of its code ([ADR-0001](docs/adr/0001-scope-and-sensor-boundary.md)). Recordings keep each source's raw output; normalizing happens on replay, so a better rule can be run over every past session ([ADR-0002](docs/adr/0002-recordings-and-session-clock.md)).

## Build and test

Rust 1.85 or newer.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p sensio-cli -- judge crates/sensio-core/tests/fixtures/drive.jsonl
cargo run -p sensio-cli -- live crates/sensio-core/tests/fixtures/drive.feed
cargo run -p sensio-cli -- describe crates/sensio-core/tests/fixtures/drive_wifi.jsonl
cargo deny check     # licenses, bans, sources, advisories (from cargo install cargo-deny)
```

Fuzz targets for every parser of untrusted input are in `fuzz/`, a crate of its own that needs a nightly toolchain and [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz):

```sh
cd fuzz
cargo +nightly fuzz list
cargo +nightly fuzz run -a live_session -- -max_total_time=600
```

The Android app needs a JDK (17 or newer), the Android SDK with an NDK, and the Rust target `aarch64-linux-android`. `build-core.sh` builds the core and places it where the app packages it:

```sh
rustup target add aarch64-linux-android
export ANDROID_SDK_ROOT=<the SDK>
android/build-core.sh
cd android && ./gradlew assembleDebug testDebugUnitTest
```

Golden verdicts are [insta](https://insta.rs) snapshots. After a deliberate rule change, review the new ones with `cargo insta review` (from `cargo install cargo-insta`) and bump `RULE_SET_VERSION`.

## Event format

One JSON object per line ([ADR-0002](docs/adr/0002-recordings-and-session-clock.md)): `t_ns` on the session clock, the `source` program, and the `receiver` that saw it, so a phone and a laptop in the same car are told apart.

```json
{"t_ns":1800000000000000000,"source":"gpsd","kind":"gnss_fix","lat_deg":60.0,"lon_deg":10.0}
{"t_ns":1800000001000000000,"source":"kismet","receiver":"laptop","kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":-58}
{"t_ns":1800000002000000000,"source":"android","receiver":"phone","kind":"wifi_ap","bssid":"cc:cc:cc:00:00:01","ssid":"example-hotspot","freq_mhz":2437,"rssi_dbm":-61}
{"t_ns":1800000003000000000,"source":"kismet","receiver":"laptop","kind":"wifi_client","addr":"dd:dd:dd:00:00:01","bssid":"cc:cc:cc:00:00:01","freq_mhz":5180,"rssi_dbm":-70}
{"t_ns":1800000004000000000,"source":"rtl_433","kind":"tpms","sensor_id":"0a1b2c3d","rssi_dbm":-12.5}
{"t_ns":1800000005000000000,"source":"kismet","receiver":"laptop","kind":"bt_classic","addr":"ee:ee:ee:00:00:01","rssi_dbm":-66,"device_class":2098180}
```

A Wi-Fi station carries its address and the network it's on, never what it sent or the network names it asks for; a device's name is never kept, since it's often a person's. A rejected line is reported by line, column and reason, never by its value.

## Live feed

`peil live` reads `sensio.feed/1`, the file the app saves, and answers with `sensio.verdicts/1`: coverage and verdicts for each request in it ([ADR-0005](docs/adr/0005-live-feed-and-verdict-stream.md)).

```json
{"type":"start","schema":"sensio.feed/1","app":"fixture","boot_ns":1000000000}
{"type":"clock","boot_ns":1000000000,"utc_ns":1799999999000000000,"basis":"gnss"}
{"type":"line","seq":1,"boot_ns":2000000000,"source":"sensio","raw":"{\"kind\":\"gnss_fix\",\"lat_deg\":60.0,\"lon_deg\":10.0}"}
{"type":"judge","boot_ns":3000000000}
```

## Use and privacy

Peil is for checking nearby objects for abnormal RF, signal or other EM activity. Receiving radio signals is legal in most places; decoding the contents of other people's communications often isn't, and Peil doesn't try. Where Peil transmits, such as an active BLE scan or a request to a vehicle's own ECUs, it does so only through certified hardware used within its certification. It never interferes with, disrupts or impersonates other devices: no jamming, no deauthentication, no spoofing. Its recordings hold other people's device identifiers, and your own movements or whereabouts, so treat them as personal data: keep them local, and don't publish raw recordings.

## Documentation

| Doc | Contents |
|---|---|
| [docs/guide.md](docs/guide.md) | Using the app, the laptop link and the `peil` command |
| [docs/adr/](docs/adr/README.md) | The decisions, each with what's built |
| [CHANGELOG.md](CHANGELOG.md) | Each release's changes and the versioning rules |
| [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) | The programs Peil works with and the crates it links, with their licenses |
| [SECURITY.md](SECURITY.md) | Reporting a vulnerability, and what's in scope |

## License

Licensed under either of

- the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), or
- the MIT License ([LICENSE-MIT](LICENSE-MIT)),

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Peil by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

Third-party programs Peil works with keep their own licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
