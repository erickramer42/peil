//! `peil summary`: a session's summary as text, JSON or an HTML page.
//!
//! The page draws the route from the session's own GNSS fixes as inline
//! SVG: no map tiles, no scripts, nothing fetched. It holds the session's
//! coordinates, so it is a local file like the feed it comes from.

use std::fmt::Write as _;

use sensio_core::Summary;
use sensio_core::describe::Address;
use sensio_core::feed::{Coverage, EndReason, RadioState};
use sensio_core::summary::{Flagged, RoutePoint};

const NS_PER_S: i64 = 1_000_000_000;

/// A serialized enum's name (`apple_find_my`, `access_point`).
fn name(value: serde_json::Result<serde_json::Value>) -> String {
    value
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A session time as `YYYY-MM-DD HH:MM:SS` UTC.
fn utc(t_ns: i64) -> String {
    let secs = t_ns.div_euclid(NS_PER_S);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to a civil date, after Howard Hinnant's days_from_civil inverse.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// Only the clock part of [`utc`].
fn clock(t_ns: i64) -> String {
    utc(t_ns)[11..].to_owned()
}

fn duration(ns: i64) -> String {
    let s = ns / NS_PER_S;
    if s < 120 {
        format!("{s} s")
    } else if s < 7_200 {
        format!("{} min", s / 60)
    } else {
        format!("{} h {} min", s / 3_600, s % 3_600 / 60)
    }
}

fn distance(m: f64) -> String {
    if m < 1_000.0 {
        format!("{m:.0} m")
    } else {
        format!("{:.1} km", m / 1_000.0)
    }
}

/// How the session ended, in words.
fn ending(end: EndReason) -> &'static str {
    match end {
        EndReason::Stopped => "stopped",
        EndReason::Cut => {
            "cut: the feed has no end line (the app killed, the phone dead, or the file cut)"
        }
    }
}

/// The session's line: its times, length, distance and stops.
fn session_line(s: &Summary) -> String {
    let mut out = String::new();
    if let (Some(first), Some(last)) = (s.session.first_ns, s.session.last_ns) {
        let to = if utc(first)[..10] == utc(last)[..10] {
            clock(last)
        } else {
            utc(last)
        };
        let _ = write!(
            out,
            "{} to {to} UTC ({}), ",
            utc(first),
            duration(last - first)
        );
    }
    let _ = write!(
        out,
        "{}, {} stop{} of {} or more",
        distance(s.route.distance_m),
        s.session.stops.len(),
        if s.session.stops.len() == 1 { "" } else { "s" },
        duration(s.session.min_stop_ns)
    );
    out
}

/// One line per receiver: its radios' states and time short of on.
fn coverage_lines(c: &Coverage) -> Vec<String> {
    let mut by_receiver: Vec<(String, Vec<String>)> = Vec::new();
    for r in &c.radios {
        let receiver = r.receiver.clone().unwrap_or_else(|| "phone".to_owned());
        let mut text = format!("{} {}", r.radio, name(serde_json::to_value(r.state)));
        let mut short = Vec::new();
        if r.filtered_ns > 0 {
            short.push(format!("filtered {}", duration(r.filtered_ns)));
        }
        if r.lost_ns > 0 {
            short.push(format!("lost {}", duration(r.lost_ns)));
        }
        if r.low_ns > 0 {
            short.push(format!("low {}", duration(r.low_ns)));
        }
        if r.state == RadioState::On && r.low_rate {
            short.push("low rate at the end".to_owned());
        }
        if !short.is_empty() {
            let _ = write!(text, " ({})", short.join(", "));
        }
        match by_receiver.iter_mut().find(|(rx, _)| *rx == receiver) {
            Some((_, radios)) => radios.push(text),
            None => by_receiver.push((receiver, vec![text])),
        }
    }
    let mut lines: Vec<String> = by_receiver
        .into_iter()
        .map(|(receiver, radios)| format!("{receiver}: {}", radios.join(", ")))
        .collect();
    let (mut lines_in, mut rejected, mut withheld, mut dropped, mut late) = (0, 0, 0, 0, 0);
    for source in &c.sources {
        lines_in += source.lines;
        rejected += source.rejected;
        withheld += source.withheld;
        dropped += source.dropped;
        late += source.late;
    }
    lines.push(format!(
        "{lines_in} lines: {rejected} rejected, {withheld} withheld, {dropped} dropped, {late} late; asleep {}",
        duration(c.slept_ns)
    ));
    lines
}

/// What the describe layer knows about a verdict's device, in a line.
fn device_line(f: &Flagged) -> String {
    let mut parts = vec![f.verdict.device.as_str().to_owned()];
    if let Some(d) = &f.described {
        parts.push(name(serde_json::to_value(d.kind)));
        if let Some(v) = &d.vendor {
            parts.push(format!("vendor {v}"));
        } else if d.address == Address::Randomized {
            parts.push("randomized address".to_owned());
        }
        if let Some(t) = &d.tracker {
            parts.push(format!("tracker {}", name(serde_json::to_value(t))));
        }
        if let Some(c) = &d.category {
            parts.push(format!("category {}", name(serde_json::to_value(c))));
        }
        if let Some(ssid) = &d.ssid {
            parts.push(format!("network {ssid:?}"));
        }
        if let Some(own) = &d.own {
            parts.push(format!("{own}'s own address"));
        }
    }
    parts.join(", ")
}

/// The text report.
pub fn text(s: &Summary) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Session {}", ending(s.session.end));
    let _ = writeln!(out, "  {}", session_line(s));
    for stop in &s.session.stops {
        let _ = writeln!(
            out,
            "  stop {} to {} ({}) at {:.4}, {:.4}",
            clock(stop.from_ns),
            clock(stop.to_ns),
            duration(stop.to_ns - stop.from_ns),
            stop.lat_deg,
            stop.lon_deg
        );
    }
    let _ = writeln!(out, "Coverage");
    for line in coverage_lines(&s.coverage) {
        let _ = writeln!(out, "  {line}");
    }
    let e = &s.emitters;
    let total = e.ble + e.access_point + e.station + e.bt_classic;
    let _ = writeln!(
        out,
        "Emitters heard: {total} (BLE {}, access points {}, stations {}, Classic {})",
        e.ble, e.access_point, e.station, e.bt_classic
    );
    let _ = writeln!(
        out,
        "Verdicts (rule set {}): {}",
        s.rule_set_version,
        s.verdicts.len()
    );
    for f in &s.verdicts {
        let v = &f.verdict;
        let _ = writeln!(out, "  {}: {}", v.rule, device_line(f));
        let unlocated = if v.unlocated_sightings > 0 {
            format!(" ({} unplaced)", v.unlocated_sightings)
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "    heard {} to {}, {} sightings{unlocated}, at {} places:",
            clock(v.first_seen_ns),
            clock(v.last_seen_ns),
            v.sightings,
            v.places.len()
        );
        for p in &v.places {
            let _ = writeln!(
                out,
                "      stop {} to {} at {:.4}, {:.4}, first heard {}",
                clock(p.stopped_from_ns),
                clock(p.stopped_to_ns),
                p.lat_deg,
                p.lon_deg,
                clock(p.first_seen_ns)
            );
        }
    }
    let _ = writeln!(
        out,
        "core {}, describe {}{}",
        s.core,
        s.describe_version,
        s.vendors
            .as_ref()
            .map(|v| format!(", vendors {v}"))
            .unwrap_or_default()
    );
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Projects latitude and longitude onto the page: an equirectangular
/// projection scaled at the route's middle latitude, north up.
struct Projection {
    lat_max: f64,
    lon_min: f64,
    cos_lat: f64,
    scale: f64,
    margin: f64,
}

const WIDTH: f64 = 800.0;
const MARGIN: f64 = 24.0;

impl Projection {
    fn new(points: &[(f64, f64)]) -> Option<(Self, f64)> {
        let (first_lat, first_lon) = *points.first()?;
        let (mut lat_min, mut lat_max, mut lon_min, mut lon_max) =
            (first_lat, first_lat, first_lon, first_lon);
        for &(lat, lon) in points {
            lat_min = lat_min.min(lat);
            lat_max = lat_max.max(lat);
            lon_min = lon_min.min(lon);
            lon_max = lon_max.max(lon);
        }
        let cos_lat = (lat_min + (lat_max - lat_min) / 2.0)
            .to_radians()
            .cos()
            .max(0.01);
        let span_x = ((lon_max - lon_min) * cos_lat).max(1e-6);
        let span_y = (lat_max - lat_min).max(1e-6);
        let span = span_x.max(span_y);
        let scale = (WIDTH - 2.0 * MARGIN) / span;
        let height = (span_y * scale + 2.0 * MARGIN).max(2.0 * MARGIN + 1.0);
        Some((
            Self {
                lat_max,
                lon_min,
                cos_lat,
                scale,
                margin: MARGIN,
            },
            height,
        ))
    }

    fn xy(&self, lat: f64, lon: f64) -> (f64, f64) {
        (
            self.margin + (lon - self.lon_min) * self.cos_lat * self.scale,
            self.margin + (self.lat_max - lat) * self.scale,
        )
    }
}

fn svg(s: &Summary) -> String {
    let route: &[RoutePoint] = &s.route.points;
    let mut all: Vec<(f64, f64)> = route.iter().map(|p| (p.lat_deg, p.lon_deg)).collect();
    all.extend(s.session.stops.iter().map(|p| (p.lat_deg, p.lon_deg)));
    let Some((proj, height)) = Projection::new(&all) else {
        return "<p>No GNSS fixes, so no route.</p>".to_owned();
    };
    let mut out = format!(
        r#"<svg viewBox="0 0 {WIDTH:.0} {height:.0}" role="img" aria-label="The route from the session's GNSS fixes">"#
    );
    let path: Vec<String> = route
        .iter()
        .map(|p| {
            let (x, y) = proj.xy(p.lat_deg, p.lon_deg);
            format!("{x:.1},{y:.1}")
        })
        .collect();
    let _ = write!(
        out,
        r#"<polyline class="route" points="{}"/>"#,
        path.join(" ")
    );
    for (i, stop) in s.session.stops.iter().enumerate() {
        let (x, y) = proj.xy(stop.lat_deg, stop.lon_deg);
        let _ = write!(
            out,
            r#"<circle class="stop" cx="{x:.1}" cy="{y:.1}" r="7"><title>Stop {} {} to {}</title></circle><text x="{:.1}" y="{:.1}">{}</text>"#,
            i + 1,
            clock(stop.from_ns),
            clock(stop.to_ns),
            x + 10.0,
            y + 4.0,
            i + 1
        );
    }
    out.push_str("</svg>");
    out
}

/// The HTML page: the route, the stops, the verdicts and coverage.
pub fn html(s: &Summary) -> String {
    let mut out = String::from(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Peil session summary</title>
<style>
:root { --bg: #fafaf7; --fg: #1d1d1b; --muted: #6b6b66; --line: #2f6f8f; --stop: #b5512b; --rule: #d9d9d3; }
@media (prefers-color-scheme: dark) { :root { --bg: #161615; --fg: #e8e8e3; --muted: #9a9a93; --line: #6fb3d2; --stop: #e08a5f; --rule: #33332f; } }
body { background: var(--bg); color: var(--fg); font: 15px/1.5 system-ui, sans-serif; margin: 0 auto; max-width: 880px; padding: 16px; }
h1 { font-size: 1.4em; margin: 0 0 4px; } h2 { font-size: 1.1em; margin: 24px 0 8px; }
p, li { margin: 4px 0; } .muted { color: var(--muted); }
svg { width: 100%; height: auto; border: 1px solid var(--rule); border-radius: 6px; }
.route { fill: none; stroke: var(--line); stroke-width: 3; stroke-linejoin: round; stroke-linecap: round; }
.stop { fill: var(--stop); } svg text { fill: var(--fg); font-size: 14px; }
table { border-collapse: collapse; width: 100%; } td, th { border-top: 1px solid var(--rule); padding: 6px 4px; text-align: left; vertical-align: top; }
code { font-size: 0.95em; }
</style>
</head>
<body>
"#,
    );
    let _ = writeln!(
        out,
        "<h1>Session {}</h1>\n<p>{}</p>",
        escape(ending(s.session.end)),
        escape(&session_line(s))
    );
    out.push_str(&svg(s));
    if !s.session.stops.is_empty() {
        out.push_str("\n<h2>Stops</h2>\n<ol>\n");
        for stop in &s.session.stops {
            let _ = writeln!(
                out,
                "<li>{} to {} ({})</li>",
                clock(stop.from_ns),
                clock(stop.to_ns),
                duration(stop.to_ns - stop.from_ns)
            );
        }
        out.push_str("</ol>\n");
    }
    let _ = writeln!(
        out,
        "<h2>Verdicts</h2>\n<p class=\"muted\">Rule set {}: {} verdict{}.</p>",
        s.rule_set_version,
        s.verdicts.len(),
        if s.verdicts.len() == 1 { "" } else { "s" }
    );
    if !s.verdicts.is_empty() {
        out.push_str("<table>\n<tr><th>Device</th><th>Heard</th><th>Places</th></tr>\n");
        for f in &s.verdicts {
            let v = &f.verdict;
            let places: Vec<String> = v
                .places
                .iter()
                .map(|p| {
                    format!(
                        "stop {} to {}, first heard {}",
                        clock(p.stopped_from_ns),
                        clock(p.stopped_to_ns),
                        clock(p.first_seen_ns)
                    )
                })
                .collect();
            let _ = writeln!(
                out,
                "<tr><td><code>{}</code><br>{}</td><td>{} to {}, {} sightings</td><td>{}</td></tr>",
                escape(v.device.as_str()),
                escape(&device_line(f)),
                clock(v.first_seen_ns),
                clock(v.last_seen_ns),
                v.sightings,
                places
                    .iter()
                    .map(|p| escape(p))
                    .collect::<Vec<_>>()
                    .join("<br>")
            );
        }
        out.push_str("</table>\n");
    }
    out.push_str("<h2>Coverage</h2>\n<ul>\n");
    for line in coverage_lines(&s.coverage) {
        let _ = writeln!(out, "<li>{}</li>", escape(&line));
    }
    out.push_str("</ul>\n");
    let _ = writeln!(
        out,
        "<p class=\"muted\">{} (core {}, rule set {}, describe {})</p>\n</body>\n</html>",
        escape(s.schema),
        escape(s.core),
        s.rule_set_version,
        s.describe_version
    );
    out
}

#[cfg(test)]
mod tests {
    use super::{duration, escape, utc};

    #[test]
    fn times_read_as_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00:00");
        assert_eq!(utc(1_800_000_000 * 1_000_000_000), "2027-01-15 08:00:00");
        assert_eq!(utc(951_782_400 * 1_000_000_000), "2000-02-29 00:00:00");
    }

    #[test]
    fn durations_and_escapes_read_plainly() {
        assert_eq!(duration(90 * 1_000_000_000), "90 s");
        assert_eq!(duration(3_000 * 1_000_000_000), "50 min");
        assert_eq!(duration(7_500 * 1_000_000_000), "2 h 5 min");
        assert_eq!(
            escape(r#"<a href="x">&'"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }
}
