//! Adapters from the sensor tier to the event schema.
//!
//! Each adapter reads one program's own output across a process boundary
//! (ADR-0001): Kismet's REST API, event stream and pcapng packet stream,
//! `rtl_433`'s JSON lines, gpsd's JSON. None links their code, which keeps
//! this crate's license independent of theirs.
//!
//! The adapters arrive with the first slice: Kismet's BLE stream and gpsd.
