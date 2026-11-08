//! The laptop's side of the link to the phone (ADR-0004 section 6, amended
//! 2026-11-07): one TCP connection over the USB tether carrying JSON lines,
//! `sensio.link/1`, each with its `type`.
//!
//! - The laptop opens with `hello`: the schema, the receiver's name
//!   (`laptop`), its version, and, to pair, the code the app shows.
//! - To a code, the phone answers `welcome` with a new shared secret, which
//!   the laptop keeps ([`Pairing`]). Without a code, the phone sends a
//!   `challenge` with a fresh nonce and the laptop answers with a `proof`,
//!   the nonce's HMAC-SHA256 under the secret ([`proof`]), so the secret
//!   never crosses the link again; then `welcome`.
//! - Either way, a refusal is `refused` with a reason, and the phone closes.
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
use sha2::{Digest, Sha256};

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
        /// The pairing code the app shows, to pair; absent for a laptop
        /// already paired.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// The receiver's name, as the event schema writes it.
        receiver: String,
        /// The laptop program and its version.
        app: String,
    },
    /// The phone asks a paired laptop to prove it holds the secret.
    Challenge {
        /// A fresh nonce, in hex.
        nonce: String,
    },
    /// The laptop's answer to a challenge: [`proof`], in hex.
    Proof {
        /// The HMAC-SHA256, in hex.
        mac: String,
    },
    /// The phone took the hello; to a code, with the new pairing's secret.
    Welcome {
        /// The shared secret, in hex, when the hello paired.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secret: Option<String>,
    },
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
    /// The phone refused the hello: a wrong code, or not paired.
    #[error("the phone refused the link: {0}")]
    Refused(String),
    /// The connection failed or closed.
    #[error("the link to the phone closed: {0}")]
    Closed(String),
}

/// A pairing with the phone: this laptop's receiver name and the secret the
/// phone gave it, kept between runs so later links need no code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pairing {
    /// The receiver name the pairing was made under.
    pub receiver: String,
    /// The shared secret, in hex.
    pub secret: String,
}

/// How the laptop shows the phone it's allowed in.
#[derive(Debug, Clone)]
pub enum Auth {
    /// The code the app shows: pairs anew.
    Code(String),
    /// A pairing made before.
    Paired(Pairing),
}

/// The bytes the secret's hex text stands for, if it's 16 to 64 bytes of hex.
fn secret_bytes(hex: &str) -> Option<Vec<u8>> {
    // Digits only: `from_str_radix` alone takes a sign ("+a").
    if hex.len() % 2 != 0
        || !(32..=128).contains(&hex.len())
        || !hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// HMAC-SHA256 (RFC 2104), written out over `sha2`: the `hmac` crate's
/// constant-time comparison helper is BSD-licensed, and this side only
/// computes MACs, never compares them.
#[must_use]
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner = Sha256::new()
        .chain_update(block.map(|b| b ^ 0x36))
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(block.map(|b| b ^ 0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

/// The answer to a challenge, in hex: the HMAC-SHA256 under the pairing's
/// secret of `sensio.link/1|<receiver>|<nonce>`, binding the answer to this
/// schema and receiver. `None` if the secret isn't hex of a usable length.
#[must_use]
pub fn proof(pairing: &Pairing, nonce: &str) -> Option<String> {
    let secret = secret_bytes(&pairing.secret)?;
    let message = format!("{SCHEMA}|{}|{nonce}", pairing.receiver);
    Some(to_hex(&hmac_sha256(&secret, message.as_bytes())))
}

/// What a link session needs.
pub struct Config {
    /// The phone's address on the tether.
    pub phone: SocketAddr,
    /// A code to pair with, or a pairing made before.
    pub auth: Auth,
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

/// The hello and its answer: a code pairs, a pairing proves itself.
fn hello(
    config: &Config,
    reader: &mut BufReader<TcpStream>,
    writer: &Mutex<BufWriter<TcpStream>>,
    log: &mut dyn FnMut(&str),
    paired: &mut dyn FnMut(Pairing),
) -> Result<(), LinkError> {
    let receiver = match &config.auth {
        Auth::Code(_) => config.receiver.clone(),
        Auth::Paired(pairing) => pairing.receiver.clone(),
    };
    send(
        writer,
        &LinkLine::Hello {
            schema: SCHEMA.to_owned(),
            code: match &config.auth {
                Auth::Code(code) => Some(code.clone()),
                Auth::Paired(_) => None,
            },
            receiver: receiver.clone(),
            app: config.app.clone(),
        },
    )?;
    let unexpected = || LinkError::Closed("the phone didn't answer the hello".to_owned());
    let mut answer = read_line(reader)?;
    if let (Auth::Paired(pairing), LinkLine::Challenge { nonce }) = (&config.auth, &answer) {
        let mac = proof(pairing, nonce).ok_or_else(|| {
            LinkError::Refused("the saved pairing's secret isn't usable".to_owned())
        })?;
        send(writer, &LinkLine::Proof { mac })?;
        answer = read_line(reader)?;
    }
    match (answer, &config.auth) {
        (
            LinkLine::Welcome {
                secret: Some(secret),
            },
            Auth::Code(_),
        ) => {
            if secret_bytes(&secret).is_none() {
                return Err(unexpected());
            }
            log("paired with the phone; later links need no code");
            paired(Pairing { receiver, secret });
        }
        (LinkLine::Welcome { secret: None }, Auth::Paired(_)) => {}
        (LinkLine::Refused { reason }, _) => return Err(LinkError::Refused(reason)),
        _ => return Err(unexpected()),
    }
    Ok(())
}

/// One link session: connects, says hello (pairing, or proving a pairing),
/// then answers pings and passes on each poll's Kismet records until the
/// connection ends or `stop` is set. `clock` is the laptop's clock in
/// nanoseconds; `log` gets a line for each change worth telling the user;
/// `paired` gets a new pairing to keep.
///
/// # Errors
///
/// [`LinkError::Refused`] if the phone refused the hello (a wrong code, or
/// not paired), [`LinkError::Closed`] if the connection failed or closed
/// (try again).
pub fn run_session(
    config: &Config,
    poller: &mut Poller,
    clock: fn() -> i64,
    stop: &AtomicBool,
    log: &mut dyn FnMut(&str),
    paired: &mut dyn FnMut(Pairing),
) -> Result<(), LinkError> {
    let closed = |err: std::io::Error| LinkError::Closed(err.to_string());
    let stream =
        TcpStream::connect_timeout(&config.phone, Duration::from_secs(5)).map_err(closed)?;
    stream.set_nodelay(true).map_err(closed)?;
    let mut reader = BufReader::new(stream.try_clone().map_err(closed)?);
    let writer = Arc::new(Mutex::new(BufWriter::new(
        stream.try_clone().map_err(closed)?,
    )));
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(closed)?;
    hello(config, &mut reader, &writer, log, paired)?;
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
