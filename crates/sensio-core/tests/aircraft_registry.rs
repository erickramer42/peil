//! The bundled FAA registry snapshot (ADR-0007 section 5) and its reader.
//! Rows of the real snapshot are picked by the tests, never written in them,
//! so no aircraft's registration is quoted here; the reader's bounds are
//! tested on synthetic snapshots.

use std::io::Cursor;

use sensio_core::aircraft_registry::{Lookup, Registration};
use sensio_core::{AircraftRegistry, DescribeParams, VendorTable, describe, parse_jsonl};

const T0: i64 = 1_800_000_000_000_000_000;

/// The registrant types whose names are people's (`ardata.pdf`).
const PEOPLE: [&str; 4] = [
    "individual",
    "partnership",
    "co_owned",
    "non_citizen_co_owned",
];

fn every_row() -> Vec<(u32, Registration)> {
    let registry = AircraftRegistry::bundled().expect("the bundled snapshot's head reads");
    (0..registry.blocks())
        .flat_map(|i| registry.block(i).expect("every block reads"))
        .collect()
}

#[test]
fn the_bundled_snapshot_reads_in_address_order_with_the_count_its_sources_give() {
    let rows = every_row();
    assert!(
        rows.windows(2).all(|w| w[0].0 < w[1].0),
        "addresses rise across blocks"
    );
    assert!(
        rows.iter()
            .all(|(a, _)| (0xa0_0000..=0xaf_ffff).contains(a))
    );
    let sources = include_str!("../data/faa-aircraft.sources.txt");
    let listed = format!("    {} aircraft, ", rows.len());
    assert!(sources.contains(&listed), "{listed:?} not in the sources");
}

#[test]
fn no_row_names_a_registrant_who_is_a_person() {
    let rows = every_row();
    let named_people = rows
        .iter()
        .filter(|(_, r)| r.owner.is_some() && r.registrant.is_none_or(|t| PEOPLE.contains(&t)))
        .count();
    assert_eq!(named_people, 0);
    assert!(rows.iter().any(|(_, r)| r.owner.is_some()));
}

fn aircraft(address: &str, fields: &str) -> String {
    format!(
        r#"{{"t_ns":{T0},"source":"readsb","receiver":"laptop","kind":"aircraft","address":"{address}"{fields}}}"#
    )
}

fn described(line: &str) -> sensio_core::describe::Description {
    let events = parse_jsonl(Cursor::new(line)).expect("parses");
    let picture = describe(
        &events,
        T0,
        &DescribeParams::default(),
        &VendorTable::default(),
    );
    picture.emitters.into_iter().next().expect("one emitter")
}

#[test]
fn a_registered_address_is_described_with_its_registration() {
    let (address, registration) = every_row().swap_remove(0);
    let e = described(&aircraft(&format!("{address:06x}"), ""));
    let state = e.aircraft.expect("an aircraft");
    assert_eq!(state.registry, Some(Lookup::Found(registration.clone())));
    assert!(registration.n_number.starts_with('N'));
}

#[test]
fn a_rotorcraft_that_announces_no_category_is_categorized_by_its_registration() {
    let rows = every_row();
    let (address, _) = rows
        .iter()
        .find(|(_, r)| r.aircraft_type == Some("rotorcraft"))
        .expect("a rotorcraft in the snapshot");
    let e = described(&aircraft(&format!("{address:06x}"), ""));
    assert_eq!(
        e.category.map(|c| (c.category, c.entry)),
        Some(("rotorcraft", "faa-rotorcraft"))
    );
    let announced = described(&aircraft(
        &format!("{address:06x}"),
        r#","emitter_category":"A1""#,
    ));
    assert_eq!(
        announced.category.map(|c| c.entry),
        Some("adsb-a1"),
        "what the aircraft announces comes first"
    );
}

#[test]
fn an_unregistered_us_address_is_not_found_and_others_are_not_looked_up() {
    let rows = every_row();
    let gap = rows
        .windows(2)
        .find(|w| w[1].0 - w[0].0 > 1)
        .map(|w| w[0].0 + 1)
        .expect("a gap between registered addresses");
    let missing = described(&aircraft(&format!("{gap:06x}"), ""));
    assert_eq!(
        missing.aircraft.and_then(|a| a.registry),
        Some(Lookup::NotFound)
    );
    for address in ["bbbb01", "~a00001", "~bbbb01"] {
        let e = described(&aircraft(address, ""));
        assert_eq!(e.aircraft.and_then(|a| a.registry), None, "{address}");
    }
}

/// A snapshot of `blocks`, each its first address and its text, raw deflate.
fn snapshot(blocks: &[(u32, &str)]) -> Vec<u8> {
    let mut index = Vec::new();
    let mut data = Vec::new();
    for (first, text) in blocks {
        data.extend(miniz_oxide::deflate::compress_to_vec(text.as_bytes(), 6));
        index.extend(first.to_le_bytes());
        index.extend(u32::try_from(data.len()).expect("small").to_le_bytes());
    }
    let mut out = b"sensio.faa/1\n".to_vec();
    out.extend(u32::try_from(blocks.len()).expect("small").to_le_bytes());
    out.extend(index);
    out.extend(data);
    out
}

#[test]
fn a_synthetic_snapshot_reads_its_rows_by_delta() {
    let bytes = snapshot(&[
        (
            0xa0_0010,
            "0\t1A\tMAKER\tMODEL\t2001\t6\t3\t5\tAN AGENCY\n2\t2A\t\t\t\t4\t1\t1\t\n",
        ),
        (0xa0_0100, "0\t3A\t\t\t\t\t\t\t\n"),
    ]);
    let registry = AircraftRegistry::new(&bytes).expect("reads");
    let Some(Lookup::Found(first)) = registry.lookup("a00010") else {
        panic!("a00010 is registered");
    };
    assert_eq!(
        first,
        Registration {
            n_number: "N1A".into(),
            make: Some("MAKER".into()),
            model: Some("MODEL".into()),
            year: Some(2001),
            aircraft_type: Some("rotorcraft"),
            engine_type: Some("turbo_shaft"),
            registrant: Some("government"),
            owner: Some("AN AGENCY".into()),
        }
    );
    let n = |address: &str| match registry.lookup(address) {
        Some(Lookup::Found(r)) => Some(r.n_number),
        _ => None,
    };
    assert_eq!(n("A00012"), Some("N2A".into()));
    assert_eq!(n("a00100"), Some("N3A".into()));
    assert_eq!(registry.lookup("a00011"), Some(Lookup::NotFound));
    assert_eq!(registry.lookup("a00000"), Some(Lookup::NotFound));
    assert_eq!(registry.lookup("c00010"), None);
}

#[test]
fn a_snapshot_whose_head_is_wrong_is_rejected_with_why() {
    let good = snapshot(&[(0xa0_0010, "0\t1A\t\t\t\t\t\t\t\n")]);
    let reason = |bytes: &[u8]| AircraftRegistry::new(bytes).err().map(|e| e.reason);
    assert_eq!(
        reason(b"sensio.faa/2\n"),
        Some("not a sensio.faa/1 snapshot")
    );
    assert_eq!(reason(b"sensio.faa/1\n\x01\0"), Some("no block count"));
    assert_eq!(
        reason(b"sensio.faa/1\n\x02\0\0\0\0\0\0\0\0\0\0\0"),
        Some("index shorter than its count")
    );
    let mut cut = good.clone();
    cut.pop();
    assert_eq!(reason(&cut), Some("the last block doesn't end the data"));
    let falling = snapshot(&[
        (0xa0_0010, "0\t1A\t\t\t\t\t\t\t\n"),
        (0xa0_0010, "0\t2A\t\t\t\t\t\t\t\n"),
    ]);
    assert_eq!(reason(&falling), Some("index addresses or ends don't rise"));
    assert_eq!(reason(&good), None);
}

#[test]
fn a_block_that_isnt_rows_is_rejected_with_why_and_looks_up_as_not_found() {
    let reason = |text: &str| {
        let bytes = snapshot(&[(0xa0_0010, text)]);
        let registry = AircraftRegistry::new(&bytes).expect("the head reads");
        assert_eq!(registry.lookup("a00010"), Some(Lookup::NotFound));
        registry.block(0).err().map(|e| e.reason)
    };
    assert_eq!(reason("0\t1A\n"), Some("a line without nine fields"));
    assert_eq!(
        reason("x\t1A\t\t\t\t\t\t\t\n"),
        Some("a delta that isn't a number")
    );
    assert_eq!(
        reason("0\t1A\t\t\t\t\t\t\t\n0\t2A\t\t\t\t\t\t\t\n"),
        Some("an address listed twice")
    );
    assert_eq!(
        reason("0\tN-1\t\t\t\t\t\t\t\n"),
        Some("an N-number that isn't 1-5 letters and digits")
    );
    assert_eq!(
        reason(&format!("0\t1A\t{}\t\t\t\t\t\t\n", "M".repeat(65))),
        Some("a field over 64 bytes")
    );
    assert_eq!(
        reason("0\t1A\t\t\t\t\t\t\t\n16777216\t2A\t\t\t\t\t\t\t\n"),
        Some("an address past 24 bits")
    );
    let big = "0\t1A\t\t\t\t\t\t\t\n".to_owned() + &"1\t1A\t\t\t\t\t\t\t\n".repeat(7000);
    assert_eq!(reason(&big), Some("block doesn't inflate within 64 KiB"));
}
