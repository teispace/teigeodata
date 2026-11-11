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
//! | 4 | `install --downloads`: the byte ranges to download first, printed as JSON |
//!
//! A closed standard output (`tgd export x.tgd | head`) ends the command quietly with 0.
//!
//! [CLI note]: https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/cli.md

mod export;
mod extract;
mod output;
mod pack;
mod query;
mod sync;

use std::fmt;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use teigeodata_format::component::Component;

/// The teigeodata command line: packs, queries and the store.
#[derive(Debug, Parser)]
#[command(
    name = "tgd",
    bin_name = "tgd",
    version,
    propagate_version = true,
    max_term_width = 100
)]
struct Cli {
    /// The store: installed packs and the verified catalog [default: the platform's data
    /// directory]
    #[arg(long, global = true, value_name = "DIR", env = "TEIGEODATA_STORE")]
    store: Option<PathBuf>,
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
        /// Prefer places near this point (D-226): up to 30 points at it, halving every
        /// `--scale-km`.
        #[arg(long, value_name = "LAT,LON", value_parser = lat_lon, allow_hyphen_values = true)]
        focus: Option<(f64, f64)>,
        /// The focus's scale: the distance at which its gain halves.
        #[arg(long, default_value_t = 50.0, requires = "focus")]
        scale_km: f64,
        /// Rank by match tier first (exact, loose, prefix, one edit, substring), then importance.
        #[arg(long)]
        strict: bool,
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
        /// The instant the UTC offset is given for, in Unix seconds; now when left out.
        #[arg(long, allow_hyphen_values = true)]
        time: Option<i64>,
        #[command(flatten)]
        packs: Packs,
        #[command(flatten)]
        out: Out,
    },
    /// The terrain height at a point, in metres above EGM2008, from the packs' DEM.
    Elevation {
        /// Latitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lat: f64,
        /// Longitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lon: f64,
        #[command(flatten)]
        packs: Packs,
        /// Print JSON: `{"lat", "lon", "elevation_m", "datum"}`.
        #[arg(long)]
        json: bool,
    },
    /// Postal codes: a code to its places (`44600`, `SW1A 1AA`), codes by prefix, or the codes
    /// at a point (latitude and longitude).
    Postal {
        /// A code, or a latitude in degrees.
        #[arg(allow_hyphen_values = true)]
        code: String,
        /// With a latitude: the longitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lon: Option<f64>,
        /// Only this scheme (`np`, `gb`).
        #[arg(long)]
        scheme: Option<String>,
        /// The codes that start with it (autocomplete).
        #[arg(long, conflicts_with = "lon")]
        prefix: bool,
        /// With `--prefix`: at most this many codes.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[command(flatten)]
        packs: Packs,
        /// Print a JSON array of `{"scheme", "code", "kind", "id", "name", …}`.
        #[arg(long)]
        json: bool,
    },
    /// Set up the store: the catalog's mirrors and its trusted root.
    Init {
        /// A metadata mirror (the catalog's TUF repository); repeat for several, best first.
        #[arg(long, required = true, value_name = "URL")]
        repo: Vec<String>,
        /// An asset mirror (the packs' host); repeat for several, best first.
        #[arg(long, required = true, value_name = "URL")]
        assets: Vec<String>,
        /// The trusted root metadata: the trust anchor, from the project's release.
        #[arg(long, value_name = "FILE")]
        root: PathBuf,
    },
    /// Fetch what changed in the catalog: the countries installed, and those named.
    Refresh {
        /// Countries to list too (two-letter codes, comma-separated).
        #[arg(long, value_delimiter = ',')]
        countries: Vec<String>,
    },
    /// List the catalog's packs, from the store alone.
    Catalog {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Find a place by name, with the packs that hold it, offline.
    Find {
        /// The name, optionally followed by a comma and a parent's name.
        query: String,
        /// How many.
        #[arg(short, default_value_t = 5)]
        k: usize,
        /// A pack to search besides the installed ones (a world pack); repeatable.
        #[arg(long = "pack", value_name = "FILE")]
        with: Vec<PathBuf>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// The countries at a point, with their packs, offline.
    Locate {
        /// Latitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lat: f64,
        /// Longitude in degrees.
        #[arg(allow_hyphen_values = true)]
        lon: f64,
        /// Also countries within this many metres.
        #[arg(long, default_value_t = teigeodata_engine::LOCATOR_RADIUS_M)]
        radius: u32,
        /// A pack to locate with besides the installed ones (a world pack); repeatable.
        #[arg(long = "pack", value_name = "FILE")]
        with: Vec<PathBuf>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Install packs (`np`, `np.dem`, or a catalog path), their dependencies first.
    Install {
        /// The packs.
        #[arg(required = true)]
        packs: Vec<String>,
        /// Only these components, comma-separated, with what they need; added to what is
        /// installed (default: the whole pack). Values as for `extract --components`.
        #[arg(long, value_delimiter = ',', value_parser = component)]
        components: Vec<Component>,
        /// Never evict them.
        #[arg(long)]
        pin: bool,
        /// Install from the byte ranges a platform downloaded into DIR/<asset>/, files named
        /// `<start>-<end>`; what is still missing is printed as JSON, with exit code 4.
        #[arg(long, value_name = "DIR")]
        downloads: Option<PathBuf>,
    },
    /// Update installed packs to the catalog's releases (all of them by default).
    Update {
        /// The packs; all installed ones when left out.
        packs: Vec<String>,
    },
    /// Remove a pack, and the dependencies nothing else needs.
    Remove {
        /// The pack.
        pack: String,
    },
    /// Pin a pack (never evicted), or unpin it.
    Pin {
        /// The pack.
        pack: String,
        /// Unpin it.
        #[arg(long)]
        off: bool,
    },
    /// Check an installed pack value by value and fetch what is damaged.
    Repair {
        /// The pack.
        pack: String,
    },
    /// The installed packs and the catalog's state.
    Status {
        /// Hash every installed section.
        #[arg(long)]
        deep: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print a shell's completion script.
    ///
    /// Save it where the shell loads completions, for example
    /// `tgd completions zsh > ~/.zfunc/_tgd`, or source it from the shell's startup file.
    Completions {
        /// The shell.
        shell: clap_complete::Shell,
    },
}

/// The packs a query reads, as one dataset.
#[derive(Debug, Args)]
struct Packs {
    /// A pack file; repeat for several. Without one, the store's installed packs.
    #[arg(long = "pack", value_name = "FILE")]
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
    Network(String),
    /// Exit 4: `install --downloads` printed the byte ranges a platform must download first.
    Pending,
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
    let store = cli.store.clone().unwrap_or_else(sync::default_store);
    let store = store.as_path();
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
            query::run(store, &packs.packs, query::Ask::Get { id }, o.mode(), out)
        }
        Command::Reverse {
            lat,
            lon,
            k,
            packs,
            out: o,
        } => query::run(
            store,
            &packs.packs,
            query::Ask::Reverse { lat, lon, k },
            o.mode(),
            out,
        ),
        Command::Search {
            query: q,
            k,
            focus,
            scale_km,
            strict,
            packs,
            out: o,
        } => query::run(
            store,
            &packs.packs,
            query::Ask::Search {
                query: q,
                k,
                rank: (
                    focus.map(|(lat, lon)| (lat, lon, scale_km * 1_000.0)),
                    strict,
                ),
            },
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
            store,
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
        } => query::run(
            store,
            &packs.packs,
            query::Ask::Admin { lat, lon },
            o.mode(),
            out,
        ),
        Command::Tz {
            at,
            lon,
            time,
            packs,
            out: o,
        } => {
            let t = time.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
            });
            let ask = match lon {
                Some(lon) => query::Ask::TzAt {
                    lat: at
                        .parse()
                        .map_err(|e| Fail::Refused(format!("{at}: {e}")))?,
                    lon,
                    t,
                },
                None => query::Ask::TzOf { id: at, t },
            };
            query::run(store, &packs.packs, ask, o.mode(), out)
        }
        Command::Elevation {
            lat,
            lon,
            packs,
            json,
        } => query::elevation(store, &packs.packs, (lat, lon), json, out),
        Command::Postal {
            code,
            lon,
            scheme,
            prefix,
            limit,
            packs,
            json,
        } => {
            let ask = match lon {
                Some(lon) => query::PostalAsk::At {
                    lat: code
                        .parse()
                        .map_err(|e| Fail::Refused(format!("{code}: {e}")))?,
                    lon,
                },
                None => query::PostalAsk::Code {
                    code,
                    scheme,
                    prefix,
                    limit,
                },
            };
            query::postal(store, &packs.packs, ask, json, out)
        }
        Command::Init { repo, assets, root } => sync::init(store, &repo, &assets, &root, out),
        Command::Refresh { countries } => sync::refresh(store, &countries, out),
        Command::Catalog { json } => sync::catalog(store, json, out),
        Command::Find {
            query: q,
            k,
            with,
            json,
        } => sync::find(store, &q, k, &with, json, out),
        Command::Locate {
            lat,
            lon,
            radius,
            with,
            json,
        } => {
            let at = query::point::<teigeodata_core::E7>(lat, lon)?;
            sync::locate(store, at, radius, &with, json, out)
        }
        Command::Install {
            packs,
            components,
            pin,
            downloads,
        } => sync::install(store, &packs, &components, pin, downloads.as_deref(), out),
        Command::Update { packs } => sync::update(store, &packs, out),
        Command::Remove { pack } => sync::remove(store, &pack, out),
        Command::Pin { pack, off } => sync::pin(store, &pack, !off, out),
        Command::Repair { pack } => sync::repair(store, &pack, out),
        Command::Status { deep, json } => sync::status(store, deep, json, out),
        Command::Completions { shell } => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut cmd, "tgd", out);
            Ok(())
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
        Err(Fail::Pending) => ExitCode::from(4),
    }
}

/// `LAT,LON` in degrees.
fn lat_lon(s: &str) -> Result<(f64, f64), String> {
    let (lat, lon) = s
        .split_once(',')
        .ok_or_else(|| format!("`{s}`: expected LAT,LON"))?;
    let num = |v: &str| v.trim().parse::<f64>().map_err(|e| format!("`{v}`: {e}"));
    Ok((num(lat)?, num(lon)?))
}
