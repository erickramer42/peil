//! `peil`: Peil's command-line front end.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use sensio_core::feed::VerdictLine;
use sensio_core::{
    DescribeParams, Live, PersistenceRule, RunError, VendorTable, describe, judge, parse_jsonl, run,
};
use sensio_sources::Sources;
use sensio_sources::kismet_poll::Poller;
use sensio_sources::link::{self, LinkError};

const NS_PER_S: i64 = 1_000_000_000;

/// The largest vendor registry file read; the IEEE's lists together are a
/// few megabytes.
const MAX_VENDORS_BYTES: u64 = 64 << 20;

/// The exit code for input that isn't a feed at all.
const EXIT_NOT_A_FEED: u8 = 2;

#[derive(Parser)]
#[command(
    name = "peil",
    version,
    about = "What is transmitting around a vehicle or a building, and is any of it unusual"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Judge a file of normalized events (JSONL) and print one verdict per line.
    Judge {
        /// The events file.
        path: PathBuf,
        #[command(flatten)]
        rule: RuleArgs,
    },
    /// Judge a live feed (sensio.feed/1) and write the verdict stream
    /// (sensio.verdicts/1), one answer per judge line.
    Live {
        /// A saved feed to replay [default: read the feed from stdin].
        path: Option<PathBuf>,
        /// Judge the lines the core didn't get live, too.
        #[arg(long)]
        all: bool,
        /// The IEEE's MA-L, MA-M and MA-S lists (CSV, one or several
        /// concatenated), for vendor names.
        #[arg(long)]
        vendors: Option<PathBuf>,
        #[command(flatten)]
        rule: RuleArgs,
    },
    /// Describe the emitters in a file of normalized events (JSONL), loud
    /// first, one per line, as of the last event.
    Describe {
        /// The events file.
        path: PathBuf,
        /// The IEEE's MA-L, MA-M and MA-S lists (CSV), for vendor names.
        #[arg(long)]
        vendors: Option<PathBuf>,
    },
    /// Send this computer's Kismet devices to the app on the phone over the
    /// USB tether (sensio.link/1), until stopped. Reads Kismet's API key from
    /// `PEIL_KISMET_API_KEY`, or else from `peil/kismet-api-key` in the
    /// user's configuration directory.
    Link(LinkArgs),
}

#[derive(Args)]
struct LinkArgs {
    /// The code the app shows, to pair this computer with the phone; once
    /// paired, later runs need none.
    #[arg(long)]
    code: Option<String>,
    /// The phone's address, `ADDRESS` or `ADDRESS:PORT` [default: the
    /// USB tether's gateway, on Linux].
    #[arg(long)]
    phone: Option<String>,
    /// This computer's name in the session's events (a-z, 0-9, '-', '_',
    /// '.'), when pairing [default: laptop; once paired, the pairing's].
    #[arg(long)]
    receiver: Option<String>,
    /// Kismet's address.
    #[arg(long, default_value = "http://127.0.0.1:2501")]
    kismet: String,
    /// Milliseconds between polls of Kismet.
    #[arg(long, default_value_t = 2000, value_parser = clap::value_parser!(u64).range(250..=60_000))]
    poll_ms: u64,
    /// Where the pairing is kept [default: peil/link-pairing.json in the
    /// user's configuration directory].
    #[arg(long)]
    pairing: Option<PathBuf>,
}

/// Reads the vendor registry, or none.
fn vendor_table(path: Option<&PathBuf>) -> Result<VendorTable> {
    let Some(path) = path else {
        return Ok(VendorTable::default());
    };
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_VENDORS_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() as u64 > MAX_VENDORS_BYTES {
        return Err(anyhow!("{}: longer than 64 MiB", path.display()));
    }
    VendorTable::parse(&bytes).with_context(|| format!("reading {}", path.display()))
}

#[derive(Args)]
struct RuleArgs {
    /// Distinct places a device must be seen at [default: the rule set's].
    #[arg(long)]
    min_places: Option<usize>,
    /// Seconds from a device's first sighting to its last, at least.
    #[arg(long)]
    min_span_s: Option<u32>,
    /// Meters between sightings for them to count as different places.
    #[arg(long)]
    place_separation_m: Option<f64>,
    /// Seconds a GNSS fix stays usable for placing a sighting.
    #[arg(long)]
    max_fix_age_s: Option<u32>,
}

impl RuleArgs {
    fn rule(&self) -> Result<PersistenceRule> {
        let defaults = PersistenceRule::default();
        let rule = PersistenceRule {
            min_places: self.min_places.unwrap_or(defaults.min_places),
            min_span_ns: self
                .min_span_s
                .map_or(defaults.min_span_ns, |s| i64::from(s) * NS_PER_S),
            place_separation_m: self
                .place_separation_m
                .unwrap_or(defaults.place_separation_m),
            max_fix_age_ns: self
                .max_fix_age_s
                .map_or(defaults.max_fix_age_ns, |s| i64::from(s) * NS_PER_S),
        };
        rule.validate()
            .map_err(|reason| anyhow!("invalid rule parameters: {reason}"))?;
        Ok(rule)
    }
}

fn main() -> ExitCode {
    match run_command(Cli::parse().command) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("Error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_command(command: Command) -> Result<ExitCode> {
    match command {
        Command::Judge { path, rule } => {
            let rule = rule.rule()?;
            let file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
            let events = parse_jsonl(BufReader::new(file))
                .with_context(|| format!("reading {}", path.display()))?;
            let mut out = std::io::stdout().lock();
            for verdict in judge(&events, &rule) {
                serde_json::to_writer(&mut out, &verdict)?;
                writeln!(out)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Describe { path, vendors } => {
            let vendors = vendor_table(vendors.as_ref())?;
            let file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
            let events = parse_jsonl(BufReader::new(file))
                .with_context(|| format!("reading {}", path.display()))?;
            let at_ns = events.iter().map(|e| e.t_ns).max().unwrap_or(0);
            let picture = describe(&events, at_ns, &DescribeParams::default(), &vendors);
            let mut out = std::io::stdout().lock();
            for emitter in &picture.emitters {
                serde_json::to_writer(&mut out, emitter)?;
                writeln!(out)?;
            }
            eprintln!(
                "{} emitters in the last {} s, {} older; movement: {}",
                picture.emitters.len(),
                DescribeParams::default().recent_ns / NS_PER_S,
                picture.older,
                serde_json::to_value(picture.movement)?
                    .as_str()
                    .unwrap_or("unknown"),
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Live {
            path,
            all,
            vendors,
            rule,
        } => {
            let rule = rule.rule()?;
            let vendors = vendor_table(vendors.as_ref())?;
            let (reader, name): (Box<dyn BufRead>, String) = match &path {
                Some(path) => {
                    let file =
                        File::open(path).with_context(|| format!("opening {}", path.display()))?;
                    (Box::new(BufReader::new(file)), path.display().to_string())
                }
                None => (Box::new(std::io::stdin().lock()), "stdin".to_owned()),
            };
            live(reader, &name, rule, all, vendors)
        }
        Command::Link(args) => link_until_stopped(&args),
    }
}

/// The laptop's clock: nanoseconds since the Unix epoch, UTC.
fn wall_clock_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

/// The phone's address: as given (the link's port if none), or the USB
/// tether's gateway.
fn phone_address(given: Option<&str>) -> Result<SocketAddr> {
    if let Some(given) = given {
        if let Ok(address) = given.parse::<SocketAddr>() {
            return Ok(address);
        }
        let ip: IpAddr = given
            .parse()
            .map_err(|_| anyhow!("--phone: not an address or address:port"))?;
        return Ok(SocketAddr::new(ip, link::PORT));
    }
    let routes = std::fs::read_to_string("/proc/net/route")
        .context("finding the phone without --phone needs Linux's /proc/net/route")?;
    let gateway = link::tether_gateway(&routes).ok_or_else(|| {
        anyhow!("no USB tether found; turn on USB tethering on the phone, or give --phone")
    })?;
    Ok(SocketAddr::new(IpAddr::V4(gateway), link::PORT))
}

/// The largest pairing or key file read; each is under 200 bytes.
const MAX_PAIRING_BYTES: u64 = 64 << 10;

/// The user's `peil` configuration directory: `peil` in `$XDG_CONFIG_HOME`,
/// `~/.config` or `%APPDATA%`.
fn config_dir() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    var("XDG_CONFIG_HOME")
        .or_else(|| var("HOME").map(|home| home.join(".config")))
        .or_else(|| var("APPDATA"))
        .map(|dir| dir.join("peil"))
}

/// Kismet's API key: `PEIL_KISMET_API_KEY`, or else the first line of
/// `kismet-api-key` in the configuration directory, or none.
fn kismet_api_key() -> Result<Option<String>> {
    if let Some(key) = std::env::var("PEIL_KISMET_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    {
        return Ok(Some(key));
    }
    let Some(path) = config_dir().map(|dir| dir.join("kismet-api-key")) else {
        return Ok(None);
    };
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("opening {}", path.display())),
    };
    let mut text = String::new();
    file.take(MAX_PAIRING_BYTES)
        .read_to_string(&mut text)
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .next()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_owned))
}

/// The pairing kept at `path`, or none.
fn load_pairing(path: &std::path::Path) -> Result<Option<link::Pairing>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("opening {}", path.display())),
    };
    let mut text = String::new();
    file.take(MAX_PAIRING_BYTES)
        .read_to_string(&mut text)
        .with_context(|| format!("reading {}", path.display()))?;
    let pairing: link::Pairing = serde_json::from_str(&text).map_err(|err| {
        anyhow!(
            "{}: not a pairing: {}",
            path.display(),
            sensio_core::event::reason_without_values(&err)
        )
    })?;
    if !sensio_core::event::is_label(&pairing.receiver) || link::proof(&pairing, "").is_none() {
        return Err(anyhow!(
            "{}: not a usable pairing; pair again with --code",
            path.display()
        ));
    }
    Ok(Some(pairing))
}

/// Keeps the pairing at `path`, readable by this user only on Unix, replacing
/// any before it in one step.
fn save_pairing(path: &std::path::Path, pairing: &link::Pairing) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let temporary = path.with_extension("json.new");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("writing {}", temporary.display()))?;
    serde_json::to_writer(&mut file, pairing)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path).with_context(|| format!("writing {}", path.display()))
}

/// Link sessions, one after another with a backoff, until a refusal.
fn link_until_stopped(args: &LinkArgs) -> Result<ExitCode> {
    let pairing_path = match &args.pairing {
        Some(path) => path.clone(),
        None => config_dir()
            .ok_or_else(|| anyhow!("no configuration directory found; give --pairing"))?
            .join("link-pairing.json"),
    };
    let mut auth = match (&args.code, load_pairing(&pairing_path)?) {
        (Some(code), _) => link::Auth::Code(code.clone()),
        (None, Some(pairing)) => {
            if args
                .receiver
                .as_ref()
                .is_some_and(|r| *r != pairing.receiver)
            {
                return Err(anyhow!(
                    "this computer is paired as {}; pair again with --code to change its name",
                    pairing.receiver
                ));
            }
            link::Auth::Paired(pairing)
        }
        (None, None) => {
            return Err(anyhow!(
                "not paired with the phone yet: run peil link --code <code> once, with the code the app shows"
            ));
        }
    };
    let receiver = args.receiver.clone().unwrap_or_else(|| "laptop".to_owned());
    if !sensio_core::event::is_label(&receiver) {
        return Err(anyhow!(
            "--receiver must be 1 to 32 characters of a-z, 0-9, '-', '_' or '.'"
        ));
    }
    let (phone, kismet, poll_ms) = (args.phone.as_deref(), args.kismet.as_str(), args.poll_ms);
    let api_key = kismet_api_key()?;
    let poll_every = Duration::from_millis(poll_ms);
    // Kismet is asked for a window wider than the poll, so a slow poll misses nothing.
    let window_s = u32::try_from(poll_ms.div_ceil(1000) * 3).unwrap_or(u32::MAX);
    let stop = AtomicBool::new(false);
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = std::time::Instant::now();
        let mut new_pairing = None;
        let ended = phone_address(phone).and_then(|address| {
            let config = link::Config {
                phone: address,
                auth: auth.clone(),
                receiver: receiver.clone(),
                app: format!("peil {}", env!("CARGO_PKG_VERSION")),
                poll_every,
            };
            let mut poller = Poller::new(kismet, api_key.clone(), window_s);
            link::run_session(
                &config,
                &mut poller,
                wall_clock_ns,
                &stop,
                &mut |text| eprintln!("{text}"),
                &mut |pairing| {
                    match save_pairing(&pairing_path, &pairing) {
                        Ok(()) => eprintln!("pairing kept in {}", pairing_path.display()),
                        Err(err) => eprintln!("Error: the pairing wasn't kept: {err:#}"),
                    }
                    new_pairing = Some(pairing);
                },
            )
            .map_err(anyhow::Error::from)
        });
        // A reconnect proves the new pairing rather than reusing the code.
        if let Some(pairing) = new_pairing {
            auth = link::Auth::Paired(pairing);
        }
        match ended {
            Ok(()) => return Ok(ExitCode::SUCCESS),
            Err(err) if matches!(err.downcast_ref(), Some(LinkError::Refused(_))) => {
                return Err(match auth {
                    link::Auth::Paired(_) => err.context("pair again with --code"),
                    link::Auth::Code(_) => err,
                });
            }
            Err(err) => {
                if started.elapsed() > Duration::from_secs(60) {
                    backoff = Duration::from_secs(1);
                }
                eprintln!("{err:#}; trying again in {} s", backoff.as_secs());
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

fn live(
    reader: impl BufRead,
    name: &str,
    rule: PersistenceRule,
    all: bool,
    vendors: VendorTable,
) -> Result<ExitCode> {
    let mut out = std::io::stdout().lock();
    let emit = |line: &VerdictLine| -> std::io::Result<()> {
        serde_json::to_writer(&mut out, line)?;
        writeln!(out)?;
        // An answer is shown as soon as it's known; the lines before it
        // (coverage, rejections) go with it.
        if matches!(line, VerdictLine::Verdicts { .. } | VerdictLine::End { .. }) {
            out.flush()?;
        }
        Ok(())
    };
    let engine = Live::new(Sources, rule, all).with_describe(DescribeParams::default(), vendors);
    match run(engine, reader, emit) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(RunError::Fatal(fatal)) => {
            eprintln!("Error: {name}: {fatal}");
            Ok(ExitCode::from(EXIT_NOT_A_FEED))
        }
        Err(err) => Err(err).with_context(|| format!("judging {name}")),
    }
}
