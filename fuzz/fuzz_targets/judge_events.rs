//! The judge over an arbitrary session, built as JSON lines and read through
//! `parse_jsonl` so only events the core accepts reach it. Beyond not
//! panicking (overflow checks included, with `-a`): every verdict is
//! consistent with the rule, and a stable sort by time changes nothing.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use sensio_core::geo::distance_m;
use sensio_core::{Event, PersistenceRule, judge, parse_jsonl};
use sensio_fuzz::{Kind, Rule, Time};

#[derive(Arbitrary, Debug)]
struct Session {
    rule: Option<Rule>,
    events: Vec<(Time, Option<u8>, Kind)>,
}

const RECEIVERS: [&str; 3] = ["phone", "laptop", "rear-pi"];
const MAX_EVENTS: usize = 2000;

fuzz_target!(|session: Session| {
    let rule = match session.rule.map(Rule::rule) {
        None => PersistenceRule::default(),
        Some(rule) => {
            let bad_separation =
                !rule.place_separation_m.is_finite() || rule.place_separation_m <= 0.0;
            if rule.validate().is_err() {
                return;
            }
            assert!(
                !bad_separation,
                "validate passed a separation that makes every sighting a place"
            );
            rule
        }
    };

    let mut cursor = 1_800_000_000_000_000_000;
    let mut events: Vec<Event> = Vec::new();
    for (time, receiver, kind) in session.events.iter().take(MAX_EVENTS) {
        let event = Event {
            t_ns: time.at(&mut cursor),
            source: "fuzz".to_owned(),
            receiver: receiver.map(|r| RECEIVERS[usize::from(r) % RECEIVERS.len()].to_owned()),
            kind: kind.event_kind(),
        };
        // Serialized the way a source would write it; serde_json writes a
        // non-finite float as null, which the core then rejects.
        let line = serde_json::to_vec(&event).expect("an event serializes");
        if let Ok(mut parsed) = parse_jsonl(&line[..]) {
            events.append(&mut parsed);
        }
    }

    let verdicts = judge(&events, &rule);
    for pair in verdicts.windows(2) {
        assert!(
            pair[0].device < pair[1].device,
            "devices out of order or repeated"
        );
    }
    for v in &verdicts {
        assert!(v.first_seen_ns <= v.last_seen_ns, "{v:?}");
        assert!(
            v.last_seen_ns.saturating_sub(v.first_seen_ns) >= rule.min_span_ns,
            "{v:?}"
        );
        assert!(v.unlocated_sightings <= v.sightings, "{v:?}");
        assert!(
            v.places.len() <= v.sightings - v.unlocated_sightings,
            "{v:?}"
        );
        assert!(v.places.len() >= rule.min_places, "{v:?}");
        for (i, p) in v.places.iter().enumerate() {
            assert!(
                (v.first_seen_ns..=v.last_seen_ns).contains(&p.first_seen_ns),
                "{v:?}"
            );
            assert!(p.stopped_from_ns <= p.first_seen_ns, "{v:?}");
            assert!(
                p.stopped_to_ns.saturating_sub(p.stopped_from_ns) >= rule.min_stop_ns,
                "a place shorter than the stop it needs: {v:?}"
            );
            for q in &v.places[i + 1..] {
                assert!(
                    p.first_seen_ns <= q.first_seen_ns,
                    "places out of time order: {v:?}"
                );
                let d = distance_m(p.lat_deg, p.lon_deg, q.lat_deg, q.lon_deg);
                assert!(d >= rule.place_separation_m, "places {d} m apart: {v:?}");
            }
        }
    }

    let mut sorted = events.clone();
    sorted.sort_by_key(|e| e.t_ns);
    assert_eq!(
        judge(&sorted, &rule),
        verdicts,
        "a stable sort by time changed the verdicts"
    );
});
