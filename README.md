# Peil

_*Peil* is Norwegian for a bearing; *radiopeiling* is radio direction finding._

What's transmitting around a vehicle or a building, and is any of it unusual or staying with you? Peil describes every emitter nearby, the loud ones first, and judges them with versioned, deterministic rules.

It takes what established sensors already see (Kismet for Wi-Fi and BLE, rtl_433 for sub-GHz devices such as tire-pressure sensors, gpsd for position) and puts it on one clock. Every verdict carries the evidence behind it, and every recording can be replayed to give the same verdicts again.

## Status

An early scaffold. What works today:

- The normalized event schema (BLE adverts, Bluetooth Classic devices, Wi-Fi access points and stations, TPMS transmissions, GNSS fixes, each with the receiver that saw it) as JSON lines, with input validation whose errors never repeat the input's values.
- The judge with one rule, `persistence.places`: a device seen at several distinct places over a span of time. It's the first of a family of anomaly evidence ([ADR-0001](docs/adr/0001-scope-and-sensor-boundary.md)); its parameters are checked before it runs.
- `peil judge <events.jsonl>`: one verdict per line, as JSON.
- `peil describe <events.jsonl>`: the emitter picture, one description per line, loud first: each emitter's kind, signal band (strength, never distance), sightings per receiver, vendor block from a registry you supply (`--vendors`, the IEEE's MA-L, MA-M and MA-S lists), the tracker network its advert's format names (Apple Find My, Google's Find Hub network, DULT), an access point's security, and a Bluetooth Classic device's category from its Class of Device (car audio, hands-free, phone, camera-class and so on) ([ADR-0006](docs/adr/0006-emitter-picture-and-describe-layer.md)). `peil live` writes the same picture as an `emitters` line before each answer's verdicts, with the ambient texture: how many emitters of each kind were seen in the last minute, how many of them are new, how many Wi-Fi addresses are randomized, and the newest few.
- `peil live [feed]`: reads a live feed from a file or stdin and answers each request in it with the verdicts over the events before it ([ADR-0005](docs/adr/0005-live-feed-and-verdict-stream.md)). A saved feed replays to the same bytes. It holds the last 10 minutes of events one by one and folds older ones into each emitter's state, so its memory grows with the emitters seen, not the sightings; an event that arrives more than 10 minutes behind the latest takes that edge as its time and is counted as late. Its sources so far are `sensio`, a normalized event carried as a raw line, `android`, the app's raw lines: Wi-Fi scan results with their security, BLE scan results with the advert's bytes parsed into manufacturer data, service data and service UUIDs (the name left out), and location fixes; and `kismet`, one Kismet device record per line: access points with the network they beacon, stations with the access point they were last on, BLE devices with their advert contents, and Bluetooth Classic devices with their Class of Device (bridged wired devices give no event; probed network names and WPS device names are never kept).
- `peil link`: the laptop's side of the link to the app over USB tethering ([ADR-0004](docs/adr/0004-android-app.md) section 6). It polls Kismet's REST API on the same computer (`--kismet`; the API key from `PEIL_KISMET_API_KEY` or `peil/kismet-api-key` in your configuration directory), finds the phone as the tether's gateway on Linux (or takes `--phone`), and sends each device Kismet has seen again, cut down to the fields the `kismet` normalizer reads, as `sensio.link/1` JSON lines over TCP; it answers the phone's pings with its clock, so the phone can place the laptop's times on its own, and reconnects when the link drops. Pair once with `peil link --code <code>`, the code the app shows; the pairing is kept (`peil/link-pairing.json`), and later runs need no arguments, so it can run as a service ([contrib/systemd/peil-link.service](contrib/systemd/peil-link.service)). Kismet's web server listens on every interface by default: bind it to localhost (`httpd_bind_address=127.0.0.1`) and give `peil link` a read-only API key.
- The Android app in `android/` ([ADR-0004](docs/adr/0004-android-app.md)), a first scaffold: a session runs as a foreground service reading the phone's Wi-Fi scans, BLE scans and GNSS fixes, writes every raw line to a saved feed on the phone (kept for 1, 7 or 30 days, or deleted all at once), and runs the core as a process inside the app, which draws the emitter picture as a ring: one dot per recent emitter, its ring the signal-strength band (strength, never distance), its sector its kind (never a direction; a kind with nothing in it gets no slice), trackers in red; the loudest emitters are listed beside it, and under it the ambient texture (emitters of each kind in the last minute, the new ones, the newest few). A tap on a dot shows what's known about that emitter and lists the others closest to it in signal strength first (strength, not location); a tap elsewhere on the ring makes it smaller or larger. The settings are behind one button, with three color themes: Dark, Low glare (amber) and Night (red), each keeping its text at a contrast of at least 4.5:1, and a tracker marked by a halo as well as its color. A permission refused twice, after which Android stops asking, is explained with a way into Android's settings for the app. The screen stays on while a session runs. The laptop link is a setting, off by default: turned on, the app listens on the USB tether only and takes one laptop at a time. A laptop pairs once with the code the app shows; after that it links whenever the phone is on its tether, with no one at the screen. Three wrong codes close pairing until the link is turned off and on, and turning it off can also forget the paired laptop. The laptop's Kismet devices join the picture under its receiver name. With "Default USB configuration" set to USB tethering in Android's developer options, plugging the phone in turns tethering on. Bluetooth Classic discovery is a setting, off by default, since it transmits inquiries; turning it on asks for the permission to tell Classic devices from BLE ones. You can mark emitters as your own (the strongest are offered first, trackers never pre-checked), and the app leaves them off the ring and the list. A core that stops is restarted with the session's saved feed, so its picture goes on, and the lines it missed are counted. Coverage is shown under the ring: each radio (Wi-Fi, BLE, GNSS, Classic, and a linked laptop's) with its state, how long since it last gave anything, and its time filtered (BLE with the screen off) or lost (off, or a scan refused), and what isn't watched. The app writes each radio's state changes into the feed, so a replay shows the same coverage. CI builds the core for Android, the APK and its tests, and checks the APK's libraries, its manifest and the core's 16 KB alignment.
- Releases: a version tag runs CI, builds `peil` for Linux on x86_64 and aarch64 and the signed APK, and drafts a GitHub release with them ([Releases](#releases)).
- Golden verdicts for a synthetic drive, as events and as a feed; property tests that verdicts don't depend on input order and that each live answer is the judge over the events before it.

Not built yet: hardware categories beyond trackers and Class of Device, find mode, Kismet's event stream and packets (`peil link` polls its device list), the adapters that read rtl_433 and gpsd, and the rtl_433 and gpsd normalizers, linking a BLE device across address rotations, fusing sources into one device, the HUD, and the bridge to vehicle data. The [ADRs](docs/adr/README.md) describe the plan.

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

## Releases

A release is tagged `vMAJOR.MINOR.PATCH`, the version in `Cargo.toml` and the app's, with its changes in [CHANGELOG.md](CHANGELOG.md). It ships `peil` for Linux on x86_64 and aarch64, linked statically so it runs on any distribution, and the APK, each with the notices of the crates linked into it. A patch release changes no verdict or description: every saved feed replays as it did on the release before. A rule's logic or a description table changes only in a minor release, with `RULE_SET_VERSION` or `DESCRIBE_VERSION` bumped, and replay of older feeds keeps working.

The APK is signed with one key; the release notes and `apksigner verify --print-certs` show its certificate's SHA-256 fingerprint. A debug build from source is signed with another key, so Android refuses to update one with the other: uninstall first, which deletes the app's saved feeds.

## Event format

One JSON object per line. `t_ns` is nanoseconds since the Unix epoch (UTC) on the session clock. `source` is the program that produced the line; the optional `receiver` names the radio that saw it, so a phone and a laptop in the same car can be told apart. The same address from two receivers is still one device.

```json
{"t_ns":1800000000000000000,"source":"gpsd","kind":"gnss_fix","lat_deg":60.0,"lon_deg":10.0}
{"t_ns":1800000001000000000,"source":"kismet","receiver":"laptop","kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":-58}
{"t_ns":1800000002000000000,"source":"android","receiver":"phone","kind":"wifi_ap","bssid":"cc:cc:cc:00:00:01","ssid":"example-hotspot","freq_mhz":2437,"rssi_dbm":-61}
{"t_ns":1800000003000000000,"source":"kismet","receiver":"laptop","kind":"wifi_client","addr":"dd:dd:dd:00:00:01","bssid":"cc:cc:cc:00:00:01","freq_mhz":5180,"rssi_dbm":-70}
{"t_ns":1800000004000000000,"source":"rtl_433","kind":"tpms","sensor_id":"0a1b2c3d","rssi_dbm":-12.5}
{"t_ns":1800000005000000000,"source":"kismet","receiver":"laptop","kind":"bt_classic","addr":"ee:ee:ee:00:00:01","rssi_dbm":-66,"device_class":2098180}
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

It answers with `sensio.verdicts/1`: a `start` line, then for each `judge` a `coverage` line (lines seen, rejected, withheld, dropped and late, and the latest arrival of each event kind, per source and receiver; each radio's state and its time filtered or lost, from the feed's `radio` lines) and a `verdicts` line with the full verdict set, a `rejected` line for each line it couldn't use, and an `end`. Input that isn't a feed exits with code 2. Lines the app saved but couldn't pass to the core live are marked `withheld`; `peil live --all` judges them too.

## Use and privacy

Peil is for checking nearby objects for abnormal RF, signal or other EM activity. Receiving radio signals is legal in most places; decoding the contents of other people's communications often isn't, and Peil doesn't try. Where Peil transmits, such as an active BLE scan or a request to a vehicle's own ECUs, it does so only through certified hardware used within its certification, and never to interfere with, disrupt or impersonate other devices: no jamming, no deauthentication, no spoofing. Its recordings hold other people's device identifiers, and your own movements or whereabouts, so treat them as personal data: keep them local, and don't publish raw recordings.

## License

Licensed under either of

- the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), or
- the MIT License ([LICENSE-MIT](LICENSE-MIT)),

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Peil by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

Third-party programs Peil works with keep their own licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
