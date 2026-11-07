//! The laptop's side of the link to the phone (ADR-0004 section 6, amended
//! 2026-11-07): one TCP connection over the USB tether carrying JSON lines,
//! `sensio.link/1`, each with its `type`.
//!
//! - The laptop opens with `hello`: the schema, the pairing code the app
//!   shows, the receiver's name (`thinkpad`) and its version.
//! - The phone answers `welcome`, or `refused` with a reason and closes.
//! - The phone sends `ping`s; the laptop answers each at once with a `pong`
//!   carrying its own clock, and the phone takes the clock offset from the
//!   round trip.
//! - The laptop sends a `line` for each source line, with its source, its
//!   time on the laptop's clock (`remote_ns`) and the raw text, and a
//!   `status` when its sources change state (Kismet stopped answering).
//!
//! Lines are at most [`sensio_core::feed::MAX_LINE_BYTES`] long either way.

use std::io::{BufReader, BufWriter, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sensio_core::feed::{BoundedLine, MAX_LINE_BYTES, read_bounded_line};
use serde::{Deserialize, Serialize};

use crate::kismet_poll::Poller;

/// The link's schema name.
pub const SCHEMA: &str = "sensio.link/1";

/// The port the app listens on.
pub const PORT: u16 = 8765;

/// One line of the link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LinkLine {
    /// The laptop's first line.
    Hello {
        /// [`SCHEMA`].
        schema: String,
        /// The pairing code the app shows.
        code: String,
        /// The receiver's name, as the event schema writes it.
        receiver: String,
        /// The laptop program and its version.
        app: String,
    },
    /// The phone took the hello.
    Welcome,
    /// The phone refused the hello, and closes.
    Refused {
        /// Why.
        reason: String,
    },
    /// The phone asks for the laptop's clock.
    Ping {
        /// Echoed in the pong.
        id: u64,
    },
    /// The laptop's clock, as soon as the ping came.
    Pong {
        /// The ping's ID.
        id: u64,
        /// The laptop's clock, nanoseconds.
        remote_ns: i64,
    },
    /// One source line, as the laptop received it.
    Line {
        /// The program it came from (`kismet`).
        source: String,
        /// When the laptop received it, on its own clock.
        remote_ns: i64,
        /// The line's text.
        raw: String,
    },
    /// A change in the laptop's sources, shown in the app.
    Status {
        /// What changed, with no values from a radio.
        text: String,
    },
}

/// Why a link session ended.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// The phone refused the hello: a wrong code, or the link isn't on.
    #[error("the phone refused the link: {0}")]
    Refused(String),
    /// The connection failed or closed.
    #[error("the link to the phone closed: {0}")]
    Closed(String),
}

/// What a link session needs.
pub struct Config {
    /// The phone's address on the tether.
    pub phone: SocketAddr,
    /// The pairing code the app shows.
    pub code: String,
    /// This laptop's receiver name.
    pub receiver: String,
    /// This program and its version.
    pub app: String,
    /// How often to poll Kismet.
    pub poll_every: Duration,
}

/// The phone's address on the USB tether: the gateway of the default route
/// through a USB network interface, from `/proc/net/route`'s text.
#[must_use]
pub fn tether_gateway(route_table: &str) -> Option<Ipv4Addr> {
    route_table.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (iface, destination, gateway) = (fields.first()?, fields.get(1)?, fields.get(2)?);
        let usb = iface.starts_with("usb")
            || iface.starts_with("enx")
            || iface.starts_with("rndis")
            || (iface.starts_with("enp") && iface.contains('u'));
        if !usb || *destination != "00000000" {
            return None;
        }
        let gateway = u32::from_str_radix(gateway, 16).ok().filter(|&g| g != 0)?;
        // The kernel writes it in the host's byte order, little-endian here.
        Some(Ipv4Addr::from(gateway.swap_bytes()))
    })
}

fn send(writer: &Mutex<BufWriter<TcpStream>>, line: &LinkLine) -> Result<(), LinkError> {
    let closed = |err: std::io::Error| LinkError::Closed(err.to_string());
    let mut text = serde_json::to_string(line).map_err(|err| LinkError::Closed(err.to_string()))?;
    text.push('\n');
    let mut writer = writer
        .lock()
        .map_err(|_| LinkError::Closed("writer poisoned".to_owned()))?;
    writer.write_all(text.as_bytes()).map_err(closed)?;
    writer.flush().map_err(closed)
}

fn read_line(reader: &mut BufReader<TcpStream>) -> Result<LinkLine, LinkError> {
    match read_bounded_line(reader, MAX_LINE_BYTES) {
        Ok(Some(BoundedLine::Text {
            bytes,
            terminated: true,
        })) => serde_json::from_slice(&bytes)
            .map_err(|err| LinkError::Closed(format!("a line from the phone isn't one: {err}"))),
        Ok(Some(BoundedLine::TooLong)) => Err(LinkError::Closed(
            "a line from the phone is too long".to_owned(),
        )),
        Ok(_) => Err(LinkError::Closed(
            "the phone closed the connection".to_owned(),
        )),
        Err(err) => Err(LinkError::Closed(err.to_string())),
    }
}

/// One link session: connects, says hello, then answers pings and passes on
/// each poll's Kismet records until the connection ends or `stop` is set.
/// `clock` is the laptop's clock in nanoseconds; `log` gets a line for each
/// change worth telling the user.
///
/// # Errors
///
/// [`LinkError::Refused`] if the phone refused the hello (try another code),
/// [`LinkError::Closed`] if the connection failed or closed (try again).
pub fn run_session(
    config: &Config,
    poller: &mut Poller,
    clock: fn() -> i64,
    stop: &AtomicBool,
    log: &mut dyn FnMut(&str),
) -> Result<(), LinkError> {
    let closed = |err: std::io::Error| LinkError::Closed(err.to_string());
    let stream =
        TcpStream::connect_timeout(&config.phone, Duration::from_secs(5)).map_err(closed)?;
    stream.set_nodelay(true).map_err(closed)?;
    let mut reader = BufReader::new(stream.try_clone().map_err(closed)?);
    let writer = Arc::new(Mutex::new(BufWriter::new(
        stream.try_clone().map_err(closed)?,
    )));
    send(
        &writer,
        &LinkLine::Hello {
            schema: SCHEMA.to_owned(),
            code: config.code.clone(),
            receiver: config.receiver.clone(),
            app: config.app.clone(),
        },
    )?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(closed)?;
    match read_line(&mut reader)? {
        LinkLine::Welcome => {}
        LinkLine::Refused { reason } => return Err(LinkError::Refused(reason)),
        _ => {
            return Err(LinkError::Closed(
                "the phone didn't answer the hello".to_owned(),
            ));
        }
    }
    stream.set_read_timeout(None).map_err(closed)?;
    log(&format!("linked to the phone at {}", config.phone));

    // Pongs go out as soon as a ping comes, from their own thread.
    let ended = Arc::new(AtomicBool::new(false));
    let pongs = {
        let (writer, ended) = (Arc::clone(&writer), Arc::clone(&ended));
        thread::spawn(move || {
            while let Ok(line) = read_line(&mut reader) {
                if let LinkLine::Ping { id } = line {
                    let pong = LinkLine::Pong {
                        id,
                        remote_ns: clock(),
                    };
                    if send(&writer, &pong).is_err() {
                        break;
                    }
                }
            }
            ended.store(true, Ordering::SeqCst);
        })
    };

    let mut kismet_ok: Option<bool> = None;
    let result = loop {
        if stop.load(Ordering::SeqCst) {
            break Ok(());
        }
        if ended.load(Ordering::SeqCst) {
            break Err(LinkError::Closed(
                "the phone closed the connection".to_owned(),
            ));
        }
        let started = Instant::now();
        let answer = poller.poll();
        let now = clock();
        let status = match &answer {
            Ok(_) => (kismet_ok != Some(true)).then(|| "Kismet is answering".to_owned()),
            Err(err) => (kismet_ok != Some(false)).then(|| err.to_string()),
        };
        kismet_ok = Some(answer.is_ok());
        if let Some(text) = status {
            log(&text);
            if let Err(err) = send(&writer, &LinkLine::Status { text }) {
                break Err(err);
            }
        }
        if let Ok(records) = answer {
            let sent = records.into_iter().try_for_each(|raw| {
                send(
                    &writer,
                    &LinkLine::Line {
                        source: crate::kismet::SOURCE.to_owned(),
                        remote_ns: now,
                        raw,
                    },
                )
            });
            if let Err(err) = sent {
                break Err(err);
            }
        }
        thread::sleep(config.poll_every.saturating_sub(started.elapsed()));
    };
    let _ = stream.shutdown(std::net::Shutdown::Both);
    let _ = pongs.join();
    result
}
