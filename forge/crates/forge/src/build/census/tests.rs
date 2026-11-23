use std::collections::BTreeSet;
use std::io::Write as _;

use teigeodata_core::PlaceKind;

use super::{Outcome, apply, cells, columns, join, shared_strings, sheet, unescape};
use crate::build::official::Row;
use crate::build::{Origin, Place, PlaceName};

fn unit(key: &str, level: u8, parent: Option<&str>, population: Option<i64>) -> Place {
    Place {
        key: key.to_string(),
        kind: PlaceKind::Admin,
        admin_level: level,
        flags: 0,
        class: 0,
        point: (0, 0),
        names: vec![PlaceName {
            text: key.to_string(),
            lang: None,
            kind: None,
            origin: Origin::Unknown,
        }],
        population,
        rank: 0,
        parent: parent.map(str::to_string),
        sources: Vec::new(),
    }
}

fn row(key: &str, names: &[&str]) -> Row {
    Row {
        key: Some(key.to_string()),
        names: names.iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn xml_text_is_unescaped() {
    assert_eq!(
        unescape("a &amp; b &lt;c&gt; &#2309;&#x0906;"),
        "a & b <c> अआ"
    );
    assert_eq!(unescape("AT&T &bogus"), "AT&T &bogus");
}

#[test]
fn shared_strings_join_runs_and_drop_phonetic_ones() {
    let xml = r#"<sst><si><t>बेलडाँडी गाउँपालिका</t></si><si><r><t xml:space="preserve">Rich </t></r><r><t>text</t></r></si><si><t>base</t><rPh sb="0" eb="1"><t>reading</t></rPh></si></sst>"#;
    assert_eq!(
        shared_strings(xml),
        ["बेलडाँडी गाउँपालिका", "Rich text", "base"]
    );
}

#[test]
fn cells_resolve_shared_inline_and_number_values() {
    let strings = vec!["zero".to_string(), "one".to_string()];
    let xml = r#"<sheetData><row r="3"><c r="A3" s="8"><v>1</v></c><c r="E3" s="9" t="s"><v>1</v></c><c r="F3" t="inlineStr"><is><t>in &amp; line</t></is></c><c r="G3" s="10"><v>11791</v></c><c r="L3" s="5"/></row></sheetData>"#;
    let got = cells(xml, &strings).unwrap();
    let at = |c: &str| got.get(&(3, c.to_string())).cloned();
    assert_eq!(at("E").as_deref(), Some("one"));
    assert_eq!(at("F").as_deref(), Some("in & line"));
    assert_eq!(at("G").as_deref(), Some("11791"));
    assert_eq!(at("L"), None);
    // A shared index past the table is refused, not read as empty.
    let bad = r#"<c r="E4" t="s"><v>9</v></c>"#;
    assert!(cells(bad, &strings).is_err());
}

type Cells = std::collections::BTreeMap<(u32, String), String>;

fn put(c: &mut Cells, r: u32, col: &str, v: &str) {
    c.insert((r, col.to_string()), v.to_string());
}

#[test]
fn columns_read_every_row_and_refuse_a_bad_count() {
    let mut c = std::collections::BTreeMap::new();
    put(&mut c, 2, "E", "header");
    put(&mut c, 3, "E", "क");
    put(&mut c, 3, "G", "100");
    put(&mut c, 4, "E", "ख");
    put(&mut c, 4, "G", "2.5E3");
    let got = columns(&c, 3, "E", "G").unwrap();
    assert_eq!(got, [("क".to_string(), 100), ("ख".to_string(), 2500)]);
    put(&mut c, 5, "E", "ग");
    put(&mut c, 5, "G", "12.5");
    assert!(columns(&c, 3, "E", "G").unwrap_err().contains("row 5"));
    put(&mut c, 5, "G", "");
    assert!(columns(&c, 3, "E", "G").is_err());
}

#[test]
fn a_row_must_name_its_list_rows_unit_or_be_checked() {
    let strip = vec!["गाउँपालिका".to_string(), "नगरपालिका".to_string()];
    let list = [
        row("a", &["फक्ताङलुङ", "पाथीभरा याङवरक"]),
        row("b", &["मिक्लाजुङ्ग"]),
        row("c", &["कालिका"]),
    ];
    let none = std::collections::BTreeMap::new();
    let table = [
        // The name since a rename, spaced differently.
        ("पाथीभरायाङवरक गाउँपालिका".to_string(), 10),
        // A near spelling: ङ for ङ्ग.
        ("मिक्लाजुङ गाउँपालिका".to_string(), 20),
        // Far: a word more, checked by hand.
        ("शुभ कालीका गाउँपालिका".to_string(), 30),
    ];
    let e = join("t", (&list, &strip), &table, &none).unwrap_err();
    assert!(e.contains("1 rows") && e.contains("row 3:"), "{e}");
    let checked: std::collections::BTreeMap<u32, String> = [(3, "the same unit".into())].into();
    let got = join("t", (&list, &strip), &table, &checked).unwrap();
    assert_eq!(got.near, 1);
    assert_eq!(
        got.rows,
        [
            (Some("a".into()), 10),
            (Some("b".into()), 20),
            (Some("c".into()), 30)
        ]
    );
    // Rows shifted by one: every row is far.
    let shifted = [table[1].clone(), table[2].clone(), table[0].clone()];
    let e = join("t", (&list, &strip), &shifted, &checked).unwrap_err();
    assert!(e.contains("row 1:") && e.contains("row 2:"), "{e}");
    // A checked row that matches is stale.
    let stale: std::collections::BTreeMap<u32, String> = [(1, "x".into()), (3, "y".into())].into();
    assert!(
        join("t", (&list, &strip), &table, &stale)
            .unwrap_err()
            .contains("rows 1 are")
    );
    // A table one row short of its list is refused.
    assert!(
        join("t", (&list, &strip), &table[..2], &checked)
            .unwrap_err()
            .contains("row by row")
    );
}

#[test]
fn a_count_scales_the_units_below_exactly_and_resums_the_units_above() {
    let mut places = vec![
        unit("country", 0, None, Some(1_000)),
        unit("district", 2, Some("country"), Some(600)),
        unit("other", 2, Some("country"), Some(400)),
        unit("local", 3, Some("district"), Some(600)),
        unit("w1", 4, Some("local"), Some(100)),
        unit("w2", 4, Some("local"), Some(200)),
        unit("w3", 4, Some("local"), Some(300)),
        unit("w4", 4, Some("local"), None),
    ];
    let mut out = Outcome::default();
    apply(&mut places, &[(3, 1_001)], &mut out);
    let pop = |k: &str| places.iter().find(|p| p.key == k).unwrap().population;
    assert_eq!(pop("local"), Some(1_001));
    // Shares 1:2:3 of 1,001, exact by largest remainder; a ward with none stays without.
    assert_eq!(
        [pop("w1"), pop("w2"), pop("w3")],
        [Some(167), Some(334), Some(500)]
    );
    assert_eq!(pop("w4"), None);
    // Above: the district is its child's count; the country sums a counted and a grid child.
    assert_eq!(pop("district"), Some(1_001));
    assert_eq!(pop("country"), Some(1_401));
    let set = |v: &[&str]| v.iter().map(ToString::to_string).collect::<BTreeSet<_>>();
    assert_eq!(out.stated, set(&["local", "district"]));
    assert_eq!(out.modelled, set(&["w1", "w2", "w3", "country"]));
    // The pack's modelled set: the grid's less the counted, plus the scaled.
    let grid = set(&["local", "district", "other", "country", "w1"]);
    assert_eq!(out.mark(grid), set(&["other", "country", "w1", "w2", "w3"]));
}

#[test]
fn a_workbook_is_read_from_its_zip() {
    let dir = std::env::temp_dir().join(format!("teigeodata-census-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.xlsx");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    z.start_file("xl/sharedStrings.xml", opts).unwrap();
    z.write_all(br"<sst><si><t>name</t></si></sst>").unwrap();
    z.start_file("xl/worksheets/sheet1.xml", opts).unwrap();
    z.write_all(br#"<c r="E3" t="s"><v>0</v></c><c r="G3"><v>42</v></c>"#)
        .unwrap();
    z.finish().unwrap();
    let c = sheet(&path, "xl/worksheets/sheet1.xml").unwrap();
    assert_eq!(
        columns(&c, 3, "E", "G").unwrap(),
        [("name".to_string(), 42)]
    );
    assert!(sheet(&path, "xl/worksheets/sheet2.xml").is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
