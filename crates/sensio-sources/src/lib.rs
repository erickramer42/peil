//! Adapters from the sensor tier to the event schema.
//!
//! Each adapter reads one program's own output across a process boundary
//! (ADR-0001): Kismet's REST API, event stream and pcapng packet stream,
//! `rtl_433`'s JSON lines, gpsd's JSON. None links their code, which keeps
//! this crate's license independent of theirs.
//!
//! Normalizing is separate from receiving (ADR-0002, ADR-0005): a feed line
//! keeps a source's raw text, and [`Sources`] turns it into events by the
//! source's name, live and on replay alike.

use sensio_core::{EventKind, Normalize};

pub mod sensio;

/// The normalizers, chosen by a feed line's `source`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sources;

impl Normalize for Sources {
    fn normalize(&self, source: &str, raw: &str) -> Result<Vec<EventKind>, String> {
        match source {
            sensio::SOURCE => sensio::normalize(raw),
            _ => Err("no normalizer for this source".to_owned()),
        }
    }
}
