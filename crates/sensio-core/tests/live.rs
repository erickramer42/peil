//! The live judge over a feed: golden verdict streams, the answer to each
//! `judge` line equal to the judge over the events before it, and feed lines
//! rejected with their line and reason.
//!
//! The feed (`fixtures/drive.feed`) is the synthetic drive of `drive.jsonl`
//! carried as `sensio` lines, with a `judge` line after each place from the
//! second on. The BLE device's sighting at the fourth place is withheld
//! (the core fell behind), the phone sleeps before the last sighting, and a
//! clock reading from the system clock follows.

use std::io::{BufRead, BufReader, Cursor};

use proptest::prelude::*;
use sensio_core::event::reason_without_values;
use sensio_core::feed::{
    BoundedLine, Coverage, FEED_SCHEMA, FeedLine, MAX_LINE_BYTES, RadioState, VERDICTS_SCHEMA,
    VerdictLine, read_bounded_line,
};
use sensio_core::{
    Event, EventKind, Fatal, Live, Normalize, PersistenceRule, RunError, Verdict, judge,
    parse_jsonl, run,
};

const FEED: &str = include_str!("fixtures/drive.feed");
const DRIVE: &str = include_str!("fixtures/drive.jsonl");
const WIFI_DRIVE: &str = include_str!("fixtures/drive_wifi.jsonl");

/// The boot clock's zero on the session clock, as the fixture's clock line says.
const BOOT_TO_UTC: i64 = 1_799_999_998_000_000_000;

/// The `sensio` source's normalizer, as `sensio-sources` has it: one event's
/// kind and fields as the raw text.
struct Sensio;

impl Normalize for Sensio {
    fn normalize(&self, source: &str, raw: &str) -> Result<Vec<EventKind>, String> {
        if source != "sensio" {
            return Err("no normalizer for this source".to_owned());
        }
        serde_json::from_str(raw)
            .map(|kind| vec![kind])
            .map_err(|err| reason_without_values(&err))
    }
}

fn try_live(feed: &str, all: bool) -> Result<Vec<VerdictLine>, RunError> {
    let mut out = Vec::new();
    let engine = Live::new(Sensio, PersistenceRule::default(), all);
    run(engine, Cursor::new(feed), |line| {
        out.push(line.clone());
        Ok(())
    })?;
    Ok(out)
}

fn live(feed: &str, all: bool) -> Vec<VerdictLine> {
    try_live(feed, all).expect("the feed is judged")
}

fn fatal(feed: &str) -> Fatal {
    match try_live(feed, false) {
        Err(RunError::Fatal(fatal)) => fatal,
        other => panic!("expected a fatal error, got {other:?}"),
    }
}

fn answers(lines: &[VerdictLine]) -> Vec<&[Verdict]> {
    lines
        .iter()
        .filter_map(|line| match line {
            VerdictLine::Verdicts { verdicts, .. } => Some(verdicts.as_slice()),
            _ => None,
        })
        .collect()
}

fn rejections(lines: &[VerdictLine]) -> Vec<(usize, Option<u64>, &str)> {
    lines
        .iter()
        .filter_map(|line| match line {
            VerdictLine::Rejected {
                feed_line,
                seq,
                reason,
            } => Some((*feed_line, *seq, reason.as_str())),
            _ => None,
        })
        .collect()
}

/// A feed's start and clock lines, then `body`; the clock maps boot time 0
/// to `BOOT_TO_UTC`.
fn feed(body: &[String]) -> String {
    let mut lines = vec![
        format!(r#"{{"type":"start","schema":"{FEED_SCHEMA}","app":"test","boot_ns":0}}"#),
        format!(r#"{{"type":"clock","boot_ns":0,"utc_ns":{BOOT_TO_UTC},"basis":"gnss"}}"#),
    ];
    lines.extend_from_slice(body);
    lines.join("\n") + "\n"
}

fn line(seq: u64, boot_ns: i64, receiver: Option<&str>, raw: &str) -> String {
    serde_json::to_string(&FeedLine::Line {
        seq,
        boot_ns,
        source: "sensio".to_owned(),
        receiver: receiver.map(str::to_owned),
        remote_ns: None,
        raw: raw.to_owned(),
        withheld: false,
    })
    .expect("a feed line serializes")
}

const BLE: &str = r#"{"kind":"ble_advert","addr":"aa:aa:aa:00:00:01","rssi_dbm":-60}"#;
const JUDGE: &str = r#"{"type":"judge","boot_ns":0}"#;

fn drive_feed(events: &[Event], judge_after: &[bool]) -> String {
    let mut body = Vec::new();
    for (i, (event, &judged)) in events.iter().zip(judge_after).enumerate() {
        let raw = serde_json::to_string(&event.kind).expect("a kind serializes");
        let seq = u64::try_from(i).expect("small") + 1;
        body.push(line(
            seq,
            event.t_ns - BOOT_TO_UTC,
            event.receiver.as_deref(),
            &raw,
        ));
        if judged {
            body.push(JUDGE.to_owned());
        }
    }
    body.push(JUDGE.to_owned());
    feed(&body)
}

#[test]
fn drive_feed_matches_golden() {
    let lines = live(FEED, false);
    assert_eq!(
        lines[0],
        VerdictLine::Start {
            schema: VERDICTS_SCHEMA,
            core: env!("CARGO_PKG_VERSION"),
            rule_set_version: sensio_core::RULE_SET_VERSION,
        }
    );
    // The start line names the core's version, which moves on each release.
    insta::assert_json_snapshot!(lines[1..]);
}

#[test]
fn drive_feed_with_withheld_lines_matches_golden() {
    insta::assert_json_snapshot!(live(FEED, true)[1..]);
}

#[test]
fn every_withheld_line_judged_gives_the_drive_verdicts() {
    let drive = parse_jsonl(Cursor::new(DRIVE)).expect("the fixture parses");
    let lines = live(FEED, true);
    let last = answers(&lines).pop().expect("an answer");
    assert_eq!(last, judge(&drive, &PersistenceRule::default()));
}

#[test]
fn a_withheld_line_is_skipped_unless_asked_for() {
    let sightings = |lines: &[VerdictLine]| {
        let last = answers(lines).pop().expect("an answer").to_vec();
        last.iter()
            .find(|v| v.device.as_str() == "ble:aa:aa:aa:00:00:01")
            .map(|v| v.sightings)
    };
    assert_eq!(sightings(&live(FEED, false)), Some(3));
    assert_eq!(sightings(&live(FEED, true)), Some(4));
}

#[test]
fn the_same_feed_gives_the_same_stream() {
    let bytes = |lines: Vec<VerdictLine>| serde_json::to_vec(&lines).expect("serializes");
    assert_eq!(bytes(live(FEED, false)), bytes(live(FEED, false)));
}

#[test]
fn a_crlf_feed_gives_the_same_stream() {
    assert_eq!(live(&FEED.replace('\n', "\r\n"), false), live(FEED, false));
}

proptest! {
    /// ADR-0005 section 4: each answer is the judge over the events that
    /// arrived before its `judge` line, in whatever order they arrived.
    #[test]
    fn each_answer_is_the_judge_over_the_events_before_it(
        (events, judge_after) in prop_oneof![Just(DRIVE), Just(WIFI_DRIVE)]
            .prop_map(|text| parse_jsonl(Cursor::new(text)).expect("the fixture parses"))
            .prop_flat_map(|events| {
                let n = events.len();
                (Just(events).prop_shuffle(), proptest::collection::vec(any::<bool>(), n))
            })
    ) {
        let rule = PersistenceRule::default();
        let lines = live(&drive_feed(&events, &judge_after), false);
        let mut expected = Vec::new();
        for (i, &judged) in judge_after.iter().enumerate() {
            if judged {
                expected.push(judge(&events[..=i], &rule));
            }
        }
        expected.push(judge(&events, &rule));
        let got: Vec<Vec<Verdict>> = answers(&lines).into_iter().map(<[Verdict]>::to_vec).collect();
        prop_assert_eq!(got, expected);
        prop_assert!(rejections(&lines).is_empty());
    }
}

#[test]
fn a_receiver_is_kept_on_its_events() {
    let lines = live(
        &feed(&[line(1, 0, Some("rear-pi"), BLE), JUDGE.to_owned()]),
        false,
    );
    let VerdictLine::Coverage(coverage) = &lines[1] else {
        panic!("coverage before the answer");
    };
    assert_eq!(coverage.sources[0].receiver.as_deref(), Some("rear-pi"));
    assert_eq!(
        (coverage.sources[0].lines, coverage.sources[0].events),
        (1, 1)
    );
}

#[test]
fn a_remote_stamp_goes_through_its_receivers_offset() {
    let gnss = |seq, boot_ns: i64, lat: f64| {
        line(
            seq,
            boot_ns,
            None,
            &format!(r#"{{"kind":"gnss_fix","lat_deg":{lat},"lon_deg":10.0}}"#),
        )
    };
    // A sighting at phone boot time `at`, stamped by a receiver whose clock
    // runs 500 s ahead of the phone's, arriving 300 s late: its own stamp
    // places it at the fix before it, its arrival wouldn't.
    let remote = |seq, at: i64| {
        serde_json::to_string(&FeedLine::Line {
            seq,
            boot_ns: at + 300_000_000_000,
            source: "sensio".to_owned(),
            receiver: Some("rear-pi".to_owned()),
            remote_ns: Some(at + 500_000_000_000),
            raw: BLE.to_owned(),
            withheld: false,
        })
        .expect("serializes")
    };
    let offset = r#"{"type":"offset","receiver":"rear-pi","offset_ns":-500000000000,"uncertainty_ns":1000000}"#;
    let body = [
        offset.to_owned(),
        gnss(1, 0, 60.0),
        remote(2, 1_000_000_000),
        gnss(3, 600_000_000_000, 60.01),
        remote(4, 601_000_000_000),
        gnss(5, 1_200_000_000_000, 60.02),
        remote(6, 1_201_000_000_000),
        JUDGE.to_owned(),
    ];
    let lines = live(&feed(&body), false);
    assert!(rejections(&lines).is_empty(), "{lines:?}");
    let verdicts = answers(&lines).pop().expect("an answer").to_vec();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0].first_seen_ns, BOOT_TO_UTC + 1_000_000_000);
    assert_eq!(verdicts[0].unlocated_sightings, 0);
}

#[test]
fn a_feed_that_doesnt_start_with_start_is_fatal() {
    assert_eq!(
        fatal(&format!("{JUDGE}\n")),
        Fatal::NotAFeed {
            line: 1,
            reason: "the first line isn't a feed's start line"
        }
    );
    assert_eq!(
        fatal("not json\n"),
        Fatal::NotAFeed {
            line: 1,
            reason: "the first line isn't a feed's start line"
        }
    );
    assert_eq!(fatal("\n\n"), Fatal::Empty);
    assert_eq!(fatal(""), Fatal::Empty);
}

#[test]
fn an_unknown_feed_schema_is_fatal() {
    let start = r#"{"type":"start","schema":"sensio.feed/2","app":"test","boot_ns":0}"#;
    assert_eq!(
        fatal(&format!("\n{start}\n")),
        Fatal::NotAFeed {
            line: 2,
            reason: "unknown feed schema"
        }
    );
}

#[test]
fn a_seq_that_doesnt_rise_is_rejected_with_its_line() {
    let lines = live(
        &feed(&[
            line(5, 0, None, BLE),
            line(5, 0, None, BLE),
            line(4, 0, None, BLE),
        ]),
        false,
    );
    assert_eq!(
        rejections(&lines),
        [
            (4, Some(5), "seq doesn't rise"),
            (5, Some(4), "seq doesn't rise")
        ]
    );
}

#[test]
fn an_unknown_line_type_is_rejected_without_quoting_it() {
    let lines = live(&feed(&[r#"{"type":"secret-value"}"#.to_owned()]), false);
    let [(3, None, reason)] = rejections(&lines)[..] else {
        panic!("one rejection: {lines:?}");
    };
    assert!(
        reason.starts_with("unknown variant (value left out), expected one of"),
        "{reason}"
    );
}

#[test]
fn a_line_with_no_clock_before_it_is_rejected() {
    let start = format!(r#"{{"type":"start","schema":"{FEED_SCHEMA}","app":"test","boot_ns":0}}"#);
    let lines = live(&format!("{start}\n{}\n", line(1, 0, None, BLE)), false);
    assert_eq!(
        rejections(&lines),
        [(
            2,
            Some(1),
            "no clock line before it, or its time maps out of range"
        )]
    );
}

#[test]
fn a_second_start_line_is_rejected() {
    let start = format!(r#"{{"type":"start","schema":"{FEED_SCHEMA}","app":"test","boot_ns":0}}"#);
    assert_eq!(
        rejections(&live(&feed(&[start]), false)),
        [(3, None, "a second start line")]
    );
}

#[test]
fn a_bad_receiver_or_source_name_is_rejected() {
    let bad_source = line(1, 0, None, BLE).replace(r#""source":"sensio""#, r#""source":"Kismet!""#);
    let lines = live(
        &feed(&[
            line(1, 0, Some("Rear Pi"), BLE),
            bad_source,
            r#"{"type":"offset","receiver":"","offset_ns":0,"uncertainty_ns":0}"#.to_owned(),
            r#"{"type":"dropped","source":"sensio","receiver":"x/y","lines":1}"#.to_owned(),
        ]),
        false,
    );
    assert_eq!(
        rejections(&lines),
        [
            (3, Some(1), "source or receiver isn't a valid name"),
            (4, Some(1), "source or receiver isn't a valid name"),
            (5, None, "receiver isn't a valid name"),
            (6, None, "source or receiver isn't a valid name"),
        ]
    );
}

#[test]
fn a_source_with_no_normalizer_is_rejected() {
    let other = line(1, 0, None, BLE).replace(r#""source":"sensio""#, r#""source":"kismet""#);
    assert_eq!(
        rejections(&live(&feed(&[other]), false)),
        [(3, Some(1), "no normalizer for this source")]
    );
}

#[test]
fn an_out_of_range_event_is_rejected_by_the_cores_checks() {
    let raw = r#"{"kind":"gnss_fix","lat_deg":91.0,"lon_deg":10.0}"#;
    assert_eq!(
        rejections(&live(&feed(&[line(1, 0, None, raw)]), false)),
        [(3, Some(1), "latitude out of range")]
    );
}

#[test]
fn times_out_of_range_are_rejected() {
    let lines = live(
        &feed(&[
            line(1, -1, None, BLE),
            r#"{"type":"clock","boot_ns":-1,"utc_ns":0,"basis":"system"}"#.to_owned(),
            r#"{"type":"judge","boot_ns":-1}"#.to_owned(),
            r#"{"type":"slept","boot_ns":0,"slept_ns":-1}"#.to_owned(),
            r#"{"type":"offset","receiver":"pi","offset_ns":0,"uncertainty_ns":-1}"#.to_owned(),
            // A clock this far ahead makes every arrival overflow.
            format!(r#"{{"type":"clock","boot_ns":0,"utc_ns":{}}}"#, i64::MAX)
                .replace('}', r#","basis":"system"}"#),
            line(2, i64::MAX, None, BLE),
        ]),
        false,
    );
    assert_eq!(
        rejections(&lines),
        [
            (3, Some(1), "line time out of range"),
            (4, None, "clock time out of range"),
            (5, None, "judge time out of range"),
            (6, None, "sleep time out of range"),
            (7, None, "offset uncertainty out of range"),
            (
                9,
                Some(2),
                "no clock line before it, or its time maps out of range"
            ),
        ]
    );
}

#[test]
fn dropped_counts_that_would_overflow_stop_at_the_maximum() {
    let dropped = |n: u64| format!(r#"{{"type":"dropped","source":"sensio","lines":{n}}}"#);
    let lines = live(&feed(&[dropped(u64::MAX), dropped(1)]), false);
    let Some(VerdictLine::Coverage(coverage)) = lines.iter().rev().nth(1) else {
        panic!("coverage before the end line");
    };
    assert_eq!(coverage.sources[0].dropped, u64::MAX);
}

#[test]
fn a_line_too_long_is_rejected_and_the_stream_goes_on() {
    let long = line(1, 0, None, &"x".repeat(MAX_LINE_BYTES));
    let lines = live(
        &feed(&[long, line(2, 0, None, BLE), JUDGE.to_owned()]),
        false,
    );
    assert_eq!(
        rejections(&lines),
        [(
            3,
            None,
            format!("line longer than {MAX_LINE_BYTES} bytes").as_str()
        )]
    );
    assert_eq!(answers(&lines)[0].len(), 0);
    let Some(VerdictLine::End { events, .. }) = lines.last() else {
        panic!("an end line");
    };
    assert_eq!(*events, 1);
}

#[test]
fn a_line_of_exactly_the_limit_is_read_and_one_byte_more_is_not() {
    const INPUT: &str = "abc
abcd

ab";
    fn read_all(mut input: impl BufRead) -> Vec<BoundedLine> {
        std::iter::from_fn(|| read_bounded_line(&mut input, 3).expect("a cursor reads")).collect()
    }
    let text = |bytes: &[u8], terminated| BoundedLine::Text {
        bytes: bytes.to_vec(),
        terminated,
    };
    let expected = [
        text(b"abc", true),
        BoundedLine::TooLong,
        text(b"", true),
        text(b"ab", false),
    ];
    assert_eq!(read_all(Cursor::new(INPUT)), expected);
    // A line split across the reader's buffer refills reads the same.
    assert_eq!(
        read_all(BufReader::with_capacity(2, Cursor::new(INPUT))),
        expected
    );
}

#[test]
fn a_torn_last_line_is_counted_not_rejected() {
    let text = feed(&[line(1, 0, None, BLE)]) + r#"{"type":"line","seq":2,"boot"#;
    let lines = live(&text, false);
    assert_eq!(rejections(&lines), vec![]);
    let Some(VerdictLine::Coverage(coverage)) = lines.iter().rev().nth(1) else {
        panic!("coverage before the end line");
    };
    assert_eq!(coverage.torn, 1);
}

#[test]
fn a_last_line_torn_inside_a_character_is_counted_not_rejected() {
    // An SSID ending in an e with an acute accent, two bytes in UTF-8.
    const E_ACUTE: char = '\u{e9}';
    let ap = format!(
        r#"{{"kind":"wifi_ap","bssid":"ee:ee:ee:00:00:01","ssid":"kaf{E_ACUTE}","freq_mhz":2437}}"#
    );
    let last = line(2, 0, None, &ap);
    let cut = last
        .find(E_ACUTE)
        .expect("the SSID has a two-byte character")
        + 1;
    let mut bytes = feed(&[line(1, 0, None, BLE)]).into_bytes();
    bytes.extend_from_slice(&last.as_bytes()[..cut]);
    let mut out = Vec::new();
    run(
        Live::new(Sensio, PersistenceRule::default(), false),
        Cursor::new(bytes),
        |line| {
            out.push(line.clone());
            Ok(())
        },
    )
    .expect("the feed is judged");
    assert_eq!(rejections(&out), vec![]);
    let Some(VerdictLine::Coverage(coverage)) = out.iter().rev().nth(1) else {
        panic!("coverage before the end line");
    };
    assert_eq!(coverage.torn, 1);
}

#[test]
fn an_unterminated_last_line_that_parses_is_used() {
    let text = feed(&[line(1, 0, None, BLE)]) + JUDGE;
    assert_eq!(answers(&live(&text, false)).len(), 1);
}

#[test]
fn a_line_that_isnt_utf8_is_rejected() {
    let mut bytes = feed(&[]).into_bytes();
    bytes.extend_from_slice(b"\xff\xfe\n");
    let mut out = Vec::new();
    run(
        Live::new(Sensio, PersistenceRule::default(), false),
        Cursor::new(bytes),
        |line| {
            out.push(line.clone());
            Ok(())
        },
    )
    .expect("the feed is judged");
    assert_eq!(rejections(&out), [(3, None, "not UTF-8")]);
}

fn radio(boot_ns: i64, radio: &str, state: &str) -> String {
    format!(
        r#"{{"type":"radio","boot_ns":{boot_ns},"radio":"{radio}","receiver":"phone","state":"{state}"}}"#
    )
}

fn judge_at(boot_ns: i64) -> String {
    format!(r#"{{"type":"judge","boot_ns":{boot_ns}}}"#)
}

/// Every coverage line in the stream, the final one last.
fn coverages(lines: &[VerdictLine]) -> Vec<&Coverage> {
    lines
        .iter()
        .filter_map(|line| match line {
            VerdictLine::Coverage(coverage) => Some(coverage),
            _ => None,
        })
        .collect()
}

#[test]
fn a_radios_filtered_and_lost_time_counts_up_to_each_answer() {
    let s = 1_000_000_000;
    let lines = live(
        &feed(&[
            radio(0, "ble", "on"),
            radio(10 * s, "ble", "filtered"),
            judge_at(25 * s),
            radio(40 * s, "ble", "on"),
            radio(50 * s, "ble", "off"),
            radio(55 * s, "ble", "failed"),
            judge_at(60 * s),
        ]),
        false,
    );
    assert!(rejections(&lines).is_empty(), "{lines:?}");
    let seen: Vec<_> = coverages(&lines)
        .iter()
        .map(|c| {
            let ble = &c.radios[0];
            (
                c.boot_ns,
                ble.state,
                ble.since_boot_ns,
                ble.filtered_ns,
                ble.lost_ns,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (Some(25 * s), RadioState::Filtered, 10 * s, 15 * s, 0),
            (Some(60 * s), RadioState::Failed, 55 * s, 30 * s, 10 * s),
            // The end covers up to the latest time the feed gave.
            (Some(60 * s), RadioState::Failed, 55 * s, 30 * s, 10 * s),
        ]
    );
}

#[test]
fn a_radio_line_that_goes_back_or_names_badly_is_rejected() {
    let lines = live(
        &feed(&[
            radio(10, "ble", "on"),
            radio(5, "ble", "off"),
            radio(-1, "wifi", "on"),
            radio(0, "bad name", "on"),
            r#"{"type":"radio","boot_ns":0,"radio":"ble","receiver":"no good","state":"on"}"#
                .to_owned(),
        ]),
        false,
    );
    assert_eq!(
        rejections(&lines),
        [
            (4, None, "radio time goes back"),
            (5, None, "radio time out of range"),
            (6, None, "radio or receiver isn't a valid name"),
            (7, None, "radio or receiver isn't a valid name"),
        ]
    );
    let end = coverages(&lines)[0];
    assert_eq!(end.radios.len(), 1);
    assert_eq!(end.radios[0].state, RadioState::On);
}

#[test]
fn an_unknown_radio_state_is_rejected() {
    let lines = live(&feed(&[radio(0, "ble", "sideways")]), false);
    assert_eq!(rejections(&lines).len(), 1);
    assert!(coverages(&lines)[0].radios.is_empty(), "{lines:?}");
}

#[test]
fn coverage_gives_each_kinds_latest_arrival() {
    let gnss = r#"{"kind":"gnss_fix","lat_deg":60.01,"lon_deg":10.01}"#;
    let lines = live(
        &feed(&[
            // A later line can carry an earlier arrival; the latest counts.
            line(1, 9, None, BLE),
            line(2, 7, None, gnss),
            line(3, 8, None, BLE),
        ]),
        false,
    );
    let source = &coverages(&lines)[0].sources[0];
    assert_eq!(source.last_boot_ns, Some(9));
    let kinds: Vec<_> = source
        .kinds
        .iter()
        .map(|(kind, k)| (*kind, k.events, k.last_boot_ns))
        .collect();
    assert_eq!(kinds, [("ble_advert", 2, 9), ("gnss_fix", 1, 7)]);
}
