//! Find mode's bearing (ADR-0004 sections 3 and 4): which way a BLE device is
//! from the receiver, estimated from a sweep.
//!
//! The user turns slowly in place with the phone held in front, so the body
//! shadows the signal from behind. Each reading of the target is paired with
//! the receiver's latest heading, binned into sectors by azimuth, and the
//! sector with the strongest median reading is the bearing. A pure function
//! of the events and [`FindParams`], like the judge: no clock, no I/O.
//!
//! The bearing is from magnetic north, as the heading sensor gives it, and
//! only BLE adverts are used: the phone's BLE scan is the only source that
//! reports a device about once a second, often enough for a sweep.

use serde::Serialize;

use crate::event::{Event, EventKind, TimeNs};

/// The version of find mode's estimate; bumped when its logic or a default changes.
pub const FIND_VERSION: u32 = 1;

/// Find mode's parameters; [`Default`] is the version's.
#[derive(Debug, Clone, PartialEq)]
pub struct FindParams {
    /// The number of sectors a turn is divided into, at least 4 and at most 72.
    pub sectors: usize,
    /// A reading pairs with a heading at most this old, in nanoseconds.
    pub max_heading_age_ns: TimeNs,
    /// A heading whose pitch is steeper than this, in degrees either way, isn't
    /// used: the azimuth is unreliable with the phone held near upright.
    pub max_pitch_deg: f64,
    /// The sectors with readings needed before a bearing is given.
    pub min_sectors: usize,
    /// The dB the strongest sector must stand above the others' median.
    pub min_contrast_db: f64,
}

impl Default for FindParams {
    fn default() -> Self {
        Self {
            sectors: 12,
            max_heading_age_ns: 1_000_000_000,
            max_pitch_deg: 70.0,
            min_sectors: 6,
            min_contrast_db: 3.0,
        }
    }
}

/// How far a sweep has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindStatus {
    /// No reading of the target paired with a heading yet.
    NoReadings,
    /// Fewer sectors than [`FindParams::min_sectors`] have readings: keep turning.
    Turn,
    /// The sweep covers enough sectors, but no sector stands out by
    /// [`FindParams::min_contrast_db`].
    Unclear,
    /// A bearing, in [`FindAnswer::bearing_deg`].
    Found,
}

/// One sector's readings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sector {
    /// The sector's center, degrees from magnetic north.
    pub center_deg: f64,
    /// The readings paired into it.
    pub readings: usize,
    /// Their median signal strength, dBm, when there are any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_dbm: Option<f64>,
}

/// Find mode's answer for one device over one sweep.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FindAnswer {
    /// [`FIND_VERSION`].
    pub find_version: u32,
    /// The device looked for, as its ID's text (`ble:aa:bb:cc:dd:ee:ff`).
    pub device: String,
    /// The sweep's start: readings before it aren't used.
    pub from_ns: TimeNs,
    /// How far the sweep has got.
    pub status: FindStatus,
    /// The strongest sector's center, degrees from magnetic north, when
    /// `status` is `found`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearing_deg: Option<f64>,
    /// The strongest sector's median over the median of the other sectors with
    /// readings, dB, once two sectors have readings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast_db: Option<f64>,
    /// Readings paired with a heading.
    pub paired: usize,
    /// Readings with no heading recent enough, or only a steep one.
    pub unpaired: usize,
    /// The latest reading of the target with a strength, paired or not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_dbm: Option<i16>,
    /// When it was heard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_ns: Option<TimeNs>,
    /// Every sector, from the one starting at north, clockwise.
    pub sectors: Vec<Sector>,
}

/// Find mode's estimate for `device` over the events from `from_ns` on.
///
/// Events are taken in time order, a heading before a reading at the same
/// instant, then input order. A reading pairs only with a heading from its
/// own receiver.
#[must_use]
pub fn find<'a>(
    events: impl IntoIterator<Item = &'a Event>,
    device: &str,
    from_ns: TimeNs,
    params: &FindParams,
) -> FindAnswer {
    // Clamped to 4..=72, so the count converts to u32 and f64 exactly.
    let count = u32::try_from(params.sectors.clamp(4, 72)).unwrap_or(12);
    let width = 360.0 / f64::from(count);
    let events: Vec<&Event> = events.into_iter().collect();
    let sweep = pair(&events, device, from_ns, params, width, count);
    let sectors: Vec<Sector> = (0..count)
        .zip(sweep.bins)
        .map(|(i, mut readings)| Sector {
            center_deg: width * (f64::from(i) + 0.5),
            readings: readings.len(),
            median_dbm: median(&mut readings),
        })
        .collect();
    let covered: Vec<(usize, f64)> = sectors
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.median_dbm.map(|m| (i, m)))
        .collect();
    // The strongest sector; a tie goes to the first from north.
    let strongest = covered
        .iter()
        .copied()
        .reduce(|best, s| if s.1 > best.1 { s } else { best });
    let contrast_db = strongest
        .filter(|_| covered.len() >= 2)
        .and_then(|(best, m)| {
            let mut others: Vec<f64> = covered
                .iter()
                .filter(|s| s.0 != best)
                .map(|s| s.1)
                .collect();
            others.sort_by(f64::total_cmp);
            median_f64(&others).map(|o| m - o)
        });
    let status = if covered.is_empty() {
        FindStatus::NoReadings
    } else if covered.len() < params.min_sectors {
        FindStatus::Turn
    } else if contrast_db.is_some_and(|c| c >= params.min_contrast_db) {
        FindStatus::Found
    } else {
        FindStatus::Unclear
    };
    FindAnswer {
        find_version: FIND_VERSION,
        device: device.to_owned(),
        from_ns,
        status,
        bearing_deg: strongest
            .filter(|_| status == FindStatus::Found)
            .map(|(i, _)| sectors[i].center_deg),
        contrast_db,
        paired: sweep.paired,
        unpaired: sweep.unpaired,
        latest_dbm: sweep.latest.map(|l| l.1),
        latest_ns: sweep.latest.map(|l| l.0),
        sectors,
    }
}

/// The target's readings from a sweep, binned by the heading each paired with.
struct Sweep {
    bins: Vec<Vec<i16>>,
    paired: usize,
    unpaired: usize,
    latest: Option<(TimeNs, i16)>,
}

fn pair(
    events: &[&Event],
    device: &str,
    from_ns: TimeNs,
    params: &FindParams,
    width: f64,
    count: u32,
) -> Sweep {
    let mut order: Vec<usize> = (0..events.len()).collect();
    order.sort_by_key(|&i| {
        (
            events[i].t_ns,
            !matches!(events[i].kind, EventKind::Heading { .. }),
            i,
        )
    });
    // The latest heading per receiver: its time, and its azimuth when not too steep.
    let mut headings: Vec<(Option<&str>, TimeNs, Option<f64>)> = Vec::new();
    let mut sweep = Sweep {
        bins: (0..count).map(|_| Vec::new()).collect(),
        paired: 0,
        unpaired: 0,
        latest: None,
    };
    for &i in &order {
        let event = events[i];
        let receiver = event.receiver.as_deref();
        if let EventKind::Heading {
            azimuth_deg,
            pitch_deg,
            ..
        } = event.kind
        {
            let usable = (pitch_deg.abs() <= params.max_pitch_deg).then_some(azimuth_deg);
            match headings.iter_mut().find(|h| h.0 == receiver) {
                Some(h) => *h = (receiver, event.t_ns, usable),
                None => headings.push((receiver, event.t_ns, usable)),
            }
            continue;
        }
        let EventKind::BleAdvert {
            rssi_dbm: Some(rssi),
            ..
        } = event.kind
        else {
            continue;
        };
        if event.t_ns < from_ns || event.kind.device().is_none_or(|d| d.as_str() != device) {
            continue;
        }
        sweep.latest = Some((event.t_ns, rssi));
        let azimuth = headings
            .iter()
            .find(|h| h.0 == receiver)
            .filter(|h| event.t_ns - h.1 <= params.max_heading_age_ns)
            .and_then(|h| h.2);
        let Some(az) = azimuth else {
            sweep.unpaired += 1;
            continue;
        };
        // The last sector starting at or before the azimuth; validation holds it to [0, 360).
        let index = (0..count)
            .rev()
            .find(|&s| width * f64::from(s) <= az)
            .unwrap_or(0);
        if let Some(bin) = sweep.bins.get_mut(index as usize) {
            bin.push(rssi);
            sweep.paired += 1;
        }
    }
    sweep
}

/// The median of readings, sorting them in place; the mean of the middle two
/// for an even count.
fn median(readings: &mut [i16]) -> Option<f64> {
    readings.sort_unstable();
    let values: Vec<f64> = readings.iter().map(|&r| f64::from(r)).collect();
    median_f64(&values)
}

/// The median of sorted values.
fn median_f64(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(sorted[n / 2]),
        _ => Some(sorted[n / 2 - 1] + (sorted[n / 2] - sorted[n / 2 - 1]) / 2.0),
    }
}
