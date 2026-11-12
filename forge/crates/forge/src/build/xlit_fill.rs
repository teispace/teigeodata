//! Tier 6 of the Nepali-names chain (nepali-names §5.1, D-228): a place with no name in a
//! transliteration profile's language and script gains its canonical Latin name transliterated
//! by the profile's shipping model (D-222), typed `alt` and marked `transliteration` (D-227),
//! so it never becomes the canonical name and an app can show it differently or drop it.
//! Countries opt in by data (`transliterate: [ne-Latn-Deva]` in `profile.yaml`, P16).

use std::fmt::Write as _;
use std::time::Instant;

use teigeodata_core::PlaceKind;
use teigeodata_format::names::NameType;
use teigeodata_names::script_code;
use teigeodata_names::xlit::{Scratch, transliterate_with};

use super::{Origin, Place, PlaceName};

/// Adds the machine names; returns the summary's line.
pub(super) fn run(
    root: &std::path::Path,
    profiles: &[String],
    places: &mut [Place],
) -> Result<String, String> {
    let mut out = String::new();
    for id in profiles {
        let t = Instant::now();
        // `ne-Latn-Deva`: the language, then the source and target scripts.
        let lang = id
            .split('-')
            .next()
            .filter(|l| !l.is_empty())
            .ok_or_else(|| format!("{id}: not a transliteration profile ID"))?;
        let (profile, model, script) = crate::xlit::shipping(root, id)?;
        let (added, had) = fill(places, lang, &script, |latin, buf| {
            thread_local! {
                static SCRATCH: std::cell::RefCell<Scratch> = std::cell::RefCell::default();
            }
            SCRATCH.with_borrow_mut(|s| transliterate_with(&profile, &model, s, latin, buf));
        });
        let _ = writeln!(
            out,
            "Transliterated names (D-228, {id}): {added} places gained a `{lang}` name in {script} \
             written by the model, marked `transliteration`; {had} had one; {:.1?}.",
            t.elapsed()
        );
    }
    Ok(out)
}

/// For each place (timezone areas left out) with no name in `lang` written in `script`, adds
/// `xlit` of its canonical name when that is Latin and the result is new and storable.
/// Returns (places that gained one, places that had one).
fn fill(
    places: &mut [Place],
    lang: &str,
    script: &str,
    xlit: impl Fn(&str, &mut String),
) -> (usize, usize) {
    let (mut added, mut had) = (0, 0);
    let mut buf = String::new();
    for p in places.iter_mut() {
        if p.kind == PlaceKind::TimezoneArea {
            continue;
        }
        if p.names
            .iter()
            .any(|n| n.lang.as_deref() == Some(lang) && script_code(&n.text) == Some(script))
        {
            had += 1;
            continue;
        }
        let Some(first) = p.names.first() else {
            continue;
        };
        if script_code(&first.text) != Some("Latn") {
            continue;
        }
        xlit(&first.text, &mut buf);
        if buf.is_empty()
            || script_code(&buf) != Some(script)
            || !super::pack::fits(&buf)
            || p.names
                .iter()
                .any(|n| n.text == buf && n.lang.as_deref() == Some(lang))
        {
            continue;
        }
        p.names.push(PlaceName {
            text: buf.clone(),
            lang: Some(lang.to_owned()),
            kind: Some(NameType::Alt),
            origin: Origin::Transliteration,
        });
        added += 1;
    }
    (added, had)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]
mod tests {
    use super::fill;
    use crate::build::{Origin, Place, PlaceName};
    use teigeodata_core::PlaceKind;
    use teigeodata_format::names::NameType;

    fn place(kind: PlaceKind, names: &[(&str, Option<&str>)]) -> Place {
        Place {
            key: names[0].0.into(),
            kind,
            admin_level: 0,
            flags: 0,
            point: (0, 0),
            names: names
                .iter()
                .map(|(t, l)| PlaceName {
                    text: (*t).into(),
                    lang: l.map(str::to_owned),
                    kind: None,
                    origin: Origin::GeoNames,
                })
                .collect(),
            population: None,
            rank: 0,
            parent: None,
            sources: Vec::new(),
        }
    }

    /// Only a Latin-named place without a Devanagari `ne` name gains one, as an alt name
    /// marked machine; a Latin `ne` name, a zone and a non-Latin canonical name gain none.
    #[test]
    fn only_a_gap_is_filled_and_marked() {
        let mut ps = vec![
            place(PlaceKind::Settlement, &[("Pokhara", None)]),
            place(
                PlaceKind::Settlement,
                &[("Lumbini", None), ("लुम्बिनी", Some("ne"))],
            ),
            place(
                PlaceKind::Settlement,
                &[("Dhulikhel", None), ("Dhulikhel", Some("ne"))],
            ),
            place(PlaceKind::TimezoneArea, &[("Asia/Kathmandu", None)]),
            place(PlaceKind::Settlement, &[("काठमाडौं", None)]),
        ];
        let (added, had) = fill(&mut ps, "ne", "Deva", |latin, out| {
            out.clear();
            out.push_str(if latin == "Pokhara" {
                "पोखरा"
            } else {
                "धुलिखेल"
            });
        });
        assert_eq!((added, had), (2, 1));
        let last = ps[0].names.last().unwrap();
        assert_eq!(last.text, "पोखरा");
        assert_eq!(last.kind, Some(NameType::Alt));
        assert!(last.origin.is_machine());
        assert_eq!(ps[0].names[0].text, "Pokhara", "never canonical");
        assert_eq!(ps[2].names.len(), 3, "a Latin `ne` name is no Nepali name");
        assert_eq!(ps[3].names.len(), 1);
        assert_eq!(ps[4].names.len(), 1);
    }

    /// Another language's label with the same text does not stand in for a Nepali name: the
    /// place still gains one (Janakpur's Awadhi जनकपुर left it with none before D-236).
    #[test]
    fn another_language_s_equal_label_does_not_block_the_gap() {
        let mut ps = vec![place(
            PlaceKind::Settlement,
            &[("Janakpur", None), ("जनकपुर", Some("awa"))],
        )];
        let (added, _) = fill(&mut ps, "ne", "Deva", |_, out| {
            out.clear();
            out.push_str("जनकपुर");
        });
        assert_eq!(added, 1);
        assert_eq!(ps[0].names.last().unwrap().lang.as_deref(), Some("ne"));
    }

    /// A transliteration is keyed in the machine map only (D-228); a romanised display name,
    /// a machine name too but the canonical one, keeps the exact map, so it is found as
    /// written.
    #[test]
    fn machine_names_are_keyed_apart() {
        use crate::build::{Profile, pack::add_keys};
        use teigeodata_format::search::Maps;
        let mut m = Maps::default();
        let p = Profile::default();
        add_keys(&mut m, ("पोखरा", Origin::Transliteration), 0, &p);
        assert!(m.exact.is_empty() && m.loose.is_empty() && m.phonetic.is_empty());
        assert_eq!(m.machine.len(), 1);
        add_keys(&mut m, ("Changunarayan", Origin::Romanisation), 1, &p);
        assert!(m.exact.contains_key("changunarayan"));
        assert_eq!(m.machine.len(), 1);
    }
}
