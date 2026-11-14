//! The ID registry: stable serials across releases (ADR-0008, D-075, D-094).
//!
//! A registry is a text file, one line per serial ever allocated in a country's namespace,
//! sorted by serial:
//!
//! ```text
//! # teigeodata id registry 1
//! 1\tlite,main\tgeonames:1283240 osm:node/123 wikidata:Q3037
//! 2\t>1\tgeonames:99
//! 3\tx\tosm:node/77
//! ```
//!
//! Three columns separated by tabs (`\t` above): the serial; its state, either the builds whose packs hold it now (`main`, `lite`,
//! or a world part of a build such as `lite-geonames-cities5000`, D-267; comma-separated and sorted), `>N` for a serial retired into serial N, or `x` for a tombstone;
//! the source keys the place was made of, sorted and separated by spaces. The registry doubles
//! as the bridge file: source key → our ID.
//!
//! A build matches its places to live serials by shared keys and allocates the next serials to
//! the rest, in row (Hilbert) order, so a first build is the D-075 layout. A serial is retired
//! only when no build holds it any more: into the live serial that now holds most of its keys
//! (a merge), else as a tombstone. Serials are never reused, and a retired serial never comes
//! back. Splits keep the serial with the place holding most of its keys.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

use teigeodata_core::SERIAL_MAX;

const HEADER: &str = "# teigeodata id registry 1";

/// What became of a serial.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum State {
    /// Held by these builds' packs (never empty).
    Live(BTreeSet<String>),
    /// Retired into this serial.
    To(u64),
    /// Retired with no successor.
    Gone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) state: State,
    pub(super) keys: BTreeSet<String>,
}

/// Every serial of one namespace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Registry {
    pub(super) entries: BTreeMap<u64, Entry>,
}

/// The outcome of a build's allocation.
#[derive(Debug)]
pub(super) struct Assignment {
    /// The serial of each row.
    pub(super) serials: Vec<u64>,
    /// REDIRECTS for this pack: every retired serial whose final successor is in the pack, or
    /// which has none; sorted by serial.
    pub(super) redirects: Vec<(u64, Option<u64>)>,
    pub(super) registry: Registry,
    pub(super) kept: usize,
    pub(super) new: usize,
    /// Serials this build retired: into a successor, and with none.
    pub(super) merged: usize,
    pub(super) gone: usize,
}

/// A holder's name: a build (`main`, `lite`), or a world part of one (`lite-geonames-cities5000`,
/// D-267): lowercase ASCII letters, digits and hyphens, starting with a letter.
fn is_build(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Registry {
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines().enumerate();
        if lines.next().map(|l| l.1) != Some(HEADER) {
            return Err(format!(
                "not an ID registry (first line must be `{HEADER}`)"
            ));
        }
        let mut entries = BTreeMap::new();
        let mut last = 0_u64;
        for (i, line) in lines {
            let at = |m: &str| format!("line {}: {m}", i + 1);
            let mut cols = line.split('\t');
            let (Some(serial), Some(state), Some(keys), None) =
                (cols.next(), cols.next(), cols.next(), cols.next())
            else {
                return Err(at("needs three tab-separated columns"));
            };
            let serial: u64 = serial.parse().map_err(|_| at("bad serial"))?;
            if serial <= last || serial > SERIAL_MAX {
                return Err(at("serials must rise, from 1 to SERIAL_MAX"));
            }
            last = serial;
            let state = match state {
                "x" => State::Gone,
                s if s.starts_with('>') => {
                    let t: u64 = s
                        .get(1..)
                        .unwrap_or("")
                        .parse()
                        .map_err(|_| at("bad target"))?;
                    if t == serial {
                        return Err(at("a serial cannot retire into itself"));
                    }
                    State::To(t)
                }
                s => {
                    let builds: BTreeSet<String> = s.split(',').map(str::to_string).collect();
                    if !builds.iter().all(|b| is_build(b)) {
                        return Err(at("bad build list"));
                    }
                    State::Live(builds)
                }
            };
            let keys: BTreeSet<String> = keys
                .split(' ')
                .filter(|k| !k.is_empty())
                .map(str::to_string)
                .collect();
            entries.insert(serial, Entry { state, keys });
        }
        let reg = Self { entries };
        for (s, e) in &reg.entries {
            if let State::To(t) = e.state
                && !reg.entries.contains_key(&t)
            {
                return Err(format!("serial {s} retires into unknown serial {t}"));
            }
        }
        // Every chain must end: at most one hop per entry.
        for &s in reg.entries.keys() {
            reg.fate(s)?;
        }
        Ok(reg)
    }

    pub(super) fn render(&self) -> String {
        let mut out = String::from(HEADER);
        out.push('\n');
        for (s, e) in &self.entries {
            let state = match &e.state {
                State::Live(b) => b.iter().map(String::as_str).collect::<Vec<_>>().join(","),
                State::To(t) => format!(">{t}"),
                State::Gone => "x".into(),
            };
            let keys: Vec<&str> = e.keys.iter().map(String::as_str).collect();
            let _ = writeln!(out, "{s}\t{state}\t{}", keys.join(" "));
        }
        out
    }

    /// Where a serial ends: `Some(live serial)`, or `None` for a tombstone.
    fn fate(&self, mut s: u64) -> Result<Option<u64>, String> {
        for _ in 0..=self.entries.len() {
            match self.entries.get(&s).map(|e| &e.state) {
                Some(State::Live(_)) => return Ok(Some(s)),
                Some(State::Gone) => return Ok(None),
                Some(State::To(t)) => s = *t,
                None => return Err(format!("unknown serial {s}")),
            }
        }
        Err(format!("serial {s} retires in a cycle"))
    }
}

/// The namespace part of a key (`osm` in `osm:node/1`).
fn space(key: &str) -> &str {
    key.split_once(':').map_or(key, |(n, _)| n)
}

/// Serials for `rows` (each its source keys) in a build named `build`, from the registry
/// `old` (empty for a first build). Keys must be unique across rows.
pub(super) fn assign(
    old: &Registry,
    rows: &[Vec<String>],
    build: &str,
) -> Result<Assignment, String> {
    if !is_build(build) {
        return Err(format!("{build}: not a build name"));
    }
    let (serials, kept, new) = match_rows(old, rows, build)?;
    let (registry, merged, gone) = update(old, rows, &serials, build)?;
    let current: BTreeSet<u64> = serials.iter().copied().collect();
    let redirects = registry
        .entries
        .iter()
        .filter_map(|(&s, e)| match e.state {
            State::To(t) if current.contains(&t) => Some((s, Some(t))),
            State::Gone => Some((s, None)),
            _ => None,
        })
        .collect();
    Ok(Assignment {
        serials,
        redirects,
        registry,
        kept,
        new,
        merged,
        gone,
    })
}

/// Each row's serial (kept, or the next free one in row order), and how many were kept and new.
fn match_rows(
    old: &Registry,
    rows: &[Vec<String>],
    build: &str,
) -> Result<(Vec<u64>, usize, usize), String> {
    let mut row_of: HashMap<&str, usize> = HashMap::new();
    for (r, keys) in rows.iter().enumerate() {
        for k in keys {
            if row_of.insert(k.as_str(), r).is_some() {
                return Err(format!("{k}: in two places"));
            }
        }
    }
    // (shared keys, held by this build before, serial, row): best first.
    let mut pairs: Vec<(usize, bool, u64, usize)> = Vec::new();
    for (&s, e) in &old.entries {
        let State::Live(builds) = &e.state else {
            continue;
        };
        let mut hits: BTreeMap<usize, usize> = BTreeMap::new();
        for k in &e.keys {
            if let Some(&r) = row_of.get(k.as_str()) {
                *hits.entry(r).or_default() += 1;
            }
        }
        for (r, n) in hits {
            pairs.push((n, !builds.contains(build), s, r));
        }
    }
    pairs.sort_unstable_by(|a, b| b.0.cmp(&a.0).then((a.1, a.2, a.3).cmp(&(b.1, b.2, b.3))));
    let mut serials: Vec<u64> = vec![0; rows.len()];
    let mut taken: BTreeSet<u64> = BTreeSet::new();
    for (_, _, s, r) in pairs {
        if let Some(slot) = serials.get_mut(r)
            && *slot == 0
            && taken.insert(s)
        {
            *slot = s;
        }
    }
    let kept = taken.len();
    let mut next = old.entries.keys().next_back().map_or(1, |s| s + 1);
    let mut new = 0;
    for slot in &mut serials {
        if *slot == 0 {
            if next > SERIAL_MAX {
                return Err("the namespace has no serials left".into());
            }
            *slot = next;
            next += 1;
            new += 1;
        }
    }
    Ok((serials, kept, new))
}

/// The registry after this build, and how many serials it retired into a successor and with
/// none.
fn update(
    old: &Registry,
    rows: &[Vec<String>],
    serials: &[u64],
    build: &str,
) -> Result<(Registry, usize, usize), String> {
    // The new registry. Keys of namespaces this build does not read are kept from before, so a
    // lite build does not strip a main place of its OSM keys.
    let read: BTreeSet<&str> = rows.iter().flatten().map(|k| space(k)).collect();
    let mut reg = old.clone();
    let mut holder: HashMap<String, u64> = HashMap::new();
    for (keys, &s) in rows.iter().zip(serials) {
        let e = reg.entries.entry(s).or_insert_with(|| Entry {
            state: State::Live(BTreeSet::new()),
            keys: BTreeSet::new(),
        });
        e.keys.retain(|k| !read.contains(space(k)));
        e.keys.extend(keys.iter().cloned());
        if let State::Live(b) = &mut e.state {
            b.insert(build.to_string());
        }
        for k in keys {
            holder.insert(k.clone(), s);
        }
    }
    // Live serials this build no longer holds leave its list; with no build left, they retire.
    let (mut merged, mut gone) = (0, 0);
    let current: BTreeSet<u64> = serials.iter().copied().collect();
    for (s, e) in &mut reg.entries {
        if current.contains(s) {
            continue;
        }
        let State::Live(b) = &mut e.state else {
            continue;
        };
        b.remove(build);
        if !b.is_empty() {
            continue;
        }
        let mut votes: BTreeMap<u64, usize> = BTreeMap::new();
        for k in &e.keys {
            if let Some(&t) = holder.get(k) {
                *votes.entry(t).or_default() += 1;
            }
        }
        // Most keys, then the smallest serial.
        let best = votes
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
        e.state = if let Some((t, _)) = best {
            merged += 1;
            State::To(t)
        } else {
            gone += 1;
            State::Gone
        };
    }
    // Flatten: every retired serial points at its final successor.
    let mut flat = Vec::new();
    for (&s, e) in &reg.entries {
        if matches!(e.state, State::To(_)) {
            flat.push((s, reg.fate(s)?));
        }
    }
    for (s, f) in flat {
        if let Some(e) = reg.entries.get_mut(&s) {
            e.state = f.map_or(State::Gone, State::To);
        }
    }
    Ok((reg, merged, gone))
}

#[cfg(test)]
mod tests {
    use super::{Registry, State, assign};
    use std::collections::BTreeSet;

    fn rows(r: &[&[&str]]) -> Vec<Vec<String>> {
        r.iter()
            .map(|k| k.iter().map(|s| (*s).to_string()).collect())
            .collect()
    }

    fn state(reg: &Registry, s: u64) -> State {
        reg.entries
            .get(&s)
            .map(|e| e.state.clone())
            .expect("serial")
    }

    fn live(b: &[&str]) -> State {
        State::Live(b.iter().map(|s| (*s).to_string()).collect::<BTreeSet<_>>())
    }

    #[test]
    fn first_build_allocates_in_row_order() {
        let a = assign(
            &Registry::default(),
            &rows(&[&["g:1"], &["g:2", "o:9"], &["g:3"]]),
            "main",
        )
        .expect("assign");
        assert_eq!(a.serials, [1, 2, 3]);
        assert_eq!((a.kept, a.new, a.merged, a.gone), (0, 3, 0, 0));
        assert_eq!(a.redirects, []);
        let text = a.registry.render();
        assert_eq!(
            text,
            "# teigeodata id registry 1\n1\tmain\tg:1\n2\tmain\tg:2 o:9\n3\tmain\tg:3\n"
        );
        assert_eq!(Registry::parse(&text).expect("parse"), a.registry);
    }

    #[test]
    fn parts_sharing_a_registry_keep_each_others_serials() {
        // D-267: a small part (big cities) and a large one (also towns) of one build.
        let small =
            assign(&Registry::default(), &rows(&[&["g:1"], &["g:2"]]), "lite").expect("small");
        let large = assign(
            &small.registry,
            &rows(&[&["g:1"], &["g:3"], &["g:2"]]),
            "lite-cities5000",
        )
        .expect("large");
        assert_eq!(large.serials, [1, 3, 2]);
        // Rebuilding the small part retires nothing the large one holds, both ways round.
        let again = assign(&large.registry, &rows(&[&["g:2"], &["g:1"]]), "lite").expect("again");
        assert_eq!(again.serials, [2, 1]);
        assert_eq!((again.merged, again.gone), (0, 0));
        assert_eq!(state(&again.registry, 3), live(&["lite-cities5000"]));
        let large2 = assign(
            &again.registry,
            &rows(&[&["g:3"], &["g:1"], &["g:2"]]),
            "lite-cities5000",
        )
        .expect("large again");
        assert_eq!(large2.serials, [3, 1, 2]);
        assert_eq!(large2.new, 0);
        assert_eq!(
            state(&large2.registry, 1),
            live(&["lite", "lite-cities5000"])
        );
        // A holder name is lowercase letters, digits and hyphens, starting with a letter.
        assert!(assign(&Registry::default(), &rows(&[&["g:1"]]), "5k").is_err());
        assert!(assign(&Registry::default(), &rows(&[&["g:1"]]), "Lite").is_err());
    }

    #[test]
    fn rebuilds_keep_serials_whatever_the_row_order() {
        let first = assign(
            &Registry::default(),
            &rows(&[&["g:1"], &["g:2"], &["g:3"]]),
            "main",
        )
        .expect("first");
        // Rows moved, one place gone, one new: survivors keep their serial.
        let second = assign(
            &first.registry,
            &rows(&[&["g:4"], &["g:3"], &["g:1"]]),
            "main",
        )
        .expect("second");
        assert_eq!(second.serials, [4, 3, 1]);
        assert_eq!(state(&second.registry, 2), State::Gone);
        assert_eq!(second.redirects, [(2, None)]);
        // Idempotent: the same inputs on the new registry change nothing.
        let third = assign(
            &second.registry,
            &rows(&[&["g:4"], &["g:3"], &["g:1"]]),
            "main",
        )
        .expect("third");
        assert_eq!(third.serials, second.serials);
        assert_eq!(third.registry, second.registry);
        assert_eq!((third.new, third.gone), (0, 0));
    }

    #[test]
    fn merges_redirect_and_splits_keep_the_larger_half() {
        let first = assign(
            &Registry::default(),
            &rows(&[&["g:1", "o:1"], &["g:2"], &["g:3", "o:3", "w:3"]]),
            "main",
        )
        .expect("first");
        // 1 and 2 merge (2 retires into 1); 3 splits, the half with two of its keys keeps it.
        let second = assign(
            &first.registry,
            &rows(&[&["w:3"], &["g:1", "o:1", "g:2"], &["g:3", "o:3"]]),
            "main",
        )
        .expect("second");
        assert_eq!(second.serials, [4, 1, 3]);
        assert_eq!(state(&second.registry, 2), State::To(1));
        assert_eq!(second.redirects, [(2, Some(1))]);
        assert_eq!((second.merged, second.gone), (1, 0));
        // A chain flattens: 1 (left with one key) merges into 3, so 2 now points at 3.
        let third = assign(
            &second.registry,
            &rows(&[&["g:3", "o:3", "g:1"], &["w:3"]]),
            "main",
        )
        .expect("third");
        assert_eq!(third.serials, [3, 4]);
        assert_eq!(state(&third.registry, 1), State::To(3));
        assert_eq!(state(&third.registry, 2), State::To(3));
        assert_eq!(third.redirects, [(1, Some(3)), (2, Some(3))]);
    }

    #[test]
    fn lite_shares_main_serials_and_retires_nothing_main_holds() {
        let main = assign(
            &Registry::default(),
            &rows(&[&["g:1", "o:1"], &["o:2"], &["g:3"]]),
            "main",
        )
        .expect("main");
        // Lite reads no OSM: place 2 is absent, place 1 matches on its GeoNames key.
        let lite = assign(&main.registry, &rows(&[&["g:3"], &["g:1"]]), "lite").expect("lite");
        assert_eq!(lite.serials, [3, 1]);
        assert_eq!(state(&lite.registry, 2), live(&["main"]));
        assert_eq!(state(&lite.registry, 1), live(&["lite", "main"]));
        assert!(
            lite.registry
                .entries
                .get(&1)
                .is_some_and(|e| e.keys.contains("o:1")),
            "OSM key kept"
        );
        assert_eq!(lite.redirects, []);
        // Main again on lite's registry: same serials; then main drops 3, which lite still holds.
        let main2 =
            assign(&lite.registry, &rows(&[&["g:1", "o:1"], &["o:2"]]), "main").expect("main2");
        assert_eq!(main2.serials, [1, 2]);
        assert_eq!(state(&main2.registry, 3), live(&["lite"]));
        assert_eq!(main2.gone, 0);
    }

    #[test]
    fn hostile_registries_are_refused() {
        for (text, why) in [
            ("", "no header"),
            ("# teigeodata id registry 1\n0\tmain\tg:1\n", "serial 0"),
            (
                "# teigeodata id registry 1\n2\tmain\tg:1\n1\tmain\tg:2\n",
                "falling",
            ),
            ("# teigeodata id registry 1\n1\t>9\tg:1\n", "unknown target"),
            ("# teigeodata id registry 1\n1\t>2\t\n2\t>1\t\n", "cycle"),
            ("# teigeodata id registry 1\n1\t>1\t\n", "self"),
            ("# teigeodata id registry 1\n1\tMain\tg:1\n", "bad build"),
            ("# teigeodata id registry 1\n1\tmain\n", "two columns"),
            (
                "# teigeodata id registry 1\n281474976710656\tmain\tg:1\n",
                "above SERIAL_MAX",
            ),
        ] {
            assert!(Registry::parse(text).is_err(), "{why}");
        }
        assert!(assign(&Registry::default(), &rows(&[&["g:1"], &["g:1"]]), "main").is_err());
    }
}
