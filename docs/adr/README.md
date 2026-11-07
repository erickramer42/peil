# Architecture decision records

Each record states a decision, why it was made, and what follows from it. Numbers are stable and never reused.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-scope-and-sensor-boundary.md) | Scope, the process boundary to the sensor tier, and Rust | Accepted |
| [0002](0002-recordings-and-session-clock.md) | Raw recordings, normalized events, and one session clock | Accepted; the event schema and the judge are built |
| [0003](0003-product-and-internal-names.md) | The product name only where a user sees it; `sensio` inside | Accepted |
| [0004](0004-android-app.md) | The Android app: Kotlin, the core as a process inside it, USB tethering to the laptop | Proposed |
