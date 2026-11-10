//! Drone Remote ID in a BLE advert (ADR-0007 section 6): ASTM F3411's
//! messages as Open Drone ID lays them out, read from service data under
//! 0xFFFA. A pure function of the advert's bytes, like the other advert
//! formats.
//!
//! Sources (all read 2026-11-09, Apache-2.0):
//! - The wrapper: Open Drone ID's Android receiver filters service data
//!   under `0000fffa-0000-1000-8000-00805f9b34fb` whose first byte is the
//!   application code 0x0D (`OPEN_DRONE_ID_AD_CODE`), reads a message
//!   counter, then the 25-byte message, little-endian
//!   (`BluetoothScanner.java`, `OpenDroneIdParser.java`, github.com/opendroneid/receiver-android).
//! - The messages: `opendroneid.h` and `opendroneid.c`, github.com/opendroneid/opendroneid-core-c:
//!   the header byte's high nibble is the message type (Basic ID 0, Location
//!   1, System 4, Operator ID 5, Message Pack 0xF), and the field offsets,
//!   scalings and invalid values below.
//!
//! Authentication and Self ID messages (free text an operator types) aren't
//! read.

/// The application code that opens Open Drone ID's service data.
const APP_CODE: u8 = 0x0d;
/// Every message's size, bytes.
const MESSAGE_SIZE: usize = 25;
/// The most messages in a pack.
const MAX_PACK: usize = 9;

/// One Remote ID message, as far as the describe layer reads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Message {
    /// Basic ID: what the aircraft is and its ID.
    BasicId {
        /// `ODID_idtype_t`: 1 serial number, 2 CAA registration, 3 UTM UUID,
        /// 4 specific session ID.
        id_type: u8,
        /// `ODID_uatype_t`, 0 to 15.
        ua_type: u8,
        /// The ID, printable ASCII without its padding; none when empty.
        uas_id: Option<String>,
    },
    /// Location: where the drone is and how it moves.
    Location {
        /// `ODID_status_t`, 0 to 4.
        status: Option<u8>,
        /// Direction of travel, degrees from true north, 0 to under 360.
        direction_deg: Option<f64>,
        /// Horizontal speed, m/s.
        speed_mps: Option<f64>,
        /// Vertical speed, m/s, up positive.
        vertical_speed_mps: Option<f64>,
        /// Position, degrees; both or neither.
        position: Option<(f64, f64)>,
        /// Pressure altitude, meters.
        alt_baro_m: Option<f64>,
        /// Geodetic altitude, meters.
        alt_geo_m: Option<f64>,
        /// Height above takeoff or ground, meters.
        height_m: Option<f64>,
    },
    /// System: where the operator is.
    System {
        /// 0 takeoff, 1 live GNSS, 2 fixed.
        operator_location_type: u8,
        /// The operator's position, degrees; both or neither.
        operator_position: Option<(f64, f64)>,
        /// The operator's geodetic altitude, meters.
        operator_alt_geo_m: Option<f64>,
    },
    /// Operator ID: the operator's registration.
    OperatorId {
        /// Printable ASCII without its padding; none when empty.
        operator_id: Option<String>,
    },
}

/// The Remote ID messages in one service data entry, or none when it isn't
/// Remote ID. `uuid` is the entry's 16-bit UUID, lowercase, and `data` the
/// bytes after it.
pub(crate) fn messages(uuid: &str, data: &[u8]) -> Option<Vec<Message>> {
    if uuid != "fffa" || data.first() != Some(&APP_CODE) {
        return None;
    }
    // After the application code and the message counter.
    let body = data.get(2..)?;
    let first = body.get(..MESSAGE_SIZE)?;
    if first[0] >> 4 == 0xf {
        // A pack: the message size, the count, then the messages.
        let (&size, &count) = (body.get(1)?, body.get(2)?);
        if usize::from(size) != MESSAGE_SIZE || usize::from(count) > MAX_PACK {
            return Some(Vec::new());
        }
        let packed = body.get(3..3 + usize::from(count) * MESSAGE_SIZE)?;
        return Some(
            packed
                .chunks_exact(MESSAGE_SIZE)
                .filter_map(message)
                .collect(),
        );
    }
    Some(message(first).into_iter().collect())
}

/// One 25-byte message, or none for a type that isn't read.
fn message(m: &[u8]) -> Option<Message> {
    match m.first()? >> 4 {
        0 => Some(Message::BasicId {
            id_type: m.get(1)? >> 4,
            ua_type: m.get(1)? & 0x0f,
            uas_id: text(m.get(2..22)?),
        }),
        1 => location(m),
        4 => Some(Message::System {
            operator_location_type: m.get(1)? & 0x03,
            operator_position: position(m.get(2..6)?, m.get(6..10)?),
            operator_alt_geo_m: altitude(m.get(18..20)?),
        }),
        5 => Some(Message::OperatorId {
            operator_id: text(m.get(2..22)?),
        }),
        _ => None,
    }
}

fn location(m: &[u8]) -> Option<Message> {
    let flags = *m.get(1)?;
    let status = Some(flags >> 4).filter(|s| *s <= 4);
    let direction = u16::from(*m.get(2)?) + if flags & 0x02 != 0 { 180 } else { 0 };
    let speed = *m.get(3)?;
    let speed_mps = (speed != 255).then(|| {
        if flags & 0x01 == 0 {
            f64::from(speed) * 0.25
        } else {
            f64::from(speed).mul_add(0.75, 63.75)
        }
    });
    let vertical = i8::from_le_bytes([*m.get(4)?]);
    Some(Message::Location {
        status,
        direction_deg: (direction < 360).then(|| f64::from(direction)),
        speed_mps,
        // 63 m/s is the encoding's own "unknown".
        vertical_speed_mps: (vertical != 126).then(|| f64::from(vertical) * 0.5),
        position: position(m.get(5..9)?, m.get(9..13)?),
        alt_baro_m: altitude(m.get(13..15)?),
        alt_geo_m: altitude(m.get(15..17)?),
        height_m: altitude(m.get(17..19)?),
    })
}

/// A latitude and longitude, each int32 x 1e-7 degrees; none when both are 0
/// (the encoding's "invalid") or either is out of range.
fn position(lat: &[u8], lon: &[u8]) -> Option<(f64, f64)> {
    let int = |b: &[u8]| Some(i32::from_le_bytes(b.try_into().ok()?));
    let (lat, lon) = (int(lat)?, int(lon)?);
    if lat == 0 && lon == 0 {
        return None;
    }
    let (lat, lon) = (f64::from(lat) * 1e-7, f64::from(lon) * 1e-7);
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon))
}

/// An altitude, uint16 x 0.5 - 1000 meters; none for 0, the encoding's
/// "invalid" (-1000 m).
fn altitude(b: &[u8]) -> Option<f64> {
    let raw = u16::from_le_bytes(b.try_into().ok()?);
    (raw != 0).then(|| f64::from(raw).mul_add(0.5, -1000.0))
}

/// An ID field: printable ASCII up to its first NUL, trimmed; none when that
/// leaves nothing or holds anything else.
fn text(b: &[u8]) -> Option<String> {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    let s = std::str::from_utf8(b.get(..end)?).ok()?.trim();
    (!s.is_empty() && s.bytes().all(|c| c.is_ascii_graphic() || c == b' ')).then(|| s.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Message, messages};

    fn wrap(message: &[u8]) -> Vec<u8> {
        let mut data = vec![0x0d, 7];
        data.extend_from_slice(message);
        data
    }

    #[test]
    fn a_basic_id_names_its_type_and_id() {
        let mut m = [0u8; 25];
        m[0] = 0x02; // Basic ID, protocol 2.
        m[1] = 0x12; // Serial number, helicopter or multirotor.
        m[2..12].copy_from_slice(b"TEST000001");
        assert_eq!(
            messages("fffa", &wrap(&m)),
            Some(vec![Message::BasicId {
                id_type: 1,
                ua_type: 2,
                uas_id: Some("TEST000001".to_owned()),
            }])
        );
    }

    #[test]
    fn other_service_data_isnt_remote_id() {
        let m = [0u8; 25];
        assert_eq!(messages("fffb", &wrap(&m)), None);
        assert_eq!(messages("fffa", &m), None, "no application code");
        assert_eq!(
            messages("fffa", &wrap(&m[..24])),
            None,
            "a message cut short"
        );
    }
}
