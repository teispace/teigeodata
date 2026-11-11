//! `TZ_RULES` (D-196): the rules of every zone of tzdb 2026f, compiled by `zic` 2026f with
//! `-b slim`, parse, encode, read back as `zic` wrote them byte for byte, and answer exactly as
//! `jiff` does on the same bytes; Nepal's history and New York's gap and overlap are golden;
//! every footer form evaluates; `check` refuses planted faults.
//!
//! The fixture `tests/data/tzif-2026f-slim.zst` holds all 598 compiled names (zstd, about
//! 40 kB). It was made by `regenerate_fixture` (ignored) from `zic -b slim -d <dir>` run on
//! tzdata 2026f's `africa antarctica asia australasia europe northamerica southamerica
//! etcetera factory backward` with `zic` built from tzcode 2026f (`data/sources.lock`).

#![cfg(feature = "write")]
#![allow(
    clippy::unwrap_used,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::panic,
    clippy::print_stderr,
    reason = "test code"
)]

use std::collections::BTreeMap;
use std::path::Path;

use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::{Timestamp, civil};
use teigeodata_core::table::{Codec, Table, encode_with};
use teigeodata_format::tz_rules::{
    self, Civil, Dst, Footer, Local, RuleDate, RuleTime, TypeRow, TzRules, ZoneInput, ZoneRules,
};
use teigeodata_format::{Error, Kind, Section};

const FIXTURE: &str = "tests/data/tzif-2026f-slim.zst";
const MAGIC: &[u8; 8] = b"TGTZIF\0\x01";

/// Every compiled name and its TZif bytes, in name order.
fn fixture() -> Vec<(String, Vec<u8>)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let raw = zstd::decode_all(std::fs::read(path).unwrap().as_slice()).unwrap();
    assert_eq!(&raw[..8], MAGIC);
    let mut at = 8;
    let mut out = Vec::new();
    while at < raw.len() {
        let n = usize::from(u16::from_le_bytes([raw[at], raw[at + 1]]));
        let name = std::str::from_utf8(&raw[at + 2..at + 2 + n])
            .unwrap()
            .to_owned();
        at += 2 + n;
        let len = u32::from_le_bytes(raw[at..at + 4].try_into().unwrap()) as usize;
        out.push((name, raw[at + 4..at + 4 + len].to_vec()));
        at += 4 + len;
    }
    out
}

/// Rewrites the fixture from a `zic -b slim -d` output tree named by `TG_TZIF_DIR`.
#[test]
#[ignore = "regenerates the fixture from a zic output tree (TG_TZIF_DIR)"]
fn regenerate_fixture() {
    let dir = std::env::var("TG_TZIF_DIR").expect("TG_TZIF_DIR");
    let mut files = BTreeMap::new();
    let mut stack = vec![std::path::PathBuf::from(&dir)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let name = p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned();
                files.insert(name, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut raw = MAGIC.to_vec();
    for (name, bytes) in &files {
        raw.extend_from_slice(&(name.len() as u16).to_le_bytes());
        raw.extend_from_slice(name.as_bytes());
        raw.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        raw.extend_from_slice(bytes);
    }
    let out = zstd::encode_all(raw.as_slice(), 19).unwrap();
    std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE), out).unwrap();
}

/// Every zone of the fixture parsed, and the encoded section.
struct World {
    files: Vec<(String, Vec<u8>)>,
    rules: Vec<ZoneRules>,
    bytes: Vec<u8>,
}

fn world() -> World {
    let files = fixture();
    let rules: Vec<ZoneRules> = files
        .iter()
        .map(|(n, b)| tz_rules::parse_tzif(b).unwrap_or_else(|e| panic!("{n}: {e}")))
        .collect();
    let inputs: Vec<ZoneInput<'_>> = files
        .iter()
        .zip(&rules)
        .enumerate()
        .map(|(i, ((name, _), rules))| ZoneInput {
            name,
            rules,
            place: Some(i as u32 * 3),
        })
        .collect();
    let bytes = tz_rules::encode(&inputs, false).unwrap();
    World {
        files,
        rules,
        bytes,
    }
}

fn open(bytes: &[u8], n: usize) -> TzRules<'_> {
    let s = Section::unpacked(Kind::TZ_RULES, n as u64, bytes);
    // A section from `unpacked` lives as long as its bytes.
    let r = TzRules::open(&s).unwrap();
    r.check().unwrap();
    r
}

fn civil(y: i32, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> Civil {
    Civil {
        year: y,
        month: mo,
        day: d,
        hour: h,
        minute: mi,
        second: s,
    }
}

#[test]
fn every_zone_parses_encodes_and_rebuilds_byte_for_byte() {
    let w = world();
    assert_eq!(w.files.len(), 598, "345 zones and 253 links in 2026f");
    let r = open(&w.bytes, w.files.len());
    assert_eq!(r.len(), 598);
    let distinct: std::collections::BTreeSet<&[u8]> =
        w.files.iter().map(|(_, b)| b.as_slice()).collect();
    assert_eq!(r.sets(), distinct.len(), "one set per distinct file");
    assert!(r.names().eq(w.files.iter().map(|(n, _)| n.as_str())));
    for (i, (name, bytes)) in w.files.iter().enumerate() {
        let z = r.zone(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(z.name(), name);
        assert_eq!(&z.tzif().unwrap(), bytes, "{name}: TZif rebuilt");
        assert_eq!(r.place(i), Some(i as u32 * 3));
        assert_eq!(
            r.zone_of_place(i as u32 * 3).map(|z| z.name()),
            Some(name.as_str())
        );
    }
    assert!(r.zone("Asia/Katmandu").is_some(), "a backward link");
    assert!(r.zone("Asia/Kathmandu ").is_none());
    assert!(r.zone("").is_none());
    assert!(r.zone_of_place(1).is_none());
    assert!(!r.backzone());
    eprintln!(
        "TZ_RULES, all 598 names: {} B ({} sets, {} transitions); slim TZif {} B, distinct {} B",
        w.bytes.len(),
        r.sets(),
        r.transitions(),
        w.files.iter().map(|f| f.1.len()).sum::<usize>(),
        distinct.iter().map(|b| b.len()).sum::<usize>()
    );
    let _ = &w.rules;
}

/// Nepal: the LMT stub until 1920, an overlap when clocks went back 11 min 16 s, +05:30, a gap
/// of 15 minutes at 1986, +05:45 since (note §5.6).
#[test]
fn nepal_history_is_exact() {
    let w = world();
    let r = open(&w.bytes, w.files.len());
    let z = r.zone("Asia/Kathmandu").unwrap();
    let first = -1_577_943_676; // 1919-12-31T18:18:44Z
    let second = 504_901_800; // 1985-12-31T18:30:00Z
    let at = |t| z.offset_at(t).unwrap();
    let lmt = at(first - 1);
    assert_eq!(
        (lmt.offset, lmt.abbr, lmt.is_dst, lmt.before_first),
        (20_476, "LMT", false, true)
    );
    let ist = at(first);
    assert_eq!(
        (ist.offset, ist.abbr, ist.before_first),
        (19_800, "+0530", false)
    );
    assert_eq!(at(second - 1).offset, 19_800);
    let npt = at(second);
    assert_eq!((npt.offset, npt.abbr, npt.is_dst), (20_700, "+0545", false));
    assert_eq!(at(1_794_355_200).offset, 20_700, "2026-11-11");
    assert_eq!(
        z.local(civil(1919, 12, 31, 23, 50, 0)).unwrap(),
        Local::Overlap {
            earlier: 20_476,
            later: 19_800
        }
    );
    assert_eq!(
        z.local(civil(1986, 1, 1, 0, 10, 0)).unwrap(),
        Local::Gap {
            before: 19_800,
            after: 20_700
        }
    );
    assert_eq!(
        z.local(civil(1986, 1, 1, 0, 15, 0)).unwrap(),
        Local::Unique(20_700)
    );
    assert_eq!(
        z.local(civil(1986, 1, 1, 0, 0, 0)).unwrap(),
        Local::Gap {
            before: 19_800,
            after: 20_700
        }
    );
    assert_eq!(
        z.local(civil(1985, 12, 31, 23, 59, 59)).unwrap(),
        Local::Unique(19_800)
    );
    let all: Vec<(i64, i32)> = z.transitions().map(|(t, l)| (t, l.offset)).collect();
    assert_eq!(all, [(first, 19_800), (second, 20_700)]);
    assert_eq!(z.transition_count(), 2);
}

/// New York in 2024: both changes come from the footer `EST5EDT,M3.2.0,M11.1.0`.
#[test]
fn new_york_gap_and_overlap_come_from_the_footer() {
    let w = world();
    let r = open(&w.bytes, w.files.len());
    let z = r.zone("America/New_York").unwrap();
    let last = z.transitions().last().unwrap().0;
    assert!(last < 1_710_054_000, "2024 is past the stored transitions");
    assert_eq!(
        z.local(civil(2024, 3, 10, 2, 30, 0)).unwrap(),
        Local::Gap {
            before: -18_000,
            after: -14_400
        }
    );
    assert_eq!(
        z.local(civil(2024, 11, 3, 1, 30, 0)).unwrap(),
        Local::Overlap {
            earlier: -14_400,
            later: -18_000
        }
    );
    assert_eq!(
        z.local(civil(2024, 7, 1, 12, 0, 0)).unwrap(),
        Local::Unique(-14_400)
    );
    let summer = z.offset_at(1_719_849_600).unwrap();
    assert_eq!(
        (summer.offset, summer.abbr, summer.is_dst),
        (-14_400, "EDT", true)
    );
    let t = 1_710_054_000; // 2024-03-10T07:00:00Z
    assert_eq!(z.offset_at(t - 1).unwrap().abbr, "EST");
    assert_eq!(z.offset_at(t).unwrap().abbr, "EDT");
    let t = 1_730_613_600; // 2024-11-03T06:00:00Z
    assert_eq!(z.offset_at(t - 1).unwrap().offset, -14_400);
    assert_eq!(z.offset_at(t).unwrap().offset, -18_000);
    assert!(z.offset_at(tz_rules::MAX_UNIX + 1).is_err());
    assert!(z.local(civil(2024, 2, 30, 0, 0, 0)).is_err());
}

/// A zone made of one footer and no transitions.
fn footer_zone(s: &str) -> ZoneRules {
    let f = tz_rules::parse_footer(s).unwrap().unwrap();
    let std = TypeRow {
        offset: f.std_offset,
        is_dst: false,
        abbr: f.std_abbr.clone(),
    };
    ZoneRules {
        version: 3,
        transitions: Vec::new(),
        types: vec![std],
        footer: Some(f),
        first_abbr: 0,
    }
}

/// jiff's view of the same rules: a POSIX TZ zone.
fn jiff_posix(s: &str) -> TimeZone {
    TimeZone::posix(s).unwrap()
}

#[test]
fn footers_parse_strictly_and_evaluate_as_jiff_does() {
    // Real 2026f footers with each extension, and the Jn and n forms (none in 2026f).
    let cases = [
        "EET-2EEST,M4.5.5/0,M10.5.4/24",
        "IST-2IDT,M3.4.4/26,M10.5.0",
        "EET-2EEST,M3.4.4/50,M10.4.4/50",
        "<-02>2<-01>,M3.5.0/-1,M10.5.0/0",
        "<+1245>-12:45<+1345>,M9.5.0/2:45,M4.1.0/3:45",
        "<-04>4<-03>,M9.1.6/24,M4.1.6/24",
        "AEST-10AEDT,M10.1.0,M4.1.0/3",
        "IST-1GMT0,M10.5.0,M3.5.0/1",
        "EST5EDT,M3.2.0,M11.1.0",
        "<+0545>-5:45",
        "XXX3YYY,J60/1:30,J300/-2",
        "XXX-3YYY-5,59/0,300/26:30:15",
    ];
    for s in cases {
        let z = footer_zone(s);
        let bytes = tz_rules::encode(
            &[ZoneInput {
                name: "Test/Zone",
                rules: &z,
                place: None,
            }],
            false,
        )
        .unwrap();
        let r = open(&bytes, 1);
        let ours = r.zone("Test/Zone").unwrap();
        let theirs = jiff_posix(s);
        let mut t = Timestamp::from_second(946_684_800).unwrap(); // 2000-01-01
        let mut n = 0;
        while let Some(tr) = theirs.following(t).next() {
            t = tr.timestamp();
            if t.as_second() > 4_102_444_800 {
                break;
            }
            for d in [-1, 0, 1] {
                let s2 = t.as_second() + d;
                let a = ours.offset_at(s2).unwrap();
                let b = theirs.to_offset_info(Timestamp::from_second(s2).unwrap());
                assert_eq!(
                    (a.offset, a.is_dst, a.abbr),
                    (b.offset().seconds(), b.dst().is_dst(), b.abbreviation()),
                    "{s} at {s2}"
                );
            }
            n += 1;
        }
        assert!(n >= 200 || !s.contains(','), "{s}: {n} switches");
        // The rebuilt TZif ends with the string, in zic's spelling (no "/2", no default DST
        // offset), and parses back to the same rules.
        let back = ours.tzif().unwrap();
        assert!(back.ends_with(format!("\n{s}\n").as_bytes()), "{s}");
        assert_eq!(tz_rules::parse_tzif(&back).unwrap(), z, "{s}");
    }
    // All-year DST (RFC 9636 §3.3.1's example): a start wins its tie with the previous end, so
    // every instant is DST. jiff 0.2.42 reads the last second of each year as standard time
    // here, so this case is checked on its own.
    let z = footer_zone("EST5EDT,0/0,J365/25");
    let bytes = tz_rules::encode(
        &[ZoneInput {
            name: "A/B",
            rules: &z,
            place: None,
        }],
        false,
    )
    .unwrap();
    let r = open(&bytes, 1);
    for t in [946_684_800, 978_307_200 + 5 * 3_600, 1_000_000_000] {
        let l = r.zone("A/B").unwrap().offset_at(t).unwrap();
        assert_eq!((l.offset, l.is_dst), (-14_400, true), "{t}");
    }
    for bad in [
        "EST",
        "EST5EDT",
        "EST5EDT,M3.2.0",
        "ES5",
        "<+05>",
        "<+05-5",
        "EST25",
        "EST5EDT,M13.1.0,M11.1.0",
        "EST5EDT,M3.6.0,M11.1.0",
        "EST5EDT,M3.2.7,M11.1.0",
        "EST5EDT,J0,J365",
        "EST5EDT,366,J365",
        "EST5EDT,M3.2.0/168,M11.1.0",
        "EST5EDT,M3.2.0/1:5,M11.1.0",
        "EST5EDT,M3.2.0,M11.1.0x",
        "EST5:60",
        "EST\u{e9}5",
    ] {
        assert!(tz_rules::parse_footer(bad).is_err(), "{bad}");
    }
    assert_eq!(tz_rules::parse_footer("").unwrap(), None);
    let f = tz_rules::parse_footer("<+1245>-12:45<+1345>,M9.5.0/2:45,M4.1.0/3:45").unwrap();
    assert_eq!(
        f,
        Some(Footer {
            std_offset: 45_900,
            std_abbr: "+1245".into(),
            dst: Some(Dst {
                offset: 49_500,
                abbr: "+1345".into(),
                start: RuleTime {
                    date: RuleDate::Month {
                        month: 9,
                        week: 5,
                        weekday: 0
                    },
                    time: 9_900
                },
                end: RuleTime {
                    date: RuleDate::Month {
                        month: 4,
                        week: 1,
                        weekday: 0
                    },
                    time: 13_500
                },
            }),
        })
    );
}

fn jiff_civil(c: Civil) -> civil::DateTime {
    civil::DateTime::new(
        c.year as i16,
        c.month as i8,
        c.day as i8,
        c.hour as i8,
        c.minute as i8,
        c.second as i8,
        0,
    )
    .unwrap()
}

/// Our answer for a civil time against jiff's.
fn same_local(ours: Local, theirs: AmbiguousOffset) -> bool {
    match (ours, theirs) {
        (Local::Unique(o), AmbiguousOffset::Unambiguous { offset }) => o == offset.seconds(),
        (
            Local::Gap { before, after },
            AmbiguousOffset::Gap {
                before: b,
                after: a,
            },
        ) => (before, after) == (b.seconds(), a.seconds()),
        (
            Local::Overlap { earlier, later },
            AmbiguousOffset::Fold {
                before: b,
                after: a,
            },
        ) => (earlier, later) == (b.seconds(), a.seconds()),
        _ => false,
    }
}

/// What a civil time `l` (local seconds) is in `tz`, from jiff's offsets at instants and the
/// definition: offset `o` is valid when the offset at `l - o` is `o`.
fn by_definition(tz: &TimeZone, l: i64) -> Local {
    let off = |t: i64| {
        tz.to_offset_info(Timestamp::from_second(t).unwrap())
            .offset()
            .seconds()
    };
    let (lo, hi) = (l - 93_599, l + 93_599);
    let mut cands = vec![off(lo)];
    let mut changes = Vec::new();
    let mut t = Timestamp::from_second(lo).unwrap();
    while let Some(tr) = tz.following(t).next() {
        t = tr.timestamp();
        if t.as_second() > hi {
            break;
        }
        let (a, b) = (off(t.as_second() - 1), off(t.as_second()));
        if a != b {
            changes.push((t.as_second(), a, b));
        }
        cands.push(b);
    }
    cands.sort_unstable();
    cands.dedup();
    let valid: Vec<i32> = cands
        .into_iter()
        .filter(|&o| off(l - i64::from(o)) == o)
        .collect();
    match valid.as_slice() {
        [o] => Local::Unique(*o),
        [] => {
            let &(_, before, after) = changes
                .iter()
                .find(|&&(at, a, b)| b > a && at + i64::from(a) <= l && l < at + i64::from(b))
                .unwrap();
            Local::Gap { before, after }
        }
        v => Local::Overlap {
            earlier: *v.iter().max().unwrap(),
            later: *v.iter().min().unwrap(),
        },
    }
}

/// Seeded instants, 1800 to 2100.
fn seeded(n: usize, seed: u64) -> impl Iterator<Item = i64> {
    let mut s = seed;
    (0..n).map(move |_| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let span = 4_102_444_800_i64 + 5_364_662_400; // 1800-01-01 .. 2100-01-01
        -5_364_662_400 + (s % span as u64) as i64
    })
}

/// Every name of 2026f, against jiff on the same bytes: offsets at every stored transition
/// ±1 s, at the footer's switches 2000–2100 ±1 s and at 1,000 seeded instants; civil times on
/// both sides of every change and at the seeded instants' local times.
#[test]
fn every_zone_agrees_with_jiff() {
    let w = world();
    let r = open(&w.bytes, w.files.len());
    let (mut instants, mut civils, mut jiff_differs) = (0_u64, 0_u64, 0_u64);
    for (name, bytes) in &w.files {
        let ours = r.zone(name).unwrap();
        let theirs = TimeZone::tzif(name, bytes).unwrap();
        let mut times: Vec<i64> = ours.transitions().map(|(t, _)| t).collect();
        let mut t = Timestamp::from_second(946_684_800).unwrap();
        while let Some(tr) = theirs.following(t).next() {
            t = tr.timestamp();
            if t.as_second() > 4_102_444_800 {
                break;
            }
            times.push(t.as_second());
        }
        let mut probe: Vec<i64> = times
            .iter()
            .flat_map(|&t| [t - 1, t, t + 1])
            .chain(seeded(1_000, 0x9e37_79b9 ^ bytes.len() as u64))
            .collect();
        probe.sort_unstable();
        for &s in &probe {
            let a = ours.offset_at(s).unwrap();
            let b = theirs.to_offset_info(Timestamp::from_second(s).unwrap());
            assert_eq!(
                (a.offset, a.is_dst, a.abbr),
                (b.offset().seconds(), b.dst().is_dst(), b.abbreviation()),
                "{name} at {s}"
            );
            instants += 1;
        }
        // Civil times at each change, on both sides of each offset, and at the samples.
        let mut walls: Vec<i64> = Vec::new();
        for &t in &times {
            let (a, b) = (
                ours.offset_at(t - 1).unwrap().offset,
                ours.offset_at(t).unwrap().offset,
            );
            for o in [a, b] {
                for d in [-1, 0, 1, 60] {
                    walls.push(t + i64::from(o) + d);
                }
            }
        }
        walls.extend(seeded(200, bytes.len() as u64).map(|s| s + 3_600));
        for l in walls {
            let c = Civil::from_seconds(l).unwrap();
            let got = ours.local(c).unwrap();
            let truth = by_definition(&theirs, l);
            assert_eq!(got, truth, "{name} at {c:?}");
            let want = theirs.to_ambiguous_timestamp(jiff_civil(c)).offset();
            if !same_local(got, want) {
                // jiff reads a civil time at or after the last stored transition's wall time
                // by the footer alone; the instant it names may still be before that
                // transition. Only there may it differ from the definition.
                let last = ours.transitions().last().map_or(i64::MIN, |t| t.0);
                assert!(
                    (l - last).abs() < 2 * 86_400,
                    "{name} at {c:?}: {got:?} vs {want:?}"
                );
                jiff_differs += 1;
            }
            civils += 1;
        }
    }
    eprintln!(
        "agreed with jiff on {instants} instants; {civils} civil times as defined, jiff's own \
         reading differing at {jiff_differs} next to a last stored transition"
    );
    assert!(jiff_differs < 50, "{jiff_differs}");
}

/// Property: for any instant, the civil time there has that instant's offset among its own.
#[test]
fn an_instants_civil_time_holds_its_offset() {
    let w = world();
    let r = open(&w.bytes, w.files.len());
    for (k, (name, _)) in w.files.iter().enumerate().step_by(7) {
        let z = r.zone(name).unwrap();
        for t in seeded(300, k as u64 + 1) {
            let o = z.offset_at(t).unwrap().offset;
            let c = Civil::from_seconds(t + i64::from(o)).unwrap();
            match z.local(c).unwrap() {
                Local::Unique(u) => assert_eq!(u, o, "{name} {t}"),
                Local::Overlap { earlier, later } => {
                    assert!(o == earlier || o == later, "{name} {t}");
                }
                Local::Gap { .. } => panic!("{name} at {t}: an instant's own civil time is a gap"),
            }
        }
    }
}

/// The host's compiled tzdb (fat files on macOS), where it is the same release: our parser
/// takes fat files too, and they answer as the slim ones.
#[test]
fn the_hosts_zoneinfo_of_the_same_release_agrees() {
    let dir = Path::new("/usr/share/zoneinfo");
    let Ok(v) = std::fs::read_to_string(dir.join("+VERSION")) else {
        eprintln!("no host zoneinfo version: skipped");
        return;
    };
    if v.trim() != "2026f" {
        eprintln!("host zoneinfo is {}, not 2026f: skipped", v.trim());
        return;
    }
    let w = world();
    let r = open(&w.bytes, w.files.len());
    let mut compared = 0;
    for (name, _) in &w.files {
        let Ok(bytes) = std::fs::read(dir.join(name)) else {
            continue;
        };
        let fat = tz_rules::parse_tzif(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let enc = tz_rules::encode(
            &[ZoneInput {
                name,
                rules: &fat,
                place: None,
            }],
            false,
        )
        .unwrap();
        let fr = open(&enc, 1);
        let (a, b) = (r.zone(name).unwrap(), fr.zone(name).unwrap());
        for t in seeded(200, 7) {
            assert_eq!(
                a.offset_at(t).unwrap().offset,
                b.offset_at(t).unwrap().offset,
                "{name} at {t}"
            );
        }
        compared += 1;
    }
    assert!(compared > 500, "{compared}");
}

/// The fixture's zones encoded alone: Nepal's and the small pack sizes.
#[test]
fn small_sets_are_small() {
    let w = world();
    let pick = |names: &[&str]| {
        let inputs: Vec<ZoneInput<'_>> = names
            .iter()
            .map(|n| {
                let i = w.files.iter().position(|f| f.0 == *n).unwrap();
                ZoneInput {
                    name: n,
                    rules: &w.rules[i],
                    place: Some(i as u32),
                }
            })
            .collect();
        tz_rules::encode(&inputs, false).unwrap().len()
    };
    // A partly filled 128-value block costs as much as a full one, so a few zones pay a fixed
    // cost per column (about 16 B per bit of width): 2.5 kB here, against 774 B of TZif.
    let nepal = pick(&["Asia/Kathmandu", "Asia/Kolkata", "Asia/Shanghai"]);
    eprintln!("TZ_RULES for Kathmandu, Kolkata, Shanghai: {nepal} B");
    assert!(nepal < 3_000, "{nepal}");
    // A list of names (one per line) to measure, such as the outlines' zones.
    if let Ok(path) = std::env::var("TG_TZ_NAMES") {
        let text = std::fs::read_to_string(path).unwrap();
        let names: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        eprintln!(
            "TZ_RULES for {} listed zones: {} B",
            names.len(),
            pick(&names)
        );
    }
}

/// A table with column `id` replaced by `values`.
fn with_column(bytes: &[u8], id: u16, values: &[u32]) -> Vec<u8> {
    let t = Table::parse(bytes).unwrap();
    let new = teigeodata_core::bitpack::encode_auto(values);
    let cols: Vec<(u16, Codec, &[u8])> = t
        .ids()
        .map(|i| {
            let (c, b) = t.column_raw(i).unwrap();
            (i, c, if i == id { &new[..] } else { b })
        })
        .collect();
    encode_with(&cols).unwrap()
}

fn values(bytes: &[u8], id: u16) -> Vec<u32> {
    let t = Table::parse(bytes).unwrap();
    let c = t.column(id).unwrap();
    c.values(0..c.len()).collect()
}

fn fails(bytes: &[u8], n: usize) -> bool {
    let s = Section::unpacked(Kind::TZ_RULES, n as u64, bytes);
    TzRules::open(&s).and_then(|r| r.check()).is_err()
}

#[test]
fn the_check_refuses_planted_faults() {
    let w = world();
    let n = w.files.len();
    assert!(!fails(&w.bytes, n));
    let edit = |id: u16, f: &dyn Fn(&mut Vec<u32>)| {
        let mut v = values(&w.bytes, id);
        f(&mut v);
        with_column(&w.bytes, id, &v)
    };
    // A footer that disagrees with its last transition: shift New York's standard offset.
    let ny = w
        .files
        .iter()
        .position(|f| f.0 == "America/New_York")
        .unwrap();
    let r = open(&w.bytes, n);
    let set = r.zone_at(ny).unwrap().set();
    let foot = values(&w.bytes, tz_rules::SET_FOOTER)[set] as usize - 1;
    let planted: [(&str, Vec<u8>); 9] = [
        (
            "a footer off by an hour",
            edit(tz_rules::FOOT_STD, &|v| v[2 * foot] += 3_600),
        ),
        (
            "a transition type past its set",
            edit(tz_rules::TRANS_TYPE, &|v| v[0] = 200),
        ),
        (
            "transitions out of order",
            edit(tz_rules::TRANS_MINUTE, &|v| v.swap(1, 2)),
        ),
        (
            "a second of 60",
            edit(tz_rules::TRANS_SECOND, &|v| v[3] = 60),
        ),
        (
            "an offset out of range",
            edit(tz_rules::TYPE_OFFSET, &|v| v[0] = 0),
        ),
        (
            "a set past the sets",
            edit(tz_rules::NAME_SET, &|v| v[5] = 10_000),
        ),
        (
            "two names with one place",
            edit(tz_rules::NAME_PLACE, &|v| v[1] = v[0]),
        ),
        (
            "a base that is not the smallest",
            edit(tz_rules::META, &|v| v[0] = v[0].wrapping_sub(1)),
        ),
        ("an unknown flag", edit(tz_rules::META, &|v| v[2] = 2)),
    ];
    for (what, bytes) in &planted {
        assert!(fails(bytes, n), "{what}");
    }
    // Names out of order: the first two swapped in the text.
    let mut text = Table::parse(&w.bytes)
        .unwrap()
        .bytes(tz_rules::NAME_TEXT)
        .unwrap()
        .to_vec();
    let ends = values(&w.bytes, tz_rules::NAME_ENDS);
    let (a, b) = (ends[0] as usize, ends[1] as usize);
    if b - a == a {
        let first = text[..a].to_vec();
        text.copy_within(a..b, 0);
        text[a..b].copy_from_slice(&first);
    } else {
        text[0] = b'Z';
    }
    let t = Table::parse(&w.bytes).unwrap();
    let cols: Vec<(u16, Codec, &[u8])> = t
        .ids()
        .map(|i| {
            let (c, b) = t.column_raw(i).unwrap();
            (
                i,
                c,
                if i == tz_rules::NAME_TEXT {
                    &text[..]
                } else {
                    b
                },
            )
        })
        .collect();
    assert!(fails(&encode_with(&cols).unwrap(), n), "names out of order");
    // A wrong item count and another kind are refused at open.
    let s = Section::unpacked(Kind::TZ_RULES, n as u64 + 1, &w.bytes);
    assert!(TzRules::open(&s).is_err());
    let s = Section::unpacked(Kind::PLACE_TZ, n as u64, &w.bytes);
    assert!(TzRules::open(&s).is_err());
    // The encoder refuses what the check would.
    let z = &w.rules[0];
    let dup = [
        ZoneInput {
            name: "A/B",
            rules: z,
            place: None,
        },
        ZoneInput {
            name: "A/B",
            rules: z,
            place: None,
        },
    ];
    assert!(matches!(
        tz_rules::encode(&dup, false),
        Err(Error::Write(_))
    ));
    let bad = ZoneInput {
        name: "A/../B",
        rules: z,
        place: None,
    };
    assert!(tz_rules::encode(&[bad], false).is_err());
    assert!(tz_rules::encode(&[], false).is_err());
}

#[test]
fn the_tzif_parser_refuses_what_it_cannot_trust() {
    let files = fixture();
    let ny = &files.iter().find(|f| f.0 == "America/New_York").unwrap().1;
    assert!(tz_rules::parse_tzif(ny).is_ok());
    let mut v1 = ny.clone();
    v1[4] = 0;
    assert!(tz_rules::parse_tzif(&v1).is_err(), "version 1");
    let mut leap = ny.clone();
    // The v2 header starts after the empty v1 block (44 + 7 bytes); leapcnt is its 3rd count.
    leap[51 + 20 + 11] = 1;
    assert!(tz_rules::parse_tzif(&leap).is_err(), "leap seconds");
    assert!(
        tz_rules::parse_tzif(&ny[..ny.len() - 1]).is_err(),
        "no final newline"
    );
    let mut extra = ny.clone();
    extra.push(b'x');
    assert!(
        tz_rules::parse_tzif(&extra).is_err(),
        "bytes after the footer"
    );
    for cut in [0, 10, 44, 60, 100, ny.len() / 2] {
        assert!(tz_rules::parse_tzif(&ny[..cut]).is_err(), "cut at {cut}");
    }
    let mut unsorted = ny.clone();
    let t0 = 51 + 44;
    let (a, b) = (
        unsorted[t0..t0 + 8].to_vec(),
        unsorted[t0 + 8..t0 + 16].to_vec(),
    );
    unsorted[t0..t0 + 8].copy_from_slice(&b);
    unsorted[t0 + 8..t0 + 16].copy_from_slice(&a);
    assert!(
        tz_rules::parse_tzif(&unsorted).is_err(),
        "transitions out of order"
    );
}
