# Architecture decision records

Each record states a decision, why it was made, and what follows from it. Numbers are stable and never reused.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-scope-and-sensor-boundary.md) | Scope, the process boundary to the sensor tier, and Rust | Accepted; amended by 0004, 0006 and 0007 |
| [0002](0002-recordings-and-session-clock.md) | Raw recordings, normalized events, and one session clock | Accepted; the event schema and the judge are built; amended by 0004 |
| [0003](0003-product-and-internal-names.md) | The product name only where a user sees it; `sensio` inside | Accepted |
| [0004](0004-android-app.md) | The Android app: Kotlin, the core as a process inside it, USB tethering to the laptop | Accepted; the normalizer for the app's lines and a first app are built |
| [0005](0005-live-feed-and-verdict-stream.md) | The live feed and the verdict stream: one format for the pipe and the saved lines, so replay equals live | Accepted; the formats and `peil live` are built; amended by 0006 and 0007 |
| [0006](0006-emitter-picture-and-describe-layer.md) | The emitter picture: one description per emitter by lookup and measurement, loud first, deterministic and versioned beside the judge | Accepted; the describe step and the `emitters` line are built; amended by 0007 |
| [0007](0007-aircraft-and-drones.md) | Aircraft and drones: readsb's ADS-B across the process boundary as `aircraft` events in the picture, the FAA registry without people's names, Remote ID as an advert format | Accepted; not yet built |
