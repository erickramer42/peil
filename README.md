# Peil

Peil lists what's transmitting around a vehicle or a building, the loudest first, and tells you whether any of it is unusual or staying with you. It reads what other programs already receive: an Android phone's own Wi-Fi, Bluetooth and GNSS scans, and Kismet on a laptop. It keeps metadata only (addresses, advertisements, signal strength), never anyone's communications. It's an early scaffold.

_*Peil* is Norwegian for a bearing; *radiopeiling* is radio direction finding._

## Engineering highlights

- Peil reads its GPL sensor programs, such as Kismet, across a process boundary and never links them; CI's `deny` job holds every crate to a license allowlist ([ADR-0001](docs/adr/0001-scope-and-sensor-boundary.md)).
- The judge is a pure function of the events and the rule set's version, and every verdict lists its evidence; `drive_verdicts_match_golden` and `verdicts_do_not_depend_on_input_order` hold it.
- The app keeps each source's raw lines, and replaying a saved feed gives the bytes the screen showed ([ADR-0002](docs/adr/0002-recordings-and-session-clock.md), [ADR-0005](docs/adr/0005-live-feed-and-verdict-stream.md)).
- Every field from a radio is hostile input: validated, rejected without quoting its value, with `unsafe` forbidden and fuzz targets run in CI.
- One Rust core runs on Linux and, built for Android, as a process inside the app that it speaks to in JSON lines ([ADR-0004](docs/adr/0004-android-app.md)).

## What it does

- See what's transmitting: each emitter's kind, signal band (strength, never distance), vendor, tracker network and Wi-Fi security, loudest first ([ADR-0006](docs/adr/0006-emitter-picture-and-describe-layer.md)).
  - `peil describe` prints the picture; the app draws it as a ring and a list.
  - The ambient texture: emitters of each kind in the last minute, the new ones, randomized Wi-Fi.
- Judge whether a device stays with you: the rule `persistence.places` flags a device heard during the vehicle's stops at distinct places. `peil judge` runs it over a file of events.
- Run live, replay later: `peil live` reads the feed the app writes and answers with the picture, coverage and verdicts; a saved feed replays to the same bytes.
  - `peil summary` tells what a saved session came to: how it ended, its coverage, stops and route, and each verdict with its evidence.
- Use the phone app on Android 12 or later: it reads Wi-Fi, BLE, GNSS and opt-in Bluetooth Classic, and keeps each session's raw lines on the phone.
  - Tap a dot for its details; mark your own devices to hide them.
  - Coverage shows each radio's state, its time lost or filtered, and what isn't watched; the [guide](docs/guide.md) has the rest.
- Add a laptop's Kismet: `peil link` sends Kismet's devices, Wi-Fi stations included, to the app over USB tethering after pairing once ([guide](docs/guide.md#the-laptop-link)).
- Install a release: `peil` for Linux on x86_64 and aarch64, and the signed APK; [CHANGELOG.md](CHANGELOG.md) has the changes.

## Status

An early scaffold: its tests and CI run on synthetic input only. Not built yet: find mode's camera view, the HUD, aircraft and drones on the app's ring (the picture describes them), the gpsd adapter, and Kismet's event stream and packets. Nor are linking a BLE device across address rotations, fusing sources into one device, the bridge to vehicle data, or hardware categories beyond those built (see ADR-0006). rtl_433 is unscheduled. Tire-pressure sensors are not pursued. Each [ADR](docs/adr/README.md)'s Status line says what's built.

## How it fits together

```
sensor tier (separate programs)            Peil
-------------------------------            ----------------------------------------------
the phone's scans  Wi-Fi, BLE, GNSS  --.
Kismet             REST              --+-->  sources -> the feed (raw lines, one clock)
gpsd               JSON (next)       --'                     |
                                                             v  live, or replayed
                                           normalized events -> picture, evidence -> verdicts
                                                                                       |
                                                                      the app, the peil command
```

Peil talks to the sensor tier only across a process boundary and links none of its code ([ADR-0001](docs/adr/0001-scope-and-sensor-boundary.md)). The feed keeps each source's raw lines; normalizing happens on replay, so a better rule runs over every past session ([ADR-0002](docs/adr/0002-recordings-and-session-clock.md)).

## Build and test

Rust 1.85 or newer. These run on the synthetic fixtures, so no radio is needed:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p sensio-cli -- describe crates/sensio-core/tests/fixtures/drive_wifi.jsonl
cargo run -p sensio-cli -- judge crates/sensio-core/tests/fixtures/drive.jsonl
cargo run -p sensio-cli -- live crates/sensio-core/tests/fixtures/drive.feed
cargo run -p sensio-cli -- summary crates/sensio-core/tests/fixtures/drive.feed
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

Golden verdicts are [insta](https://insta.rs) snapshots. After a deliberate rule change, review them with `cargo insta review` and bump `RULE_SET_VERSION`.

## Event format

One JSON object per line ([ADR-0002](docs/adr/0002-recordings-and-session-clock.md)): `t_ns` on the session clock, the `source` program, and the `receiver` that saw it, so a phone and a laptop in the same car are told apart.

```json
{"t_ns":1800000000000000000,"source":"android","receiver":"phone","kind":"gnss_fix","lat_deg":60.0,"lon_deg":10.0,"course_deg":90.0}
{"t_ns":1800000001000000000,"source":"kismet","receiver":"laptop","kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":-58}
{"t_ns":1800000002000000000,"source":"android","receiver":"phone","kind":"wifi_ap","bssid":"cc:cc:cc:00:00:01","ssid":"example-hotspot","freq_mhz":2437,"rssi_dbm":-61}
{"t_ns":1800000003000000000,"source":"kismet","receiver":"laptop","kind":"wifi_client","addr":"dd:dd:dd:00:00:01","bssid":"cc:cc:cc:00:00:01","freq_mhz":5180,"rssi_dbm":-70}
{"t_ns":1800000004000000000,"source":"kismet","receiver":"laptop","kind":"bt_classic","addr":"ee:ee:ee:00:00:01","rssi_dbm":-66,"device_class":2098180}
{"t_ns":1800000005000000000,"source":"android","receiver":"phone","kind":"heading","azimuth_deg":271.5,"pitch_deg":-12.0,"roll_deg":3.25,"accuracy_deg":11.0}
```

A fix's `course_deg` is its direction of travel, when the receiver gave one; a `heading` is which way the receiver faces, from its orientation sensor, and is never a sighting. A Wi-Fi station carries its address and the network it's on, never what it sent or the network names it asks for; a device's name is never kept, since it's often a person's. A rejected line is reported by line, column and reason, never by its value.

## Live feed

`peil live` reads `sensio.feed/1`, the file the app saves, and answers with `sensio.verdicts/1`: the picture, coverage and verdicts for each request in it ([ADR-0005](docs/adr/0005-live-feed-and-verdict-stream.md)). The stream ends `stopped` when the feed closes with an `end` line, and `cut` when it doesn't.

```json
{"type":"start","schema":"sensio.feed/1","app":"fixture","boot_ns":1000000000}
{"type":"clock","boot_ns":1000000000,"utc_ns":1799999999000000000,"basis":"gnss"}
{"type":"line","seq":1,"boot_ns":2000000000,"source":"sensio","raw":"{\"kind\":\"gnss_fix\",\"lat_deg\":60.0,\"lon_deg\":10.0}"}
{"type":"judge","boot_ns":3000000000}
{"type":"end","boot_ns":4000000000}
```

## Use and privacy

Peil is for checking what's transmitting nearby for unusual RF activity. Receiving radio signals is legal in most places; decoding the contents of other people's communications often isn't, and Peil doesn't try. Where Peil transmits, such as an active BLE scan or a request to a vehicle's own ECUs, it does so only through certified hardware used within its certification. It never interferes with, disrupts or impersonates other devices: no jamming, no deauthentication, no spoofing. Its recordings hold other people's device identifiers and your own movements, so treat them as personal data: keep them local, and don't publish raw recordings.

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
