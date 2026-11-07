//! `peil`: Peil's command-line front end.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use sensio_core::feed::VerdictLine;
use sensio_core::{
    DescribeParams, Live, PersistenceRule, RunError, VendorTable, describe, judge, parse_jsonl, run,
};
use sensio_sources::Sources;

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
