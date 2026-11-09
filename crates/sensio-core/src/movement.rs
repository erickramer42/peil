//! Whether the vehicle is moving, measured over a window of GNSS fixes; the
//! describe layer's movement and the judge's stops both read it.
//!
//! Speed between two fixes a second apart reads a parked receiver's GNSS
//! jitter (a few meters) as moving; over a window of seconds it averages out.

use std::collections::VecDeque;

use crate::event::TimeNs;
use crate::geo::distance_m;

/// A fix's time, latitude and longitude.
pub(crate) type Fix = (TimeNs, f64, f64);

const NS_PER_S: i64 = 1_000_000_000;

/// The fixes a movement window can still reach, oldest first: the newest
/// fix at least a window before the latest, and those after it, at most one
/// a second but the latest, so a burst of fixes can't grow it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Window {
    fixes: VecDeque<Fix>,
}

impl Window {
    /// Adds the next fix and measures the window ending at it: whether the
    /// vehicle moved at least `moving_mps` from the newest fix at least
    /// `window_ns` older, and that fix; `None` until the track spans a
    /// window. Fixes come in the judge's order, so a fix earlier than the
    /// latest can't arrive; one would be skipped.
    pub(crate) fn observe(
        &mut self,
        fix: Fix,
        window_ns: TimeNs,
        moving_mps: f64,
    ) -> Option<(bool, Fix)> {
        let (t1, lat1, lon1) = fix;
        if self.fixes.back().is_some_and(|b| t1 < b.0) {
            return None;
        }
        let len = self.fixes.len();
        if len >= 2 && self.fixes[len - 1].0.saturating_sub(self.fixes[len - 2].0) < NS_PER_S {
            self.fixes.pop_back();
        }
        self.fixes.push_back(fix);
        while self
            .fixes
            .get(1)
            .is_some_and(|f| t1.saturating_sub(f.0) >= window_ns)
        {
            self.fixes.pop_front();
        }
        let start @ (t0, lat0, lon0) = *self.fixes.front()?;
        let elapsed = t1.saturating_sub(t0);
        if elapsed < window_ns {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        let seconds = elapsed as f64 / 1e9;
        let fast = distance_m(lat0, lon0, lat1, lon1) / seconds >= moving_mps;
        Some((fast, start))
    }
}

#[cfg(test)]
mod tests {
    use super::{NS_PER_S, Window};

    #[test]
    fn a_burst_of_fixes_keeps_the_window_bounded() {
        let mut window = Window::default();
        let t0 = 1_800_000_000 * NS_PER_S;
        // Ten thousand fixes at one instant, then a thousand a millisecond
        // apart, then a minute at 1 Hz.
        let times = std::iter::repeat_n(t0, 10_000)
            .chain((1..=1_000).map(|i| t0 + i * 1_000_000))
            .chain((1..=60).map(|i| t0 + 2 * NS_PER_S + i * NS_PER_S));
        let mut last = None;
        for t in times {
            last = window.observe((t, 60.0, 10.0), 10 * NS_PER_S, 2.0);
            // One a second across the window, its oldest and the latest.
            assert!(
                window.fixes.len() <= 13,
                "{} fixes held",
                window.fixes.len()
            );
        }
        assert_eq!(last.map(|(fast, _)| fast), Some(false));
    }
}
