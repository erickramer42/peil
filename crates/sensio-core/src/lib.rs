//! The core: the normalized event schema, the deterministic judge, and the
//! live judge that reads a feed.
//!
//! Sources produce [`event::Event`]s; the judge turns a session's events into
//! [`judge::Verdict`]s. The judge is a pure function of the events and a
//! versioned rule set, so the same input always gives the same verdicts
//! (ADR-0001, ADR-0002). The live judge ([`live::Live`]) answers each request
//! in a feed with the judge over the events before it (ADR-0005).

pub mod describe;
pub mod event;
pub mod feed;
pub mod geo;
pub mod judge;
pub mod live;
mod movement;
mod rate;

pub use describe::{
    DESCRIBE_VERSION, DescribeParams, OwnAddresses, Picture, VendorTable, describe,
};
pub use event::{DeviceId, Event, EventKind, ParseError, TimeNs, parse_jsonl};
pub use judge::{PersistenceRule, Place, RULE_SET_VERSION, Verdict, judge};
pub use live::{Fatal, LATENESS_NS, Live, Normalize, RunError, run};
