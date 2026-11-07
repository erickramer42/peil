//! `peil`: Peil's command-line front end.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use sensio_core::feed::VerdictLine;
use sensio_core::{Live, PersistenceRule, RunError, judge, parse_jsonl, run};
use sensio_sources::Sources;

const NS_PER_S: i64 = 1_000_000_000;

/// The exit code for input that isn't a feed at all.
const EXIT_NOT_A_FEED: u8 = 2;

#[derive(Parser)]
#[command(
    name = "peil",
    version,
    about = "Anomaly detection for the emitters around a vehicle or a building"
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
        #[command(flatten)]
        rule: RuleArgs,
    },
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
        Command::Live { path, all, rule } => {
            let rule = rule.rule()?;
            let (reader, name): (Box<dyn BufRead>, String) = match &path {
                Some(path) => {
                    let file =
                        File::open(path).with_context(|| format!("opening {}", path.display()))?;
                    (Box::new(BufReader::new(file)), path.display().to_string())
                }
                None => (Box::new(std::io::stdin().lock()), "stdin".to_owned()),
            };
            live(reader, &name, rule, all)
        }
    }
}

fn live(reader: impl BufRead, name: &str, rule: PersistenceRule, all: bool) -> Result<ExitCode> {
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
    match run(Live::new(Sources, rule, all), reader, emit) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(RunError::Fatal(fatal)) => {
            eprintln!("Error: {name}: {fatal}");
            Ok(ExitCode::from(EXIT_NOT_A_FEED))
        }
        Err(err) => Err(err).with_context(|| format!("judging {name}")),
    }
}
