//! The live judge over a structured feed, against a model of ADR-0005: the
//! target builds feed lines (many valid, some not), tracks what the core
//! should accept and the time it should give each event, and checks that
//! every answer is `judge()` over exactly those events, with the same
//! number of rejections and torn lines.

#![no_main]

use std::collections::BTreeMap;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use sensio_core::event::is_label;
use sensio_core::feed::{
    ClockBasis, EndReason, FEED_SCHEMA, FeedLine, MAX_OWN, OwnAddress, OwnRadio, RadioState,
    VerdictLine,
};
use sensio_core::{Event, EventKind, Live, TimeNs, Verdict, judge, run};
use sensio_fuzz::{Kind, Rule, Time};
use sensio_sources::Sources;

const RECEIVERS: [&str; 5] = ["phone", "laptop", "rear-pi", "Rear Pi", ""];
const RADIOS: [&str; 4] = ["ble", "wifi", "gnss", "no good"];
const STATES: [RadioState; 5] = [
    RadioState::On,
    RadioState::Filtered,
    RadioState::Off,
    RadioState::Failed,
    RadioState::Unused,
];
const BOOT_TO_UTC: TimeNs = 1_799_999_998_000_000_000;
const MAX_OPS: usize = 500;

#[derive(Arbitrary, Debug)]
enum Utc {
    /// The session's own mapping from boot time.
    Consistent,
    Raw(i64),
}

#[derive(Arbitrary, Debug)]
enum Seq {
    /// This many past the last one.
    Next(u8),
    Raw(u64),
}

#[derive(Arbitrary, Debug)]
enum Remote {
    /// A receiver whose clock runs 5 s ahead of the phone's.
    Ahead,
    Raw(i64),
}

#[derive(Arbitrary, Debug)]
enum Op {
    Clock(Time, Utc),
    Offset(u8, Option<i64>, i64),
    Line {
        seq: Seq,
        boot: Time,
        receiver: Option<u8>,
        remote: Option<Remote>,
        kind: Kind,
        withheld: bool,
    },
    Judge(Time),
    Dropped(Option<u8>, u64),
    Slept(Time, i64),
    End(Time),
    /// A receiver's own addresses: each a radio (Wi-Fi or Bluetooth), six
    /// octets, and whether to cut the address short.
    Own(Time, u8, Vec<(bool, [u8; 6], bool)>),
    Radio(Time, u8, Option<u8>, u8),
    /// Never a feed line: it starts with '#'.
    Garbage(Vec<u8>),
    Blank(u8),
}

#[derive(Arbitrary, Debug)]
struct Session {
    all: bool,
    rule: Option<Rule>,
    ops: Vec<Op>,
    torn_tail: Option<Vec<u8>>,
}

#[derive(Clone, Default)]
struct Model {
    clock: Option<(TimeNs, TimeNs)>,
    offsets: BTreeMap<String, TimeNs>,
    last_seq: Option<u64>,
    events: Vec<Event>,
    rejected: usize,
    answers: Vec<Vec<Verdict>>,
    /// Each radio's state and when it entered it.
    radios: BTreeMap<(String, Option<String>), (RadioState, TimeNs)>,
    /// Whether a valid `end` line was written: every line after it is
    /// rejected and changes nothing else.
    ended: bool,
}

impl Model {
    fn utc(&self, boot: TimeNs, remote: Option<TimeNs>, receiver: Option<&str>) -> Option<TimeNs> {
        let (clock_boot, clock_utc) = self.clock?;
        let boot = match (remote, receiver.and_then(|r| self.offsets.get(r))) {
            (Some(remote), Some(&offset)) => remote.checked_add(offset)?,
            _ => boot,
        };
        clock_utc
            .checked_add(boot.checked_sub(clock_boot)?)
            .filter(|&t| t >= 0)
    }
}

fn receiver(r: u8) -> String {
    RECEIVERS[usize::from(r) % RECEIVERS.len()].to_owned()
}

fn json(line: &FeedLine) -> Vec<u8> {
    serde_json::to_vec(line).expect("a feed line serializes")
}

fuzz_target!(|session: Session| {
    let rule = session
        .rule
        .map(Rule::rule)
        .filter(|r| r.validate().is_ok())
        .unwrap_or_default();
    let mut model = Model::default();
    let mut cursor: TimeNs = 0;
    let mut feed = json(&FeedLine::Start {
        schema: FEED_SCHEMA.to_owned(),
        app: "fuzz".to_owned(),
        boot_ns: 0,
    });
    feed.push(b'\n');

    for op in session.ops.iter().take(MAX_OPS) {
        // After the end, an op runs against a copy, so it changes nothing;
        // the core rejects its line instead (a blank line stays skipped).
        let ended = model.ended;
        let mut scratch = model.clone();
        let m = if ended { &mut scratch } else { &mut model };
        let line = match op {
            Op::Clock(boot, utc) => {
                let boot_ns = boot.at(&mut cursor);
                let utc_ns = match utc {
                    Utc::Consistent => boot_ns.saturating_add(BOOT_TO_UTC),
                    Utc::Raw(t) => *t,
                };
                if boot_ns < 0 || utc_ns < 0 {
                    m.rejected += 1;
                } else {
                    m.clock = Some((boot_ns, utc_ns));
                }
                json(&FeedLine::Clock {
                    boot_ns,
                    utc_ns,
                    basis: ClockBasis::Gnss,
                })
            }
            Op::Offset(r, offset, uncertainty_ns) => {
                let receiver = receiver(*r);
                let offset_ns = offset.unwrap_or(-5_000_000_000);
                if !is_label(&receiver) || *uncertainty_ns < 0 {
                    m.rejected += 1;
                } else {
                    m.offsets.insert(receiver.clone(), offset_ns);
                }
                json(&FeedLine::Offset {
                    receiver,
                    offset_ns,
                    uncertainty_ns: *uncertainty_ns,
                })
            }
            Op::Line {
                seq,
                boot,
                receiver: r,
                remote,
                kind,
                withheld,
            } => {
                let seq = match seq {
                    Seq::Next(n) => m
                        .last_seq
                        .map_or(0, |last| last.saturating_add(1 + u64::from(*n))),
                    Seq::Raw(s) => *s,
                };
                let boot_ns = boot.at(&mut cursor);
                let receiver = r.map(receiver);
                let remote_ns = remote.as_ref().map(|remote| match remote {
                    Remote::Ahead => boot_ns.saturating_add(5_000_000_000),
                    Remote::Raw(t) => *t,
                });
                let raw = serde_json::to_string(&kind.event_kind()).expect("a kind serializes");
                let line = json(&FeedLine::Line {
                    seq,
                    boot_ns,
                    source: "sensio".to_owned(),
                    receiver: receiver.clone(),
                    remote_ns,
                    raw: raw.clone(),
                    withheld: *withheld,
                });
                if receiver.as_deref().is_some_and(|r| !is_label(r))
                    || m.last_seq.is_some_and(|last| seq <= last)
                {
                    m.rejected += 1;
                } else {
                    m.last_seq = Some(seq);
                    if !*withheld || session.all {
                        let t_ns = (boot_ns >= 0 && remote_ns.is_none_or(|t| t >= 0))
                            .then(|| m.utc(boot_ns, remote_ns, receiver.as_deref()))
                            .flatten();
                        // The normalizer's own parse: a non-finite float was written as null.
                        let parsed: Option<EventKind> = serde_json::from_str(&raw).ok();
                        let event = t_ns.zip(parsed).map(|(t_ns, kind)| Event {
                            t_ns,
                            source: "sensio".to_owned(),
                            receiver,
                            kind,
                        });
                        match event.filter(|e| e.validate().is_ok()) {
                            Some(event) => m.events.push(event),
                            None => m.rejected += 1,
                        }
                    }
                }
                line
            }
            Op::Judge(boot) => {
                let boot_ns = boot.at(&mut cursor);
                if boot_ns < 0 {
                    m.rejected += 1;
                } else {
                    m.answers.push(judge(&m.events, &rule));
                }
                json(&FeedLine::Judge { boot_ns })
            }
            Op::Dropped(r, lines) => {
                let receiver = r.map(receiver);
                if receiver.as_deref().is_some_and(|r| !is_label(r)) {
                    m.rejected += 1;
                }
                json(&FeedLine::Dropped {
                    source: "sensio".to_owned(),
                    receiver,
                    lines: *lines,
                })
            }
            Op::Slept(boot, slept_ns) => {
                let boot_ns = boot.at(&mut cursor);
                if boot_ns < 0 || *slept_ns < 0 {
                    m.rejected += 1;
                }
                json(&FeedLine::Slept {
                    boot_ns,
                    slept_ns: *slept_ns,
                })
            }
            Op::Radio(boot, r, rx, state) => {
                let boot_ns = boot.at(&mut cursor);
                let radio = RADIOS[usize::from(*r) % RADIOS.len()].to_owned();
                let receiver = rx.map(receiver);
                let state = STATES[usize::from(*state) % STATES.len()];
                let key = (radio.clone(), receiver.clone());
                let named = is_label(&radio) && receiver.as_deref().is_none_or(is_label);
                let back = m.radios.get(&key).is_some_and(|&(_, since)| boot_ns < since);
                if !named || boot_ns < 0 || back {
                    m.rejected += 1;
                } else {
                    m.radios.insert(key, (state, boot_ns));
                }
                json(&FeedLine::Radio {
                    boot_ns,
                    radio,
                    receiver,
                    state,
                })
            }
            Op::Garbage(bytes) => {
                m.rejected += 1;
                let mut line = vec![b'#'];
                line.extend(bytes.iter().filter(|&&b| b != b'\n'));
                line
            }
            Op::Own(boot, r, addresses) => {
                let boot_ns = boot.at(&mut cursor);
                let receiver = receiver(*r);
                let addresses: Vec<OwnAddress> = addresses
                    .iter()
                    .map(|&(bluetooth, octets, short)| {
                        let mut address = octets
                            .iter()
                            .map(|o| format!("{o:02x}"))
                            .collect::<Vec<_>>()
                            .join(":");
                        if short {
                            address.truncate(14);
                        }
                        OwnAddress {
                            radio: if bluetooth {
                                OwnRadio::Bluetooth
                            } else {
                                OwnRadio::Wifi
                            },
                            address,
                        }
                    })
                    .collect();
                let bad = boot_ns < 0
                    || !is_label(&receiver)
                    || addresses.len() > MAX_OWN
                    || addresses.iter().any(|a| a.address.len() != 17);
                if bad {
                    m.rejected += 1;
                }
                json(&FeedLine::Own {
                    boot_ns,
                    receiver,
                    addresses,
                })
            }
            Op::End(boot) => {
                let boot_ns = boot.at(&mut cursor);
                if boot_ns < 0 {
                    m.rejected += 1;
                } else {
                    m.ended = true;
                }
                json(&FeedLine::End { boot_ns })
            }
            Op::Blank(n) => [b"".as_slice(), b" ", b"\r", b"\t "][usize::from(n % 4)].to_vec(),
        };
        if ended && !matches!(op, Op::Blank(_)) {
            model.rejected += 1;
        }
        feed.extend_from_slice(&line);
        feed.push(b'\n');
    }
    let torn = session.torn_tail.as_ref().map(|tail| {
        feed.push(b'#');
        feed.extend(tail.iter().filter(|&&b| b != b'\n'));
    });

    let mut lines = Vec::new();
    run(
        Live::new(Sources, rule.clone(), session.all),
        &feed[..],
        |line| {
            lines.push(line.clone());
            Ok(())
        },
    )
    .expect("a feed that starts with start is judged");

    let answers: Vec<Vec<Verdict>> = lines
        .iter()
        .filter_map(|line| match line {
            VerdictLine::Verdicts { verdicts, .. } => Some(verdicts.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        answers, model.answers,
        "an answer isn't the judge over the model's events"
    );
    let rejected = lines
        .iter()
        .filter(|l| matches!(l, VerdictLine::Rejected { .. }))
        .count();
    if rejected != model.rejected {
        let text = String::from_utf8_lossy(&feed);
        let feed_lines: Vec<&str> = text.split('\n').collect();
        for line in &lines {
            if let VerdictLine::Rejected {
                feed_line, reason, ..
            } = line
            {
                eprintln!(
                    "rejected {feed_line}: {reason}: {}",
                    feed_lines[feed_line - 1]
                );
            }
        }
    }
    assert_eq!(
        rejected, model.rejected,
        "rejections differ from the model's"
    );
    let Some(VerdictLine::End { events, reason, .. }) = lines.last() else {
        panic!("no end line");
    };
    assert_eq!(*events, model.events.len());
    let expected = if model.ended {
        EndReason::Stopped
    } else {
        EndReason::Cut
    };
    assert_eq!(*reason, expected, "the end reason isn't the model's");
    let Some(VerdictLine::Coverage(coverage)) = lines.iter().rev().nth(1) else {
        panic!("no final coverage");
    };
    assert_eq!(coverage.torn, u64::from(torn.is_some()));
    let radios: BTreeMap<_, _> = coverage
        .radios
        .iter()
        .map(|r| ((r.radio.clone(), r.receiver.clone()), (r.state, r.since_boot_ns)))
        .collect();
    assert_eq!(radios, model.radios, "radio states differ from the model's");
    for r in &coverage.radios {
        assert!(r.filtered_ns >= 0 && r.lost_ns >= 0, "a radio's time is negative");
    }
});
