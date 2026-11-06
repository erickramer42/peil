//! `peil`: Peil's command-line front end.

use std::fs::File;
use std::io::{BufReader, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sensio_core::{PersistenceRule, judge, parse_jsonl};

const NS_PER_S: i64 = 1_000_000_000;

#[derive(Parser)]
#[command(
    name = "peil",
    version,
    about = "RF awareness for vehicles and buildings"
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
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Judge {
            path,
            min_places,
            min_span_s,
            place_separation_m,
            max_fix_age_s,
        } => {
            let defaults = PersistenceRule::default();
            let rule = PersistenceRule {
                min_places: min_places.unwrap_or(defaults.min_places),
                min_span_ns: min_span_s.map_or(defaults.min_span_ns, |s| i64::from(s) * NS_PER_S),
                place_separation_m: place_separation_m.unwrap_or(defaults.place_separation_m),
                max_fix_age_ns: max_fix_age_s
                    .map_or(defaults.max_fix_age_ns, |s| i64::from(s) * NS_PER_S),
            };
            let file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
            let events = parse_jsonl(BufReader::new(file))
                .with_context(|| format!("reading {}", path.display()))?;
            let mut out = std::io::stdout().lock();
            for verdict in judge(&events, &rule) {
                serde_json::to_writer(&mut out, &verdict)?;
                writeln!(out)?;
            }
        }
    }
    Ok(())
}
