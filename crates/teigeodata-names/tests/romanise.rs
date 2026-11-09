//! The romanisation engine on a small Devanagari profile built in the test (the real profile,
//! `data/romanise/ne-Deva.yaml`, is loaded and checked by the forge and `check-data`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "tests"
)]

use teigeodata_names::romanise::{Assimilate, Profile, ProfileData};

fn pairs(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    rows.iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

fn data() -> ProfileData {
    ProfileData {
        inherent: "a".into(),
        virama: Some('्'),
        nukta: Some('़'),
        anusvara: Some('ं'),
        chandrabindu: Some('ँ'),
        consonants: pairs(&[
            ("क", "k"),
            ("ख", "kh"),
            ("ग", "g"),
            ("ङ", "ng"),
            ("च", "ch"),
            ("ज", "j"),
            ("ञ", "n"),
            ("ट", "t"),
            ("ड", "d"),
            ("त", "t"),
            ("थ", "th"),
            ("द", "d"),
            ("न", "n"),
            ("प", "p"),
            ("ब", "b"),
            ("म", "m"),
            ("य", "y"),
            ("र", "r"),
            ("ल", "l"),
            ("व", "w"),
            ("स", "s"),
            ("ह", "h"),
            ("ज़", "z"),
        ]),
        vowels: pairs(&[("अ", "a"), ("आ", "a"), ("इ", "i"), ("उ", "u"), ("ए", "e")]),
        signs: pairs(&[
            ("ा", "a"),
            ("ि", "i"),
            ("ी", "i"),
            ("ु", "u"),
            ("े", "e"),
            ("ो", "o"),
            ("ौ", "au"),
        ]),
        conjuncts: pairs(&[("ज्ञ", "gy")]),
        labials: vec!["प".into(), "ब".into(), "म".into()],
        geminate_once: vec!["ख".into()],
        keep_before: vec!["ह".into()],
        assimilate: vec![Assimilate {
            consonant: "ङ".into(),
            before: vec!["क".into(), "ग".into()],
            latin: "n".into(),
        }],
        repairs: pairs(&[("ाे", "ो")]),
        zero: Some('०'),
        examples: pairs(&[("ललितपुर", "Lalitpur")]),
    }
}

fn profile() -> Profile {
    Profile::new(
        &data(),
        &pairs(&[
            ("चोक", "Chowk"),
            ("नगर", "Nagar"),
            ("टोल", "Tole"),
            ("बजार", "Bazar"),
        ]),
    )
    .unwrap()
}

#[test]
fn rules_each_do_their_part() {
    let p = profile();
    for (input, want) in [
        // Word-final schwa deleted; kept after a conjunct.
        ("कमल", "Kamal"),
        ("यज्ञ", "Yagya"),
        ("मन्त्र", "Mantra"),
        // Medial schwa, left to right: Halbade, not Halabde.
        ("हलबडे", "Halbade"),
        // No medial deletion before ह.
        ("सपही", "Sapahi"),
        // Morphemes: whole word, suffix, and a suffix of a stem.
        ("चोक", "Chowk"),
        ("लहाचोक", "Lahachowk"),
        ("सारदानगर", "Sardanagar"),
        ("नगरटोल", "Nagartole"),
        ("बजारटोल", "Bazartole"),
        // A suffix that leaves one character is not a morpheme.
        ("कटोल", "Katol"),
        // Nasal signs: n before a consonant, m before a labial, n at the end after a letter.
        ("संगम", "Sangam"),
        ("चंपा", "Champa"),
        ("गाउँ", "Gaun"),
        ("डाँडा", "Danda"),
        ("गाँइ", "Gai"),
        // Velar nasal before a velar; geminates.
        ("जङ्गल", "Jangal"),
        ("नख्खु", "Nakhu"),
        ("तल्ले", "Talle"),
        // Nukta letters, repairs, digits, unknown letters dropped, ASCII kept.
        ("ज़मीन", "Zamin"),
        ("गाेदाम", "Godam"),
        ("६ सय", "6 Say"),
        ("Ward ३", "Ward 3"),
        ("क\u{0BF5}ल", "Kal"),
        ("", ""),
        ("  कमल   टोल ", "Kamal Tole"),
    ] {
        assert_eq!(p.romanise(input), want, "{input:?}");
    }
}

#[test]
fn bad_profiles_are_refused() {
    let morph = pairs(&[]);
    let mut d = data();
    d.consonants.push(("कख".into(), "x".into()));
    assert!(
        Profile::new(&d, &morph)
            .unwrap_err()
            .contains("must be a letter")
    );
    let mut d = data();
    d.consonants.push(("क".into(), "k".into()));
    assert!(Profile::new(&d, &morph).unwrap_err().contains("twice"));
    let mut d = data();
    d.signs.push(("ू".into(), "ū".into()));
    assert!(Profile::new(&d, &morph).unwrap_err().contains("not ASCII"));
    let mut d = data();
    d.labials.push("फ".into());
    assert!(
        Profile::new(&d, &morph)
            .unwrap_err()
            .contains("not a consonant")
    );
    let mut d = data();
    d.examples.push(("कमल".into(), "Kamala".into()));
    assert!(Profile::new(&d, &morph).unwrap_err().contains("expected"));
    assert!(Profile::new(&data(), &pairs(&[("", "X")])).is_err());
    assert!(Profile::new(&data(), &pairs(&[("चोक", "Čowk")])).is_err());
}

/// A small deterministic generator (xorshift), so the property runs are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn any_input_gives_ascii_deterministically() {
    let p = profile();
    let alphabet: Vec<char> = ('\u{0900}'..='\u{097F}')
        .chain([' ', 'a', 'Z', '-', '\u{200D}', '\u{0BF5}', '😀', '\u{0301}'])
        .collect();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let len = usize::try_from(rng.next() % 12).unwrap();
        let s: String = (0..len)
            .map(|_| alphabet[usize::try_from(rng.next() % alphabet.len() as u64).unwrap()])
            .collect();
        let a = p.romanise(&s);
        assert!(a.is_ascii(), "{s:?} → {a:?}");
        assert!(!a.contains("  ") && a.trim() == a, "{s:?} → {a:?}");
        assert_eq!(a, p.romanise(&s));
    }
}
