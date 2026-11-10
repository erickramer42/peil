//! Distances and bearings between positions.

/// Mean Earth radius in meters (IUGG).
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Great-circle distance in meters between two positions given in degrees.
#[must_use]
pub fn distance_m(lat1_deg: f64, lon1_deg: f64, lat2_deg: f64, lon2_deg: f64) -> f64 {
    let (lat1, lat2) = (lat1_deg.to_radians(), lat2_deg.to_radians());
    let d_lat = lat2 - lat1;
    let d_lon = (lon2_deg - lon1_deg).to_radians();
    let a = (d_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().min(1.0).asin()
}

/// The initial great-circle bearing from the first position to the second,
/// degrees clockwise from true north, 0 to under 360; 0 for the same point.
#[must_use]
pub fn bearing_deg(lat1_deg: f64, lon1_deg: f64, lat2_deg: f64, lon2_deg: f64) -> f64 {
    let (lat1, lat2) = (lat1_deg.to_radians(), lat2_deg.to_radians());
    let d_lon = (lon2_deg - lon1_deg).to_radians();
    let y = d_lon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * d_lon.cos();
    let deg = y.atan2(x).to_degrees().rem_euclid(360.0);
    // rem_euclid can round a tiny negative angle up to exactly 360.
    if deg >= 360.0 { 0.0 } else { deg }
}

#[cfg(test)]
mod tests {
    use super::{bearing_deg, distance_m};

    #[test]
    fn bearings_point_the_four_ways() {
        let near = |a: f64, b: f64| (a - b).abs() < 0.01;
        assert!(near(bearing_deg(60.0, 10.0, 60.01, 10.0), 0.0));
        assert!(near(bearing_deg(60.0, 10.0, 60.0, 10.01), 90.0));
        assert!(near(bearing_deg(60.0, 10.0, 59.99, 10.0), 180.0));
        assert!(near(bearing_deg(60.0, 10.0, 60.0, 9.99), 270.0));
        assert!((0.0..360.0).contains(&bearing_deg(60.0, 10.0, 60.0, 10.0)));
    }

    #[test]
    fn same_point_is_zero() {
        assert!(distance_m(60.0, 10.0, 60.0, 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_hundredth_of_a_degree_of_latitude_is_about_1112_m() {
        let d = distance_m(60.0, 10.0, 60.01, 10.0);
        assert!((d - 1111.95).abs() < 1.0, "{d}");
    }
}
