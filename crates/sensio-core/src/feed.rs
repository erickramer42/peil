//! The live feed, `sensio.feed/1`: what the app writes to the core's stdin and
//! to a session's saved raw lines, and the verdict stream, `sensio.verdicts/1`,
//! the core writes back (ADR-0005).
//!
//! One format serves the pipe and the saved file, so a replay is the same
//! command over the file. Every line is a JSON object with a `type`. Lines
//! the app writes are still untrusted: a saved file can be edited, and a
//! source's `raw` text came from a radio.

use std::collections::BTreeMap;
use std::io::{self, BufRead};

use serde::{Deserialize, Serialize};

use crate::describe::Picture;
use crate::event::TimeNs;
use crate::judge::Verdict;

/// The feed's schema, named on its first line.
pub const FEED_SCHEMA: &str = "sensio.feed/1";

/// The verdict stream's schema, named on its first line.
pub const VERDICTS_SCHEMA: &str = "sensio.verdicts/1";

/// The longest feed line accepted, in bytes; a longer one is rejected without
/// being held in memory.
pub const MAX_LINE_BYTES: usize = 1 << 20;

/// What a boot-clock reading's UTC time came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockBasis {
    /// GNSS time, while there's a fix.
    Gnss,
    /// The system clock (network time), otherwise.
    System,
}

/// One line of the feed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedLine {
    /// The first line.
    Start {
        /// [`FEED_SCHEMA`].
        schema: String,
        /// The writer's version, for the record.
        app: String,
        /// The boot clock at the session's start.
        boot_ns: TimeNs,
    },
    /// A boot-clock reading and the UTC time it maps to.
    Clock {
        /// The boot clock (`CLOCK_BOOTTIME` on the phone).
        boot_ns: TimeNs,
        /// The UTC time at that reading, nanoseconds since the Unix epoch.
        utc_ns: TimeNs,
        /// Where the UTC time came from.
        basis: ClockBasis,
    },
    /// A receiver's measured clock offset: the phone's boot clock minus the
    /// receiver's own clock.
    Offset {
        /// The receiver it applies to.
        receiver: String,
        /// Phone boot clock minus the receiver's clock, nanoseconds.
        offset_ns: TimeNs,
        /// The measurement's uncertainty, nanoseconds.
        uncertainty_ns: TimeNs,
    },
    /// One source's output as received.
    Line {
        /// Rises from line to line, never repeated; gaps are allowed.
        seq: u64,
        /// Arrival on the phone's boot clock.
        boot_ns: TimeNs,
        /// The program that produced it (`android`, `kismet`, `sensio`, ...).
        source: String,
        /// The radio that received it, if known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        receiver: Option<String>,
        /// The time a remote receiver stamped it with, on its own clock.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_ns: Option<TimeNs>,
        /// The received line's exact text.
        raw: String,
        /// Kept in the saved file but never passed to the core live, because
        /// the core had fallen behind.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        withheld: bool,
    },
    /// A request for verdicts over every line before it.
    Judge {
        /// When it was asked, on the boot clock.
        boot_ns: TimeNs,
    },
    /// Lines the core didn't get live.
    Dropped {
        /// Their source.
        source: String,
        /// Their receiver, if known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        receiver: Option<String>,
        /// How many.
        lines: u64,
    },
    /// Time the phone slept, so nothing was received.
    Slept {
        /// When it woke, on the boot clock.
        boot_ns: TimeNs,
        /// How long it slept, nanoseconds.
        slept_ns: TimeNs,
    },
    /// A radio's state changed: from here on it's in `state` (ADR-0005,
    /// amended 2026-11-07).
    Radio {
        /// When, on the boot clock.
        boot_ns: TimeNs,
        /// Which radio (`wifi`, `ble`, `gnss`, `bt_classic`, `link`, ...).
        radio: String,
        /// The receiver it belongs to, if known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        receiver: Option<String>,
        /// Its state from now on.
        state: RadioState,
    },
}

/// A radio's state, as the app reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioState {
    /// Receiving everything it can.
    On,
    /// Receiving only part of what it could, such as BLE's screen-off
    /// filters (ADR-0004).
    Filtered,
    /// Turned off, by the user or the system, such as Bluetooth or
    /// location turned off.
    Off,
    /// Asked to receive and refused, such as a failed BLE scan or a
    /// throttled Wi-Fi scan.
    Failed,
    /// Not run, by the user's choice in the app, such as Classic discovery
    /// left off in its settings: neither filtered nor lost.
    Unused,
}

/// One radio's state and the time it spent short of `on`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RadioCoverage {
    /// The radio.
    pub radio: String,
    /// Its receiver, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    /// Its latest state.
    pub state: RadioState,
    /// When it entered that state, on the boot clock.
    pub since_boot_ns: TimeNs,
    /// Time spent `filtered`, nanoseconds, up to the coverage line's time.
    pub filtered_ns: TimeNs,
    /// Time spent `off` or `failed`, nanoseconds, up to the coverage line's
    /// time.
    pub lost_ns: TimeNs,
}

/// One event kind's count from a source and receiver.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct KindCoverage {
    /// Events of this kind.
    pub events: u64,
    /// The latest one's line arrival, on the boot clock.
    pub last_boot_ns: TimeNs,
}

/// Counts for one source and receiver.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SourceCoverage {
    /// The source.
    pub source: String,
    /// The receiver, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    /// Lines received.
    pub lines: u64,
    /// Normalized events they gave.
    pub events: u64,
    /// Lines that couldn't be normalized.
    pub rejected: u64,
    /// Lines in the saved file that the core didn't get live.
    pub withheld: u64,
    /// Lines the app couldn't pass to the core, as it reported.
    pub dropped: u64,
    /// Lines whose events arrived more than [`LATENESS_NS`] behind the
    /// latest event and took the window's edge as their time (ADR-0005
    /// section 3).
    ///
    /// [`LATENESS_NS`]: crate::live::LATENESS_NS
    #[serde(skip_serializing_if = "is_zero")]
    pub late: u64,
    /// The latest line's arrival, on the boot clock.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_boot_ns: Option<TimeNs>,
    /// The events by kind (`ble_advert`, `wifi_ap`, ...), ordered by kind.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub kinds: BTreeMap<&'static str, KindCoverage>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference
fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// What a session covered so far.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Coverage {
    /// The time it covers up to, on the boot clock: the `judge` line's, or at
    /// the end the latest time the feed gave.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_ns: Option<TimeNs>,
    /// Per source and receiver, ordered by both.
    pub sources: Vec<SourceCoverage>,
    /// Per radio and receiver, ordered by both; empty when the feed reported
    /// no radio's state.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub radios: Vec<RadioCoverage>,
    /// Feed lines rejected before any source could be named.
    pub feed_rejected: u64,
    /// A torn last line, dropped.
    pub torn: u64,
    /// Time the phone slept, nanoseconds.
    pub slept_ns: TimeNs,
}

/// One line of the verdict stream.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VerdictLine {
    /// The first line.
    Start {
        /// [`VERDICTS_SCHEMA`].
        schema: &'static str,
        /// The core's version.
        core: &'static str,
        /// The rule set the verdicts come from.
        rule_set_version: u32,
    },
    /// The emitter picture at one `judge` line, before its verdicts (ADR-0006).
    Emitters {
        /// The `judge` line's 1-based line number in the feed.
        judge_line: usize,
        /// The describe layer's version.
        describe_version: u32,
        /// The vendor registry's identity, if one was given.
        #[serde(skip_serializing_if = "Option::is_none")]
        vendors: Option<String>,
        /// The recent window, nanoseconds back from the answer's time.
        recent_ns: TimeNs,
        /// The answer's time: the `judge` line's arrival as UTC.
        at_ns: TimeNs,
        /// The picture.
        #[serde(flatten)]
        picture: Picture,
    },
    /// The answer to one `judge` line: every verdict over the events before it.
    Verdicts {
        /// The `judge` line's 1-based line number in the feed.
        judge_line: usize,
        /// Its boot-clock time.
        boot_ns: TimeNs,
        /// The full verdict set.
        verdicts: Vec<Verdict>,
    },
    /// A line that couldn't be used; it's skipped and the stream goes on.
    Rejected {
        /// Its 1-based line number in the feed.
        feed_line: usize,
        /// Its `seq`, if it had one.
        #[serde(skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
        /// Why, never quoting its values.
        reason: String,
    },
    /// What the session covered, written with each answer and at the end.
    Coverage(Coverage),
    /// The last line.
    End {
        /// Why the stream ended.
        reason: EndReason,
        /// Feed lines read.
        feed_lines: usize,
        /// `judge` lines answered.
        judged: usize,
        /// Normalized events judged.
        events: usize,
    },
}

/// Why a verdict stream ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// The feed ended.
    InputClosed,
}

/// One line read with [`read_bounded_line`].
#[derive(Debug, PartialEq, Eq)]
pub enum BoundedLine {
    /// A line, without its newline (a carriage return before it stays, and
    /// JSON reads it as whitespace); `terminated` is false for a last line
    /// with no newline, which may be torn.
    Text {
        /// The line's bytes.
        bytes: Vec<u8>,
        /// Whether a newline ended it.
        terminated: bool,
    },
    /// A line longer than [`MAX_LINE_BYTES`], read past and discarded.
    TooLong,
}

/// Reads one line of at most `max` bytes, or `None` at the end of input.
///
/// A longer line is consumed to its end without being kept, so a hostile
/// feed can't make the reader hold it in memory.
///
/// # Errors
///
/// Returns the reader's own error.
pub fn read_bounded_line(reader: &mut impl BufRead, max: usize) -> io::Result<Option<BoundedLine>> {
    let mut bytes = Vec::new();
    let mut too_long = false;
    let mut any = false;
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(any.then_some(if too_long {
                BoundedLine::TooLong
            } else {
                BoundedLine::Text {
                    bytes,
                    terminated: false,
                }
            }));
        }
        any = true;
        let (chunk, found) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (&buf[..i], Some(i)),
            None => (buf, None),
        };
        if !too_long {
            if bytes.len() + chunk.len() > max {
                too_long = true;
                bytes = Vec::new();
            } else {
                bytes.extend_from_slice(chunk);
            }
        }
        let used = found.map_or(buf.len(), |i| i + 1);
        reader.consume(used);
        if found.is_some() {
            return Ok(Some(if too_long {
                BoundedLine::TooLong
            } else {
                BoundedLine::Text {
                    bytes,
                    terminated: true,
                }
            }));
        }
    }
}
