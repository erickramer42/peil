# Using Peil

How to use the app, the laptop link and the `peil` command day to day. The decisions behind them are in the [ADRs](adr/README.md); the formats are in [ADR-0002](adr/0002-recordings-and-session-clock.md), [ADR-0005](adr/0005-live-feed-and-verdict-stream.md) and [ADR-0006](adr/0006-emitter-picture-and-describe-layer.md).

## The Android app

The app needs Android 12 or later on an arm64 phone ([ADR-0004](adr/0004-android-app.md)).

**A session.** Start runs a session as a foreground service: it reads the phone's Wi-Fi scans, BLE scans and GNSS fixes, and runs the core as a process inside the app. The screen stays on while a session runs, which keeps BLE's full scan going. A session asks for location, nearby devices and notifications when you first start one; if Android has stopped asking after two refusals, the app says what's missing and offers to open Android's settings for it.

**The ring.** One dot per emitter heard in the last minute. How far out it sits is its strongest signal in the last minute, from -40 dBm at the center to -100 at the edge, with circles at -60 and -80, the band edges: strength, never distance. An emitter with no strength sits on a dashed rim outside -100, drawn only when one is there. Its sector is its kind, never a direction; a kind with nothing in it gets no slice. A tracker has a halo. The loudest emitters are listed below the ring, with the ambient texture: emitters of each kind in the last minute, the new ones, randomized Wi-Fi, and the newest few.

- A tap on a dot shows what's known about it (kind, signal, vendor, tracker network, how long it's been heard) and lists the other emitters closest to it in signal strength. Equal strength isn't equal distance: transmit power and what's in the way differ by tens of dB.
- A tap away from the dots clears the selection, or with none, makes the ring smaller or larger.
- **Mine** lists the emitters heard, strongest first, for you to mark your own; the first time, the strong ones are checked for you, never a tracker. The app leaves yours off the ring and the list.

**Coverage,** under the status: each radio (Wi-Fi, BLE, GNSS, Classic, and a linked laptop's) with its state, how long since it last gave anything, its time filtered (BLE with the screen off) or lost (off, or a scan refused), BLE's low rate (on, but under 100 results in the last minute and under 30 a device, as a scan Android quietly downgraded gives), and what isn't watched (Wi-Fi stations without the laptop, sub-GHz, cellular). The app writes each radio's state changes into the session's feed, so a replay shows the same coverage. If the core stops, the app restarts it with the session's saved feed and counts the lines it missed.

**Settings:**

- **Classic:** Bluetooth Classic discovery, off by default, since it transmits inquiries. Turning it on asks for the permission that tells Classic devices from BLE ones.
- **Laptop:** the laptop link, off by default (below).
- **Keep:** how long saved feeds stay on the phone, 1, 7 or 30 days. **Delete all** deletes every saved feed at once.
- **Theme:** Dark, Low glare (amber) or Night (red). All three keep text at a contrast of at least 4.5:1, and Low glare and Night keep every dot dimmer than the text.
- **About:** Peil's licenses and the notices of the libraries and crates in the app.

**Saved feeds** hold every raw line a session received, with its arrival time: other people's device identifiers and your movements. They stay in the app's private storage, out of backups, until their retention ends or you delete them.

## The laptop link

A laptop running Kismet adds what the phone can't see, such as Wi-Fi stations and the access points they're on ([ADR-0004](adr/0004-android-app.md) section 6).

1. Bind Kismet's web server to localhost (`httpd_bind_address=127.0.0.1` in its configuration: it listens on every interface by default) and make it a read-only API key.
2. Give `peil link` the key: `PEIL_KISMET_API_KEY`, or the first line of `peil/kismet-api-key` in your configuration directory. `--kismet` takes Kismet's address if it isn't `http://127.0.0.1:2501`.
3. On the phone, turn on USB tethering, then Settings, Laptop. With "Default USB configuration" set to USB tethering in Android's developer options, plugging the phone in turns tethering on.
4. Pair once with the code the app shows: `peil link --code <code>`. `--receiver` names the laptop in the session's events (`laptop` unless you say otherwise). On Linux, `peil link` finds the phone as the tether's gateway; elsewhere, `--phone` gives its address.

The pairing is kept (`peil/link-pairing.json` in your configuration directory), so later runs need no arguments and the link can run as a service: [contrib/systemd/peil-link.service](../contrib/systemd/peil-link.service). The app listens on the USB tether only and takes one laptop at a time. Three wrong codes close pairing until the link is turned off and on; turning it off can also forget the paired laptop.

`peil link` polls Kismet's device list and sends each device seen again, cut down to the fields the core reads. It answers the phone's pings with its clock, so the phone places the laptop's times on its own, and it reconnects when the link drops. The laptop's devices join the picture under its receiver name.

With readsb on the laptop (writing its aircraft list with `--write-json`), `peil link` sends its aircraft too, read from `/run/readsb/aircraft.json` unless `--readsb` names another path; `--no-readsb` sends none. Aircraft aren't in the picture yet ([ADR-0007](adr/0007-aircraft-and-drones.md)).

Kismet hears the laptop's own radios too, so `peil link` tells the phone their addresses: each wireless interface and Bluetooth controller it finds in `/sys` on Linux, plus any you give with `--own wifi:ADDRESS` or `--own bluetooth:ADDRESS` (repeatable; for an adapter `/sys` doesn't show). The Mine list offers them first, labeled as the laptop's own and unchecked; check them to take the laptop off the ring and the list. The verdicts still include them, since a device copying the laptop's address would carry the label too.

## The `peil` command

- `peil describe <events.jsonl>`: the emitter picture over a file of normalized events, loud first. Vendor names come from a snapshot of the IEEE's MA-L, MA-M and MA-S lists built into the core (its date and sources in `crates/sensio-core/data/ieee-vendors.sources.txt`); `--vendors` takes a newer copy of those lists (CSV, one or several concatenated) instead, here and on `peil live`.
- `peil judge <events.jsonl>`: one verdict per line.
- `peil live [feed]`: reads a feed from a file or stdin and answers each `judge` line in it. A saved feed from the phone replays to the same output the app showed, and its last line says whether the session was stopped or cut (the app killed, or the phone dead). Lines the app saved but couldn't pass to the core live are marked withheld; `--all` judges them too.
- `peil summary <feed>`: what a saved session came to: whether it was stopped or cut, its times, distance and stops (2 minutes or more), each radio's time lost, filtered or low, the emitters heard by kind, and each verdict with what's known about its device and the stops it was heard at. No sighting is listed. `--format json` gives it as one object (`sensio.summary/1`), and `--format html` as a page with the route drawn from the session's own GNSS fixes, with no map tiles and nothing fetched. The page holds the session's coordinates, so keep it where you keep the feed.

The feed's sources are `sensio` (a normalized event as a raw line), `android` (the app's raw lines: Wi-Fi scans with their security, BLE adverts parsed into manufacturer data, service data and service UUIDs, location fixes) and `kismet` (one device record per line: access points, stations with the access point they were last on, BLE devices, and Bluetooth Classic devices with their Class of Device). Device names, the network names a station probes for and WPS device names are never kept.
