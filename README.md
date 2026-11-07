# Peil

What's transmitting around a vehicle or a building, and is any of it unusual or staying with you? Peil describes every emitter nearby, the loud ones first, and judges them with versioned, deterministic rules.

It takes what established sensors already see (Kismet for Wi-Fi and BLE, rtl_433 for sub-GHz devices such as tire-pressure sensors, gpsd for position) and puts it on one clock. Every verdict carries the evidence behind it, and every recording can be replayed to give the same verdicts again.

*Peil* is Norwegian for a bearing; *radiopeiling* is radio direction finding.

## Status

An early scaffold. What works today:

- The normalized event schema (BLE adverts, Bluetooth Classic devices, Wi-Fi access points and stations, TPMS transmissions, GNSS fixes, each with the receiver that saw it) as JSON lines, with input validation whose errors never repeat the input's values.
- The judge with one rule, `persistence.places`: a device seen at several distinct places over a span of time. It's the first of a family of anomaly evidence ([ADR-0001](docs/adr/0001-scope-and-sensor-boundary.md)); its parameters are checked before it runs.
- `peil judge <events.jsonl>`: one verdict per line, as JSON.
- `peil describe <events.jsonl>`: the emitter picture, one description per line, loud first: each emitter's kind, signal band (strength, never distance), sightings per receiver, vendor block from a registry you supply (`--vendors`, the IEEE's MA-L, MA-M and MA-S lists), the tracker network its advert's format names (Apple Find My, Google's Find Hub network, DULT), and an access point's security ([ADR-0006](docs/adr/0006-emitter-picture-and-describe-layer.md)). `peil live` writes the same picture as an `emitters` line before each answer's verdicts.
- `peil live [feed]`: reads a live feed from a file or stdin and answers each request in it with the verdicts over the events before it ([ADR-0005](docs/adr/0005-live-feed-and-verdict-stream.md)). A saved feed replays to the same bytes. Its sources so far are `sensio`, a normalized event carried as a raw line, `android`, the app's raw lines: Wi-Fi scan results with their security, BLE scan results with the advert's bytes parsed into manufacturer data, service data and service UUIDs (the name left out), and location fixes; and `kismet`, one Kismet device record per line: access points with the network they beacon, stations with the access point they were last on, and BLE devices with their advert contents Bluetooth Classic devices with their Class of Device (bridged wired devices give no event; probed network names and WPS device names are never kept).
- The Android app in `android/` ([ADR-0004](docs/adr/0004-android-app.md)), a first scaffold: a session runs as a foreground service reading the phone's Wi-Fi scans, BLE scans and GNSS fixes, writes every raw line to a saved feed on the phone (kept for 1, 7 or 30 days, or deleted all at once), and runs the core as a process inside the app, which draws the emitter picture as a ring: one dot per recent emitter, its ring the signal-strength band (strength, never distance), its sector its kind (never a direction; a kind with nothing in it gets no slice), trackers in red; the loudest emitters are listed beside it. The screen stays on while a session runs. Bluetooth Classic discovery is a setting, off by default, since it transmits inquiries; turning it on asks for the permission to tell Classic devices from BLE ones. You can mark emitters as your own (the strongest are offered first, trackers never pre-checked), and the app leaves them off the ring and the list. A core that stops is restarted with the session's saved feed, so its picture goes on, and the lines it missed are counted. CI builds the core for Android, the APK and its tests, and checks the APK's libraries, its manifest and the core's 16 KB alignment.
- Golden verdicts for a synthetic drive, as events and as a feed; property tests that verdicts don't depend on input order and that each live answer is the judge over the events before it.

Not built yet: hardware categories beyond trackers, the app's release signing and in-app license notices, the laptop link, find mode, the adapters that read Kismet, rtl_433 and gpsd, and the rtl_433 and gpsd normalizers, linking a BLE device across address rotations, fusing sources into one device, the HUD, and the bridge to vehicle data. The [ADRs](docs/adr/README.md) describe the plan.

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

Fuzz targets for the parsers of untrusted input (the event lines, the feed, the live judge, the app's lines, Kismet's device records) are in `fuzz/`, a crate of its own that needs a nightly toolchain and [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz):

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

One JSON object per line. `t_ns` is nanoseconds since the Unix epoch (UTC) on the session clock. `source` is the program that produced the line; the optional `receiver` names the radio that saw it, so a phone and a laptop in the same car can be told apart. The same address from two receivers is still one device.

```json
{"t_ns":1800000000000000000,"source":"gpsd","kind":"gnss_fix","lat_deg":60.0,"lon_deg":10.0}
{"t_ns":1800000001000000000,"source":"kismet","receiver":"thinkpad","kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":-58}
{"t_ns":1800000002000000000,"source":"android","receiver":"phone","kind":"wifi_ap","bssid":"cc:cc:cc:00:00:01","ssid":"example-hotspot","freq_mhz":2437,"rssi_dbm":-61}
{"t_ns":1800000003000000000,"source":"kismet","receiver":"thinkpad","kind":"wifi_client","addr":"dd:dd:dd:00:00:01","bssid":"cc:cc:cc:00:00:01","freq_mhz":5180,"rssi_dbm":-70}
{"t_ns":1800000004000000000,"source":"rtl_433","kind":"tpms","sensor_id":"0a1b2c3d","rssi_dbm":-12.5}
{"t_ns":1800000005000000000,"source":"kismet","receiver":"thinkpad","kind":"bt_classic","addr":"ee:ee:ee:00:00:01","rssi_dbm":-66,"device_class":2098180}
```

A Wi-Fi station carries its address and the network it's on, never what it sent or the network names it asks for. A BLE advert can carry its manufacturer data, service data and service UUIDs (hex, each entry at most 255 bytes), and an access point the security it announces (`{"protocol":"wpa2","enterprise":true}`); a Bluetooth Classic device carries its Class of Device (24 bits); an advertised or discovered device name is never kept, since it is often a person's name ([ADR-0006](docs/adr/0006-emitter-picture-and-describe-layer.md)).

A line that can't be used is rejected with its line, its column and what's wrong (`line 3, column 110: invalid type: string (value left out), expected i16`), never with the value itself: those are other people's identifiers.

## Live feed

`peil live` reads `sensio.feed/1`, the format the app writes both to the core and to a session's saved file, so a replay is the same command over the file ([ADR-0005](docs/adr/0005-live-feed-and-verdict-stream.md)). Each `line` keeps a source's output as received, with its arrival on the phone's boot clock; a `clock` line maps that clock to UTC, and a `judge` line asks for verdicts.

```json
{"type":"start","schema":"sensio.feed/1","app":"fixture","boot_ns":1000000000}
{"type":"clock","boot_ns":1000000000,"utc_ns":1799999999000000000,"basis":"gnss"}
{"type":"line","seq":1,"boot_ns":2000000000,"source":"sensio","raw":"{\"kind\":\"gnss_fix\",\"lat_deg\":60.0,\"lon_deg\":10.0}"}
{"type":"judge","boot_ns":3000000000}
```

It answers with `sensio.verdicts/1`: a `start` line, then for each `judge` a `coverage` line (lines seen, rejected, withheld and dropped, per source and receiver) and a `verdicts` line with the full verdict set, a `rejected` line for each line it couldn't use, and an `end`. Input that isn't a feed exits with code 2. Lines the app saved but couldn't pass to the core live are marked `withheld`; `peil live --all` judges them too.

## Use and privacy

Peil is for checking nearby objects for abnormal RF, signal or other EM activity. Receiving radio signals is legal in most places; decoding the contents of other people's communications often isn't, and Peil doesn't try. Where Peil transmits, such as an active BLE scan or a request to a vehicle's own ECUs, it does so only through certified hardware used within its certification, and never to interfere with, disrupt or impersonate other devices: no jamming, no deauthentication, no spoofing. Its recordings hold other people's device identifiers, and your own movements or whereabouts, so treat them as personal data: keep them local, and don't publish raw recordings.

## License

Licensed under [MIT](LICENSE-MIT). Third-party programs Peil works with keep their own licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
