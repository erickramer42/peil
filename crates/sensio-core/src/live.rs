//! The live judge: reads a feed (`sensio.feed/1`) line by line and answers each
//! `judge` line with the verdicts over the events before it (ADR-0005).
//!
//! It is a pure function of the feed's lines, like the judge: no clock, no
//! I/O, no randomness. The same feed always gives the same verdict stream,
//! which is what makes a replay equal to what was shown live.

use std::collections::BTreeMap;
use std::io::{self, BufRead};

use crate::event::{Event, EventKind, TimeNs, is_label, reason_without_values};
use crate::feed::{
    BoundedLine, Coverage, EndReason, FEED_SCHEMA, FeedLine, MAX_LINE_BYTES, SourceCoverage,
    VERDICTS_SCHEMA, VerdictLine, read_bounded_line,
};
use crate::judge::{PersistenceRule, RULE_SET_VERSION, judge};

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
    include_withheld: bool,
    started: bool,
    feed_lines: usize,
    judged: usize,
    last_seq: Option<u64>,
    clock: Option<(TimeNs, TimeNs)>,
    offsets: BTreeMap<String, TimeNs>,
    events: Vec<Event>,
    sources: BTreeMap<(String, Option<String>), SourceCoverage>,
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
            include_withheld,
            started: false,
            feed_lines: 0,
            judged: 0,
            last_seq: None,
            clock: None,
            offsets: BTreeMap::new(),
            events: Vec::new(),
            sources: BTreeMap::new(),
            feed_rejected: 0,
            torn: 0,
            slept_ns: 0,
        }
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
            VerdictLine::Coverage(self.coverage()),
            VerdictLine::End {
                reason: EndReason::InputClosed,
                feed_lines: self.feed_lines,
                judged: self.judged,
                events: self.events.len(),
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
                self.judged += 1;
                vec![
                    VerdictLine::Coverage(self.coverage()),
                    VerdictLine::Verdicts {
                        judge_line: feed_line,
                        boot_ns,
                        verdicts: judge(&self.events, &self.rule),
                    },
                ]
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
                self.slept_ns = self.slept_ns.saturating_add(slept_ns);
                Vec::new()
            }
        }
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
        let Some(t_ns) = self.utc(boot_ns, remote_ns, key.1.as_deref()) else {
            self.entry(key.0, key.1).rejected += 1;
            return rejected("no clock line before it, or its time maps out of range");
        };
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
        self.entry(key.0, key.1).events += events.len() as u64;
        self.events.extend(events);
        Vec::new()
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

    fn coverage(&self) -> Coverage {
        Coverage {
            sources: self.sources.values().cloned().collect(),
            feed_rejected: self.feed_rejected,
            torn: self.torn,
            slept_ns: self.slept_ns,
        }
    }
}
