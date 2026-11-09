//! Phase 0 spike S3: shared loading and text folding for the golden-set generator and the
//! search evaluation. World-general (principle P16): nothing here names a country. Country
//! rules come from `data/countries/<cc>/profile.yaml`; script rules are per Unicode script.
//!
//! Sources: a `GeoNames` country file `<CC>.txt` (CC-BY 4.0) and, where the country has one, an
//! OCHA COD-AB attribute workbook (licence per dataset; Nepal v02 is CC BY-IGO). Records keep
//! their source key (`geonames:<id>`, `codab:<pcode>`); records of the same entity across
//! sources share a group.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    clippy::needless_range_loop,
    clippy::many_single_char_names,
    clippy::similar_names,
    missing_docs,
    missing_debug_implementations,
    reason = "a measurement spike over pinned local inputs"
)]

pub mod engine;
pub mod golden;

use std::collections::BTreeMap;

use calamine::{Data, Reader, Xlsx, open_workbook};
use serde::Deserialize;

// ---------- country profile (the subset this spike reads; full schema: xtask check-data) ----

#[derive(Deserialize)]
pub struct Profile {
    pub country: String,
    pub iso3: String,
    #[serde(default)]
    pub scripts: Vec<String>,
    /// The phonetic key the forge writes the SEARCH section with (`indic-v0`).
    #[serde(default)]
    pub phonetic_key: Option<String>,
    #[serde(default)]
    pub unit_types: Vec<UnitType>,
    #[serde(default)]
    pub droppable_suffixes: Vec<ProfileTerm>,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceRules>,
}

#[derive(Deserialize)]
pub struct UnitType {
    pub id: String,
    pub level: u8,
    #[serde(default)]
    pub abbreviations: Vec<ProfileTerm>,
    #[serde(default)]
    pub source_codes: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct ProfileTerm {
    pub text: String,
}

#[derive(Deserialize)]
pub struct SourceRules {
    pub unit_type_code: Option<CodeSlice>,
}

#[derive(Deserialize)]
pub struct CodeSlice {
    pub field: String,
    pub offset: usize,
    pub length: usize,
}

#[must_use]
pub fn profile(path: &str) -> Profile {
    serde_saphyr::from_str(&std::fs::read_to_string(path).expect("read profile"))
        .expect("parse profile")
}

// ---------- records ----------

/// One searchable record.
#[derive(Clone, Debug)]
pub struct Rec {
    pub key: String,
    pub name: String,
    pub alts: Vec<String>,
    pub fcode: String,
    pub district: String,
    pub importance: u32,
    pub current: bool,
    /// The entity group: records of the same entity across sources share it.
    pub group: usize,
    /// The profile's unit type, for admin units from a source that encodes it.
    pub unit_type: Option<String>,
}

// ---------- folding and phonetic keys: the shared crate (one code path) ----------

pub use teigeodata_names::{fold, is_native_script, join_key, script_of};

#[must_use]
pub fn pkey_indic_v0(s: &str) -> String {
    teigeodata_names::PhoneticKey::IndicV0.key(s)
}

#[must_use]
pub fn pkey_indic_v1(s: &str) -> String {
    teigeodata_names::PhoneticKey::IndicV1.key(s)
}

fn fcode_importance(fcode: &str, population: u64) -> u32 {
    let base = match fcode {
        "PPLC" => 100,
        "ADM1" => 90,
        "PPLA" => 80,
        "ADM2" => 70,
        "PPLA2" => 60,
        "PPLA3" => 50,
        "ADM3" => 45,
        "PPLA4" | "ADM4" => 40,
        "PPL" | "PPLX" => 30,
        "PPLL" | "PPLF" => 10,
        f if f.ends_with('H') => 5,
        _ => 1,
    };
    let pop = if population > 0 {
        ((population as f64).log10() * 5.0) as u32
    } else {
        0
    };
    base + pop
}

/// Loads a `GeoNames` country file and, optionally, a COD-AB workbook. COD-AB units at the
/// profile's unit-type level join a `GeoNames` record of the same admin level, folded name and
/// parent unit.
pub fn load(profile: &Profile, geonames: &str, codab_xlsx: Option<&str>) -> Vec<Rec> {
    let text = std::fs::read_to_string(geonames).expect("read GeoNames file");
    let rows: Vec<Vec<&str>> = text
        .lines()
        .map(|l| l.split('\t').collect())
        .filter(|f: &Vec<&str>| f.len() >= 19)
        .collect();
    // The parent shown as context: the ADM2 unit. GeoNames admin2 codes are country-specific
    // strings; map both the ADM2 row's own code and its ID.
    let mut adm2_name: BTreeMap<String, &str> = BTreeMap::new();
    for f in &rows {
        if f[7] == "ADM2" {
            adm2_name.insert(format!("{}.{}", f[10], f[11]), f[1]);
        }
    }
    let mut recs = Vec::new();
    for f in &rows {
        if !matches!(f[6], "P" | "A") {
            continue;
        }
        let fcode = f[7];
        let population: u64 = f[14].parse().unwrap_or(0);
        let district = if fcode == "ADM2" {
            f[1]
        } else {
            adm2_name
                .get(&format!("{}.{}", f[10], f[11]))
                .copied()
                .unwrap_or("")
        };
        let mut alts: Vec<String> = f[3]
            .split(',')
            .filter(|a| !a.is_empty() && *a != f[1])
            .map(str::to_string)
            .collect();
        alts.sort();
        alts.dedup();
        recs.push(Rec {
            key: format!("geonames:{}", f[0]),
            name: f[1].to_string(),
            alts,
            fcode: fcode.to_string(),
            district: district.to_string(),
            importance: fcode_importance(fcode, population),
            current: !fcode.ends_with('H'),
            group: 0,
            unit_type: None,
        });
    }
    for (i, r) in recs.iter_mut().enumerate() {
        r.group = i;
    }
    let (Some(path), Some(level)) = (codab_xlsx, profile.unit_types.first().map(|u| u.level))
    else {
        return recs;
    };
    let rules = profile
        .sources
        .get("codab")
        .and_then(|s| s.unit_type_code.as_ref());

    let mut wb: Xlsx<_> = open_workbook(path).expect("open COD-AB workbook");
    let sheet = format!("{}_admin{level}", profile.iso3.to_lowercase());
    let range = wb.worksheet_range(&sheet).expect("COD-AB sheet");
    let mut it = range.rows();
    let header: Vec<String> = it.next().expect("header").iter().map(cell).collect();
    let col = |n: &str| {
        header
            .iter()
            .position(|h| h == n)
            .unwrap_or_else(|| panic!("column {n}"))
    };
    let (c_name, c_pcode, c_parent) = (
        col(&format!("adm{level}_name")),
        col(&format!("adm{level}_pcode")),
        col("adm2_name"),
    );
    let code_col = rules.map(|r| col(&r.field));
    let base = recs.len();
    // Parents: COD-AB may spell them differently from GeoNames (Chitawan/Chitwan); match by
    // fold, else by the smallest edit distance ≤ 2.
    let gn_parents: Vec<String> = adm2_name.values().map(|s| fold(s)).collect();
    let map_parent = |d: &str| -> String {
        let f = fold(d);
        if gn_parents.contains(&f) {
            return f;
        }
        gn_parents
            .iter()
            .map(|g| (levenshtein(g, &f), g))
            .filter(|(k, _)| *k <= 2)
            .min()
            .map_or(f, |(_, g)| g.clone())
    };
    let gn_level = format!("ADM{level}");
    let mut same_level: BTreeMap<(String, String), usize> = BTreeMap::new();
    for r in &recs {
        if r.fcode == gn_level {
            same_level.insert((join_key(&r.name), fold(&r.district)), r.group);
        }
    }
    for (j, row) in it.enumerate() {
        let name = cell(&row[c_name]);
        let pcode = cell(&row[c_pcode]);
        let parent = cell(&row[c_parent]);
        let unit = code_col.and_then(|c| unit_type_of(profile, "codab", &cell(&row[c])));
        // Importance by the unit type's position in the profile (first = most important).
        let rank = unit
            .as_ref()
            .and_then(|u| profile.unit_types.iter().position(|t| &t.id == u))
            .unwrap_or(profile.unit_types.len());
        let importance = 85u32.saturating_sub(10 * rank as u32).max(20);
        let group = same_level
            .get(&(join_key(&name), map_parent(&parent)))
            .copied()
            .unwrap_or(base + j);
        recs.push(Rec {
            key: format!("codab:{pcode}"),
            name,
            alts: Vec::new(),
            fcode: format!("CODAB{level}"),
            district: parent,
            importance,
            current: true,
            group,
            unit_type: unit,
        });
    }
    recs
}

/// The profile's unit type of a unit, from the code field its source rules name (for COD-AB,
/// one digit of the P-code): `None` when the source has no rule or the code no unit type.
#[must_use]
pub fn unit_type_of(profile: &Profile, source: &str, code: &str) -> Option<String> {
    let rule = profile.sources.get(source)?.unit_type_code.as_ref()?;
    let digit = code.get(rule.offset..rule.offset + rule.length)?;
    profile
        .unit_types
        .iter()
        .find(|u| u.source_codes.get(source).is_some_and(|c| c == digit))
        .map(|u| u.id.clone())
}

/// The cross-source join key: the fold without spaces, since sources split compound names
/// differently ("Necha Salyan" and "Nechasalyan").
fn cell(d: &Data) -> String {
    match d {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => format!("{f}"),
        Data::Int(i) => i.to_string(),
        _ => String::new(),
    }
}

/// Levenshtein distance on chars.
#[must_use]
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// A tiny deterministic generator (xorshift64).
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

// ---------- lexicon (the subset this spike reads; full schema: xtask check-data) ----------

#[derive(Deserialize)]
pub struct Lexicon {
    pub terms: Vec<LexTerm>,
}

#[derive(Deserialize)]
pub struct LexTerm {
    pub canonical: String,
    #[serde(default)]
    pub variants: Vec<String>,
    pub kind: String,
    #[serde(default)]
    pub native: Vec<LexNative>,
}

#[derive(Deserialize)]
pub struct LexNative {
    pub text: String,
}

impl LexTerm {
    /// The canonical spelling first, then the variants.
    pub fn spellings(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.canonical).chain(&self.variants)
    }
}

#[must_use]
pub fn lexicon(path: &str) -> Lexicon {
    serde_saphyr::from_str(&std::fs::read_to_string(path).expect("read lexicon"))
        .expect("parse lexicon")
}
