//! `inspect`, `verify` and `diff`: a pack read as it is stored.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use serde_json::{Value, json};
use teigeodata_core::{E6, PlaceId, Point};
use teigeodata_format::component::Component;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::names::Names;
use teigeodata_format::places::Places;
use teigeodata_format::{Encoding, Kind, Pack, Tag, key_id};

use crate::Fail;

/// The bytes of the pack at `path`.
pub(crate) fn read(path: &Path) -> Result<Vec<u8>, Fail> {
    std::fs::read(path).map_err(|e| Fail::of(path.display(), e))
}

/// The pack in `bytes` (from `path`), its head checked; the sections are checked when read.
pub(crate) fn open<'a>(path: &Path, bytes: &'a [u8]) -> Result<Pack<'a>, Fail> {
    Pack::open(bytes).map_err(|e| Fail::of(path.display(), e))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn encoding(e: Encoding) -> String {
    match e {
        Encoding::RAW => "raw".into(),
        Encoding::TABLE => "table".into(),
        Encoding(n) => format!("encoding-{n}"),
    }
}

/// `unix` seconds as an RFC 3339 UTC time (proleptic Gregorian, Howard Hinnant's
/// `civil_from_days`).
pub(crate) fn utc(unix: u64) -> String {
    let days = i64::try_from(unix / 86_400).unwrap_or(i64::MAX / 2);
    let secs = unix % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// The component a section belongs to, by name; `-` for none (`BLOCK_HASHES`, unknown kinds).
fn component_of(kind: Kind) -> &'static str {
    Component::of(kind).map_or("-", Component::name)
}

/// The manifest and sections as JSON: the one description `inspect` prints in either form.
fn describe(path: &Path, pack: &Pack<'_>, len: usize) -> Value {
    let m = pack.manifest();
    let mut manifest = serde_json::Map::new();
    for (tag, value) in m.entries() {
        let v = match tag {
            Tag::SCHEMA => m.u32(tag).map_or(Value::Null, Value::from),
            Tag::BBOX => m.bbox().map_or(Value::Null, |b| {
                json!([
                    b.min.lon_degrees(),
                    b.min.lat_degrees(),
                    b.max.lon_degrees(),
                    b.max.lat_degrees()
                ])
            }),
            Tag::ATTRIBUTION => m
                .attribution()
                .map_or(Value::Null, |a| json!(a.lines().collect::<Vec<_>>())),
            _ => core::str::from_utf8(value).map_or_else(|_| json!(hex(value)), Value::from),
        };
        manifest.insert(tag.to_string(), v);
    }
    let mut by_component: BTreeMap<&str, u64> = BTreeMap::new();
    let sections: Vec<Value> = pack
        .sections()
        .map(|s| {
            let bytes = s.bytes().len() as u64;
            *by_component.entry(component_of(s.kind())).or_default() += bytes;
            json!({
                "kind": s.kind().to_string(),
                "component": component_of(s.kind()),
                "encoding": encoding(s.encoding()),
                "items": s.item_count(),
                "bytes": bytes,
                "decoded": s.decoded_length(),
                "hash": hex(s.hash()),
            })
        })
        .collect();
    let (major, minor) = pack.format_version();
    json!({
        "path": path.display().to_string(),
        "bytes": len,
        "manifest": manifest,
        "format": format!("{major}.{minor}"),
        "min_reader": pack.min_reader_major(),
        "uuid": hex(&pack.uuid()),
        "build_id": format!("{:016x}", pack.build_id()),
        "created": utc(pack.created_unix()),
        "signed_by": pack.signature().map(|(id, _)| hex(&id)),
        "sections": sections,
        "components": by_component,
    })
}

pub(crate) fn inspect(path: &Path, as_json: bool, out: &mut dyn Write) -> Result<(), Fail> {
    let bytes = read(path)?;
    let pack = open(path, &bytes)?;
    let d = describe(path, &pack, bytes.len());
    if as_json {
        serde_json::to_writer_pretty(&mut *out, &d)?;
        writeln!(out)?;
        return Ok(());
    }
    let m = pack.manifest();
    let (major, minor) = pack.format_version();
    writeln!(
        out,
        "{} {} {}  ({} bytes, format {major}.{minor}, created {})",
        m.address(),
        m.component(),
        m.release(),
        bytes.len(),
        utc(pack.created_unix()),
    )?;
    match pack.signature() {
        Some((id, _)) => writeln!(out, "signed by key {}", hex(&id))?,
        None => writeln!(out, "unsigned")?,
    }
    for (tag, _) in m.entries() {
        match tag {
            Tag::ADDRESS | Tag::COMPONENT | Tag::RELEASE => {}
            Tag::ATTRIBUTION => {
                let a = m.attribution().unwrap_or_default();
                writeln!(out, "attribution: {} notices", a.lines().count())?;
                for line in a.lines() {
                    writeln!(out, "  {line}")?;
                }
            }
            _ => {
                let v = d.get("manifest").and_then(|v| v.get(tag.to_string()));
                writeln!(out, "{tag}: {}", v.unwrap_or(&Value::Null))?;
            }
        }
    }
    writeln!(
        out,
        "\n{:<14} {:<14} {:<6} {:>10} {:>12} {:>12}",
        "section", "component", "enc", "items", "bytes", "decoded"
    )?;
    for s in pack.sections() {
        writeln!(
            out,
            "{:<14} {:<14} {:<6} {:>10} {:>12} {:>12}",
            s.kind().to_string(),
            component_of(s.kind()),
            encoding(s.encoding()),
            s.item_count(),
            s.bytes().len(),
            s.decoded_length()
        )?;
    }
    writeln!(out, "\n{:<14} {:>12}", "component", "bytes")?;
    if let Some(c) = d.get("components").and_then(Value::as_object) {
        for (name, n) in c {
            writeln!(out, "{name:<14} {:>12}", n.as_u64().unwrap_or(0))?;
        }
    }
    Ok(())
}

pub(crate) fn verify(
    path: &Path,
    deep: bool,
    keys: &[String],
    as_json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let trusted = keys
        .iter()
        .map(|k| {
            let mut b = [0_u8; 32];
            let ok = k.len() == 64
                && b.iter_mut().enumerate().all(|(i, v)| {
                    k.get(2 * i..2 * i + 2)
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                        .map(|x| *v = x)
                        .is_some()
                });
            if !ok {
                return Err(Fail::Refused(format!("--key {k}: not 64 hex digits")));
            }
            ed25519_dalek::VerifyingKey::from_bytes(&b)
                .map_err(|e| Fail::of(format_args!("--key {k}"), e))
        })
        .collect::<Result<Vec<_>, Fail>>()?;
    let bytes = read(path)?;
    let pack = open(path, &bytes)?;
    let mut problems: Vec<String> = Vec::new();
    let mut checked = 0_usize;
    for s in pack.sections() {
        match s.verify() {
            Ok(()) => checked += 1,
            Err(e) => problems.push(e.to_string()),
        }
    }
    let mut deep_checked = 0_usize;
    if deep && problems.is_empty() {
        teigeodata_format::check::sections(&pack, |kind, r| match r {
            Ok(true) => deep_checked += 1,
            Ok(false) => {}
            Err(e) => problems.push(format!("{kind}: {e}")),
        });
    }
    let signer = pack.signature().map(|(id, _)| hex(&id));
    if !trusted.is_empty() {
        match pack.verify_signature(&trusted) {
            Ok(i) => {
                let id = trusted.get(i).map(|k| hex(&key_id(k)));
                debug_assert_eq!(id, signer);
            }
            Err(e) => problems.push(e.to_string()),
        }
    }
    let m = pack.manifest();
    let name = format!("{} {} {}", m.address(), m.component(), m.release());
    if as_json {
        serde_json::to_writer_pretty(
            &mut *out,
            &json!({
                "pack": name,
                "ok": problems.is_empty(),
                "sections": checked,
                "deep": deep.then_some(deep_checked),
                "signed_by": signer,
                "signature_checked": !trusted.is_empty() && problems.is_empty(),
                "problems": problems,
            }),
        )?;
        writeln!(out)?;
    } else if problems.is_empty() {
        let mut line = format!("ok: {name}: {checked} sections verified");
        if deep {
            let _ = write!(line, ", {deep_checked} checked value by value");
        }
        match (&signer, trusted.is_empty()) {
            (Some(id), false) => {
                let _ = write!(line, ", signed by trusted key {id}");
            }
            (Some(id), true) => {
                let _ = write!(line, ", signed by key {id} (not checked: no --key)");
            }
            (None, _) => line.push_str(", unsigned"),
        }
        writeln!(out, "{line}")?;
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Fail::Refused(format!(
            "{}: {}",
            path.display(),
            problems.join("; ")
        )))
    }
}

/// The great-circle distance in metres (mean Earth radius).
fn meters(a: Point<E6>, b: Point<E6>) -> f64 {
    let (la1, lo1) = (a.lat_degrees().to_radians(), a.lon_degrees().to_radians());
    let (la2, lo2) = (b.lat_degrees().to_radians(), b.lon_degrees().to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * 6_371_008.8 * h.sqrt().min(1.0).asin()
}

/// The readers `diff` compares.
struct Rows<'a> {
    ids: IdMap<'a>,
    places: Places<'a>,
    names: Option<Names<'a>>,
}

impl<'a> Rows<'a> {
    fn of(path: &Path, pack: &Pack<'a>) -> Result<Self, Fail> {
        let e = |e| Fail::of(path.display(), e);
        let need = |k: Kind| {
            pack.section(k)
                .ok_or_else(|| Fail::Refused(format!("{}: no {k} section", path.display())))
        };
        let ids = need(Kind::IDMAP)?;
        let places = need(Kind::PLACES)?;
        ids.verify().map_err(e)?;
        places.verify().map_err(e)?;
        let names = match pack.section(Kind::NAMES) {
            Some(s) => {
                s.verify().map_err(e)?;
                Some(Names::open(&s).map_err(e)?)
            }
            None => None,
        };
        Ok(Self {
            ids: IdMap::open(&ids).map_err(e)?,
            places: Places::open(&places).map_err(e)?,
            names,
        })
    }

    fn name<'b>(&self, row: usize, buf: &'b mut [u8]) -> Option<&'b str> {
        self.names.as_ref()?.canonical(row)?.text(buf).ok()
    }
}

/// One kind of change: how many, and the first few.
#[derive(Default)]
struct Changes {
    count: u64,
    shown: Vec<Value>,
}

impl Changes {
    fn add(&mut self, limit: usize, example: impl FnOnce() -> Value) {
        self.count += 1;
        if self.shown.len() < limit {
            self.shown.push(example());
        }
    }
}

/// One section's change between two packs.
#[derive(serde::Serialize)]
struct SectionChange {
    kind: String,
    change: &'static str,
    old_bytes: Option<usize>,
    new_bytes: Option<usize>,
}

fn section_changes(old: &Pack<'_>, new: &Pack<'_>) -> Vec<SectionChange> {
    let kinds: std::collections::BTreeSet<u32> = old
        .sections()
        .chain(new.sections())
        .map(|s| s.kind().0)
        .collect();
    kinds
        .into_iter()
        .map(|k| {
            let kind = Kind(k);
            let (before, after) = (old.section(kind), new.section(kind));
            let change = match (&before, &after) {
                (Some(x), Some(y)) if x.hash() == y.hash() => "same",
                (Some(_), Some(_)) => "changed",
                (None, _) => "added",
                (_, None) => "removed",
            };
            SectionChange {
                kind: kind.to_string(),
                change,
                old_bytes: before.map(|s| s.bytes().len()),
                new_bytes: after.map(|s| s.bytes().len()),
            }
        })
        .collect()
}

fn print_diff(
    sections: &[SectionChange],
    places: &[(&str, Changes)],
    out: &mut dyn Write,
) -> Result<(), Fail> {
    writeln!(out, "sections:")?;
    let bytes = |n: Option<usize>| n.map_or_else(|| "-".to_string(), |n| n.to_string());
    for s in sections.iter().filter(|s| s.change != "same") {
        writeln!(
            out,
            "  {:<14} {:<8} {} -> {} bytes",
            s.kind,
            s.change,
            bytes(s.old_bytes),
            bytes(s.new_bytes)
        )?;
    }
    let same = sections.iter().filter(|s| s.change == "same").count();
    writeln!(out, "  {same} unchanged")?;
    writeln!(out, "places:")?;
    for (what, c) in places {
        writeln!(out, "  {what}: {}", c.count)?;
        for e in &c.shown {
            writeln!(out, "    {e}")?;
        }
    }
    Ok(())
}

pub(crate) fn diff(
    old_path: &Path,
    new_path: &Path,
    limit: usize,
    as_json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let (old_bytes, new_bytes) = (read(old_path)?, read(new_path)?);
    let (old, new) = (open(old_path, &old_bytes)?, open(new_path, &new_bytes)?);
    let sections = section_changes(&old, &new);
    // Places, by ID.
    let (older, newer) = (Rows::of(old_path, &old)?, Rows::of(new_path, &new)?);
    let (mut added, mut removed, mut moved, mut renamed, mut retyped) = Default::default();
    let (mut na, mut nb) = ([0_u8; 1024], [0_u8; 1024]);
    let id_of = |id: PlaceId| id.to_string();
    for row in 0..older.ids.len() {
        let Some(id) = older.ids.id(row) else {
            continue;
        };
        let Some(new_row) = newer.ids.row(id) else {
            let name = older.name(row, &mut na).map(str::to_owned);
            Changes::add(
                &mut removed,
                limit,
                || json!({"id": id_of(id), "name": name}),
            );
            continue;
        };
        let (pa, pb) = (older.places.point(row), newer.places.point(new_row));
        if let (Some(p), Some(q)) = (pa, pb)
            && p != q
        {
            let m = meters(p, q);
            Changes::add(
                &mut moved,
                limit,
                || json!({"id": id_of(id), "meters": (m * 10.0).round() / 10.0}),
            );
        }
        let (x, y) = (older.name(row, &mut na), newer.name(new_row, &mut nb));
        if x != y {
            let (x, y) = (x.map(str::to_owned), y.map(str::to_owned));
            Changes::add(
                &mut renamed,
                limit,
                || json!({"id": id_of(id), "old": x, "new": y}),
            );
        }
        let (ka, kb) = (
            (older.places.kind(row), older.places.admin_level(row)),
            (
                newer.places.kind(new_row),
                newer.places.admin_level(new_row),
            ),
        );
        if ka != kb {
            let show = |(k, l): (Option<teigeodata_core::PlaceKind>, Option<u8>)| {
                format!("{}/{}", k.map_or("?", |k| k.name()), l.unwrap_or(0))
            };
            Changes::add(
                &mut retyped,
                limit,
                || json!({"id": id_of(id), "old": show(ka), "new": show(kb)}),
            );
        }
    }
    for row in 0..newer.ids.len() {
        let Some(id) = newer.ids.id(row) else {
            continue;
        };
        if older.ids.row(id).is_none() {
            let name = newer.name(row, &mut nb).map(str::to_owned);
            Changes::add(&mut added, limit, || json!({"id": id_of(id), "name": name}));
        }
    }
    let places: [(&str, Changes); 5] = [
        ("added", added),
        ("removed", removed),
        ("moved", moved),
        ("renamed", renamed),
        ("retyped", retyped),
    ];
    if as_json {
        let mut p = serde_json::Map::new();
        for (what, c) in places {
            p.insert(what.into(), json!({"count": c.count, "first": c.shown}));
        }
        serde_json::to_writer_pretty(&mut *out, &json!({"sections": sections, "places": p}))?;
        writeln!(out)?;
        return Ok(());
    }
    print_diff(&sections, &places, out)
}
