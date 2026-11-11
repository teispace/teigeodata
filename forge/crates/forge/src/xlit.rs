//! Gate G-xlit: trains a transliteration profile's language model and measures it on
//! held-out name pairs ([note](../../../../docs/02-research/notes/nepali-names.md) §5.3, D-222).
//!
//! ```text
//! forge xlit-eval <lang>-Latn-<Script> [--lm-weight <w>] [--misses <file>]
//! ```
//!
//! Everything language-specific is data: the profile `data/xlit/<id>.yaml`, the gate
//! `data/xlit/<id>.gate.yaml` (training sources, test sets, floors) and the country lexicon it
//! names. Each pair's fold is fixed by a hash of its ID; the model trains on the training folds
//! and the gate's text sources, never on a test item or on a label holding a test reference as
//! a run of whole words. The report gives exact, folded exact and CER per set, the model's
//! size and the time per name; the run fails when a set is below its floor or the profile's
//! examples do not come out as written.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufRead, BufReader, Write as _};
use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use teigeodata_names::xlit::{
    Model, ModelBuilder, Profile, ProfileFile, Scratch, transliterate_with,
};
use teigeodata_names::{join_key, script_code};
use unicode_normalization::UnicodeNormalization;

use crate::lock::Lock;

/// A gate file, `data/xlit/<id>.gate.yaml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Gate {
    schema: String,
    profile: String,
    /// The country whose lexicon the profile uses.
    lexicon: String,
    order: usize,
    folds: u64,
    #[serde(default)]
    train: Vec<Train>,
    sets: Vec<Set>,
    floors: BTreeMap<String, Floor>,
}

const SCHEMA: &str = "teigeodata/xlit-gate/v1";

/// A source of target-script text for the model.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Train {
    source: String,
    /// `labels` (a `QLever` TSV `?item ?gn ?text`) or `wikidata` (a places snapshot).
    reader: String,
    /// `wikidata`: the language of the labels and aliases to read.
    #[serde(default)]
    native: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Set {
    name: String,
    parts: Vec<Part>,
    /// How many times each of the set's pairs counts in the word memory (default 1).
    #[serde(default = "one")]
    memory_weight: u32,
    #[serde(default)]
    strip: Strip,
}

fn one() -> u32 {
    1
}

/// One source of pairs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Part {
    source: String,
    /// `csv` (columns named by header) or `wikidata` (a places snapshot, languages).
    reader: String,
    /// `csv`: the ID column.
    #[serde(default)]
    id: Option<String>,
    /// The Latin column (`csv`) or language (`wikidata`).
    latin: String,
    /// The target column (`csv`) or language (`wikidata`).
    native: String,
    /// `wikidata`: the kinds of item kept (empty: all).
    #[serde(default)]
    kinds: Vec<String>,
}

/// Trailing words stripped from each side (folded Latin; target text).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Strip {
    /// Text from the first of these on is cut from both sides first (a qualifier after a
    /// comma: "Khalanga, Baitadi").
    #[serde(default)]
    cut: Vec<String>,
    #[serde(default)]
    latin: Vec<String>,
    #[serde(default)]
    native: Vec<String>,
}

/// A set's floors: exact and folded exact at least, CER at most.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Floor {
    pub(crate) exact: f64,
    pub(crate) folded: f64,
    pub(crate) cer: f64,
}

/// A name pair with its ID.
#[derive(Clone, Debug)]
pub(crate) struct Pair {
    pub(crate) id: String,
    pub(crate) latin: String,
    pub(crate) native: String,
    /// How many times the pair counts in the word memory.
    pub(crate) weight: u32,
}

/// A set's scores.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Score {
    pub(crate) n: usize,
    pub(crate) exact: f64,
    pub(crate) folded: f64,
    pub(crate) cer: f64,
}

/// The command line of `forge xlit-eval`.
#[derive(Default)]
struct Args {
    id: String,
    lm_weight: Option<f32>,
    dev: bool,
    order: Option<usize>,
    misses: Option<String>,
    input: Option<String>,
}

const USAGE: &str = "usage: forge xlit-eval <lang>-Latn-<Script> [--lm-weight <w>] [--misses <file>] [--input <file>] [--dev] [--order <n>]";

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut id = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dev" => a.dev = true,
            "--order" => {
                let o = it.next().ok_or(USAGE)?;
                a.order = Some(o.parse::<usize>().map_err(|e| format!("--order: {e}"))?);
            }
            "--lm-weight" => {
                let w = it.next().ok_or(USAGE)?;
                a.lm_weight = Some(w.parse::<f32>().map_err(|e| format!("--lm-weight: {e}"))?);
            }
            "--misses" => a.misses = Some(it.next().ok_or(USAGE)?.clone()),
            "--input" => a.input = Some(it.next().ok_or(USAGE)?.clone()),
            s if !s.starts_with("--") && id.is_none() => id = Some(s.to_string()),
            _ => return Err(USAGE.into()),
        }
    }
    a.id = id.ok_or(USAGE)?;
    Ok(a)
}

/// A set's name, training pairs and measured pairs.
type Split = (String, Vec<Pair>, Vec<Pair>);

/// `forge xlit-eval <id> [--lm-weight <w>] [--misses <file>] [--input <file>] [--dev]
/// [--order <n>]`.
///
/// # Errors
/// A bad gate, profile or source, a source not fetched, a floor not met or an example not
/// reproduced.
/// Profile `id`'s gate file, checked.
fn gate(root: &Path, id: &str) -> Result<Gate, String> {
    let gate_path = root.join("data/xlit").join(format!("{id}.gate.yaml"));
    let gate_yaml =
        std::fs::read_to_string(&gate_path).map_err(|e| format!("{}: {e}", gate_path.display()))?;
    let gate: Gate =
        serde_saphyr::from_str(&gate_yaml).map_err(|e| format!("{}: {e}", gate_path.display()))?;
    if gate.schema != SCHEMA || gate.profile != id {
        return Err(format!(
            "{}: schema must be {SCHEMA} and profile {id}",
            gate_path.display()
        ));
    }
    if gate.folds < 2 {
        return Err("folds must be at least 2".into());
    }
    Ok(gate)
}

/// The fetched file of `source`, or an error naming the fetch.
fn fetched_in<'l>(
    root: &'l Path,
    lock: &'l Lock,
) -> impl Fn(&str) -> Result<std::path::PathBuf, String> + 'l {
    move |source: &str| {
        let p = lock.path(root, source)?;
        if p.is_file() {
            Ok(p)
        } else {
            Err(format!("{source} is not fetched yet: forge fetch {source}"))
        }
    }
}

/// Every source profile `id`'s shipping model reads: its gate's training texts and pairs, in
/// file order, each once (`forge make --fetch`, D-228).
pub(crate) fn sources(root: &Path, id: &str) -> Result<Vec<String>, String> {
    let gate = gate(root, id)?;
    let mut out: Vec<String> = Vec::new();
    let all = gate.train.iter().map(|t| &t.source).chain(
        gate.sets
            .iter()
            .flat_map(|s| s.parts.iter().map(|p| &p.source)),
    );
    for s in all {
        if !out.contains(s) {
            out.push(s.clone());
        }
    }
    Ok(out)
}

/// The model a build ships for profile `id` (D-222): trained on every fold of its gate's
/// sources, as `xlit-eval` checks its examples; with the profile and its target script.
pub(crate) fn shipping(root: &Path, id: &str) -> Result<(Profile, Model, String), String> {
    let gate = gate(root, id)?;
    let (profile, script) = load_profile(root, id, &gate.lexicon)?;
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let fetched = fetched_in(root, &lock);
    let (sets, hidden) = load_sets(&gate, &fetched, 0)?;
    let mut all: Vec<&Pair> = sets.iter().flat_map(|(_, train, _)| train).collect();
    all.extend(&hidden);
    let texts = train_texts(&gate, &fetched, &all, &script)?;
    let train = Training {
        profile: &profile,
        order: gate.order,
        texts: &texts,
        pairs: &all,
    };
    let (model, _, _) = train.model(&BTreeSet::new(), &BTreeSet::new())?;
    Ok((profile, model, script))
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let args = parse_args(args)?;
    let id = args.id.as_str();
    let gate = gate(root, id)?;
    let (profile, script) = load_profile(root, id, &gate.lexicon)?;
    let profile = match args.lm_weight {
        Some(w) => {
            let mut c = profile.costs().clone();
            c.lm_weight = w;
            profile.with_costs(c)
        }
        None => profile,
    };
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let fetched = fetched_in(root, &lock);
    // The fold measured: 0 (test), or 1 (dev) for tuning, so the test fold stays unseen.
    let (sets, hidden) = load_sets(&gate, &fetched, u64::from(args.dev))?;
    // Held-out references as runs of words, so no training text holds one.
    let held: BTreeSet<String> = hidden
        .iter()
        .map(|p| join_key(&p.native))
        .filter(|k| !k.is_empty())
        .collect();
    let hidden_ids: BTreeSet<&str> = hidden.iter().map(|p| p.id.as_str()).collect();

    // Training.
    let t0 = Instant::now();
    let mut all: Vec<&Pair> = Vec::new();
    for (_, train, _) in &sets {
        all.extend(train);
    }
    all.extend(&hidden);
    let texts = train_texts(&gate, &fetched, &all, &script)?;
    let train = Training {
        profile: &profile,
        order: args.order.unwrap_or(gate.order),
        texts: &texts,
        pairs: &all,
    };
    let (model, used, dropped) = train.model(&hidden_ids, &held)?;
    let trained_s = t0.elapsed().as_secs_f64();
    // The model a build ships: trained on everything, held-out folds included.
    let (full, _, _) = train.model(&BTreeSet::new(), &BTreeSet::new())?;
    if let Some(file) = &args.input {
        return write_input(&profile, &full, file);
    }

    let mut report = format!(
        "xlit-eval {id}: order {}, {used} training labels ({dropped} held out), {} n-gram entries, {} memory words, {:.2} MB, trained in {trained_s:.1} s\n",
        model.order(),
        model.entries(),
        model.memory_words(),
        mb(model.size_bytes()),
    );
    let mut failures = measure(
        &profile,
        &model,
        &sets,
        &gate.floors,
        args.misses.as_deref(),
        &mut report,
    )?;
    let failed = profile.failed_examples(&full);
    let _ = write!(
        report,
        "  examples (model trained on every fold): {} of {} as written",
        profile.examples().len() - failed.len(),
        profile.examples().len()
    );
    failures.extend(failed.into_iter().map(|f| format!("example {f}")));
    if failures.is_empty() {
        Ok(report)
    } else {
        Err(format!(
            "{report}\nG-xlit failed:\n  {}",
            failures.join("\n  ")
        ))
    }
}

/// Every set's pairs, stripped, split into training and the measured fold `target`; and every
/// pair training must not see (the measured fold and the test fold 0).
fn load_sets(
    gate: &Gate,
    fetched: &dyn Fn(&str) -> Result<std::path::PathBuf, String>,
    target: u64,
) -> Result<(Vec<Split>, Vec<Pair>), String> {
    let mut sets = Vec::new();
    let mut hidden = Vec::new();
    for set in &gate.sets {
        let mut pairs = Vec::new();
        for part in &set.parts {
            let path = fetched(&part.source)?;
            match part.reader.as_str() {
                "csv" => csv_pairs(&path, &part.source, part, &mut pairs)?,
                "wikidata" => wikidata_pairs(&path, part, &mut pairs)?,
                r => return Err(format!("set {}: unknown reader {r}", set.name)),
            }
        }
        let mut pairs: Vec<Pair> = pairs
            .into_iter()
            .filter_map(|p| strip_pair(p, &set.strip))
            .map(|p| Pair {
                weight: set.memory_weight,
                ..p
            })
            .collect();
        pairs.sort_by(|a, b| a.id.cmp(&b.id));
        pairs.dedup_by(|a, b| a.id == b.id);
        let (out, train): (Vec<Pair>, Vec<Pair>) = pairs.into_iter().partition(|p| {
            let f = fold(&p.id, gate.folds);
            f == 0 || f == target
        });
        // In dev mode the test fold is held out of training too, and never measured.
        let measured = out
            .iter()
            .filter(|p| fold(&p.id, gate.folds) == target)
            .cloned()
            .collect();
        hidden.extend(out);
        sets.push((set.name.clone(), train, measured));
    }
    Ok((sets, hidden))
}

/// The model's texts in the target script: the gate's sources and every pair's target side,
/// in NFC, by item.
fn train_texts(
    gate: &Gate,
    fetched: &dyn Fn(&str) -> Result<std::path::PathBuf, String>,
    pairs: &[&Pair],
    script: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut texts: Vec<(String, String)> = Vec::new();
    for t in &gate.train {
        let path = fetched(&t.source)?;
        match t.reader.as_str() {
            "labels" => label_texts(&path, &mut texts)?,
            "wikidata" => {
                let lang = t
                    .native
                    .as_deref()
                    .ok_or("a wikidata source needs native")?;
                wikidata_texts(&path, lang, &mut texts)?;
            }
            r => return Err(format!("train {}: unknown reader {r}", t.source)),
        }
    }
    Ok(texts
        .into_iter()
        .chain(pairs.iter().map(|p| (p.id.clone(), p.native.clone())))
        .map(|(i, t)| (i, t.nfc().collect::<String>()))
        .filter(|(_, t)| script_code(t) == Some(script))
        .collect())
}

/// Scores every set into `report`, writes the misses when asked, and returns the floors
/// missed.
fn measure(
    profile: &Profile,
    model: &Model,
    sets: &[Split],
    floors: &BTreeMap<String, Floor>,
    misses: Option<&str>,
    report: &mut String,
) -> Result<Vec<String>, String> {
    let mut failures = Vec::new();
    let mut miss_out = match misses {
        Some(m) => Some(std::io::BufWriter::new(
            File::create(m).map_err(|e| format!("{m}: {e}"))?,
        )),
        None => None,
    };
    let (mut count, mut seconds) = (0_usize, 0.0_f64);
    for (name, train, test) in sets {
        let t = Instant::now();
        let (score, outputs) = evaluate(profile, model, test);
        seconds += t.elapsed().as_secs_f64();
        count += test.len();
        let _ = writeln!(
            report,
            "  {name}: {} pairs, {} test; exact {:.1}%, folded {:.1}%, CER {:.3}",
            train.len() + test.len(),
            score.n,
            score.exact * 100.0,
            score.folded * 100.0,
            score.cer,
        );
        if let Some(w) = miss_out.as_mut() {
            for (p, got) in test.iter().zip(&outputs) {
                if join_key(got) != join_key(&p.native) {
                    writeln!(w, "{name}\t{}\t{}\t{}\t{got}", p.id, p.latin, p.native)
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        match floors.get(name) {
            Some(f) => failures.extend(below(name, &score, f)),
            None => failures.push(format!("{name}: no floor in the gate file")),
        }
    }
    if let Some(mut w) = miss_out {
        w.flush().map_err(|e| e.to_string())?;
    }
    #[allow(clippy::cast_precision_loss, reason = "a name count")]
    let per = seconds * 1e6 / count.max(1) as f64;
    let _ = writeln!(report, "  {per:.1} µs per name over {count} test names");
    Ok(failures)
}

/// Transliterates each line of `file` to standard output (`latin<TAB>target`), for review.
fn write_input(profile: &Profile, model: &Model, file: &str) -> Result<String, String> {
    let f = File::open(file).map_err(|e| format!("{file}: {e}"))?;
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let mut scratch = Scratch::default();
    let mut text = String::new();
    let mut n = 0_usize;
    for line in BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        transliterate_with(profile, model, &mut scratch, &line, &mut text);
        writeln!(out, "{line}\t{text}").map_err(|e| e.to_string())?;
        n += 1;
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(format!("transliterated {n} names"))
}

/// What a model trains on: target-script texts by item and name pairs.
struct Training<'a> {
    profile: &'a Profile,
    order: usize,
    texts: &'a [(String, String)],
    pairs: &'a [&'a Pair],
}

impl Training<'_> {
    /// A model that never sees an item of `hidden_ids` nor a text holding a reference of
    /// `held` as a run of whole words; with the counts of texts used and dropped.
    fn model(
        &self,
        hidden_ids: &BTreeSet<&str>,
        held: &BTreeSet<String>,
    ) -> Result<(Model, usize, usize), String> {
        let mut builder = ModelBuilder::new(self.profile, self.order)?;
        let mut seen = BTreeSet::new();
        let (mut used, mut dropped) = (0_usize, 0_usize);
        for (item, text) in self.texts {
            if hidden_ids.contains(item.as_str()) || holds_reference(text, held) {
                dropped += 1;
                continue;
            }
            // One count per distinct (item, text).
            if seen.insert((item, text)) {
                builder.add_text(text);
                used += 1;
            }
        }
        for p in self.pairs {
            if !hidden_ids.contains(p.id.as_str()) && !holds_reference(&p.native, held) {
                builder.add_pair(self.profile, &p.latin, &p.native, p.weight);
            }
        }
        Ok((builder.build(self.profile), used, dropped))
    }
}

fn mb(bytes: usize) -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "a byte count")]
    let b = bytes as f64;
    b / 1e6
}

/// The profile `id` with country `cc`'s lexicon, and its target script.
pub(crate) fn load_profile(root: &Path, id: &str, cc: &str) -> Result<(Profile, String), String> {
    let path = root.join("data/xlit").join(format!("{id}.yaml"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: ProfileFile =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let terms = crate::names::xlit_terms(root, cc, &file.script)?;
    let p = file
        .compile(id, &terms)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((p, file.script))
}

/// The fold of `id`: BLAKE3 of the ID, first 8 bytes little-endian, modulo `folds`. Fold 0
/// is the test fold, fold 1 the dev fold for tuning.
pub(crate) fn fold(id: &str, folds: u64) -> u64 {
    let h = blake3::hash(id.as_bytes());
    let mut b = [0_u8; 8];
    b.copy_from_slice(h.as_bytes().get(..8).unwrap_or(&[0; 8]));
    u64::from_le_bytes(b) % folds
}

/// Whether `text` holds a held-out reference as a run of whole words.
fn holds_reference(text: &str, held: &BTreeSet<String>) -> bool {
    let words: Vec<String> = text.split_whitespace().map(join_key).collect();
    for i in 0..words.len() {
        let mut run = String::new();
        for w in words.iter().skip(i).take(6) {
            run.push_str(w);
            if held.contains(&run) {
                return true;
            }
        }
    }
    false
}

/// Strips trailing designator words; a pair left empty on either side is dropped.
fn strip_pair(mut p: Pair, s: &Strip) -> Option<Pair> {
    for c in &s.cut {
        for side in [&mut p.latin, &mut p.native] {
            if let Some(at) = side.find(c.as_str()) {
                side.truncate(at);
            }
        }
    }
    let mut latin = teigeodata_names::fold(&p.latin);
    for w in &s.latin {
        if let Some(rest) = latin.strip_suffix(w.as_str())
            && (rest.is_empty() || rest.ends_with(' '))
        {
            latin = rest.trim_end().to_string();
            break;
        }
    }
    let mut native: String = p.native.nfc().collect();
    for w in &s.native {
        let w: String = w.nfc().collect();
        if let Some(rest) = native.trim_end().strip_suffix(w.as_str())
            && (rest.is_empty() || rest.ends_with(' '))
        {
            native = rest.trim_end().to_string();
            break;
        }
    }
    if latin.is_empty() || native.trim().is_empty() {
        return None;
    }
    p.latin = latin;
    p.native = native.trim().to_string();
    Some(p)
}

/// Transliterates every pair's Latin side and scores it against the target side: exact (NFC,
/// spaces and hyphens ignored), folded exact ([`join_key`]) and CER (edits over the folded
/// reference's length, summed over the set).
pub(crate) fn evaluate(profile: &Profile, model: &Model, pairs: &[Pair]) -> (Score, Vec<String>) {
    let mut scratch = Scratch::default();
    let mut out = String::new();
    let (mut exact, mut folded, mut edits, mut chars) = (0_usize, 0_usize, 0_usize, 0_usize);
    let mut outputs = Vec::with_capacity(pairs.len());
    for p in pairs {
        transliterate_with(profile, model, &mut scratch, &p.latin, &mut out);
        let plain = |s: &str| -> String {
            s.nfc()
                .filter(|c| !c.is_whitespace() && *c != '-' && *c != '\u{200C}' && *c != '\u{200D}')
                .collect()
        };
        if plain(&out) == plain(&p.native) {
            exact += 1;
        }
        let (a, b) = (join_key(&out), join_key(&p.native));
        if a == b {
            folded += 1;
        }
        let b: Vec<char> = b.chars().collect();
        edits += levenshtein(&a.chars().collect::<Vec<char>>(), &b);
        chars += b.len();
        outputs.push(out.clone());
    }
    #[allow(clippy::cast_precision_loss, reason = "counts")]
    let ratio = |a: usize, b: usize| if b == 0 { 0.0 } else { a as f64 / b as f64 };
    (
        Score {
            n: pairs.len(),
            exact: ratio(exact, pairs.len()),
            folded: ratio(folded, pairs.len()),
            cer: ratio(edits, chars),
        },
        outputs,
    )
}

/// The floors `s` misses, as messages.
pub(crate) fn below(name: &str, s: &Score, f: &Floor) -> Vec<String> {
    let mut out = Vec::new();
    if s.exact < f.exact {
        out.push(format!(
            "{name}: exact {:.4} below the floor {}",
            s.exact, f.exact
        ));
    }
    if s.folded < f.folded {
        out.push(format!(
            "{name}: folded exact {:.4} below the floor {}",
            s.folded, f.folded
        ));
    }
    if s.cer > f.cer {
        out.push(format!(
            "{name}: CER {:.4} above the ceiling {}",
            s.cer, f.cer
        ));
    }
    out
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        if let Some(c) = cur.first_mut() {
            *c = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let sub = prev.get(j).copied().unwrap_or(0) + usize::from(ca != cb);
            let del = prev.get(j + 1).copied().unwrap_or(0) + 1;
            let ins = cur.get(j).copied().unwrap_or(0) + 1;
            if let Some(c) = cur.get_mut(j + 1) {
                *c = sub.min(del).min(ins);
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev.get(b.len()).copied().unwrap_or(0)
}

/// Pairs from a CSV with a header row and no quoted fields.
fn csv_pairs(path: &Path, source: &str, part: &Part, out: &mut Vec<Pair>) -> Result<(), String> {
    let id_col = part.id.as_deref().ok_or("a csv part needs an id column")?;
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .ok_or("empty csv")?
        .map_err(|e| e.to_string())?;
    let header = header.trim_start_matches('\u{FEFF}');
    let cols: Vec<String> = header
        .split(',')
        .map(|h| h.trim().nfc().collect())
        .collect();
    let col = |name: &str| -> Result<usize, String> {
        let name: String = name.nfc().collect();
        cols.iter()
            .position(|c| *c == name)
            .ok_or_else(|| format!("{source}: no column {name:?}"))
    };
    let (ci, cl, cn) = (col(id_col)?, col(&part.latin)?, col(&part.native)?);
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        if line.contains('"') {
            return Err(format!("{source}: quoted CSV fields are not read"));
        }
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        let get = |i: usize| f.get(i).copied().unwrap_or("");
        if get(ci).is_empty() || get(cl).is_empty() || get(cn).is_empty() {
            continue;
        }
        out.push(Pair {
            id: format!("{source}:{}", get(ci)),
            latin: get(cl).to_string(),
            native: get(cn).to_string(),
            weight: 1,
        });
    }
    Ok(())
}

/// The Wikidata item ID (`Q123`) of a reader record ID (`wikidata:Q123`).
fn item(id: &str) -> &str {
    id.strip_prefix("wikidata:").unwrap_or(id)
}

/// Pairs of an item's labels in two languages from a places snapshot.
fn wikidata_pairs(path: &Path, part: &Part, out: &mut Vec<Pair>) -> Result<(), String> {
    let o = crate::read::wikidata::read(path)?;
    let keep: BTreeSet<&str> = o
        .records
        .iter()
        .filter(|r| part.kinds.is_empty() || part.kinds.contains(&r.kind))
        .map(|r| r.id.as_str())
        .collect();
    let mut by: BTreeMap<&str, (Option<&str>, Option<&str>)> = BTreeMap::new();
    for n in &o.names {
        if n.kind != "label" || !keep.contains(n.id.as_str()) {
            continue;
        }
        let e = by.entry(n.id.as_str()).or_default();
        if n.lang == part.latin {
            e.0 = Some(&n.text);
        } else if n.lang == part.native {
            e.1 = Some(&n.text);
        }
    }
    for (id, (l, n)) in by {
        if let (Some(l), Some(n)) = (l, n) {
            out.push(Pair {
                id: item(id).to_string(),
                latin: l.to_string(),
                native: n.to_string(),
                weight: 1,
            });
        }
    }
    Ok(())
}

/// Labels and aliases in `lang` from a places snapshot, by item.
fn wikidata_texts(path: &Path, lang: &str, out: &mut Vec<(String, String)>) -> Result<(), String> {
    let o = crate::read::wikidata::read(path)?;
    for n in o.names {
        if n.lang == lang {
            out.push((item(&n.id).to_string(), n.text));
        }
    }
    Ok(())
}

/// Labels from a `QLever` TSV `?item ?gn ?text`, by item.
fn label_texts(path: &Path, out: &mut Vec<(String, String)>) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = BufReader::with_capacity(1 << 20, file).lines();
    let header = lines
        .next()
        .ok_or("empty labels file")?
        .map_err(|e| e.to_string())?;
    let cols: Vec<&str> = header.split('\t').collect();
    let at = |n: &str| {
        cols.iter()
            .position(|c| *c == n)
            .ok_or_else(|| format!("{}: no column {n}", path.display()))
    };
    let (ci, ct) = (at("?item")?, at("?text")?);
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let f: Vec<&str> = line.split('\t').collect();
        let id = crate::read::wikidata::unquote(f.get(ci).copied().unwrap_or(""));
        let q = id.rsplit('/').next().unwrap_or("").to_string();
        let text = crate::read::wikidata::unquote(f.get(ct).copied().unwrap_or(""));
        if !q.is_empty() && !text.is_empty() {
            out.push((q, text));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use teigeodata_names::xlit::{Model, ModelBuilder, Profile, ProfileFile};

    use super::{Floor, Pair, below, evaluate, fold, levenshtein};

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    fn profile_text() -> String {
        std::fs::read_to_string(root().join("data/xlit/ne-Latn-Deva.yaml")).expect("profile")
    }

    fn compile(text: &str) -> Profile {
        let file: ProfileFile = serde_saphyr::from_str(text).expect("parses");
        let terms = crate::names::xlit_terms(&root(), "np", &file.script).expect("lexicon");
        file.compile("ne-Latn-Deva", &terms).expect("compiles")
    }

    /// A small model: names that share letters and words with the pairs, not the pairs.
    fn model(p: &Profile) -> Model {
        let mut b = ModelBuilder::new(p, 5).expect("order");
        b.add_text(
            "ललित ललिता पुर पुरा नगर नगरी विराट पोखरी खरा कपुर रामपुर सुन्दरपुर लहान \
             भक्त भक्तपुर जनक जनकपुर धन धनगढी गोरखा बनेपा धुलिखेल हेटौंडा कालिका \
             तानसेन इटहरी दमक बिरगंज भरत भरतपुर तुलसी गाउँ टोल",
        );
        b.build(p)
    }

    fn pairs() -> Vec<Pair> {
        [
            ("1", "Lalitpur", "ललितपुर"),
            ("2", "Pokhara", "पोखरा"),
            ("3", "Bhaktapur", "भक्तपुर"),
            ("4", "Janakpur", "जनकपुर"),
            ("5", "Bharatpur", "भरतपुर"),
            ("6", "Ramgaun", "रामगाउँ"),
            ("7", "Dhangadhi", "धनगढी"),
            ("8", "Banepa", "बनेपा"),
        ]
        .into_iter()
        .map(|(i, l, n)| Pair {
            id: i.into(),
            latin: l.into(),
            native: n.into(),
            weight: 1,
        })
        .collect()
    }

    #[test]
    fn the_checked_in_profile_loads_with_the_lexicon() {
        let p = compile(&profile_text());
        assert!(p.examples().len() >= 6);
        let (q, script) = super::load_profile(&root(), "ne-Latn-Deva", "np").expect("loads");
        assert_eq!(script, "Deva");
        assert_eq!(q.examples().len(), p.examples().len());
    }

    #[test]
    #[allow(clippy::many_single_char_names, reason = "scores side by side")]
    fn the_gate_fails_on_a_corrupted_profile() {
        let good = compile(&profile_text());
        let m = model(&good);
        let (s, _) = evaluate(&good, &m, &pairs());
        // The floor at the good profile's own score passes.
        let floor = Floor {
            exact: s.exact,
            folded: s.folded,
            cer: s.cer,
        };
        assert!(s.exact >= 0.5, "{s:?}");
        assert_eq!(below("t", &s, &floor), Vec::<String>::new());
        // Planted fault: the long vowel sign ा written as ि.
        let bad = compile(&profile_text().replace("sign: \"ा\"", "sign: \"ि\""));
        let (b, _) = evaluate(&bad, &model(&bad), &pairs());
        let msgs = below("t", &b, &floor);
        assert!(msgs.iter().any(|m| m.contains("exact")), "{b:?} {msgs:?}");
        // The model switched off falls below too.
        let mut c = good.costs().clone();
        c.lm_weight = 0.0;
        let off = good.with_costs(c);
        let (o, _) = evaluate(&off, &m, &pairs());
        assert!(!below("t", &o, &floor).is_empty(), "{o:?}");
    }

    #[test]
    fn folds_are_stable_and_split() {
        let ids: Vec<String> = (0..1000).map(|i| format!("Q{i}")).collect();
        let n = ids.iter().filter(|i| fold(i, 5) == 0).count();
        assert!((150..250).contains(&n), "{n}");
        assert_eq!(fold("Q1", 5), fold("Q1", 5));
    }

    #[test]
    fn edit_distance() {
        let c = |s: &str| s.chars().collect::<Vec<char>>();
        assert_eq!(levenshtein(&c("kitten"), &c("sitting")), 3);
        assert_eq!(levenshtein(&c(""), &c("abc")), 3);
        assert_eq!(levenshtein(&c("ललितपुर"), &c("ललितपुर")), 0);
    }
}
