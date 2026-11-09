//! Which names a place takes from its records (D-060), world-general. A place is a conflation
//! cluster; its **anchor** is its first record, in source priority, with a name. Every name of
//! the anchor is the place's. Another record lends its names only under two rules:
//!
//! - **Corroboration:** at least one of its names matches a name of the anchor by fold, join
//!   key or the profile's phonetic key, of the name or of its romanisation by the country's
//!   profile (so ललितपुर matches Lalitpur). A record with no matching name (an OSM node at
//!   Lukla named "नेपाल") lends none.
//! - **Collision:** a lent name that the anchor does not have by fold (of its names or their
//!   romanisations) and that is, by fold or phonetic key, the primary name of an admin unit
//!   anywhere or of another place within [`COLLISION_M`] names that unit or place, so the
//!   place does not take it (Wikidata lists हेटौंडा, Hetauda, as an alias of Makwanpur
//!   District, whose seat it is). Exempting names the anchor matches only phonetically costs
//!   search: S3 recall@1 0.949 against 0.957 by fold (D-107).
//!
//! The S5 measurement of the rules is in `docs/02-research/benches/s5-nepal.md`.

use std::collections::{HashMap, HashSet};

use teigeodata_names::{PhoneticKey, fold, join_key};

use crate::links::haversine_e7;
use crate::names::Namer;

/// A name equal to the primary name of another place this close names that place.
pub(crate) const COLLISION_M: f64 = 30_000.0;

/// More than [`COLLISION_M`] of latitude, in 1e-7°: the scan window of a collision query.
const WINDOW_E7: i32 = 2_800_000;

/// Why a record's name was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Dropped {
    /// The record shares no name with the anchor.
    Uncorroborated,
    /// The name is the primary name of an admin unit or of another place nearby.
    Collision,
}

impl Dropped {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Uncorroborated => "uncorroborated",
            Self::Collision => "collision",
        }
    }
}

/// The primary names of every admin unit and place, by key, for the collision rule.
pub(crate) struct Rules<'a> {
    phonetic: Option<PhoneticKey>,
    /// The country's romanisers, for names in other scripts.
    namer: Option<&'a Namer>,
    /// Fold and phonetic keys of admin units' primary names → the first such name.
    admin: HashMap<String, String>,
    /// Fold and phonetic keys of places' primary names → (lat, lon, owner), sorted by latitude.
    places: HashMap<String, Vec<(i32, i32, u32)>>,
    /// A place's primary name, by owner.
    primary: HashMap<u32, String>,
}

impl<'a> Rules<'a> {
    pub(crate) fn new(phonetic: Option<PhoneticKey>, namer: Option<&'a Namer>) -> Self {
        Self {
            phonetic,
            namer,
            admin: HashMap::new(),
            places: HashMap::new(),
            primary: HashMap::new(),
        }
    }

    /// The tagged keys of a name (fold, join, phonetic), empty ones left out. With `join`
    /// false, only fold and phonetic: the collision keys.
    fn keys(&self, t: &str, join: bool, mut f: impl FnMut(String)) {
        let fold = fold(t);
        if !fold.is_empty() {
            f(format!("f:{fold}"));
        }
        if join {
            let j = join_key(t);
            if !j.is_empty() {
                f(format!("j:{j}"));
            }
        }
        // A phonetic key drops digits, which tell places apart (Namche, Namche3): a name with
        // one has none.
        if let Some(k) = self.phonetic.filter(|_| !t.chars().any(char::is_numeric)) {
            let p = k.key(t);
            if !p.is_empty() {
                f(format!("p:{p}"));
            }
        }
    }

    pub(crate) fn add_admin(&mut self, primary: &str) {
        let mut keys = Vec::new();
        self.keys(primary, false, |k| keys.push(k));
        for k in keys {
            self.admin.entry(k).or_insert_with(|| primary.to_string());
        }
    }

    /// Adds a place's primary name; call [`Rules::finish`] after the last.
    pub(crate) fn add_place(&mut self, owner: u32, (lon, lat): (i32, i32), primary: &str) {
        let mut keys = Vec::new();
        self.keys(primary, false, |k| keys.push(k));
        for k in keys {
            self.places.entry(k).or_default().push((lat, lon, owner));
        }
        self.primary.insert(owner, primary.to_string());
    }

    pub(crate) fn finish(&mut self) {
        for v in self.places.values_mut() {
            v.sort_unstable();
        }
    }

    /// The match keys of a name: [`Rules::keys`] of the name and of its romanisation.
    fn match_keys(&self, t: &str, mut f: impl FnMut(String)) {
        self.keys(t, true, &mut f);
        if let Some(r) = self.namer.and_then(|n| n.romanise(t)) {
            self.keys(&r, true, f);
        }
    }

    /// Every match key of an anchor's names, for [`Rules::known`].
    pub(crate) fn anchor<'n>(&self, names: impl IntoIterator<Item = &'n str>) -> HashSet<String> {
        let mut out = HashSet::new();
        for t in names {
            self.match_keys(t, |k| {
                out.insert(k);
            });
        }
        out
    }

    /// Whether any of a record's names matches a name of the anchor (corroboration).
    pub(crate) fn corroborates<'n>(
        &self,
        anchor: &HashSet<String>,
        names: impl IntoIterator<Item = &'n str>,
    ) -> bool {
        names.into_iter().any(|t| self.known(anchor, t))
    }

    /// Whether the anchor has this name, by fold.
    pub(crate) fn fold_known(anchor: &HashSet<String>, t: &str) -> bool {
        anchor.contains(&format!("f:{}", fold(t)))
    }

    /// Whether a name matches a name of the anchor by any match key.
    pub(crate) fn known(&self, anchor: &HashSet<String>, t: &str) -> bool {
        let mut hit = false;
        self.match_keys(t, |k| hit = hit || anchor.contains(&k));
        hit
    }

    /// The admin unit (`admin:<name>`) or the place other than `owner` within
    /// [`COLLISION_M`] of `point` (`<name> at <km> km`) whose primary name this name is.
    pub(crate) fn collides(&self, owner: u32, point: (i32, i32), t: &str) -> Option<String> {
        let mut hit = None;
        self.keys(t, false, |k| {
            if hit.is_none() {
                hit = self
                    .admin
                    .get(&k)
                    .map(|n| format!("admin:{n}"))
                    .or_else(|| self.near(&k, owner, point));
            }
        });
        hit
    }

    /// The admin unit (`admin:<name>`) whose primary name this name is, by fold or phonetic key.
    pub(crate) fn names_unit(&self, t: &str) -> Option<String> {
        let mut hit = None;
        self.keys(t, false, |k| {
            if hit.is_none() {
                hit = self.admin.get(&k).map(|n| format!("admin:{n}"));
            }
        });
        hit
    }

    /// Whether a country whose labels have `label_keys` ([`Rules::anchor`]) keeps name `t`:
    /// a spelling of a label stays (a unit may share the country's name, as Luxembourg's), any
    /// other name must not be a unit's primary name (Wikidata lists Palpa, a district and a
    /// former kingdom, as an alias of Nepal). `Err` names the unit.
    pub(crate) fn country_keeps(
        &self,
        label_keys: &HashSet<String>,
        t: &str,
    ) -> Result<(), String> {
        match self.names_unit(t) {
            Some(unit) if !self.known(label_keys, t) => Err(unit),
            _ => Ok(()),
        }
    }

    fn near(&self, key: &str, owner: u32, (lon, lat): (i32, i32)) -> Option<String> {
        let v = self.places.get(key)?;
        let lo = v.partition_point(|&(la, _, _)| la < lat.saturating_sub(WINDOW_E7));
        v.get(lo..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&(la, _, _)| la <= lat.saturating_add(WINDOW_E7))
            .find_map(|&(la, lo, o)| {
                let d = haversine_e7((lon, lat), (lo, la));
                (o != owner && d < COLLISION_M).then(|| {
                    let name = self.primary.get(&o).map_or("", String::as_str);
                    format!("{name} at {:.1} km", d / 1000.0)
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_country_keeps_its_spellings_but_no_unit_name() {
        let mut r = Rules::new(PhoneticKey::from_name("indic-v0"), None);
        r.add_admin("Palpa");
        r.add_admin("Nepal");
        r.finish();
        let labels = r.anchor(["Nepal"]);
        assert_eq!(
            r.country_keeps(&labels, "Palpa"),
            Err("admin:Palpa".to_string())
        );
        assert_eq!(
            r.country_keeps(&labels, "Pālpā"),
            Err("admin:Palpa".to_string())
        );
        assert_eq!(r.country_keeps(&labels, "नेपाल"), Ok(()));
        assert_eq!(r.country_keeps(&labels, "Nepāl"), Ok(()));
        assert_eq!(
            r.country_keeps(&labels, "Federal Democratic Republic of Nepal"),
            Ok(())
        );
        assert_eq!(r.names_unit("palpa"), Some("admin:Palpa".to_string()));
        assert_eq!(r.names_unit("Kathmandu"), None);
    }

    fn rules() -> Rules<'static> {
        let mut r = Rules::new(PhoneticKey::from_name("indic-v0"), None);
        r.add_admin("Makwanpur");
        // Two places named Bhimphedi: one 10 km north, one 100 km north.
        r.add_place(1, (850_000_000, 271_000_000), "Bhimphedi");
        r.add_place(2, (850_000_000, 279_000_000), "Bhimphedi");
        r.add_place(3, (850_000_000, 270_000_000), "Hetauda");
        r.finish();
        r
    }

    #[test]
    fn collision_by_admin_and_by_place_nearby() {
        let r = rules();
        let here = (850_000_000, 270_000_000);
        assert!(
            r.collides(3, here, "Makwanpur").is_some(),
            "admin, any distance"
        );
        assert!(r.collides(3, here, "makwanpur").is_some(), "by fold");
        assert!(
            r.collides(3, here, "Bhimphedi").is_some(),
            "place 1 is 11 km away"
        );
        assert!(
            r.collides(3, here, "Hetauda").is_none(),
            "its own primary name"
        );
        // Place 2 only: 100 km away, so no collision.
        assert!(
            r.collides(1, (850_000_000, 271_000_000), "Bhimphedi")
                .is_none()
        );
        assert!(
            r.collides(3, (850_000_000, 260_000_000), "Bhimphedi")
                .is_none(),
            "both too far"
        );
        assert_eq!(
            r.collides(3, here, "Bhimphedi").as_deref(),
            Some("Bhimphedi at 11.1 km")
        );
        assert_eq!(
            r.collides(3, here, "Makwanpur").as_deref(),
            Some("admin:Makwanpur")
        );
        assert!(r.collides(3, here, "Lukla").is_none());
        // Digits tell places apart: no phonetic key for a name with one.
        assert!(r.collides(3, here, "Bhimphedi 2").is_none());
        assert!(r.collides(3, here, "Makwanpur2").is_none());
    }

    #[test]
    fn window_edges() {
        let mut r = Rules::new(None, None);
        // 29.9 km and 30.1 km north of the origin (one degree of latitude is 111.195 km).
        r.add_place(1, (0, 2_689_000), "Near");
        r.add_place(2, (0, 2_707_000), "Far");
        r.finish();
        assert!(r.collides(9, (0, 0), "Near").is_some());
        assert!(r.collides(9, (0, 0), "Far").is_none());
        assert!(
            r.collides(9, (0, 5_396_000), "Far").is_some(),
            "from the north, too"
        );
    }

    #[test]
    fn corroboration() {
        let r = Rules::new(PhoneticKey::from_name("indic-v0"), None);
        let anchor = r.anchor(["Lukla", "लुक्ला"]);
        assert!(r.corroborates(&anchor, ["नेपाल", "Lukla"]));
        assert!(r.corroborates(&anchor, ["LUKLA"]), "fold");
        assert!(r.corroborates(&anchor, ["Luklaa"]), "phonetic");
        assert!(!r.corroborates(&anchor, ["नेपाल"]));
        assert!(!r.corroborates(&anchor, [""]));
        assert!(r.known(&anchor, "lukla"));
        assert!(r.known(&anchor, "Luklaa"), "phonetic");
        assert!(!r.known(&anchor, "Namche"));
        assert!(Rules::fold_known(&anchor, "LUKLA"));
        assert!(!Rules::fold_known(&anchor, "Luklaa"));
    }

    /// The romanisation matches what the phonetic key misses (measured in the Nepal build:
    /// 24 names kept).
    #[test]
    fn romanised_names_match() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let namer = Namer::load(&root, "np", &["ne-Deva".into()]).expect("np loads");
        let key = PhoneticKey::from_name("indic-v0");
        for (with, known) in [(Some(&namer), true), (None, false)] {
            let r = Rules::new(key, with);
            let a = r.anchor(["Methinkot"]);
            assert_eq!(r.known(&a, "मेथिनकोट"), known);
        }
    }
}
