//! Reading Kismet across the process boundary (ADR-0001 sections 1-2):
//! its REST API on the same computer, over plain HTTP.
//!
//! Each poll asks for the devices Kismet saw in the last few seconds
//! (`/devices/last-time/-N/devices.json`) and hands on, pruned
//! ([`kismet::prune`]), each one Kismet has seen again since the last time
//! it was handed on. Kismet's own log keeps the whole records.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use crate::kismet;

/// The largest response read from Kismet: a busy road's devices with their
/// histories are a few megabytes.
pub const MAX_RESPONSE_BYTES: u64 = 64 << 20;

/// Why a poll gave nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PollError {
    /// Kismet isn't answering.
    #[error("Kismet isn't answering at {0}")]
    Unreachable(String),
    /// Kismet answered with an HTTP error, a 401 for a wrong or missing API key.
    #[error("Kismet answered HTTP {0}{hint}", hint = if *.0 == 401 { " (check PEIL_KISMET_API_KEY)" } else { "" })]
    Status(u16),
    /// Kismet's answer wasn't a list of device records, or was too long.
    #[error("Kismet's answer isn't a list of device records: {0}")]
    NotDevices(String),
}

/// A poller of one Kismet server.
pub struct Poller {
    agent: ureq::Agent,
    base: String,
    api_key: Option<String>,
    window_s: u32,
    sent: HashMap<String, i64>,
}

impl Poller {
    /// A poller of the Kismet at `base` (`http://127.0.0.1:2501`), with an API
    /// key if Kismet asks for one, asking each time for the devices seen in
    /// the last `window_s` seconds.
    #[must_use]
    pub fn new(base: &str, api_key: Option<String>, window_s: u32) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
            base: base.trim_end_matches('/').to_owned(),
            api_key,
            window_s: window_s.max(1),
            sent: HashMap::new(),
        }
    }

    /// The pruned records, as JSON text, of the devices Kismet has seen
    /// again since they were last handed on.
    ///
    /// # Errors
    ///
    /// Returns why Kismet gave nothing; the poller keeps its state, so the
    /// next poll picks up where this one would have.
    pub fn poll(&mut self) -> Result<Vec<String>, PollError> {
        let url = format!(
            "{}/devices/last-time/-{}/devices.json",
            self.base, self.window_s
        );
        let mut request = self.agent.get(&url);
        if let Some(key) = &self.api_key {
            request = request.header("Cookie", format!("KISMET={key}"));
        }
        let mut response = request.call().map_err(|err| match err {
            ureq::Error::StatusCode(code) => PollError::Status(code),
            _ => PollError::Unreachable(self.base.clone()),
        })?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(|_| PollError::NotDevices("unreadable or too long".to_owned()))?;
        self.take_body(&bytes)
    }

    /// The pruned records, as JSON text, of the devices in one answer's
    /// body that Kismet has seen again since they were last handed on: the
    /// step of [`Poller::poll`] after the request, which bounds the body to
    /// [`MAX_RESPONSE_BYTES`] before it gets here.
    ///
    /// # Errors
    ///
    /// Returns [`PollError::NotDevices`] when the body isn't a JSON list;
    /// the poller's state is unchanged.
    pub fn take_body(&mut self, body: &[u8]) -> Result<Vec<String>, PollError> {
        let records: Vec<Value> = serde_json::from_slice(body).map_err(|err| {
            PollError::NotDevices(sensio_core::event::reason_without_values(&err))
        })?;
        Ok(self.fresh(&records))
    }

    /// The records seen again since they were last handed on, pruned.
    fn fresh(&mut self, records: &[Value]) -> Vec<String> {
        let mut out = Vec::new();
        for record in records {
            let text = |key: &str| record.get(key).and_then(Value::as_str);
            let Some(key) =
                text("kismet.device.base.key").or_else(|| text("kismet.device.base.macaddr"))
            else {
                continue;
            };
            let last = record
                .get("kismet.device.base.last_time")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if self.sent.get(key).is_some_and(|&sent| sent >= last) {
                continue;
            }
            self.sent.insert(key.to_owned(), last);
            out.push(kismet::prune(record).to_string());
        }
        out
    }
}
