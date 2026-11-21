//! Drone Remote ID in a BLE advert (ADR-0007 section 6): ASTM F3411's
//! messages as Open Drone ID lays them out, read from service data under
//! 0xFFFA. A pure function of the advert's bytes, like the other advert
//! formats.
//!
//! The decoding is the `remoteid` crate's (MIT, `no_std`, no dependencies),
//! which follows `opendroneid-core-c` field for field and checks message
//! packs by the reference library's rules. This module maps its messages to
//! what the describe layer reads.
//!
//! Authentication and Self ID messages (free text an operator types) aren't
//! read.

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
    if uuid != "fffa" {
        return None;
    }
    let frame = match remoteid::ble::parse_service_data(data).ok()?.frame() {
        Ok(frame) => frame,
        Err(remoteid::Error::TooShort { .. }) => return None,
        // Remote ID, but a pack the reference library rejects.
        Err(_) => return Some(Vec::new()),
    };
    let mut out = Vec::new();
    frame.for_each_message(|m| out.extend(message(m)));
    Some(out)
}

/// One decoded message, or none for a type that isn't read.
fn message(m: remoteid::Message) -> Option<Message> {
    use remoteid::Message as M;
    match m {
        M::BasicId(b) => Some(Message::BasicId {
            id_type: b.id_type.into(),
            ua_type: b.ua_type.into(),
            uas_id: text(b.uas_id_str()),
        }),
        M::Location(l) => Some(Message::Location {
            status: Some(u8::from(l.status)).filter(|s| *s <= 4),
            direction_deg: l.direction.filter(|d| *d < 360.0).map(f64::from),
            speed_mps: l.speed_horizontal.map(f64::from),
            vertical_speed_mps: l.speed_vertical.map(f64::from),
            position: in_range(l.position()),
            alt_baro_m: l.altitude_baro.map(f64::from),
            alt_geo_m: l.altitude_geo.map(f64::from),
            height_m: l.height.map(f64::from),
        }),
        M::System(s) => Some(Message::System {
            operator_location_type: s.operator_location_type.into(),
            operator_position: in_range(s.operator_position()),
            operator_alt_geo_m: s.operator_altitude_geo.map(f64::from),
        }),
        M::OperatorId(o) => Some(Message::OperatorId {
            operator_id: text(o.operator_id_str()),
        }),
        _ => None,
    }
}

/// A position, none when either coordinate is out of range.
fn in_range(p: Option<(f64, f64)>) -> Option<(f64, f64)> {
    p.filter(|(lat, lon)| (-90.0..=90.0).contains(lat) && (-180.0..=180.0).contains(lon))
}

/// An ID field, trimmed; none when that leaves nothing.
fn text(s: Option<&str>) -> Option<String> {
    let s = s?.trim();
    (!s.is_empty()).then(|| s.to_owned())
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

    #[test]
    fn a_pack_with_two_locations_is_rejected() {
        let mut location = [0u8; 25];
        location[0] = 0x12; // Location, protocol 2.
        let mut pack = vec![0xf2, 25, 2];
        pack.extend_from_slice(&location);
        pack.extend_from_slice(&location);
        assert_eq!(messages("fffa", &wrap(&pack)), Some(Vec::new()));
    }
}
