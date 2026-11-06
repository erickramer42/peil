# Peil

Anomaly detection for the emitters and sensor context around a vehicle or a building: what is unusual in this place, or traveling with this vehicle, right now, and should you care?

It takes what established sensors already see (Kismet for Wi-Fi and BLE, rtl_433 for sub-GHz devices such as tire-pressure sensors, gpsd for position), puts it on one clock, and judges it with versioned, deterministic rules. Every verdict carries the evidence behind it, and every recording can be replayed to give the same verdicts again.

*Peil* is Norwegian for a bearing; *radiopeiling* is radio direction finding.

## Status

An early scaffold. What works today:

- The normalized event schema (BLE adverts, TPMS transmissions, GNSS fixes) as JSON lines, with input validation whose errors never repeat the input's values.
- The judge with one rule, `persistence.places`: a device seen at several distinct places over a span of time. It's the first of a family of anomaly evidence ([ADR-0001](docs/adr/0001-scope-and-sensor-boundary.md)); its parameters are checked before it runs.
- `peil judge <events.jsonl>`: one verdict per line, as JSON.
- Golden verdicts for a synthetic drive, and a property test that verdicts don't depend on input order.

Not built yet: the source adapters, recordings and replay, linking a BLE device across address rotations, fusing sources into one device, the HUD, and the bridge to vehicle data. The [ADRs](docs/adr/README.md) describe the plan.

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
cargo deny check     # licenses, bans, sources, advisories (from cargo install cargo-deny)
```

Golden verdicts are [insta](https://insta.rs) snapshots. After a deliberate rule change, review the new ones with `cargo insta review` (from `cargo install cargo-insta`) and bump `RULE_SET_VERSION`.

## Event format

One JSON object per line. `t_ns` is nanoseconds since the Unix epoch (UTC) on the session clock.

```json
{"t_ns":1800000000000000000,"source":"gpsd","kind":"gnss_fix","lat_deg":60.0,"lon_deg":10.0}
{"t_ns":1800000001000000000,"source":"kismet","kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":-58}
{"t_ns":1800000004000000000,"source":"rtl_433","kind":"tpms","sensor_id":"0a1b2c3d","rssi_dbm":-12.5}
```

A line that can't be used is rejected with its line, its column and what's wrong (`line 3, column 110: invalid type: string (value left out), expected i16`), never with the value itself: those are other people's identifiers.

## Use and privacy

Peil is for checking nearby objects for abnormal RF, signal or other EM activity. Receiving radio signals is legal in most places; decoding the contents of other people's communications often isn't, and Peil doesn't try. Where Peil transmits, such as an active BLE scan or a request to a vehicle's own ECUs, it does so only through certified hardware used within its certification, and never to interfere with, disrupt or impersonate other devices: no jamming, no deauthentication, no spoofing. Its recordings hold other people's device identifiers, and your own movements or whereabouts, so treat them as personal data: keep them local, and don't publish raw recordings.

## License

Licensed under [MIT](LICENSE-MIT). Third-party programs Peil works with keep their own licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
