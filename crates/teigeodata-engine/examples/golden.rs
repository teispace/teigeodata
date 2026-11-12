//! A consumer's golden search cases on real layers (D-236): each query must find one of its
//! expected places within its rank, with the options the app passes. Exits non-zero when a case
//! fails, so it gates a layer release. A layer without SUBSTRING is searched as an app searches
//! it once the device has derived the section (D-218): the runner derives it the same way and
//! adds it to the layer in memory.
//!
//! Usage: `cargo run --release -p teigeodata-engine --all-features --example golden --
//! <cases.tsv> <layer.tgd>...`
//!
//! The cases file is tab-separated, one case per line, `#` starting a comment:
//! `query  expect  rank  options  note`. `expect` is one or more stable IDs (`tgd:np/s78`)
//! joined by commas, any of which passes; `rank` the worst rank that passes (1 = first);
//! `options` `-` or space-separated `focus=LAT,LON`, `scale_km=KM`, `strict`, `k=N` (how many
//! the app asks for, 40 by default, as the MVP listed); `note` says what the case checks.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a gate over local inputs: it reports on stdout"
)]

use std::process::ExitCode;

use teigeodata_core::{E6, PlaceId, Point};
use teigeodata_engine::{Engine, Focus, Hit, Layer, LayerOptions, Scratch, SearchOptions};
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};

/// How many an app asks for unless a case says: the MVP listed 40 at most.
const DEFAULT_K: usize = 40;

/// One case.
struct Case {
    line: usize,
    query: String,
    expect: Vec<PlaceId>,
    rank: usize,
    opts: SearchOptions,
    k: usize,
    note: String,
}

fn parse(text: &str) -> Result<Vec<Case>, String> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = raw.split('\t').collect();
        let [query, expect, rank, options, note] = cols.as_slice() else {
            return Err(format!(
                "line {line}: want 5 tab-separated columns, got {}",
                cols.len()
            ));
        };
        let expect = expect
            .split(',')
            .map(|id| {
                id.trim()
                    .parse::<PlaceId>()
                    .map_err(|e| format!("line {line}: {id}: {e:?}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rank: usize =
            rank.parse().ok().filter(|&r| r >= 1).ok_or_else(|| {
                format!("line {line}: rank `{rank}` is not a whole number from 1")
            })?;
        let (mut opts, mut k, mut at, mut scale) =
            (SearchOptions::default(), DEFAULT_K, None, None);
        for o in options.split(' ').filter(|o| !o.is_empty() && *o != "-") {
            let bad = || format!("line {line}: option `{o}`");
            match o.split_once('=') {
                None if o == "strict" => opts = opts.with_strict(true),
                Some(("k", n)) => k = n.parse().map_err(|_| bad())?,
                Some(("scale_km", s)) => scale = Some(s.parse::<f64>().map_err(|_| bad())?),
                Some(("focus", p)) => {
                    let (lat, lon) = p.split_once(',').ok_or_else(bad)?;
                    let (lat, lon) = (
                        lat.parse().map_err(|_| bad())?,
                        lon.parse().map_err(|_| bad())?,
                    );
                    at = Some(Point::<E6>::from_degrees(lat, lon).map_err(|_| bad())?);
                }
                _ => return Err(bad()),
            }
        }
        if let Some(at) = at {
            let mut f = Focus::new(at);
            if let Some(km) = scale {
                f.scale_m = km * 1000.0;
            }
            opts = opts.with_focus(f);
        } else if scale.is_some() {
            return Err(format!("line {line}: scale_km without a focus"));
        }
        if rank > k {
            return Err(format!("line {line}: rank {rank} past k {k}"));
        }
        out.push(Case {
            line,
            query: (*query).to_owned(),
            expect,
            rank,
            opts,
            k,
            note: (*note).to_owned(),
        });
    }
    Ok(out)
}

/// The layer at `path`, with SUBSTRING derived and added when it has none, as a device does.
fn layer(path: &str) -> Result<(Layer, bool), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let pack = Pack::open(&bytes).map_err(|e| e.to_string())?;
    if pack.section(Kind::SUBSTRING).is_some() {
        return Ok((
            Layer::from_bytes(bytes, LayerOptions::default()).map_err(|e| e.to_string())?,
            false,
        ));
    }
    let inputs: Vec<(Kind, u64, Vec<u8>)> = [Kind::NAMES, Kind::SEARCH]
        .into_iter()
        .filter_map(|k| {
            pack.section(k)
                .map(|s| (k, s.item_count(), s.bytes().to_vec()))
        })
        .collect();
    let derived = teigeodata_engine::derive::substring(&inputs)?;
    let rows = inputs
        .iter()
        .find(|i| i.0 == Kind::NAMES)
        .map_or(0, |i| i.1);
    let m = pack.manifest();
    let mut w = Writer::new(m.address(), m.component(), m.release()).map_err(|e| e.to_string())?;
    for (tag, value) in m.entries() {
        if ![Tag::ADDRESS, Tag::COMPONENT, Tag::RELEASE].contains(&tag) {
            w.manifest(tag, value).map_err(|e| e.to_string())?;
        }
    }
    let mut sections: Vec<(Kind, Encoding, u64, &[u8])> = pack
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    sections.push((Kind::SUBSTRING, Encoding::TABLE, rows, &derived));
    let file = w.pack(&sections, None).map_err(|e| e.to_string())?;
    Ok((
        Layer::from_bytes(file, LayerOptions::default()).map_err(|e| e.to_string())?,
        true,
    ))
}

/// The canonical name of a hit, for the report.
fn name(e: &Engine, h: &Hit, buf: &mut [u8]) -> String {
    let mut out = String::new();
    let _ = e.names(&h.place, buf, |n, _| {
        if out.is_empty() {
            n.clone_into(&mut out);
        }
    });
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [cases, layers @ ..] = args.as_slice() else {
        eprintln!("usage: golden <cases.tsv> <layer.tgd>...");
        return ExitCode::from(2);
    };
    if layers.is_empty() {
        eprintln!("usage: golden <cases.tsv> <layer.tgd>...");
        return ExitCode::from(2);
    }
    let cases = match std::fs::read_to_string(cases)
        .map_err(|e| e.to_string())
        .and_then(|t| parse(&t))
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{cases}: {e}");
            return ExitCode::from(2);
        }
    };
    let mut e = Engine::new();
    for l in layers {
        match layer(l) {
            Ok((layer, derived)) => {
                if derived {
                    println!("{l}: SUBSTRING derived, as on a device");
                }
                e.add(layer);
            }
            Err(err) => {
                eprintln!("{l}: {err}");
                return ExitCode::from(2);
            }
        }
    }
    let (mut s, mut hits, mut buf) = (Scratch::default(), Vec::new(), vec![0u8; 1 << 16]);
    let mut failed = 0;
    for c in &cases {
        if let Err(err) = e.search_with(&c.query, c.k, &c.opts, &mut s, &mut hits) {
            eprintln!("line {}: {}: {err}", c.line, c.query);
            return ExitCode::from(2);
        }
        let at = hits.iter().position(|h| c.expect.contains(&h.place.id));
        if at.is_some_and(|i| i < c.rank) {
            continue;
        }
        failed += 1;
        let found = at.map_or_else(|| "not in the list".to_owned(), |i| format!("at {}", i + 1));
        let top: Vec<String> = hits
            .iter()
            .take(3)
            .map(|h| format!("{} {} ({})", name(&e, h, &mut buf), h.place.id, h.score))
            .collect();
        println!(
            "FAIL line {}: `{}` wants {} by {}, {found}; first: {} [{}]",
            c.line,
            c.query,
            c.expect
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" or "),
            c.rank,
            top.join("; "),
            c.note
        );
    }
    println!("{} of {} cases pass", cases.len() - failed, cases.len());
    if failed > 0 {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
