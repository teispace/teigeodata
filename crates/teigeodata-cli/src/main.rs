//! `tgd`, the teigeodata command line (D-178, [CLI note]).
//!
//! Pack commands read a pack file the CLI does not own, so it is read into memory, never mapped
//! (D-113). Every command writes its result to standard output and its problems to standard
//! error, and ends with an exit code a script can act on:
//!
//! | Code | Meaning |
//! |---|---|
//! | 0 | done |
//! | 1 | refused: verification failed, not found, a file could not be read or written |
//! | 2 | a usage error (clap's) |
//! | 3 | the network: try again later |
//!
//! A closed standard output (`tgd export x.tgd | head`) ends the command quietly with 0.
//!
//! [CLI note]: https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/cli.md

mod export;
mod extract;
mod output;
mod pack;
mod query;

use std::fmt;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use teigeodata_format::component::Component;

/// The teigeodata command line: packs, queries and the store.
#[derive(Debug, Parser)]
#[command(name = "tgd", version, propagate_version = true, max_term_width = 100)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show a pack's manifest, sections and bytes per component.
    Inspect {
        /// The pack file.
        pack: PathBuf,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Check a pack: its head and every section's hash; with --deep every value too; with
    /// --key its signature.
    Verify {
        /// The pack file.
        pack: PathBuf,
        /// Also read and check every value of every section.
        #[arg(long)]
        deep: bool,
        /// A trusted Ed25519 public key, 64 hex digits; repeat for several. The pack must be
        /// signed by one of them.
        #[arg(long = "key", value_name = "HEX")]
        keys: Vec<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show what changed between two packs: sections, then places (added, removed, moved,
    /// renamed, retyped).
    Diff {
        /// The older pack.
        old: PathBuf,
        /// The newer pack.
        new: PathBuf,
        /// Places listed for each kind of change (all are counted).
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Write a smaller pack: some components, the places in a box (with their ancestors and
    /// timezone areas), or both.
    Extract {
        /// The pack file.
        pack: PathBuf,
        /// The file to write.
        #[arg(short, long)]
        out: PathBuf,
        /// The components to keep, comma-separated, with what they need (point, elevation,
        /// admin, names, search, timezone, timezone-index, boundaries, admin-index, postal,
        /// dem, extra).
        #[arg(long, value_delimiter = ',', value_parser = component)]
        components: Vec<Component>,
        /// Keep the places in this box: west,south,east,north in degrees.
        #[arg(long, value_name = "W,S,E,N", value_parser = extract::parse_bbox, allow_hyphen_values = true)]
        bbox: Option<extract::Bbox>,
        /// With --bbox: the boundaries' level of detail, 0 (full) to 6 (coarsest).
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=6))]
        lod: u8,
        /// The manifest's component name of the new pack (its identity is its address,
        /// component and release, so it must differ from the source's).
        #[arg(long, default_value = "extract")]
        name: String,
    },
    /// The place with this ID (a retired ID gives its survivor).
    Get {
        /// The ID, as `tgd:np/abc123`.
        id: String,
        #[command(flatten)]
        packs: Packs,
        #[command(flatten)]
        out: OutN,
    },
    /// The places nearest a point, nearest first.
    Reverse {
        /// Latitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lat: f64,
        /// Longitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lon: f64,
        /// How many.
        #[arg(short, default_value_t = 1)]
        k: usize,
        #[command(flatten)]
        packs: Packs,
        #[command(flatten)]
        out: OutN,
    },
    /// Places matching a name, best first; "Name, Parent" ranks a named parent first.
    Search {
        /// The name, optionally followed by a comma and a parent's name.
        query: String,
        /// How many.
        #[arg(short, default_value_t = 10)]
        k: usize,
        #[command(flatten)]
        packs: Packs,
        #[command(flatten)]
        out: OutN,
    },
    /// The places nearest a place, nearest first, itself left out.
    Near {
        /// The place's ID.
        id: String,
        /// How many.
        #[arg(short, default_value_t = 10)]
        k: usize,
        /// Only those within this many metres.
        #[arg(long)]
        radius: Option<f64>,
        #[command(flatten)]
        packs: Packs,
        #[command(flatten)]
        out: Out,
    },
    /// The admin units holding a point, coarse to fine.
    Admin {
        /// Latitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lat: f64,
        /// Longitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lon: f64,
        #[command(flatten)]
        packs: Packs,
        #[command(flatten)]
        out: Out,
    },
    /// The timezone area of a place (one ID) or at a point (latitude and longitude).
    Tz {
        /// A place's ID, or a latitude in degrees.
        #[arg(allow_hyphen_values = true)]
        at: String,
        /// With a latitude: the longitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lon: Option<f64>,
        #[command(flatten)]
        packs: Packs,
        #[command(flatten)]
        out: Out,
    },
    /// Write every place of a pack, streamed, in its stored order.
    Export {
        /// The pack file.
        pack: PathBuf,
        /// The output format.
        #[arg(long, value_enum, default_value_t = Format::Ndjson)]
        format: Format,
        /// The file to write; standard output when left out.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

/// The packs a query reads, as one dataset.
#[derive(Debug, Args)]
struct Packs {
    /// A pack file; repeat for several (a later pack wins where packs disagree only by the
    /// engine's precedence rules).
    #[arg(long = "pack", value_name = "FILE", required = true)]
    packs: Vec<PathBuf>,
}

/// Output modes of a query (ADR-0038).
#[derive(Debug, Args)]
struct Out {
    /// Print the native JSON records.
    #[arg(long, conflicts_with = "geojson")]
    json: bool,
    /// Print a GeoJSON FeatureCollection.
    #[arg(long)]
    geojson: bool,
}

/// Output modes of `get`, `reverse` and `search`, which Nominatim also has.
#[derive(Debug, Args)]
struct OutN {
    #[command(flatten)]
    out: Out,
    /// Print Nominatim's `json` format.
    #[arg(long, conflicts_with_all = ["json", "geojson"])]
    nominatim: bool,
}

impl Out {
    fn mode(&self) -> output::Mode {
        match (self.json, self.geojson) {
            (true, _) => output::Mode::Json,
            (_, true) => output::Mode::Geojson,
            _ => output::Mode::Text,
        }
    }
}

impl OutN {
    fn mode(&self) -> output::Mode {
        if self.nominatim {
            output::Mode::Nominatim
        } else {
            self.out.mode()
        }
    }
}

/// An export format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    /// One JSON object per line.
    Ndjson,
    /// An RFC 7946 FeatureCollection of points.
    Geojson,
}

fn component(s: &str) -> Result<Component, String> {
    Component::from_name(s).ok_or_else(|| {
        let names: Vec<&str> = Component::ALL.iter().map(|c| c.name()).collect();
        format!("unknown component (expected one of {})", names.join(", "))
    })
}

/// Why a command failed, and its exit code.
#[derive(Debug)]
pub(crate) enum Fail {
    /// Exit 1: the data is bad, missing, or a file could not be read or written.
    Refused(String),
    /// Exit 3: the network failed; trying again may work.
    #[allow(dead_code, reason = "the sync commands (4.9d) return it")]
    Network(String),
    /// Standard output was closed: stop quietly.
    Closed,
}

impl Fail {
    pub(crate) fn of(what: impl fmt::Display, e: impl fmt::Display) -> Self {
        Self::Refused(format!("{what}: {e}"))
    }
}

impl From<io::Error> for Fail {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::BrokenPipe {
            Self::Closed
        } else {
            Self::Refused(e.to_string())
        }
    }
}

impl From<teigeodata_engine::Error> for Fail {
    fn from(e: teigeodata_engine::Error) -> Self {
        Self::Refused(e.to_string())
    }
}

impl From<serde_json::Error> for Fail {
    fn from(e: serde_json::Error) -> Self {
        match e.io_error_kind() {
            Some(io::ErrorKind::BrokenPipe) => Self::Closed,
            _ => Self::Refused(e.to_string()),
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one arm per command: the dispatch table"
)]
fn run(cli: Cli, out: &mut dyn Write) -> Result<(), Fail> {
    match cli.command {
        Command::Inspect { pack, json } => pack::inspect(&pack, json, out),
        Command::Verify {
            pack,
            deep,
            keys,
            json,
        } => pack::verify(&pack, deep, &keys, json, out),
        Command::Diff {
            old,
            new,
            limit,
            json,
        } => pack::diff(&old, &new, limit, json, out),
        Command::Extract {
            pack,
            out: to,
            components,
            bbox,
            lod,
            name,
        } => extract::run(
            &pack,
            &to,
            &extract::Options {
                components: &components,
                bbox,
                lod,
                name: &name,
            },
            out,
        ),
        Command::Get { id, packs, out: o } => {
            query::run(&packs.packs, query::Ask::Get { id }, o.mode(), out)
        }
        Command::Reverse {
            lat,
            lon,
            k,
            packs,
            out: o,
        } => query::run(
            &packs.packs,
            query::Ask::Reverse { lat, lon, k },
            o.mode(),
            out,
        ),
        Command::Search {
            query: q,
            k,
            packs,
            out: o,
        } => query::run(
            &packs.packs,
            query::Ask::Search { query: q, k },
            o.mode(),
            out,
        ),
        Command::Near {
            id,
            k,
            radius,
            packs,
            out: o,
        } => query::run(
            &packs.packs,
            query::Ask::Near { id, k, radius },
            o.mode(),
            out,
        ),
        Command::Admin {
            lat,
            lon,
            packs,
            out: o,
        } => query::run(&packs.packs, query::Ask::Admin { lat, lon }, o.mode(), out),
        Command::Tz {
            at,
            lon,
            packs,
            out: o,
        } => {
            let ask = match lon {
                Some(lon) => query::Ask::TzAt {
                    lat: at
                        .parse()
                        .map_err(|e| Fail::Refused(format!("{at}: {e}")))?,
                    lon,
                },
                None => query::Ask::TzOf { id: at },
            };
            query::run(&packs.packs, ask, o.mode(), out)
        }
        Command::Export {
            pack,
            format,
            out: to,
        } => match to {
            Some(path) => {
                let file = std::fs::File::create(&path).map_err(|e| Fail::of(path.display(), e))?;
                let mut w = BufWriter::with_capacity(1 << 16, file);
                export::run(&pack, format, &mut w)?;
                w.flush().map_err(|e| Fail::of(path.display(), e))
            }
            None => export::run(&pack, format, out),
        },
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let mut out = BufWriter::with_capacity(1 << 16, stdout.lock());
    let result = run(cli, &mut out).and_then(|()| out.flush().map_err(Fail::from));
    match result {
        Ok(()) | Err(Fail::Closed) => ExitCode::SUCCESS,
        Err(Fail::Refused(m)) => {
            let _ = writeln!(io::stderr(), "tgd: {m}");
            ExitCode::from(1)
        }
        Err(Fail::Network(m)) => {
            let _ = writeln!(io::stderr(), "tgd: {m}");
            ExitCode::from(3)
        }
    }
}
