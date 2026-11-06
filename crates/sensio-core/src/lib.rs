//! The core: the normalized event schema and the deterministic judge.
//!
//! Sources produce [`event::Event`]s; the judge turns a session's events into
//! [`judge::Verdict`]s. The judge is a pure function of the events and a
//! versioned rule set, so the same input always gives the same verdicts
//! (ADR-0001, ADR-0002).

pub mod event;
pub mod geo;
pub mod judge;

pub use event::{DeviceId, Event, EventKind, ParseError, TimeNs, parse_jsonl};
pub use judge::{PersistenceRule, Place, RULE_SET_VERSION, Verdict, judge};
