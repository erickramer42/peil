# Changelog

Each release's changes, newest first. The release workflow takes a release's notes from its section here, and a release tag needs the section dated.

A release is tagged `vMAJOR.MINOR.PATCH`, the version in `Cargo.toml` and the app's. A patch release changes no verdict or description: every saved feed replays as it did on the release before. A rule's logic or a description table changes only in a minor release, with `RULE_SET_VERSION` or `DESCRIBE_VERSION` bumped, and older feeds still replay.

The APK is signed with one key; each release's notes and `apksigner verify --print-certs` show its certificate's SHA-256 fingerprint, which `android/release-cert.sha256` pins.

## 0.2.0 - Unreleased

### Added

- Tile's tracker format in the describe layer: service data under one of Tile's UUIDs names the Tile network, and the app shows it as a tracker. `DESCRIBE_VERSION` is 2.
- A Find My advert's device, from its status byte: an AirTag, another maker's Find My accessory, AirPods or another Apple device, as the category entry (`apple-find-my-airtag` and the rest); the app shows it after the network ("Find My AirTag"). AirPods are categorized `audio` and other Apple devices `apple-device`, no longer `tracker`; all stay on the Find My network.
- Samsung's SmartTag format: service data under 0xFD5A whose first byte, masked with 0xF8, is 0x10 names the SmartThings Find network (`smartthings_find`, `samsung-smarttag`); the app shows "SmartThings SmartTag".
- Flock Safety's equipment as the category `flock-class`, meaning consistent with it: an access point named `Flock-` and six hex digits, `Flock`, `Flock Camera net.` or `FS Ext Battery`, a Wi-Fi address in the block b4:1e:52, or BLE manufacturer data under company ID 0x09C8 (XUNTONG's, seen on the battery pack). The entry that matched says which. The app's screen-off BLE scan also keeps that company ID.

- Vendor names with no file to supply: the core carries a snapshot of the IEEE's MA-L, MA-M and MA-S lists (read 2026-11-09), so the app shows each emitter's vendor block and `peil live` and `peil describe` name it without `--vendors`, which takes a newer list instead. `packaging/vendors.py` refreshes the snapshot and records its date and each list's SHA-256 beside it.

### Changed

- The ring places each dot by its strongest recent signal on one scale, -40 dBm at the center to -100 at the edge, with circles at -60 and -80; the outer ring for emitters with no strength is drawn only when one is there. Most emitters on a drive are below -80 dBm, and the scale gives them more than half the ring's area.

### Fixed

- Kismet's `Wi-Fi WDS AP` devices (an access point linking to another, which Kismet types so over `Wi-Fi AP`) are access points, and its `Wi-Fi WDS` devices (addresses carried across that link) give no event, as bridged ones don't; both were rejected lines.
- The BLE scan restarts every 4 minutes, so Android's 5-minute scan timeout no longer turns it opportunistic and cuts the lines a minute.
- The Mine list offers every recognized tracker whatever its strength, then the 40 loudest others, so a weak tag in a wallet can be marked without holding it to the phone.

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
