//! The live judge: reads a feed (`sensio.feed/1`) line by line and answers each
//! `judge` line with the verdicts over the events before it (ADR-0005).
//!
//! It is a pure function of the feed's lines, like the judge: no clock, no
//! I/O, no randomness. The same feed always gives the same verdict stream,
//! which is what makes a replay equal to what was shown live.

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, BufRead};

use crate::describe::{DESCRIBE_VERSION, DescribeFold, DescribeParams, VendorTable};
use crate::event::{Event, EventKind, TimeNs, is_label, reason_without_values};
use crate::feed::{
    BoundedLine, Coverage, EndReason, FEED_SCHEMA, FeedLine, MAX_LINE_BYTES, RadioCoverage,
    RadioState, SourceCoverage, VERDICTS_SCHEMA, VerdictLine, read_bounded_line,
};
use crate::judge::{JudgeFold, PersistenceRule, RULE_SET_VERSION, order_key};

/// How far behind the latest event an event may arrive and keep its own
/// time; an earlier one takes the latest time minus this (ADR-0005 section
/// 3). Events older than this are folded into the judge's and the describe
/// layer's state and no longer held one by one.
pub const LATENESS_NS: TimeNs = 600 * 1_000_000_000;

/// Turns one source's raw text into what it observed.
pub trait Normalize {
    /// The events in `raw`, a line received from `source`.
    ///
    /// # Errors
    ///
    /// Returns why the line can't be used, never quoting its values.
    fn normalize(&self, source: &str, raw: &str) -> Result<Vec<EventKind>, String>;
}

/// A feed that can't be read at all.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Fatal {
    /// The first line isn't a `start` naming a known schema.
    #[error("feed line {line}: {reason}")]
    NotAFeed {
        /// The 1-based line number.
        line: usize,
        /// What's wrong.
        reason: &'static str,
    },
    /// The input ended before a `start` line.
    #[error("the feed is empty")]
    Empty,
}

/// Why a feed couldn't be judged to its end.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The feed can't be read at all.
    #[error(transparent)]
    Fatal(#[from] Fatal),
    /// The input failed.
    #[error("reading feed line {line}: {source}")]
    Read {
        /// The 1-based line number.
        line: usize,
        /// The underlying error.
        source: io::Error,
    },
    /// The verdict stream couldn't be written.
    #[error("writing the verdict stream: {0}")]
    Write(io::Error),
}

/// Judges a whole feed from `reader`, handing each verdict-stream line to
/// `emit` as soon as it's known. The front end and a replay both go through
/// here, so they read a feed the same way.
///
/// # Errors
///
/// Returns [`RunError`] if the feed isn't one, or reading or `emit` fails.
pub fn run<N: Normalize>(
    mut engine: Live<N>,
    mut reader: impl BufRead,
    mut emit: impl FnMut(&VerdictLine) -> io::Result<()>,
) -> Result<(), RunError> {
    let mut line: usize = 0;
    loop {
        line += 1;
        let read = read_bounded_line(&mut reader, MAX_LINE_BYTES)
            .map_err(|source| RunError::Read { line, source })?;
        let out = match read {
            None => break,
            Some(BoundedLine::Text { bytes, terminated }) => {
                engine.push(line, &bytes, terminated)?
            }
            Some(BoundedLine::TooLong) => engine.too_long(line)?,
        };
        for verdict_line in &out {
            emit(verdict_line).map_err(RunError::Write)?;
        }
    }
    for verdict_line in &engine.finish()? {
        emit(verdict_line).map_err(RunError::Write)?;
    }
    Ok(())
}

/// The live judge's state over one feed.
pub struct Live<N> {
    normalizer: N,
    rule: PersistenceRule,
    describe: DescribeParams,
    vendors: VendorTable,
    include_withheld: bool,
    started: bool,
    feed_lines: usize,
    judged: usize,
    last_seq: Option<u64>,
    clock: Option<(TimeNs, TimeNs)>,
    offsets: BTreeMap<String, TimeNs>,
    /// The latest event time so far.
    frontier: Option<TimeNs>,
    /// Events within [`LATENESS_NS`] of the frontier, in the judge's order.
    pending: BTreeMap<(TimeNs, u8, usize), Event>,
    /// Folded events the recent window may still reach, oldest first.
    window: VecDeque<Event>,
    judge_fold: JudgeFold,
    describe_fold: DescribeFold,
    events: usize,
    sources: BTreeMap<(String, Option<String>), SourceCoverage>,
    radios: BTreeMap<(String, Option<String>), RadioCoverage>,
    latest_boot_ns: Option<TimeNs>,
    feed_rejected: u64,
    torn: u64,
    slept_ns: TimeNs,
}

impl<N: Normalize> Live<N> {
    /// A live judge with `rule`. With `include_withheld`, lines the core
    /// didn't get live are judged too (`peil live --all`); without it, a
    /// replay gives exactly what was shown live.
    pub fn new(normalizer: N, rule: PersistenceRule, include_withheld: bool) -> Self {
        Self {
            normalizer,
            rule,
            describe: DescribeParams::default(),
            vendors: VendorTable::default(),
            include_withheld,
            started: false,
            feed_lines: 0,
            judged: 0,
            last_seq: None,
            clock: None,
            offsets: BTreeMap::new(),
            frontier: None,
            pending: BTreeMap::new(),
            window: VecDeque::new(),
            judge_fold: JudgeFold::default(),
            describe_fold: DescribeFold::default(),
            events: 0,
            sources: BTreeMap::new(),
            radios: BTreeMap::new(),
            latest_boot_ns: None,
            feed_rejected: 0,
            torn: 0,
            slept_ns: 0,
        }
    }

    /// Describes with these parameters and this vendor registry instead of
    /// the defaults and none.
    #[must_use]
    pub fn with_describe(mut self, params: DescribeParams, vendors: VendorTable) -> Self {
        self.describe = params;
        self.vendors = vendors;
        self
    }

    /// Reads the feed's line `feed_line` (1-based); `terminated` is false for
    /// a last line with no newline, which is dropped as torn if it doesn't
    /// parse.
    ///
    /// # Errors
    ///
    /// Returns [`Fatal`] if the first line isn't a known feed's `start`.
    pub fn push(
        &mut self,
        feed_line: usize,
        bytes: &[u8],
        terminated: bool,
    ) -> Result<Vec<VerdictLine>, Fatal> {
        self.feed_lines = feed_line;
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(Vec::new());
        }
        let not_a_feed = |reason| Fatal::NotAFeed {
            line: feed_line,
            reason,
        };
        let Ok(text) = std::str::from_utf8(bytes) else {
            if !self.started {
                return Err(not_a_feed("not UTF-8"));
            }
            // A line torn at any byte can end inside a character.
            if !terminated {
                self.torn += 1;
                return Ok(Vec::new());
            }
            return Ok(self.reject_feed_line(feed_line, "not UTF-8".to_owned()));
        };
        let line: FeedLine = match serde_json::from_str(text) {
            Ok(line) => line,
            Err(_) if !self.started => {
                return Err(not_a_feed("the first line isn't a feed's start line"));
            }
            Err(_) if !terminated => {
                self.torn += 1;
                return Ok(Vec::new());
            }
            Err(err) => return Ok(self.reject_feed_line(feed_line, reason_without_values(&err))),
        };
        if !self.started {
            return match line {
                FeedLine::Start { schema, .. } if schema == FEED_SCHEMA => {
                    self.started = true;
                    Ok(vec![VerdictLine::Start {
                        schema: VERDICTS_SCHEMA,
                        core: env!("CARGO_PKG_VERSION"),
                        rule_set_version: RULE_SET_VERSION,
                    }])
                }
                FeedLine::Start { .. } => Err(not_a_feed("unknown feed schema")),
                _ => Err(not_a_feed("the first line isn't a feed's start line")),
            };
        }
        Ok(self.apply(feed_line, line))
    }

    /// Notes a line longer than [`MAX_LINE_BYTES`], which was read past.
    ///
    /// # Errors
    ///
    /// Returns [`Fatal`] if it was the first line.
    pub fn too_long(&mut self, feed_line: usize) -> Result<Vec<VerdictLine>, Fatal> {
        self.feed_lines = feed_line;
        if !self.started {
            return Err(Fatal::NotAFeed {
                line: feed_line,
                reason: "the first line isn't a feed's start line",
            });
        }
        Ok(self.reject_feed_line(
            feed_line,
            format!("line longer than {MAX_LINE_BYTES} bytes"),
        ))
    }

    /// Ends the stream: the final coverage and the `end` line.
    ///
    /// # Errors
    ///
    /// Returns [`Fatal::Empty`] if no `start` line was read.
    pub fn finish(self) -> Result<Vec<VerdictLine>, Fatal> {
        if !self.started {
            return Err(Fatal::Empty);
        }
        Ok(vec![
            VerdictLine::Coverage(self.coverage(self.latest_boot_ns)),
            VerdictLine::End {
                reason: EndReason::InputClosed,
                feed_lines: self.feed_lines,
                judged: self.judged,
                events: self.events,
            },
        ])
    }

    fn apply(&mut self, feed_line: usize, line: FeedLine) -> Vec<VerdictLine> {
        let reject =
            |this: &mut Self, reason: &str| this.reject_feed_line(feed_line, reason.to_owned());
        match line {
            FeedLine::Start { .. } => reject(self, "a second start line"),
            FeedLine::Clock {
                boot_ns, utc_ns, ..
            } => {
                if boot_ns < 0 || utc_ns < 0 {
                    return reject(self, "clock time out of range");
                }
                self.saw(boot_ns);
                self.clock = Some((boot_ns, utc_ns));
                Vec::new()
            }
            FeedLine::Offset {
                receiver,
                offset_ns,
                uncertainty_ns,
            } => {
                if !is_label(&receiver) {
                    return reject(self, "receiver isn't a valid name");
                }
                if uncertainty_ns < 0 {
                    return reject(self, "offset uncertainty out of range");
                }
                self.offsets.insert(receiver, offset_ns);
                Vec::new()
            }
            FeedLine::Line {
                seq,
                boot_ns,
                source,
                receiver,
                remote_ns,
                raw,
                withheld,
            } => self.apply_line(
                feed_line, seq, boot_ns, source, receiver, remote_ns, &raw, withheld,
            ),
            FeedLine::Judge { boot_ns } => {
                if boot_ns < 0 {
                    return reject(self, "judge time out of range");
                }
                self.answer(feed_line, boot_ns)
            }
            FeedLine::Dropped {
                source,
                receiver,
                lines,
            } => {
                if !is_label(&source) || receiver.as_deref().is_some_and(|r| !is_label(r)) {
                    return reject(self, "source or receiver isn't a valid name");
                }
                // The count is the feed's own number, so it can be any u64.
                let entry = self.entry(source, receiver);
                entry.dropped = entry.dropped.saturating_add(lines);
                Vec::new()
            }
            FeedLine::Slept { boot_ns, slept_ns } => {
                if boot_ns < 0 || slept_ns < 0 {
                    return reject(self, "sleep time out of range");
                }
                self.saw(boot_ns);
                self.slept_ns = self.slept_ns.saturating_add(slept_ns);
                Vec::new()
            }
            FeedLine::Radio {
                boot_ns,
                radio,
                receiver,
                state,
            } => match self.apply_radio(boot_ns, radio, receiver, state) {
                Ok(()) => Vec::new(),
                Err(reason) => reject(self, reason),
            },
        }
    }

    /// Answers a `judge` line at `boot_ns`: the coverage so far, the
    /// emitter picture and the verdicts.
    fn answer(&mut self, feed_line: usize, boot_ns: TimeNs) -> Vec<VerdictLine> {
        self.saw(boot_ns);
        self.judged += 1;
        // The answer's time: its arrival through the clock, held to
        // the lateness window's edge, or, without a clock, the latest
        // event's.
        let at_ns = self
            .utc(boot_ns, None, None)
            .map(|t| self.edge().map_or(t, |edge| t.max(edge)))
            .or(self.frontier)
            .unwrap_or(0);
        // Only events in the recent window count toward it; both
        // collections are in time order, so the rest is skipped.
        let recent_from = at_ns.saturating_sub(self.describe.recent_ns);
        let first = self.window.partition_point(|e| e.t_ns < recent_from);
        let recent = self.window.range(first..).chain(
            self.pending
                .range((recent_from, 0, 0)..)
                .map(|(_, event)| event),
        );
        vec![
            VerdictLine::Coverage(self.coverage(Some(boot_ns))),
            VerdictLine::Emitters {
                judge_line: feed_line,
                describe_version: DESCRIBE_VERSION,
                vendors: self.vendors.id().map(str::to_owned),
                recent_ns: self.describe.recent_ns,
                at_ns,
                picture: self.describe_fold.picture_with(
                    self.pending.values(),
                    recent,
                    at_ns,
                    &self.describe,
                    &self.vendors,
                ),
            },
            VerdictLine::Verdicts {
                judge_line: feed_line,
                boot_ns,
                verdicts: self
                    .judge_fold
                    .verdicts_with(self.pending.values(), &self.rule),
            },
        ]
    }

    /// A radio's state change: the time in the state it leaves goes to that
    /// state's total.
    fn apply_radio(
        &mut self,
        boot_ns: TimeNs,
        radio: String,
        receiver: Option<String>,
        state: RadioState,
    ) -> Result<(), &'static str> {
        if !is_label(&radio) || receiver.as_deref().is_some_and(|r| !is_label(r)) {
            return Err("radio or receiver isn't a valid name");
        }
        if boot_ns < 0 {
            return Err("radio time out of range");
        }
        let key = (radio, receiver);
        if let Some(track) = self.radios.get_mut(&key) {
            if boot_ns < track.since_boot_ns {
                return Err("radio time goes back");
            }
            close_interval(track, boot_ns);
            track.state = state;
            track.since_boot_ns = boot_ns;
        } else {
            let (radio, receiver) = key.clone();
            self.radios.insert(
                key,
                RadioCoverage {
                    radio,
                    receiver,
                    state,
                    since_boot_ns: boot_ns,
                    filtered_ns: 0,
                    lost_ns: 0,
                },
            );
        }
        self.saw(boot_ns);
        Ok(())
    }

    /// Notes a valid boot-clock time the feed gave, for the final coverage.
    fn saw(&mut self, boot_ns: TimeNs) {
        self.latest_boot_ns = Some(self.latest_boot_ns.map_or(boot_ns, |t| t.max(boot_ns)));
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_line(
        &mut self,
        feed_line: usize,
        seq: u64,
        boot_ns: TimeNs,
        source: String,
        receiver: Option<String>,
        remote_ns: Option<TimeNs>,
        raw: &str,
        withheld: bool,
    ) -> Vec<VerdictLine> {
        let rejected = |reason: &str| {
            vec![VerdictLine::Rejected {
                feed_line,
                seq: Some(seq),
                reason: reason.to_owned(),
            }]
        };
        if !is_label(&source) || receiver.as_deref().is_some_and(|r| !is_label(r)) {
            self.feed_rejected += 1;
            return rejected("source or receiver isn't a valid name");
        }
        if self.last_seq.is_some_and(|last| seq <= last) {
            self.entry(source, receiver).rejected += 1;
            return rejected("seq doesn't rise");
        }
        self.last_seq = Some(seq);
        let key = (source, receiver);
        self.entry(key.0.clone(), key.1.clone()).lines += 1;
        if withheld {
            self.entry(key.0.clone(), key.1.clone()).withheld += 1;
            if !self.include_withheld {
                return Vec::new();
            }
        }
        if boot_ns < 0 || remote_ns.is_some_and(|r| r < 0) {
            self.entry(key.0, key.1).rejected += 1;
            return rejected("line time out of range");
        }
        let Some(mapped_ns) = self.utc(boot_ns, remote_ns, key.1.as_deref()) else {
            self.entry(key.0, key.1).rejected += 1;
            return rejected("no clock line before it, or its time maps out of range");
        };
        let t_ns = self.edge().map_or(mapped_ns, |edge| mapped_ns.max(edge));
        let kinds = match self.normalizer.normalize(&key.0, raw) {
            Ok(kinds) => kinds,
            Err(reason) => {
                self.entry(key.0, key.1).rejected += 1;
                return rejected(&reason);
            }
        };
        let events: Vec<Event> = kinds
            .into_iter()
            .map(|kind| Event {
                t_ns,
                source: key.0.clone(),
                receiver: key.1.clone(),
                kind,
            })
            .collect();
        if let Some(reason) = events.iter().find_map(|e| e.validate().err()) {
            self.entry(key.0, key.1).rejected += 1;
            return rejected(reason);
        }
        self.saw(boot_ns);
        let entry = self.entry(key.0, key.1);
        entry.events += events.len() as u64;
        if t_ns > mapped_ns {
            entry.late += 1;
        }
        entry.last_boot_ns = Some(entry.last_boot_ns.map_or(boot_ns, |t| t.max(boot_ns)));
        for event in &events {
            let kind = entry.kinds.entry(event.kind.name()).or_default();
            kind.events += 1;
            kind.last_boot_ns = kind.last_boot_ns.max(boot_ns);
        }
        self.frontier = Some(self.frontier.map_or(t_ns, |f| f.max(t_ns)));
        for event in events {
            self.pending.insert(order_key(&event, self.events), event);
            self.events += 1;
        }
        self.settle();
        Vec::new()
    }

    /// The lateness window's edge: the latest event time minus
    /// [`LATENESS_NS`].
    fn edge(&self) -> Option<TimeNs> {
        self.frontier.map(|f| f.saturating_sub(LATENESS_NS))
    }

    /// Folds the events now older than the window's edge, in the judge's
    /// order, and lets go of those the recent window can no longer reach.
    /// Every later event and answer is at or after the edge, so nothing
    /// folded is ever preceded by what comes next.
    fn settle(&mut self) {
        let Some(edge) = self.edge() else {
            return;
        };
        while let Some(entry) = self.pending.first_entry() {
            if entry.key().0 >= edge {
                break;
            }
            let event = entry.remove();
            self.judge_fold.observe(&event, &self.rule);
            self.describe_fold.observe(&event, &self.describe);
            self.window.push_back(event);
        }
        let reach = edge.saturating_sub(self.describe.recent_ns);
        while self.window.front().is_some_and(|e| e.t_ns < reach) {
            self.window.pop_front();
        }
    }

    /// A line's arrival as UTC: a remote receiver's own stamp through its
    /// offset when both are known, otherwise the phone's boot clock, then
    /// through the latest clock reading.
    fn utc(
        &self,
        boot_ns: TimeNs,
        remote_ns: Option<TimeNs>,
        receiver: Option<&str>,
    ) -> Option<TimeNs> {
        let (clock_boot, clock_utc) = self.clock?;
        let offset = receiver.and_then(|r| self.offsets.get(r));
        let boot = match (remote_ns, offset) {
            (Some(remote), Some(&offset)) => remote.checked_add(offset)?,
            _ => boot_ns,
        };
        clock_utc
            .checked_add(boot.checked_sub(clock_boot)?)
            .filter(|&t| t >= 0)
    }

    fn reject_feed_line(&mut self, feed_line: usize, reason: String) -> Vec<VerdictLine> {
        self.feed_rejected += 1;
        vec![VerdictLine::Rejected {
            feed_line,
            seq: None,
            reason,
        }]
    }

    fn entry(&mut self, source: String, receiver: Option<String>) -> &mut SourceCoverage {
        self.sources
            .entry((source.clone(), receiver.clone()))
            .or_insert_with(|| SourceCoverage {
                source,
                receiver,
                ..SourceCoverage::default()
            })
    }

    /// What the session covered, with each radio's open interval counted up
    /// to `boot_ns`.
    fn coverage(&self, boot_ns: Option<TimeNs>) -> Coverage {
        let radios = self
            .radios
            .values()
            .map(|track| {
                let mut track = track.clone();
                if let Some(now) = boot_ns {
                    close_interval(&mut track, now);
                }
                track
            })
            .collect();
        Coverage {
            boot_ns,
            sources: self.sources.values().cloned().collect(),
            radios,
            feed_rejected: self.feed_rejected,
            torn: self.torn,
            slept_ns: self.slept_ns,
        }
    }
}

/// Adds the time from `track`'s state change up to `now` to that state's
/// total; a `now` before the change adds nothing.
fn close_interval(track: &mut RadioCoverage, now: TimeNs) {
    let spent = now.saturating_sub(track.since_boot_ns).max(0);
    match track.state {
        RadioState::On => {}
        RadioState::Filtered => track.filtered_ns = track.filtered_ns.saturating_add(spent),
        RadioState::Off | RadioState::Failed => {
            track.lost_ns = track.lost_ns.saturating_add(spent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One normalized event per line, as the `sensio` source has it.
    struct Sensio;

    impl Normalize for Sensio {
        fn normalize(&self, _source: &str, raw: &str) -> Result<Vec<EventKind>, String> {
            serde_json::from_str(raw)
                .map(|kind| vec![kind])
                .map_err(|_| "not an event".to_owned())
        }
    }

    #[test]
    fn a_long_session_holds_only_the_windows_events() {
        let second: TimeNs = 1_000_000_000;
        let mut live = Live::new(Sensio, PersistenceRule::default(), false);
        let push = |live: &mut Live<Sensio>, n: usize, line: String| {
            live.push(n, line.as_bytes(), true).expect("a feed line");
        };
        push(
            &mut live,
            1,
            r#"{"type":"start","schema":"sensio.feed/1","app":"test","boot_ns":0}"#.to_owned(),
        );
        push(
            &mut live,
            2,
            r#"{"type":"clock","boot_ns":0,"utc_ns":1800000000000000000,"basis":"gnss"}"#
                .to_owned(),
        );
        // Four hours, one advert a second from a few devices.
        let hours: u32 = 4;
        for s in 1..=hours * 3600 {
            let raw = format!(
                r#"{{\"kind\":\"ble_advert\",\"addr\":\"aa:aa:aa:00:00:0{}\",\"rssi_dbm\":-60}}"#,
                s % 4
            );
            let text = format!(
                r#"{{"type":"line","seq":{s},"boot_ns":{},"source":"sensio","raw":"{raw}"}}"#,
                i64::from(s) * second
            );
            push(&mut live, 2 + usize::try_from(s).expect("small"), text);
        }
        // The window and the recent window's reach, a second apart each.
        let held = live.pending.len() + live.window.len();
        let reach =
            usize::try_from((LATENESS_NS + live.describe.recent_ns) / second + 1).expect("small");
        assert!(held <= reach, "{held} events held, more than {reach}");
        assert_eq!(live.events, usize::try_from(hours * 3600).expect("small"));
    }
}
