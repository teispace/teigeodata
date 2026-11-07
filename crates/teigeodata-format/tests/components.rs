//! COMPONENTS through the writer and the reader: every presence form against a model, the form
//! the writer picks, every rejection, the deep check, and no panic on mutated payloads.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    reason = "tests: a failure panics with its message; inputs are cut from a 64-bit generator"
)]

use teigeodata_core::bitpack::{Coding, encode as packed};
use teigeodata_core::bits;
use teigeodata_core::table::{Codec, Table, encode_with};
use teigeodata_format::components::{self, ComponentColumns, Components, PRESENCE, column};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn pack_of(rows: u64, body: &[u8]) -> Vec<u8> {
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.section_bytes(Kind::COMPONENTS, Encoding::TABLE, rows, body)
        .unwrap();
    w.layout(false).unwrap().assemble(&[body]).unwrap()
}

fn with_view<R>(rows: usize, body: &[u8], f: impl FnOnce(Result<Components<'_>, Error>) -> R) -> R {
    let p = pack_of(rows as u64, body);
    let pack = Pack::open(&p).unwrap();
    f(Components::open(&pack.section(Kind::COMPONENTS).unwrap()))
}

/// A component's model: present rows and, per field, one value per present row.
struct Model {
    id: u8,
    rows: Vec<u32>,
    fields: Vec<(u8, Vec<u32>)>,
}

fn model(id: u8, n: usize, per_mille: u64, rng: &mut Rng) -> Model {
    let rows: Vec<u32> = (0..n as u32)
        .filter(|_| per_mille >= 1000 || rng.next() % 1000 < per_mille)
        .collect();
    let fields = [1, 2, 7]
        .into_iter()
        .map(|f| {
            (
                f,
                rows.iter()
                    .map(|&r| r * u32::from(f) + (rng.next() % 9) as u32)
                    .collect(),
            )
        })
        .collect();
    Model { id, rows, fields }
}

fn encode(n: usize, models: &[Model]) -> Result<Vec<u8>, Error> {
    let fields: Vec<Vec<(u8, &[u32])>> = models
        .iter()
        .map(|m| m.fields.iter().map(|(f, v)| (*f, v.as_slice())).collect())
        .collect();
    let comps: Vec<ComponentColumns<'_>> = models
        .iter()
        .zip(&fields)
        .map(|(m, f)| ComponentColumns {
            id: m.id,
            rows: &m.rows,
            fields: f,
        })
        .collect();
    components::encode(n, &comps)
}

#[test]
fn round_trip_every_presence_form() {
    let mut rng = Rng(11);
    for n in [0_usize, 1, 2, 127, 128, 129, 2047, 2048, 2049, 30_000] {
        // All rows, dense (bitmap), sparse (sorted rows), and none at all.
        let models = [
            model(1, n, 1000, &mut rng),
            model(3, n, 700, &mut rng),
            model(4, n, 5, &mut rng),
            model(200, n, 0, &mut rng),
        ];
        let body = encode(n, &models).unwrap();
        // The writer's choice of form, when it has one to make.
        let t = Table::parse(&body).unwrap();
        let form = |c| t.column_raw(column(c, PRESENCE)).map(|r| r.0);
        assert_eq!(form(1), None, "every row: no presence column");
        if n >= 2048 {
            assert_eq!(form(3), Some(Codec::Bits));
            assert_eq!(form(4), Some(Codec::Bitpack));
        }
        with_view(n, &body, |v| {
            let v = v.unwrap();
            v.check().unwrap();
            assert_eq!(v.rows(), n);
            assert_eq!(v.ids().collect::<Vec<_>>(), [1, 3, 4, 200]);
            assert!(v.component(2).is_none());
            for m in &models {
                let c = v.component(m.id).unwrap();
                assert_eq!(
                    (c.id(), c.len(), c.is_empty()),
                    (m.id, m.rows.len(), m.rows.is_empty())
                );
                let mut at = 0;
                for row in 0..n {
                    let has = m.rows.get(at) == Some(&(row as u32));
                    assert_eq!(c.index(row), has.then_some(at), "{} row {row}", m.id);
                    for (f, values) in &m.fields {
                        let field = c.field(*f).unwrap();
                        assert_eq!(field.get(row), has.then(|| values[at]));
                    }
                    at += usize::from(has);
                }
                assert_eq!(c.index(n), None);
                assert!(c.field(5).is_none());
                assert!(c.field(PRESENCE).is_none());
                let seven = c.field(7).unwrap();
                assert_eq!(seven.values().len(), m.rows.len());
                assert_eq!(seven.get(n), None);
            }
        });
    }
    // No components at all.
    let body = encode(10, &[]).unwrap();
    with_view(10, &body, |v| assert_eq!(v.unwrap().ids().count(), 0));
}

/// Present rows, fields, component ID.
type Case<'a> = (&'a [u32], &'a [(u8, &'a [u32])], u8);

#[test]
fn the_writer_refuses_bad_input() {
    let one: &[u32] = &[5];
    let cases: [Case<'_>; 9] = [
        (&[1, 1], &[(1, &[0, 0])], 1),
        (&[2, 1], &[(1, &[0, 0])], 1),
        (&[10], &[(1, one)], 1),
        (&[1], &[], 1),
        (&[1], &[(0, one)], 1),
        (&[1], &[(2, one), (1, one)], 1),
        (&[1], &[(1, one), (1, one)], 1),
        (&[1], &[(1, &[5, 6])], 1),
        (&[1, 2], &[(1, one)], 1),
    ];
    for (rows, fields, id) in cases {
        let c = ComponentColumns { id, rows, fields };
        assert!(
            matches!(components::encode(10, &[c]), Err(Error::Write(_))),
            "{rows:?} {fields:?}"
        );
    }
    // Field 0 when every row is present, where no presence column would collide with it.
    let all = ComponentColumns {
        id: 1,
        rows: &[0],
        fields: &[(0, one)],
    };
    assert!(matches!(
        components::encode(1, &[all]),
        Err(Error::Write(_))
    ));
    let c = |id| ComponentColumns {
        id,
        rows: &[1],
        fields: &[(1, &[5])],
    };
    for ids in [[0, 1], [2, 2], [3, 2]] {
        assert!(matches!(
            components::encode(10, &[c(ids[0]), c(ids[1])]),
            Err(Error::Write(_))
        ));
    }
}

/// Hand-built tables the writer never makes: each must be refused at open, naming the column.
#[test]
fn open_and_check_refuse_bad_tables() {
    let col = |v: &[u32]| packed(v, Coding::For);
    let bm = |len: usize, ones: &[usize]| bits::encode(len, ones.iter().copied()).unwrap();
    let opened = |rows: usize, cols: &[(u16, Codec, &[u8])]| {
        let body = encode_with(cols).unwrap();
        with_view(rows, &body, |v| match v {
            Ok(v) => match v.check() {
                Ok(()) => None,
                Err(Error::Payload { column, .. }) => Some(("check", column)),
                Err(e) => panic!("{e}"),
            },
            Err(Error::Payload { column, .. }) => Some(("open", column)),
            Err(e) => panic!("{e}"),
        })
    };
    let (p1, f1) = (column(1, PRESENCE), column(1, 1));
    let (two, three) = (col(&[7, 8]), col(&[7, 8, 9]));
    // Good: a bitmap of 2 present rows and a field of 2 values.
    let good = bm(4, &[0, 3]);
    assert_eq!(
        opened(4, &[(p1, Codec::Bits, &good), (f1, Codec::Bitpack, &two)]),
        None
    );
    // Component 0, with a field that is otherwise right (every row).
    let four = col(&[1, 2, 3, 4]);
    assert_eq!(
        opened(4, &[(column(0, 1), Codec::Bitpack, &four)]),
        Some(("open", 1))
    );
    // The bitmap's length is not the row count: shorter and longer.
    for rows in [3, 5] {
        assert_eq!(
            opened(
                rows,
                &[(p1, Codec::Bits, &good), (f1, Codec::Bitpack, &two)]
            ),
            Some(("open", p1))
        );
    }
    // A field of the wrong length: against the bitmap, the sorted rows, every row.
    assert_eq!(
        opened(4, &[(p1, Codec::Bits, &good), (f1, Codec::Bitpack, &three)]),
        Some(("open", f1))
    );
    assert_eq!(
        opened(
            4,
            &[
                (p1, Codec::Bitpack, &col(&[0, 3])),
                (f1, Codec::Bitpack, &three)
            ]
        ),
        Some(("open", f1))
    );
    assert_eq!(
        opened(4, &[(f1, Codec::Bitpack, &three)]),
        Some(("open", f1))
    );
    // A field stored as a bitmap; a presence with no fields.
    assert_eq!(
        opened(4, &[(p1, Codec::Bits, &good), (f1, Codec::Bits, &good)]),
        Some(("open", f1))
    );
    assert_eq!(opened(4, &[(p1, Codec::Bits, &good)]), Some(("open", p1)));
    // Sorted rows out of order, repeated, or past the end: found by the check.
    for rows in [[3, 0], [2, 2], [1, 4]] {
        assert_eq!(
            opened(
                4,
                &[
                    (p1, Codec::Bitpack, &col(&rows)),
                    (f1, Codec::Bitpack, &two)
                ]
            ),
            Some(("check", p1)),
            "{rows:?}"
        );
    }
    // A bitmap whose directory lies: found by the check. 3000 rows, the last entry's
    // cumulative count is off by one.
    let ones: Vec<usize> = (0..3000).step_by(2).collect();
    let mut lying = bm(3000, &ones);
    let last = lying.len() - 8;
    lying[last] ^= 1;
    let count = bits::Bits::parse(&lying).unwrap().count();
    let field = col(&vec![1; count]);
    assert_eq!(
        opened(
            3000,
            &[(p1, Codec::Bits, &lying), (f1, Codec::Bitpack, &field)]
        ),
        Some(("check", p1))
    );
}

#[test]
fn mutated_payloads_never_panic() {
    let mut rng = Rng(21);
    let n = 5000;
    let models = [
        model(1, n, 600, &mut rng),
        model(2, n, 8, &mut rng),
        model(9, n, 1000, &mut rng),
    ];
    let body = encode(n, &models).unwrap();
    for i in 0..3000 {
        let mut b = body.clone();
        for _ in 0..=rng.next() % 3 {
            let at = (rng.next() % b.len() as u64) as usize;
            b[at] = rng.next() as u8;
        }
        with_view(n, &b, |v| {
            if let Ok(v) = v {
                let _ = v.check();
                for c in v.ids().take(300).collect::<Vec<_>>() {
                    if let Some(c) = v.component(c) {
                        let f = |k| c.field(k).and_then(|f| f.get(i));
                        let _ = (c.index(i), f(1), f(2), c.len());
                    }
                }
            }
        });
    }
}
