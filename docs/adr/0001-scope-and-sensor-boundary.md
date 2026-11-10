# ADR-0001: Scope, the process boundary to the sensor tier, and the language

Status: Accepted

## Context

Detecting Wi-Fi and BLE devices, logging them and decoding sub-GHz transmissions is solved. Kismet captures Wi-Fi and BLE (including through dedicated sniffers), keeps device records, and serves them over a REST API, an event stream and a pcapng packet stream. rtl_433 decodes hundreds of sub-GHz devices, tire-pressure sensors among them, to JSON. gpsd serves position and time. Rebuilding any of them would cost years and give a worse result.

What none of them answers is the question Peil exists for: what is unusual in this place, or traveling with this vehicle, right now, and should the person using Peil care? The place is a vehicle (moving or parked) or a building (a home, an office, a hotel room). Peil is an anomaly detector for the emitters and the sensor context around it. Rules about devices (persistence across places, co-travel, a newcomer that stays overnight) are one family of anomaly evidence inside that question, not the whole of it. Answering it takes:

- linking a BLE device across its address rotations,
- fusing sources (a BLE device and four TPMS sensors moving together are one car),
- a common clock across sources,
- recordings that can be replayed through newer rules,
- verdicts that are deterministic and explain themselves,
- context: for a vehicle, whether it's moving or parked and its ignition; for a building, a baseline of what normally belongs there,
- and a display fit for a car or a walk through a building.

Amended 2026-11-07: the question is two, the first leading: what is transmitting near me right now, and is any of it unusual or staying with me, and should I care? Descriptions answer the first (ADR-0006): every emitter nearby, the loud ones first, named by lookup and measurement. The judge's verdicts answer the second.
## Decision

1. **Peil captures nothing itself.** Kismet, rtl_433 and gpsd are the sensor tier. Peil reads their output as a separate process: HTTP and websockets for Kismet, a pipe for rtl_433, a TCP socket for gpsd.

   Amended 2026-11-06: on an Android phone the sensor tier is the operating system itself; the app reads Android's scan results and sensor readings and captures nothing below those interfaces (ADR-0004 section 3). Havne's `havne-cli`, for the vehicle's own data, is read the same way as the other programs: its JSON lines through a pipe.

   Amended 2026-11-09: tire-pressure sensors are not pursued. rtl_433 stays the sub-GHz program Peil would read across the boundary, with no device class scheduled; the TPMS examples in the context and in section 6 date from before this.
2. **Peil links none of their code.** The boundary is a process boundary, so Peil's own code stays under MIT while Kismet and rtl_433 stay under the GPL. Peil doesn't ship them; a user installs them. If a future package bundles them, it meets their licenses for those programs (source offer, license texts) and records that in THIRD_PARTY_NOTICES.md.

   Amended 2026-11-07: Peil's own code is under MIT or Apache-2.0, at the recipient's option, as is the convention for Rust crates; Apache-2.0 adds an express patent license from each contributor. The MIT option keeps Peil usable wherever MIT alone was, and the process boundary to the GPL programs is unchanged.
3. **Peil's own parts** are the source adapters, recordings and replay, address-rotation linking, fusion, the judge, and its front ends: the CLI first, then a HUD.

   Amended 2026-11-06: the first HUD is the Android app (ADR-0004), which runs the same core as the CLI.

   Amended 2026-11-07: and the describe layer, which turns events into one description per emitter beside the judge (ADR-0006).
4. **The judge is deterministic.** Detectors produce evidence, never verdicts. The judge is a pure function of a session's events and a versioned rule set (`RULE_SET_VERSION`), and each verdict lists the evidence that produced it. No learned model or language model sits in the verdict path.
5. **Peil is defensive.** It doesn't decode the contents of anyone's communications. It may transmit where that helps answer the question, for example an active BLE scan or querying a vehicle's own ECUs over the OBD port, but only through certified hardware used within its certification. It never transmits to interfere with, disrupt or impersonate other devices: no jamming, no deauthentication, no spoofing, and no transmitting from a general-purpose SDR outside a license the user holds.
6. **Peil is written in Rust.** Because the sensor tier does the capture, the language is chosen for Peil's own work, not for driver or DSP libraries:
   - **Hostile input.** Every field that came from a radio was sent by a stranger. Rust is memory-safe without a garbage collector, and it has no null.
   - **A growing event schema.** Event kinds (BLE adverts, TPMS, GNSS fixes, and later CAN samples and bearings) are enums with data. An exhaustive `match` makes a new kind a compile error everywhere it isn't handled yet.
   - **Determinism.** Data races are rejected at compile time, and with no garbage collector, timing is predictable.
   - **Deployment.** One static binary, cross-compiled to arm64 for a small computer in the car or on site.
   - **Testing tools:** snapshot tests for golden verdicts (insta), property tests (proptest), fuzzing (cargo-fuzz).

   The alternatives weighed:
   - **Go** deploys as easily and is quicker to learn, but has no sum types for the event schema. Its `select` picks at random among ready channels, so a naive merge orders events differently from run to run. Calling a C ABI through cgo is costly.
   - **C++20** would reuse Havne's tooling directly, but it isn't memory-safe on hostile input; fuzzing and sanitizers narrow that gap without closing it.
   - **Java** is memory-safe and has good sealed types, but needs a JVM on the edge, and garbage collection and JIT warm-up make timing less predictable.

## Consequences

- Peil depends on the sensor tier being installed and running; the source adapters must handle it being absent, restarted or slow.
- Address-rotation linking needs advert timing that a plain host Bluetooth scan blurs, so it will want Kismet fed by a dedicated BLE sniffer, read through its pcapng stream rather than its device records.
- Rules can change freely, because recorded sessions replay through them and golden verdicts show what changed.
- Rules differ by setting. In a moving vehicle the signal is a device that follows it across places (`persistence.places`). In a building, or a parked vehicle, it's a device that's new against the place's baseline, or one that stays through a night when nobody is there.
- Linux is the primary platform: a small computer in the car or on site, with a GNSS receiver that has a PPS output for the session clock (ADR-0002). In a building GNSS reception may be poor; the clock then holds from its last sync, and the recording notes that. Amended 2026-11-06: Android is a primary platform beside Linux, with the core running as a process inside the app (ADR-0004).
- The workspace forbids `unsafe` code. Amended 2026-11-06: it has no planned exception. A crate binding Havne's C ABI was the one planned; Havne's C ABI may change with each 0.x release while `havne-cli`'s JSON lines (schema 1) only grow, so Peil reads `havne-cli` through a pipe instead.
- The minimum Rust version is 1.85 (the 2024 edition), and CI checks it. The HUD's toolkit is decided when the HUD is built. Amended 2026-11-06: for the Android app, Android's platform APIs (ADR-0004 section 8).
