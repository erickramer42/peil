# Changelog

Each release's changes, newest first. The release workflow takes a release's notes from its section here, and a release tag needs the section dated.

A release is tagged `vMAJOR.MINOR.PATCH`, the version in `Cargo.toml` and the app's. A patch release changes no verdict or description: every saved feed replays as it did on the release before. A rule's logic or a description table changes only in a minor release, with `RULE_SET_VERSION` or `DESCRIBE_VERSION` bumped, and older feeds still replay.

The APK is signed with one key; each release's notes and `apksigner verify --print-certs` show its certificate's SHA-256 fingerprint, which `android/release-cert.sha256` pins.

## 0.2.0 - Unreleased

### Added

- Tile's tracker format in the describe layer: service data under one of Tile's UUIDs names the Tile network, and the app shows it as a tracker.
- A Find My advert's device, from its status byte: an AirTag, another maker's Find My accessory, AirPods or another Apple device, as the category entry (`apple-find-my-airtag` and the rest); the app shows it after the network ("Find My AirTag"). AirPods are categorized `audio` and other Apple devices `apple-device`, no longer `tracker`; all stay on the Find My network.
- Samsung's SmartTag format: service data under 0xFD5A whose first byte, masked with 0xF8, is 0x10 names the SmartThings Find network (`smartthings_find`, `samsung-smarttag`); the app shows "SmartThings SmartTag".
- Flock Safety's equipment as the category `flock-class`, meaning consistent with it: an access point named `Flock-` and six hex digits, `Flock`, `Flock Camera net.` or `FS Ext Battery`, a Wi-Fi address in the block b4:1e:52, or BLE manufacturer data under company ID 0x09C8 (XUNTONG's, seen on the battery pack). The entry that matched says which. The app's screen-off BLE scan also keeps that company ID.
- Vendor names with no file to supply: the core carries a snapshot of the IEEE's MA-L, MA-M and MA-S lists (read 2026-11-09), so the app shows each emitter's vendor block and `peil live` and `peil describe` name it without `--vendors`, which takes a newer list instead. `packaging/vendors.py` refreshes the snapshot and records its date and each list's SHA-256 beside it.
- The laptop's own radio addresses over the link: `peil link` reads its wireless interfaces and Bluetooth controllers from `/sys` (and takes more with `--own wifi:ADDRESS` or `--own bluetooth:ADDRESS`) and sends them in its hello; the app offers them in the Mine list, unchecked, and shows the label the core gives those emitters. They label and never leave a device out of the verdicts.
- Coverage shows a BLE scan that reads on but gives fewer results than a full scan: each radio's results and devices over the last minute (`last_minute`), and for BLE `low_rate` (under 100 results and under 30 a device, or none, through a minute it read on) with its time low (`low_ns`). Android can make a running scan opportunistic without telling the app; the app's coverage row reads "BLE low rate".
- A stopped session is told from a cut one: the app ends a session's feed with an `end` line when you stop it, and `peil live` ends its stream `stopped` when the feed has one and `cut` when it doesn't (a killed app, a dead phone, a cut file). The stream's `input_closed` reason is gone; every feed saved before this release replays as `cut`.
- The `own` label: an emitter whose address a receiver reported as its own, such as the laptop's over the link, carries `own` with that receiver's name in the picture (an `own` feed line, the latest per receiver). It names an address, not an identity, and changes no verdict.
- Axon Enterprise's equipment as the category `axon-class` (`axon-oui`): a BLE advert from an address in the block 00:25:df, which Axon's body cameras, conducted energy weapons, holster sensors and vehicle units share, so the category names the maker, not the kind. Equipment that rotates its address doesn't match, and the app's screen-off scan, which filters by advert content, doesn't keep these adverts. `DESCRIBE_VERSION` is 5.
- `peil summary <feed>`: what a saved session came to, as text, JSON (`sensio.summary/1`) or an HTML page with the route drawn from the session's own fixes and nothing fetched: stopped or cut, times, distance and stops, each radio's lost, filtered and low time, the emitters by kind, and each verdict with its device and the stops it was heard at. The app shows a session's summary when you stop it, and any saved feed's under Settings, Summaries.
- The phone's heading: with the screen on, the app reads the rotation-vector sensor four times a second (a `rot` raw line, a `heading` event with azimuth, pitch, roll and the sensor's accuracy estimate), and each fix carries its GNSS course (`course_deg`) when Android reports one. Find mode will use them; no verdict changes.

### Changed

- `persistence.places` counts stops, not points along the road: a place is a stop of at least 2 minutes (`min_stop_ns`, `peil judge --min-stop-s`), and a device is flagged when it is heard during stops at two places 500 m apart, at least 10 minutes from its first sighting to its last. A device heard along a whole drive collected a place every 500 m. Each place gives the stop's start and latest fix (`stopped_from_ns`, `stopped_to_ns`). `RULE_SET_VERSION` is 2.
- Movement is measured over a 10 s window (`moving_window_ns`): moving when the newest fix at least 10 s before the latest is at least 2 m/s times the time between them away. Speed between consecutive fixes read a parked phone's GNSS jitter as moving and restarted the parked clock; driving off reads moving after about 20 m, and a stop reads moving for up to 10 s. Movement is unknown until the track spans the window.
- The ring places each dot by its strongest recent signal on one scale, -40 dBm at the center to -100 at the edge, with circles at -60 and -80; the outer ring for emitters with no strength is drawn only when one is there. Most emitters on a drive are below -80 dBm, and the scale gives them more than half the ring's area.

### Fixed

- Kismet's `Wi-Fi WDS AP` devices (an access point linking to another, which Kismet types so over `Wi-Fi AP`) are access points, and its `Wi-Fi WDS` devices (addresses carried across that link) give no event, as bridged ones don't; both were rejected lines.
- The BLE scan restarts every 4 minutes, so Android's 5-minute scan timeout no longer turns it opportunistic and cuts the lines a minute.
- The Mine list offers every recognized tracker whatever its strength, then the 40 loudest others, so a weak tag in a wallet can be marked without holding it to the phone.
- The app's screen-off BLE scan keeps Tile's third UUID, 0xFD84, which the describe layer matches; a Tile advertising under it went unheard with the screen off.

## 0.1.0 - 2026-11-08

The emitter picture around a vehicle or a building, live on the phone, with a laptop running Kismet when one is connected.

### Added

- The Android app: a session reads the phone's Wi-Fi scans, BLE scans and GNSS fixes, keeps every raw line in a saved feed on the phone (1, 7 or 30 days, or deleted all at once), and draws the emitters as a ring by signal-strength band and kind, the loudest listed beside it, with the ambient texture and each radio's coverage under it. A tap on a dot shows its details and lists the emitters closest to it in signal strength. Three color themes: Dark, Low glare and Night. Bluetooth Classic discovery and the laptop link are settings, off by default. Emitters marked as your own leave the ring.
- `peil live`: the live feed in, the emitter picture and verdicts out (`sensio.feed/1`, `sensio.verdicts/1`); a saved feed replays to the same bytes. Memory grows with the emitters seen, not the sightings.
- `peil describe`: each emitter's kind, signal band, sightings per receiver, vendor block (from the IEEE registry you supply), tracker network (Apple Find My, Google's Find Hub network, DULT), an access point's security, and a Bluetooth Classic device's category.
- `peil judge` with one rule, `persistence.places`: a device seen at several distinct places over a span of time.
- `peil link`: Kismet's devices from a laptop to the app over USB tethering, paired once with a code, with a systemd unit.
- The sources: the app's raw lines, Kismet's device records, and normalized events.

### Downloads

- `peil-<version>-x86_64-linux.tar.gz` and `peil-<version>-aarch64-linux.tar.gz`: the `peil` command, built static (musl), with the licenses and the notices of the crates linked into it.
- `peil-<version>.apk`: the app, for Android 12 or later on arm64. It's signed with the release key; a debug build installed from source has a different key, so it has to be uninstalled first, which deletes its saved feeds.
