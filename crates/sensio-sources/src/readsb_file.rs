//! Reading readsb across the process boundary (ADR-0007 section 1): the
//! aircraft list it writes as JSON (`--write-json`, `aircraft.json`, about
//! once a second), read as a file on the same computer.
//!
//! Its lines carry the source [`crate::readsb::SOURCE`]. Each read hands on, as one line, each aircraft with a message since it was
//! last handed on: its object as readsb wrote it, with the file's `now`
//! (seconds since the Unix epoch) added, the same shape readsb's JSON-lines
//! output gives. readsb's code is never linked.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Where readsb writes its aircraft list by default.
pub const DEFAULT_PATH: &str = "/run/readsb/aircraft.json";

/// The largest aircraft list read: a busy sky's few hundred aircraft are
/// well under a megabyte.
pub const MAX_FILE_BYTES: u64 = 16 << 20;

/// The most aircraft taken from one list.
pub const MAX_AIRCRAFT: usize = 4096;

/// How long the list's `now` may stand still before readsb reads as stopped.
pub const STALE_NS: i64 = 10_000_000_000;

/// The longest aircraft address kept: six hex digits, after a `~` for a
/// non-ICAO one.
const MAX_HEX_LEN: usize = 7;

/// Why a read gave nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// The file isn't there: readsb isn't running, or writes elsewhere.
    #[error("readsb isn't writing {0}")]
    Absent(String),
    /// The file is there but couldn't be read.
    #[error("readsb's {0} can't be read")]
    Unreadable(String),
    /// The file isn't an aircraft list, or is too long.
    #[error("readsb's file isn't an aircraft list: {0}")]
    NotAircraft(String),
    /// The list's time hasn't moved: readsb stopped writing it.
    #[error("readsb stopped updating its aircraft list")]
    Stale,
}

/// A reader of one readsb aircraft list.
pub struct Reader {
    path: PathBuf,
    last_now: Option<f64>,
    since_ns: i64,
    sent: HashMap<String, u64>,
}

impl Reader {
    /// A reader of the list at `path` ([`DEFAULT_PATH`] unless readsb was
    /// told otherwise).
    #[must_use]
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            last_now: None,
            since_ns: 0,
            sent: HashMap::new(),
        }
    }

    /// The lines, as JSON text, of the aircraft with a message since they
    /// were last handed on. `clock_ns` is the laptop's clock, which tells a
    /// list that stopped moving from one read twice in its second.
    ///
    /// # Errors
    ///
    /// Returns why readsb gave nothing; the reader keeps its state.
    pub fn read(&mut self, clock_ns: i64) -> Result<Vec<String>, ReadError> {
        let shown = self.path.display().to_string();
        let file = std::fs::File::open(&self.path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                ReadError::Absent(shown.clone())
            } else {
                ReadError::Unreadable(shown.clone())
            }
        })?;
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ReadError::Unreadable(shown))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(ReadError::NotAircraft("longer than 16 MiB".to_owned()));
        }
        self.take_body(&bytes, clock_ns)
    }

    /// The step of [`Reader::read`] after the file is read, bounded to
    /// [`MAX_FILE_BYTES`] before it gets here.
    ///
    /// # Errors
    ///
    /// [`ReadError::NotAircraft`] when the text isn't an object with a
    /// finite `now` and an `aircraft` list; [`ReadError::Stale`] when `now`
    /// hasn't moved for [`STALE_NS`].
    pub fn take_body(&mut self, body: &[u8], clock_ns: i64) -> Result<Vec<String>, ReadError> {
        let list: Value = serde_json::from_slice(body).map_err(|err| {
            ReadError::NotAircraft(sensio_core::event::reason_without_values(&err))
        })?;
        let now = list
            .get("now")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite())
            .ok_or_else(|| ReadError::NotAircraft("no time (`now`)".to_owned()))?;
        let aircraft = list
            .get("aircraft")
            .and_then(Value::as_array)
            .ok_or_else(|| ReadError::NotAircraft("no `aircraft` list".to_owned()))?;
        if self.last_now == Some(now) {
            return if clock_ns.saturating_sub(self.since_ns) > STALE_NS {
                Err(ReadError::Stale)
            } else {
                Ok(Vec::new())
            };
        }
        self.last_now = Some(now);
        self.since_ns = clock_ns;
        let mut out = Vec::new();
        let mut present = HashMap::new();
        for one in aircraft.iter().take(MAX_AIRCRAFT) {
            let Some(object) = one.as_object() else {
                continue;
            };
            let Some(hex) = object
                .get("hex")
                .and_then(Value::as_str)
                .filter(|h| !h.is_empty() && h.len() <= MAX_HEX_LEN)
            else {
                continue;
            };
            // An address listed twice in one list is taken once, the first time.
            if present.contains_key(hex) {
                continue;
            }
            let messages = object.get("messages").and_then(Value::as_u64).unwrap_or(0);
            present.insert(hex.to_owned(), messages);
            // A count that went down is a restarted readsb: new messages too.
            if self.sent.get(hex) == Some(&messages) {
                continue;
            }
            let mut line = object.clone();
            line.insert("now".to_owned(), list["now"].clone());
            out.push(Value::Object(line).to_string());
        }
        self.sent = present;
        Ok(out)
    }
}
