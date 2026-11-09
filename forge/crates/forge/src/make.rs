//! `forge make`: one country's pack from its recipe, every stage keyed by content (ADR-0035,
//! D-095).
//!
//! ```text
//! forge make <cc> --release <version> [--lite] [--key <seed-file>] [--ids <registry>] [--force]
//! ```
//!
//! The recipe is `data/countries/<cc>/build.yaml`. The stages are `read` for each points
//! source, `conflate` over them, `buildings` and `core`, `topology`, `dem` (the recipe's DEM
//! tileset over its extent, for place elevations, D-111), then `build`. Each stage's key is
//! a BLAKE3 hash of the stage name, the stage's code, the pinned inputs (their `sources.lock`
//! entries), the configuration (the country profile, the arguments, the ID registry) and the
//! keys of the stages it reads.
//! A stage whose output directory holds a stamp with its key is skipped. A stamp is written
//! only after the stage succeeds, and removed before it starts, so a killed run resumes from
//! the first stage it did not finish. `--force` runs every stage.
//!
//! A stage's code is the hash `build.rs` computes over the modules it reaches and the path
//! dependencies (D-108): a change to the names code reruns `build` only, not the 40–120 s of
//! stages before it.

use std::fmt::Write as _;
use std::fs;
use std::io::{ErrorKind, Read as _};
use std::path::Path;
use std::time::Instant;

use serde::Deserialize;

use crate::build;
use crate::buildings;
use crate::conflate;
use crate::core;
use crate::dem;
use crate::lock::Lock;
use crate::read;
use crate::topo;

/// Each stage's code version, from `build.rs`.
const CODE_READ: &[u8] = env!("FORGE_CODE_READ").as_bytes();
const CODE_CONFLATE: &[u8] = env!("FORGE_CODE_CONFLATE").as_bytes();
const CODE_BUILDINGS: &[u8] = env!("FORGE_CODE_BUILDINGS").as_bytes();
const CODE_CORE: &[u8] = env!("FORGE_CODE_CORE").as_bytes();
const CODE_TOPOLOGY: &[u8] = env!("FORGE_CODE_TOPOLOGY").as_bytes();
const CODE_DEM: &[u8] = env!("FORGE_CODE_DEM").as_bytes();
const CODE_BUILD: &[u8] = env!("FORGE_CODE_BUILD").as_bytes();

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    #[expect(dead_code, reason = "checked by `cargo xtask check-data`")]
    schema: String,
    admin: String,
    #[serde(default)]
    timezones: Option<String>,
    points: Vec<Points>,
    #[serde(default)]
    extent: Option<String>,
    #[serde(default)]
    buildings: Vec<Buildings>,
    /// The DEM tileset place elevations come from (D-111).
    #[serde(default)]
    dem: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Buildings {
    reader: String,
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Points {
    reader: String,
    source: String,
    #[serde(default)]
    extra: Vec<String>,
}

/// A stage key: BLAKE3 over length-prefixed parts, so no two part lists collide.
fn key(parts: &[&[u8]]) -> String {
    let mut h = blake3::Hasher::new();
    for p in parts {
        h.update(&(p.len() as u64).to_le_bytes());
        h.update(p);
    }
    h.finalize().to_hex().to_string()
}

/// The files a stage owns in `dir` (all but stamps, or those whose name contains `owns`),
/// one line each: BLAKE3, size, name; sorted by name.
fn manifest(dir: &Path, owns: Option<&str>) -> Result<String, String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with(".key") && owns.is_none_or(|o| n.contains(o)))
        .collect();
    names.sort();
    let mut out = String::new();
    // Streamed in fixed chunks: memory stays bounded whatever the output size.
    let mut buf = vec![0_u8; 1 << 20];
    for n in names {
        let path = dir.join(&n);
        let mut h = blake3::Hasher::new();
        let mut f = fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        loop {
            let n = f
                .read(&mut buf)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let Some(chunk) = buf.get(..n).filter(|c| !c.is_empty()) else {
                break;
            };
            h.update(chunk);
        }
        let size = fs::metadata(&path).map_err(|e| e.to_string())?.len();
        let _ = writeln!(out, "{} {size} {n}", h.finalize().to_hex());
    }
    Ok(out)
}

/// Runs `run` unless `dir/stamp` holds `key` and the stage's files are as it left them;
/// `Ok(None)` for a hit, else the stage's summary.
fn stage(
    (dir, stamp, owns): (&Path, &str, Option<&str>),
    key: &str,
    force: bool,
    run: impl FnOnce() -> Result<String, String>,
) -> Result<Option<String>, String> {
    let path = dir.join(stamp);
    if !force
        && let Ok(text) = fs::read_to_string(&path)
        && text.lines().next() == Some(key)
        && text.split_once('\n').map(|(_, m)| m) == Some(manifest(dir, owns)?.as_str())
    {
        return Ok(None);
    }
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", path.display())),
    }
    let summary = run()?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_atomic(&path, format!("{key}\n{}", manifest(dir, owns)?).as_bytes())?;
    Ok(Some(summary))
}

/// Writes `bytes` to `path` through `<path>.tmp` and a rename, so a reader or a killed run never
/// sees a partial file at `path`.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The pinned identity of sources: each one's ID, SHA-256 and size.
fn pinned(lock: &Lock, ids: &[&str]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for id in ids {
        let s = lock.get(id)?;
        out.extend_from_slice(format!("{}\t{}\t{}\n", s.id, s.sha256, s.size).as_bytes());
    }
    Ok(out)
}

/// The postal inputs of a build: `postal.yaml`, each codes file it names, and the pinned
/// source each is checked against (D-105). Empty when the country has no postal file.
fn postal_inputs(country: &Path, lock: &Lock) -> Result<Vec<u8>, String> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        codes: Vec<Codes>,
    }
    #[derive(Deserialize)]
    struct Codes {
        file: String,
        source: String,
    }
    let yaml = read_or_empty(&country.join("postal.yaml"))?;
    if yaml.is_empty() {
        return Ok(yaml);
    }
    let text = String::from_utf8_lossy(&yaml);
    let file: File = serde_saphyr::from_str(&text).map_err(|e| format!("postal.yaml: {e}"))?;
    let mut out = yaml.clone();
    for c in &file.codes {
        out.extend_from_slice(blake3::hash(&read_or_empty(&country.join(&c.file))?).as_bytes());
        out.extend(pinned(lock, &[c.source.as_str()])?);
    }
    Ok(out)
}

/// The naming inputs of a build: the country's lexicon and each romanisation profile its
/// `profile.yaml` lists (names-canonical §5.2).
fn names_inputs(root: &Path, country: &Path) -> Result<Vec<u8>, String> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        romanise: Vec<String>,
    }
    let mut out = blake3::hash(&read_or_empty(&country.join("lexicon.yaml"))?)
        .as_bytes()
        .to_vec();
    let text = String::from_utf8_lossy(&read_or_empty(&country.join("profile.yaml"))?).into_owned();
    if text.is_empty() {
        return Ok(out);
    }
    let file: File = serde_saphyr::from_str(&text).map_err(|e| format!("profile.yaml: {e}"))?;
    for id in &file.romanise {
        let p = root.join("data/romanise").join(format!("{id}.yaml"));
        out.extend_from_slice(id.as_bytes());
        out.extend_from_slice(blake3::hash(&read_or_empty(&p)?).as_bytes());
    }
    Ok(out)
}

fn read_or_empty(path: &Path) -> Result<Vec<u8>, String> {
    match fs::read(path) {
        Ok(b) => Ok(b),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Runs stages and keeps the stage table.
struct Runner {
    force: bool,
    log: String,
    ran: usize,
}

impl Runner {
    fn step(
        &mut self,
        name: &str,
        at: (&Path, &str, Option<&str>),
        key: &str,
        run: impl FnOnce() -> Result<String, String>,
    ) -> Result<(), String> {
        let out = stage(at, key, self.force, run)?;
        let how = out.map_or_else(
            || "cached".to_string(),
            |s| {
                self.ran += 1;
                first_line(&s)
            },
        );
        let short = key.get(..12).unwrap_or(key);
        let _ = writeln!(self.log, "| {name} | `{short}` | {how} |");
        Ok(())
    }
}

/// One `read` stage per points source the build may read (G-lic at schedule time): the
/// sources read and their stage keys, in recipe order.
fn read_stages(
    root: &Path,
    recipe: &Recipe,
    lock: &Lock,
    lite: bool,
    r: &mut Runner,
) -> Result<(Vec<String>, Vec<String>), String> {
    let may_read = |id: &str| -> Result<bool, String> {
        Ok(!lite || build::LITE.contains(&lock.get(id)?.licence_class.as_str()))
    };
    let (mut sources, mut read_keys) = (Vec::new(), Vec::new());
    for p in &recipe.points {
        if !may_read(&p.source)? {
            continue;
        }
        let mut ids: Vec<String> = vec![p.source.clone()];
        for e in &p.extra {
            if may_read(e)? {
                ids.push(e.clone());
            }
        }
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let k = key(&[
            b"read",
            CODE_READ,
            p.reader.as_bytes(),
            &pinned(lock, &id_refs)?,
        ]);
        let dir = root.join("data/build/read").join(&p.source);
        r.step(
            &format!("read {}", p.source),
            (&dir, ".key", None),
            &k,
            || read::run(root, &p.reader, &ids),
        )?;
        sources.push(p.source.clone());
        read_keys.push(k);
    }
    Ok((sources, read_keys))
}

/// The `buildings` stage of every building source the build may read, then `core` over them:
/// the core run's name and key, or `None` when the recipe names no building source (or the
/// build may read none).
fn core_stages(
    root: &Path,
    recipe: &Recipe,
    lock: &Lock,
    ck: &str,
    sources: &[String],
    lite: bool,
    r: &mut Runner,
) -> Result<Option<(String, String)>, String> {
    let (mut names, mut keys) = (Vec::new(), Vec::new());
    for b in &recipe.buildings {
        if lite && !build::LITE.contains(&lock.get(&b.source)?.licence_class.as_str()) {
            continue;
        }
        let extent = recipe
            .extent
            .as_deref()
            .ok_or("buildings need an extent in build.yaml")?;
        let k = key(&[
            b"buildings",
            CODE_BUILDINGS,
            b.reader.as_bytes(),
            &pinned(lock, &[&b.source])?,
            extent.as_bytes(),
        ]);
        let dir = root.join("data/build/buildings").join(&b.source);
        r.step(
            &format!("buildings {}", b.source),
            (&dir, ".key", None),
            &k,
            || buildings::run(root, &b.reader, &b.source, extent),
        )?;
        names.push(b.source.clone());
        keys.push(k);
    }
    if names.is_empty() {
        return Ok(None);
    }
    let (points_run, run) = (sources.join("+"), names.join("+"));
    let k = key(&[b"core", CODE_CORE, ck.as_bytes(), keys.join(",").as_bytes()]);
    let dir = root.join("data/build/core").join(&points_run).join(&run);
    r.step("core", (&dir, ".key", None), &k, || {
        core::run(root, &points_run, &run)
    })?;
    Ok(Some((run, k)))
}

/// The admin source's shared arcs and polygons (B10), the same for main and lite; its key.
fn topology_stage(root: &Path, admin: &str, lock: &Lock, r: &mut Runner) -> Result<String, String> {
    let k = key(&[b"topology", CODE_TOPOLOGY, &pinned(lock, &[admin])?]);
    let dir = root.join("data/build/topology").join(admin);
    r.step(
        &format!("topology {admin}"),
        (&dir, ".key", None),
        &k,
        || topo::run(root, admin),
    )?;
    Ok(k)
}

/// The box of whole-degree tiles that covers an extent `west,south,east,north`: the tiles
/// holding its corners, so an edge on a whole degree takes no tile beyond it.
fn tile_box(extent: &str) -> Result<String, String> {
    let v: Vec<f64> = extent
        .split(',')
        .map(|x| x.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("extent `{extent}`: {e}"))?;
    let &[west, south, east, north] = v.as_slice() else {
        return Err(format!("extent `{extent}`: expected west,south,east,north"));
    };
    #[allow(clippy::cast_possible_truncation, reason = "whole degrees")]
    let deg = |x: f64| x as i32;
    Ok(format!(
        "{},{},{},{}",
        deg(west.floor()),
        deg(south.floor()),
        deg(east.ceil()) - 1,
        deg(north.ceil()) - 1
    ))
}

/// The `dem` stage: the recipe's DEM tileset as a DEM section over the tiles of its extent,
/// when the build may read it; its key.
fn dem_stage(
    root: &Path,
    recipe: &Recipe,
    lock: &Lock,
    lite: bool,
    r: &mut Runner,
) -> Result<Option<String>, String> {
    let Some(id) = &recipe.dem else {
        return Ok(None);
    };
    if lite && !build::LITE.contains(&lock.get(id)?.licence_class.as_str()) {
        return Ok(None);
    }
    let extent = recipe
        .extent
        .as_deref()
        .ok_or("a DEM needs an extent in build.yaml")?;
    let bx = tile_box(extent)?;
    let k = key(&[b"dem", CODE_DEM, &pinned(lock, &[id])?, bx.as_bytes()]);
    let dir = root.join("data/build/dem-section").join(id);
    r.step(&format!("dem {id}"), (&dir, ".key", None), &k, || {
        dem::section::run(root, &[id.clone(), bx.clone()])
    })?;
    Ok(Some(k))
}

/// `forge make <cc> …`.
pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let usage = "usage: forge make <cc> --release <version> [--lite] [--key <seed-file>] [--ids <registry>] [--force]";
    let cc = args.first().ok_or(usage)?;
    let pass: Vec<String> = args
        .iter()
        .skip(1)
        .filter(|a| *a != "--force")
        .cloned()
        .collect();
    let lite = pass.iter().any(|a| a == "--lite");
    let flag = |name: &str| {
        pass.iter()
            .position(|a| a == name)
            .and_then(|i| pass.get(i + 1))
    };
    if flag("--release").is_none() {
        return Err(usage.into());
    }
    let country = root.join("data/countries").join(cc);
    let recipe_path = country.join("build.yaml");
    let text =
        fs::read_to_string(&recipe_path).map_err(|e| format!("{}: {e}", recipe_path.display()))?;
    let recipe: Recipe =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", recipe_path.display()))?;
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let profile = fs::read(country.join("profile.yaml")).map_err(|e| e.to_string())?;
    let mut r = Runner {
        force: args.iter().any(|a| a == "--force"),
        log: String::from("| Stage | Key | Run |\n|---|---|---|\n"),
        ran: 0,
    };

    let (sources, read_keys) = read_stages(root, &recipe, &lock, lite, &mut r)?;
    if sources.len() < 2 {
        return Err(format!(
            "{cc}: this build may read {} points source(s); conflation needs two",
            sources.len()
        ));
    }

    // conflate: over the read stages, configured by the profile.
    let joined_keys = read_keys.join(",");
    let ck = key(&[
        b"conflate",
        CODE_CONFLATE,
        cc.as_bytes(),
        &profile,
        joined_keys.as_bytes(),
    ]);
    let dir = root.join("data/build/conflate").join(sources.join("+"));
    r.step("conflate", (&dir, ".key", None), &ck, || {
        conflate::run(root, cc, &sources)
    })?;

    // buildings and core: the consensus points with the settlement core (D-091).
    let core_key = core_stages(root, &recipe, &lock, &ck, &sources, lite, &mut r)?;

    let tk = topology_stage(root, &recipe.admin, &lock, &mut r)?;
    let dem_key = dem_stage(root, &recipe, &lock, lite, &mut r)?;

    // build: the conflation (and core), the topology, the admin and timezone sources, the
    // postal and naming inputs, the arguments, the ID registry and the signing seed.
    let mut build_args: Vec<String> = vec![cc.clone(), "--topology".to_string()];
    build_args.extend(pass.iter().cloned());
    if let Some((run, _)) = &core_key {
        build_args.extend(["--core".to_string(), run.clone()]);
    }
    build_args.extend(["--admin".to_string(), recipe.admin.clone()]);
    if let Some(tz) = &recipe.timezones {
        build_args.extend(["--tz".to_string(), tz.clone()]);
    }
    // Passed in the lite build too, whose licence gate then reports it as not read.
    if let Some(d) = &recipe.dem {
        build_args.extend(["--dem".to_string(), d.clone()]);
    }
    build_args.extend(sources.iter().cloned());
    let mut inputs: Vec<&str> = vec![recipe.admin.as_str()];
    inputs.extend(recipe.timezones.as_deref());
    let file = |name: &str| flag(name).map_or(Ok(Vec::new()), |p| read_or_empty(&root.join(p)));
    let bk = key(&[
        b"build",
        CODE_BUILD,
        ck.as_bytes(),
        core_key.as_ref().map_or(&b""[..], |k| k.1.as_bytes()),
        tk.as_bytes(),
        dem_key.as_deref().unwrap_or_default().as_bytes(),
        &pinned(&lock, &inputs)?,
        &profile,
        &postal_inputs(&country, &lock)?,
        &names_inputs(root, &country)?,
        build_args.join("\0").as_bytes(),
        &file("--ids")?,
        blake3::hash(&file("--key")?).as_bytes(),
    ]);
    let build_name = if lite { "lite" } else { "main" };
    let dir = root.join("data/build/pack").join(cc);
    // The pack directory holds both builds: each stamp covers only its build's files.
    let stamp = format!(".key-{build_name}");
    let owns = format!("-{build_name}.");
    r.step("build", (&dir, &stamp, Some(&owns)), &bk, || {
        build::run(root, &build_args)
    })?;
    let summary = dir.join(format!("summary-{build_name}.md"));
    Ok(format!(
        "{}\n{} in {:.1?}: {} of the stages ran.",
        r.log,
        summary.strip_prefix(root).unwrap_or(&summary).display(),
        t.elapsed(),
        r.ran,
    ))
}

/// The first line of a stage summary, for the stage table.
fn first_line(s: &str) -> String {
    let line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("ran");
    line.trim_start_matches('#').trim().replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::{key, stage, tile_box};
    use std::cell::Cell;
    use std::fs;

    #[test]
    fn a_dem_box_takes_the_tiles_of_the_extent_corners() {
        assert_eq!(tile_box("80.0,26.3,88.25,30.5"), Ok("80,26,88,30".into()));
        assert_eq!(tile_box("80,26,88,30"), Ok("80,26,87,29".into()));
        assert_eq!(tile_box("-1.5,-0.5,0.5,1"), Ok("-2,-1,0,0".into()));
        assert!(tile_box("80,26,88").is_err());
    }

    #[test]
    fn stages_skip_on_their_key_and_resume_after_a_failure() {
        let dir = std::env::temp_dir().join(format!("forge-make-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let at = (dir.as_path(), ".key", None);
        let runs = Cell::new(0);
        let ok = || {
            runs.set(runs.get() + 1);
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            fs::write(dir.join("out.parquet"), b"rows").map_err(|e| e.to_string())?;
            Ok("done".to_string())
        };
        let ran = Ok(Some("done".to_string()));
        assert_eq!(stage(at, "a", false, ok), ran);
        assert_eq!(stage(at, "a", false, ok), Ok(None), "a hit");
        assert_eq!(stage(at, "a", true, ok), ran, "forced");
        assert_eq!(stage(at, "b", false, ok), ran, "a new key");
        assert_eq!(runs.get(), 3);
        // An output changed behind the stage's back: it runs again.
        fs::write(dir.join("out.parquet"), b"rowz").expect("write");
        assert_eq!(stage(at, "b", false, ok), ran, "a changed output");
        // A file the stage does not own does not count.
        fs::write(dir.join("np-places-lite.tgd"), b"x").expect("write");
        let main = (dir.as_path(), ".key-main", Some("-main."));
        assert_eq!(stage(main, "m", false, ok), ran);
        fs::write(dir.join("np-places-lite.tgd"), b"y").expect("write");
        assert_eq!(
            stage(main, "m", false, ok),
            Ok(None),
            "another build's file"
        );
        assert_eq!(runs.get(), 5);
        // A failed run leaves no stamp, so the next run with the old key reruns too.
        assert!(stage(at, "c", false, || Err("killed".into())).is_err());
        assert!(!dir.join(".key").exists());
        assert_eq!(stage(at, "b", false, ok), ran);
        assert_eq!(runs.get(), 6);
        let _ = fs::remove_dir_all(&dir);
        // Parts are length-prefixed: moving a byte between parts changes the key.
        assert_ne!(key(&[b"ab", b"c"]), key(&[b"a", b"bc"]));
        assert_eq!(key(&[b"ab", b"c"]), key(&[b"ab", b"c"]));
    }
}
