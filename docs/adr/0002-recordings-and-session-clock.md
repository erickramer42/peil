# ADR-0002: Raw recordings, normalized events, and one session clock

Status: Accepted. Built: the normalized event schema and the judge's ordering. Not yet built: recordings, replay, clock checks, identifier hashing, redaction.

## Context

Rules will keep changing, and each change should be checked against every session recorded so far. If a recording kept only Peil's normalized events, a rule could only use what an older normalizer chose to keep. Sources also stamp time differently, and fusing them, or linking a BLE address to the next one it rotates to, depends on their clocks agreeing to well under a second.

## Decision

1. **A recording keeps each source's raw output as received:** Kismet's pcapng and event JSON, rtl_433's JSON lines, gpsd's JSON, each with the session time it arrived. A manifest names the sources, their versions and the session's start. Amended 2026-11-06: on the phone, the raw lines are the app's records of Android's scan results and sensor readings, written from the first session with arrival and judging times; the recorder completes them with the manifest (ADR-0004 sections 4 and 7). Raw files are the source of truth; anything derived from them, such as an index, can be rebuilt.
2. **Normalized events are derived on replay, never stored as the recording.** They are JSON lines (`crates/sensio-core/src/event.rs`), validated on read, because every field that came from a radio is untrusted.
3. **One session clock:** the system clock, disciplined to GNSS time through its PPS output (chrony with gpsd). Times are nanoseconds since the Unix epoch, UTC. A recording also notes the clock's sync state, so a session with a poorly synced clock can be flagged rather than trusted. Amended 2026-11-06: a phone exposes no PPS, so on the phone the session clock is `CLOCK_BOOTTIME`, which counts sleep, mapped to UTC through GNSS time while there's a fix and through the system clock otherwise, with time asleep recorded as lost coverage; a laptop's times are mapped onto it by an offset measured over the link (ADR-0004 sections 5 and 6).
4. **The judge orders events by time,** GNSS fixes before sightings at the same instant, then input order, so verdicts don't depend on the order sources were merged in. A property test holds it to that.
5. **Personal data:** recordings hold other people's device identifiers and the user's movements.
   - Anything exported from a recording (a verdict shared, a sample attached to an issue) hashes identifiers with a per-install salt.
   - A home zone can be set; positions inside it are redacted on export.
   - Recordings have a retention limit the user sets.

## Consequences

- Recordings are larger than normalized events would be; the raw sources are compressed.
- Every rule change can be checked against all recorded sessions before it ships.
- Fixtures for tests are normalized events (`crates/sensio-core/tests/fixtures/`), synthetic and free of real identifiers or places; a raw recording is never committed.
