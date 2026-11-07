//! The `sensio` source: a raw line that is already one normalized event's
//! kind and fields, without `t_ns` or `source` (ADR-0005 section 8).
//!
//! It carries synthetic drives and fake sources through the feed before the
//! other normalizers exist. The event's time and receiver come from the feed
//! line, as for every source.

use sensio_core::EventKind;
use sensio_core::event::reason_without_values;

/// The feed's name for this source.
pub const SOURCE: &str = "sensio";

/// The one event in `raw`.
///
/// # Errors
///
/// Returns why `raw` isn't an event kind, without quoting its values. Range
/// checks are the core's, made on the event the line gives.
pub fn normalize(raw: &str) -> Result<Vec<EventKind>, String> {
    serde_json::from_str(raw)
        .map(|kind| vec![kind])
        .map_err(|err| reason_without_values(&err))
}
