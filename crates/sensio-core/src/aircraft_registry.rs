//! The FAA aircraft registry snapshot the core bundles (ADR-0007 section 5):
//! what an aircraft's Mode S address is registered as, by lookup.
//!
//! The snapshot is `data/faa-aircraft.bin`, made by `packaging/faa_registry.py`
//! from the FAA's releasable aircraft database on the date in
//! `data/faa-aircraft.sources.txt`. It holds no name the FAA lists as a
//! person's: a registrant's name is kept only for an organization, and an
//! amateur build's builder is never its manufacturer. Rows are sorted by address in
//! blocks, each raw deflate on its own, so a lookup inflates one block and
//! nothing is held in memory between lookups. The snapshot is a table like
//! the describe layer's tables in code, so `DESCRIBE_VERSION` names it.
//!
//! Its layout: `sensio.faa/1\n`, the block count (u32, little-endian), then
//! for each block its first address and the end of its bytes in the data
//! (u32 each), then the data. A block inflates to lines of tab-separated
//! fields: the address less the previous line's (the block's first address
//! for its first line) in decimal, the N-number without its `N`, the
//! manufacturer, the model, the year built, the aircraft type, the engine
//! type, the registrant type and the organization's name, any of them but the
//! first two empty. The reader checks every bound and never panics, whatever
//! the bytes.

use serde::Serialize;

const MAGIC: &[u8] = b"sensio.faa/1\n";
/// The most a block inflates to.
const MAX_BLOCK: usize = 64 << 10;
/// The longest field kept.
const MAX_FIELD: usize = 64;
/// The block of addresses the US holds (ICAO Annex 10, volume III, the
/// allocation table: A00000 to AFFFFF); the FAA registers nothing outside it.
const US_BLOCK: std::ops::RangeInclusive<u32> = 0xa0_0000..=0xaf_ffff;

/// The snapshot as of the date in `data/faa-aircraft.sources.txt`.
const BUNDLED: &[u8] = include_bytes!("../data/faa-aircraft.bin");

/// What the registry says about an address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "lookup", rename_all = "snake_case")]
pub enum Lookup {
    /// The address is registered.
    Found(Registration),
    /// The address is in the US block but not in the snapshot: an aircraft
    /// whose owner had it withheld, one sending a privacy address, or one
    /// registered since the snapshot.
    NotFound,
}

/// One registered aircraft, as the FAA's database lists it. The type codes'
/// meanings are the FAA's `ardata.pdf` (dated 5/8/2025, read 2026-11-09).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Registration {
    /// Its N-number, with the `N`.
    pub n_number: String,
    /// Its manufacturer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub make: Option<String>,
    /// Its model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The year it was built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// Its type (`rotorcraft`, `fixed_wing_single_engine` and the rest).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aircraft_type: Option<&'static str>,
    /// Its engine type (`turbo_shaft`, `reciprocating` and the rest).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_type: Option<&'static str>,
    /// Who it is registered to, by kind (`corporation`, `government`, `llc`,
    /// `individual` and the rest).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registrant: Option<&'static str>,
    /// The registrant's name, only where the registrant is an organization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

/// Why a snapshot couldn't be read.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct RegistryError {
    /// What's wrong.
    pub reason: &'static str,
}

fn err(reason: &'static str) -> RegistryError {
    RegistryError { reason }
}

/// A registry snapshot in the layout above, read in place.
#[derive(Debug, Clone, Copy)]
pub struct AircraftRegistry<'a> {
    index: &'a [u8],
    data: &'a [u8],
}

impl AircraftRegistry<'static> {
    /// The snapshot built into the core.
    ///
    /// # Errors
    ///
    /// Returns why its head doesn't read, which a test holds never happens.
    pub fn bundled() -> Result<Self, RegistryError> {
        AircraftRegistry::new(BUNDLED)
    }
}

impl<'a> AircraftRegistry<'a> {
    /// Reads a snapshot's head; blocks are read as lookups reach them.
    ///
    /// # Errors
    ///
    /// Returns why the head doesn't read: the magic, the count, or an index
    /// whose addresses or ends don't rise.
    pub fn new(bytes: &'a [u8]) -> Result<Self, RegistryError> {
        let rest = bytes
            .strip_prefix(MAGIC)
            .ok_or_else(|| err("not a sensio.faa/1 snapshot"))?;
        let count = u32_at(rest, 0).ok_or_else(|| err("no block count"))?;
        let index_len = usize::try_from(count)
            .ok()
            .and_then(|n| n.checked_mul(8))
            .ok_or_else(|| err("block count too large"))?;
        let index = rest
            .get(4..4 + index_len)
            .ok_or_else(|| err("index shorter than its count"))?;
        let data = rest.get(4 + index_len..).unwrap_or_default();
        let registry = Self { index, data };
        let mut previous: Option<(u32, u32)> = None;
        for i in 0..registry.blocks() {
            let (first, end) = registry.entry(i).ok_or_else(|| err("index cut short"))?;
            if previous.is_some_and(|(a, e)| first <= a || end < e) {
                return Err(err("index addresses or ends don't rise"));
            }
            previous = Some((first, end));
        }
        if previous.is_some_and(|(_, end)| usize::try_from(end).ok() != Some(data.len())) {
            return Err(err("the last block doesn't end the data"));
        }
        Ok(registry)
    }

    /// How many blocks it holds.
    #[must_use]
    pub fn blocks(&self) -> usize {
        self.index.len() / 8
    }

    /// What the registry says about an aircraft's address (six hex digits);
    /// `None` for an address outside the US block, which the FAA doesn't
    /// register, or one that isn't an ICAO address (readsb's `~`).
    #[must_use]
    pub fn lookup(&self, address: &str) -> Option<Lookup> {
        if address.len() != 6 || !address.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let address = u32::from_str_radix(address, 16).ok()?;
        if !US_BLOCK.contains(&address) {
            return None;
        }
        let found = self
            .block_for(address)
            .and_then(|i| self.block(i).ok())
            .and_then(|rows| {
                rows.into_iter()
                    .find_map(|(a, r)| (a == address).then_some(r))
            });
        Some(found.map_or(Lookup::NotFound, Lookup::Found))
    }

    /// Every row of block `i`, with its address, in order.
    ///
    /// # Errors
    ///
    /// Returns why the block doesn't read: its bounds, its deflate stream,
    /// its size inflated, or a line that isn't a row.
    pub fn block(&self, i: usize) -> Result<Vec<(u32, Registration)>, RegistryError> {
        let (first, end) = self.entry(i).ok_or_else(|| err("no such block"))?;
        let start = match i.checked_sub(1) {
            Some(before) => self.entry(before).map_or(0, |(_, e)| e),
            None => 0,
        };
        let bytes = usize::try_from(start)
            .ok()
            .zip(usize::try_from(end).ok())
            .and_then(|(s, e)| self.data.get(s..e))
            .ok_or_else(|| err("block outside the data"))?;
        let text = miniz_oxide::inflate::decompress_to_vec_with_limit(bytes, MAX_BLOCK)
            .map_err(|_| err("block doesn't inflate within 64 KiB"))?;
        let text = std::str::from_utf8(&text).map_err(|_| err("block isn't UTF-8"))?;
        let mut address = first;
        let mut rows = Vec::new();
        for line in text.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            let [
                delta,
                n_number,
                make,
                model,
                year,
                aircraft,
                engine,
                registrant,
                owner,
            ] = fields.as_slice()
            else {
                return Err(err("a line without nine fields"));
            };
            if fields.iter().any(|f| f.len() > MAX_FIELD) {
                return Err(err("a field over 64 bytes"));
            }
            let delta: u32 = delta
                .parse()
                .map_err(|_| err("a delta that isn't a number"))?;
            if !rows.is_empty() && delta == 0 {
                return Err(err("an address listed twice"));
            }
            address = address
                .checked_add(delta)
                .ok_or_else(|| err("an address past 24 bits"))?;
            let valid_n = (1..=5).contains(&n_number.len())
                && n_number.bytes().all(|b| b.is_ascii_alphanumeric());
            if !valid_n {
                return Err(err("an N-number that isn't 1-5 letters and digits"));
            }
            let text = |s: &str| (!s.is_empty()).then(|| s.to_owned());
            rows.push((
                address,
                Registration {
                    n_number: format!("N{n_number}"),
                    make: text(make),
                    model: text(model),
                    year: year.parse().ok(),
                    aircraft_type: aircraft_type(aircraft),
                    engine_type: engine_type(engine),
                    registrant: registrant_type(registrant),
                    owner: text(owner),
                },
            ));
        }
        if address > 0xff_ffff {
            return Err(err("an address past 24 bits"));
        }
        Ok(rows)
    }

    /// Block `i`'s first address and end.
    fn entry(&self, i: usize) -> Option<(u32, u32)> {
        let at = i.checked_mul(8)?;
        Some((
            u32_at(self.index, at)?,
            u32_at(self.index, at.checked_add(4)?)?,
        ))
    }

    /// The block that would hold `address`: the last whose first address is
    /// at or below it.
    fn block_for(&self, address: u32) -> Option<usize> {
        let (mut low, mut high) = (0, self.blocks());
        while low < high {
            let mid = low + (high - low) / 2;
            if self.entry(mid)?.0 <= address {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low.checked_sub(1)
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let four = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(four.try_into().ok()?))
}

/// TYPE AIRCRAFT (`ardata.pdf`).
fn aircraft_type(code: &str) -> Option<&'static str> {
    Some(match code {
        "1" => "glider",
        "2" => "balloon",
        "3" => "blimp",
        "4" => "fixed_wing_single_engine",
        "5" => "fixed_wing_multi_engine",
        "6" => "rotorcraft",
        "7" => "weight_shift_control",
        "8" => "powered_parachute",
        "9" => "gyroplane",
        "H" => "hybrid_lift",
        "O" => "other",
        _ => return None,
    })
}

/// TYPE ENGINE (`ardata.pdf`).
fn engine_type(code: &str) -> Option<&'static str> {
    Some(match code {
        "0" => "none",
        "1" => "reciprocating",
        "2" => "turbo_prop",
        "3" => "turbo_shaft",
        "4" => "turbo_jet",
        "5" => "turbo_fan",
        "6" => "ramjet",
        "7" => "two_cycle",
        "8" => "four_cycle",
        "9" => "unknown",
        "10" => "electric",
        "11" => "rotary",
        _ => return None,
    })
}

/// TYPE REGISTRANT (`ardata.pdf`).
fn registrant_type(code: &str) -> Option<&'static str> {
    Some(match code {
        "1" => "individual",
        "2" => "partnership",
        "3" => "corporation",
        "4" => "co_owned",
        "5" => "government",
        "7" => "llc",
        "8" => "non_citizen_corporation",
        "9" => "non_citizen_co_owned",
        _ => return None,
    })
}
