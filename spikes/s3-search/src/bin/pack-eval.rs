//! Scores a forge-built pack on the golden query set: the S3 query pipeline and scoring
//! (`spike_s3_search::engine`, `::golden`) over the pack's own SEARCH section, so every names
//! or search change in the forge is measured in one command.
//!
//! Usage: `pack-eval <pack.tgd> <golden.yaml> <ids.tsv>`; `S3_MISSES=1` lists every miss and
//! every expected key that does not resolve. The S3 A/B switches (`S3_NO_FUZZY`, …) apply.
//! `S3_GATE=<baseline.tsv>` fails when a class loses a hit; `S3_GATE_WRITE=<file>` writes one.
//!
//! Golden expectations name source keys (`geonames:<id>`, `codab:<pcode>`); the ID registry
//! the build wrote maps them to serials, and IDMAP serials to rows. A row is one entity (the
//! forge merged its sources), so a group is a row. Record fields the S3 pipeline reads come
//! from the pack: the canonical name, the importance column, the district (the ancestor at
//! admin level 2, as S3's `GeoNames` ADM2 parent) and the unit type (the profile's source rule
//! on the unit's P-code). World-general (P16): the country comes from the golden file.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::cast_precision_loss,
    missing_docs,
    reason = "an evaluation tool over local inputs: it reports on stdout and fails on stderr"
)]

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::Instant;

use spike_s3_search::engine::{Engine, Packed};
use spike_s3_search::{Profile, Rec, golden, profile, unit_type_of};
use teigeodata_core::PlaceKind;
use teigeodata_format::hierarchy::Hierarchy;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::names::{Names, TEXT_BUFFER};
use teigeodata_format::places::Places;
use teigeodata_format::search::Search;
use teigeodata_format::{Kind, Pack};

/// Source namespaces the golden set names that the forge keys differently: the golden set's
/// `codab:<pcode>` and the forge's `admin:<pcode>` are the same OCHA COD-AB P-code (forge
/// `admin_places`, one release: `codab-np-*` in `data/sources.lock`).
const NAMESPACES: [(&str, &str); 1] = [("codab", "admin")];

/// The first line of an ID registry (forge `build::ids`).
const REGISTRY_HEADER: &str = "# teigeodata id registry 1";

/// The admin level whose name is the context of "Name, Parent" queries (S3: `GeoNames` ADM2).
const CONTEXT_LEVEL: u8 = 2;

/// One registry line: where the serial went, and its source keys.
enum State {
    Live,
    To(u64),
    Gone,
}

struct Registry {
    state: BTreeMap<u64, State>,
    keys: BTreeMap<u64, Vec<String>>,
    /// Key → every serial that holds it (the forge lets one key sit on several places).
    serial_of: BTreeMap<String, Vec<u64>>,
}

fn registry(path: &str) -> Result<Registry, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut lines = text.lines().enumerate();
    if lines.next().map(|l| l.1) != Some(REGISTRY_HEADER) {
        return Err(format!(
            "{path}: not an ID registry (no `{REGISTRY_HEADER}`)"
        ));
    }
    let mut r = Registry {
        state: BTreeMap::new(),
        keys: BTreeMap::new(),
        serial_of: BTreeMap::new(),
    };
    for (i, line) in lines {
        let at = |m: &str| format!("{path}:{}: {m}", i + 1);
        let mut cols = line.split('\t');
        let (Some(serial), Some(state), Some(keys), None) =
            (cols.next(), cols.next(), cols.next(), cols.next())
        else {
            return Err(at("needs three tab-separated columns"));
        };
        let serial: u64 = serial.parse().map_err(|_| at("bad serial"))?;
        let state = match state {
            "x" => State::Gone,
            s if s.starts_with('>') => State::To(
                s.get(1..)
                    .unwrap_or("")
                    .parse()
                    .map_err(|_| at("bad target"))?,
            ),
            "" => return Err(at("empty state")),
            _ => State::Live,
        };
        if r.state.insert(serial, state).is_some() {
            return Err(at("serial repeated"));
        }
        let keys: Vec<String> = keys.split(' ').map(str::to_string).collect();
        for k in &keys {
            r.serial_of.entry(k.clone()).or_default().push(serial);
        }
        r.keys.insert(serial, keys);
    }
    Ok(r)
}

/// Why an expected key does not reach a row.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Unresolved {
    /// No serial in the registry holds the key: the forge never made a place of it.
    NotInRegistry,
    /// The key's serial was retired with no successor.
    Tombstone,
    /// The serial is live but this pack has no row for it (another build holds it).
    NotInPack,
}

/// The key in the registry's namespace.
fn registry_key(key: &str) -> String {
    for (golden_ns, forge_ns) in NAMESPACES {
        if let Some(rest) = key
            .strip_prefix(golden_ns)
            .and_then(|r| r.strip_prefix(':'))
        {
            return format!("{forge_ns}:{rest}");
        }
    }
    key.to_string()
}

/// Where one serial ends: its row in this pack, or why it has none.
fn fate(reg: &Registry, row_of: &BTreeMap<u64, usize>, mut s: u64) -> Result<usize, Unresolved> {
    for _ in 0..=reg.state.len() {
        match reg.state.get(&s) {
            Some(State::To(t)) => s = *t,
            Some(State::Gone) => return Err(Unresolved::Tombstone),
            Some(State::Live) => return row_of.get(&s).copied().ok_or(Unresolved::NotInPack),
            None => return Err(Unresolved::NotInRegistry),
        }
    }
    Err(Unresolved::NotInRegistry)
}

/// Every row of this pack that a key reaches, or why none does (the most specific reason).
fn resolve(
    reg: &Registry,
    row_of: &BTreeMap<u64, usize>,
    key: &str,
) -> Result<Vec<usize>, Unresolved> {
    let serials = reg
        .serial_of
        .get(&registry_key(key))
        .ok_or(Unresolved::NotInRegistry)?;
    let mut rows = Vec::new();
    let mut why = Unresolved::NotInRegistry;
    for &s in serials {
        match fate(reg, row_of, s) {
            Ok(r) => rows.push(r),
            Err(w) => why = why.max(w),
        }
    }
    rows.sort_unstable();
    rows.dedup();
    if rows.is_empty() { Err(why) } else { Ok(rows) }
}

/// The pack's sections the evaluator reads.
struct Sections<'a> {
    places: Places<'a>,
    hier: Hierarchy<'a>,
    ids: IdMap<'a>,
    names: Names<'a>,
    search: Search<'a>,
}

/// One S3 record per pack row, and serial → row.
fn records(
    p: &Sections<'_>,
    reg: &Registry,
    prof: &Profile,
) -> Result<(Vec<Rec>, BTreeMap<u64, usize>), String> {
    let Sections {
        places,
        hier,
        ids,
        names,
        search,
    } = p;
    let n = search.rows();
    // Serial → row, from IDMAP.
    let mut row_of: BTreeMap<u64, usize> = BTreeMap::new();
    for row in 0..n {
        let id = ids.id(row).ok_or_else(|| format!("row {row} has no ID"))?;
        row_of.insert(id.serial(), row);
    }
    let serial_of_row: BTreeMap<usize, u64> = row_of.iter().map(|(&s, &r)| (r, s)).collect();
    if let Some(s) = row_of.keys().find(|s| !reg.state.contains_key(s)) {
        return Err(format!(
            "serial {s} of the pack is not in the registry (wrong registry?)"
        ));
    }

    // One record per row, from the pack.
    let mut buf = vec![0_u8; TEXT_BUFFER];
    let mut canonical = |row: usize| -> Result<String, String> {
        names
            .canonical(row)
            .map(|nm| nm.text(&mut buf).map(str::to_string))
            .transpose()
            .map_err(|e| e.to_string())
            .map(Option::unwrap_or_default)
    };
    let unit_levels: Vec<u8> = prof.unit_types.iter().map(|u| u.level).collect();
    let mut recs = Vec::with_capacity(n);
    for row in 0..n {
        let level_of = |r: usize| {
            (places.kind(r) == Some(PlaceKind::Admin))
                .then(|| places.admin_level(r))
                .flatten()
        };
        let district_row = std::iter::once(row)
            .chain(hier.ancestors(row))
            .find(|&r| level_of(r) == Some(CONTEXT_LEVEL));
        let district = district_row.map(&mut canonical).transpose()?;
        // The unit type, by the profile's source rule on the unit's own code.
        let keys = serial_of_row
            .get(&row)
            .and_then(|s| reg.keys.get(s))
            .map_or(&[][..], Vec::as_slice);
        let unit_type = level_of(row)
            .filter(|l| unit_levels.contains(l))
            .and_then(|_| {
                keys.iter().find_map(|k| {
                    NAMESPACES.iter().find_map(|(source, forge_ns)| {
                        let code = k.strip_prefix(forge_ns)?.strip_prefix(':')?;
                        unit_type_of(prof, source, code)
                    })
                })
            });
        recs.push(Rec {
            key: keys.first().cloned().unwrap_or_default(),
            name: canonical(row)?,
            alts: Vec::new(),
            fcode: String::new(),
            district: district.unwrap_or_default(),
            importance: u32::from(search.importance(u32::try_from(row).unwrap_or(u32::MAX))),
            current: true,
            group: row,
            unit_type,
        });
    }

    Ok((recs, row_of))
}

/// The golden set's expected keys, resolved to rows.
struct Expectations<'g> {
    /// Key → rows.
    key_group: BTreeMap<&'g str, Vec<usize>>,
    /// Key → why it reaches no row.
    unresolved: BTreeMap<&'g str, Unresolved>,
    /// Keys looked up through `NAMESPACES`.
    aliased: usize,
    /// Queries none of whose answers (grade ≥ 2) reach a row: misses whatever the search does.
    unanswerable: Vec<&'g golden::GoldenQuery>,
}

impl Expectations<'_> {
    fn by_reason(&self) -> BTreeMap<Unresolved, usize> {
        let mut m = BTreeMap::new();
        for why in self.unresolved.values() {
            *m.entry(*why).or_default() += 1;
        }
        m
    }

    fn unanswerable_by_class(&self) -> BTreeMap<&str, usize> {
        let mut m = BTreeMap::new();
        for q in &self.unanswerable {
            *m.entry(q.class.as_str()).or_default() += 1;
        }
        m
    }
}

fn expectations<'g>(
    golden: &'g golden::GoldenFile,
    reg: &Registry,
    row_of: &BTreeMap<u64, usize>,
) -> Expectations<'g> {
    let mut x = Expectations {
        key_group: BTreeMap::new(),
        unresolved: BTreeMap::new(),
        aliased: 0,
        unanswerable: Vec::new(),
    };
    for q in &golden.queries {
        for k in q.expect.keys() {
            if x.key_group.contains_key(k.as_str()) || x.unresolved.contains_key(k.as_str()) {
                continue;
            }
            x.aliased += usize::from(registry_key(k) != *k);
            match resolve(reg, row_of, k) {
                Ok(rows) => {
                    x.key_group.insert(k, rows);
                }
                Err(why) => {
                    x.unresolved.insert(k, why);
                }
            }
        }
    }
    x.unanswerable = golden
        .queries
        .iter()
        .filter(|q| {
            !q.expect
                .iter()
                .any(|(k, &g)| g >= 2 && x.key_group.contains_key(k.as_str()))
        })
        .collect();
    x
}

fn run(args: &[String]) -> Result<(), String> {
    let [pack_path, golden_path, ids_path] = args else {
        return Err("usage: pack-eval <pack.tgd> <golden.yaml> <ids.tsv>".into());
    };
    let t0 = Instant::now();
    let bytes = std::fs::read(pack_path).map_err(|e| format!("{pack_path}: {e}"))?;
    let e = |e: teigeodata_format::Error| format!("{pack_path}: {e}");
    let pack = Pack::open(&bytes).map_err(e)?;
    pack.verify_sections().map_err(e)?;
    let section = |kind| {
        pack.section(kind)
            .ok_or_else(|| format!("{pack_path}: no {kind} section"))
    };
    let search_section = section(Kind::SEARCH)?;
    let places = Places::open(&section(Kind::PLACES)?).map_err(e)?;
    let hier = Hierarchy::open(&section(Kind::HIERARCHY)?).map_err(e)?;
    let ids = IdMap::open(&section(Kind::IDMAP)?).map_err(e)?;
    let names = Names::open(&section(Kind::NAMES)?).map_err(e)?;
    let search = Search::open(&search_section).map_err(e)?;
    search.check().map_err(e)?;
    let n = search.rows();

    let golden = golden::read(golden_path)?;
    let cc = golden
        .country
        .as_deref()
        .ok_or_else(|| format!("{golden_path}: no `country`"))?
        .to_lowercase();
    let address = pack.manifest().address();
    if !address.eq_ignore_ascii_case(&cc) {
        return Err(format!(
            "{pack_path} is for {address:?}, {golden_path} for {cc:?}"
        ));
    }
    let cc_dir = format!("data/countries/{cc}");
    let prof = profile(&format!("{cc_dir}/profile.yaml"));
    let reg = registry(ids_path)?;

    let sections = Sections {
        places,
        hier,
        ids,
        names,
        search,
    };
    let (recs, row_of) =
        records(&sections, &reg, &prof).map_err(|m| format!("{pack_path} with {ids_path}: {m}"))?;

    let x = expectations(&golden, &reg, &row_of);
    let t_load = t0.elapsed();

    let engine = Engine::on_pack(&recs, Packed::new(search, search_section.bytes()), &cc_dir);
    // `S3_ENGINE=product` scores the product's search (`teigeodata-engine`) instead of the
    // spike's: the gate of what users get.
    let product = std::env::var_os("S3_ENGINE").is_some_and(|v| v == "product");
    let real = product_search(pack_path)?;
    if let Some(qs) = std::env::var_os("S3_EXPLAIN") {
        explain(&qs.to_string_lossy(), &real, &recs);
        return Ok(());
    }
    let search_with = |q: &str| {
        if product {
            real(q).into_iter().map(|h| h.0).collect()
        } else {
            engine.search(q, 10)
        }
    };
    let which = if product {
        "teigeodata-engine (S3_ENGINE=product)"
    } else {
        "the spike's"
    };
    println!("search: {which}");
    let t0 = Instant::now();
    let scores = golden::score(search_with, &golden, &x.key_group, |g| {
        recs.get(g).map_or(String::new(), |r| {
            format!("{} [{}] ({})", r.name, r.district, r.key)
        })
    });
    let t_score = t0.elapsed();

    println!(
        "pack {pack_path}: {n} rows; SEARCH {} B, {} keys; {} of {n} rows with a district, {} with a unit type",
        search_section.bytes().len(),
        engine.pack.as_ref().map_or(0, |p| p.ix.keys()),
        recs.iter().filter(|r| !r.district.is_empty()).count(),
        recs.iter().filter(|r| r.unit_type.is_some()).count(),
    );
    println!(
        "golden {golden_path}: {} queries, {} distinct expected keys: {} resolved ({} through a namespace alias, {} to several rows), unresolved {:?}; {} queries with no answer in the pack {:?}",
        golden.queries.len(),
        x.key_group.len() + x.unresolved.len(),
        x.key_group.len(),
        x.aliased,
        x.key_group.values().filter(|r| r.len() > 1).count(),
        x.by_reason(),
        x.unanswerable.len(),
        x.unanswerable_by_class(),
    );
    println!("load {t_load:.2?}, score {t_score:.2?} (each query 22 times)");
    golden::print(&scores, std::env::var_os("S3_MISSES").is_some());
    if std::env::var_os("S3_MISSES").is_some() {
        print_gaps(&x);
    }
    gate_or_write(&scores)
}

/// `S3_GATE_WRITE=<file>` writes the golden baseline; `S3_GATE=<file>` fails the run when a
/// class loses a hit against it (the golden gate, phase 3.7).
fn gate_or_write(scores: &golden::Scores) -> Result<(), String> {
    if let Some(path) = std::env::var_os("S3_GATE_WRITE") {
        std::fs::write(&path, golden::baseline(scores))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("baseline written: {}", path.display());
    }
    let Some(path) = std::env::var_os("S3_GATE") else {
        return Ok(());
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let problems = golden::gate(scores, &text);
    if problems.is_empty() {
        println!("golden gate: ok against {}", path.display());
        return Ok(());
    }
    Err(format!(
        "golden gate failed against {}:\n  {}",
        path.display(),
        problems.join("\n  ")
    ))
}

/// The expected keys that did not resolve and the queries with no answer in the pack.
fn print_gaps(x: &Expectations<'_>) {
    println!("\n## unresolved expected keys ({})", x.unresolved.len());
    for (k, why) in &x.unresolved {
        println!("  {k}: {why:?}");
    }
    println!(
        "\n## queries with no answer in the pack ({})",
        x.unanswerable.len()
    );
    for q in &x.unanswerable {
        println!("  {} {} {:?} {:?}", q.id, q.class, q.query, q.expect);
    }
}

/// `S3_EXPLAIN="q1|q2"`: the product's hits for each query, with their scores.
fn explain(queries: &str, real: impl Fn(&str) -> Vec<(usize, i32)>, recs: &[Rec]) {
    for q in queries.split('|') {
        println!("{q:?}");
        for (row, score) in real(q) {
            let l = recs.get(row).map_or(String::new(), |r| {
                format!("{} [{}] ({})", r.name, r.district, r.key)
            });
            println!("  {score:>5} {l}");
        }
    }
}

/// The product's search (`teigeodata-engine`) on the pack: the best 10 rows of a query, with
/// their scores.
fn product_search(pack_path: &str) -> Result<impl Fn(&str) -> Vec<(usize, i32)>, String> {
    // `S3_LANGS=en,hi` reads only those languages' names parts (D-251), as a client that
    // installed them.
    let mut options = teigeodata_engine::LayerOptions::default();
    if let Some(langs) = std::env::var_os("S3_LANGS") {
        let langs = langs.to_string_lossy().into_owned();
        let parts: Vec<_> = langs
            .split(',')
            .map(|l| teigeodata_format::Part::language(l).ok_or(format!("{l}: not a language")))
            .collect::<Result<_, _>>()?;
        options.installed_parts = teigeodata_engine::Parts::ALL
            .languages(&parts)
            .ok_or("too many languages")?;
    }
    let layer = teigeodata_engine::Layer::open(std::path::Path::new(pack_path), options)
        .map_err(|e| format!("{pack_path}: {e}"))?;
    let notices = layer.attribution().map_or(0, |a| a.lines().count());
    println!("attribution (D-126): {notices} notices");
    let mut real = teigeodata_engine::Engine::new();
    real.add(layer);
    let scratch = std::cell::RefCell::new((
        teigeodata_engine::Scratch::default(),
        Vec::<teigeodata_engine::Hit>::new(),
    ));
    Ok(move |q: &str| -> Vec<(usize, i32)> {
        let mut guard = scratch.borrow_mut();
        let (s, hits) = &mut *guard;
        if real.search(q, 10, s, hits).is_err() {
            return Vec::new();
        }
        hits.iter()
            .map(|h| (h.place.row as usize, h.score))
            .collect()
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pack-eval: {e}");
            ExitCode::from(2)
        }
    }
}
