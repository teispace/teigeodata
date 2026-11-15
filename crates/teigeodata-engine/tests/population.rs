//! Populations (D-351): each place's number of people and its source, read from COMPONENTS,
//! refused as missing when the `extra` component is not installed, and kept by a slice and a
//! merge of packs (`extract`, the shipped baseline).

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    reason = "tests: failures panic with their message; rows fit u32"
)]

mod common;

use common::{Model, synthetic};
use teigeodata_engine::{
    Component, Components, Engine, Error, Layer, LayerOptions, Population, PopulationSource,
};
use teigeodata_format::components::encode_populations;
use teigeodata_format::region::Options;
use teigeodata_format::{Encoding, Kind, Pack, Writer, extract};

/// Every third row has people: admin units modelled, the rest stated; a third of those none.
fn people(n: usize) -> Vec<Option<(u32, PopulationSource)>> {
    (0..n)
        .map(|r| {
            let source = if r % 2 == 0 {
                PopulationSource::Stated
            } else {
                PopulationSource::Modelled
            };
            (r % 3 == 0).then_some((r as u32 * 7_919 + 11, source))
        })
        .collect()
}

fn with_people(seed: u64) -> (Model, Vec<Option<(u32, PopulationSource)>>) {
    let mut model = synthetic(900, seed);
    let n = model.ids.len();
    let p = people(n);
    let body = encode_populations(&p).unwrap().unwrap();
    model.raw.push((Kind::COMPONENTS, n as u64, body));
    (model, p)
}

fn engine(bytes: Vec<u8>, installed: Option<Components>) -> Engine {
    let mut e = Engine::new();
    let options = LayerOptions {
        installed,
        ..LayerOptions::default()
    };
    e.add(Layer::from_bytes(bytes, options).unwrap());
    e
}

/// What `Engine::population` answered, as plain values.
type Read = Option<(u32, Option<PopulationSource>)>;

fn read(e: &Engine, place: &teigeodata_engine::Place) -> Read {
    e.population(place)
        .unwrap()
        .map(|p: Population| (p.people, p.source))
}

fn expect(p: Option<(u32, PopulationSource)>) -> Read {
    p.map(|(n, s)| (n, Some(s)))
}

#[test]
fn each_place_reads_its_population_and_source() {
    let (model, p) = with_people(5);
    let e = engine(model.encode(), None);
    let mut with = 0;
    for (r, want) in p.iter().enumerate() {
        let place = e.get(model.ids[r]).unwrap().unwrap();
        let got = read(&e, &place);
        assert_eq!(got, expect(*want), "row {r}");
        with += usize::from(got.is_some());
    }
    assert!(with > 100, "{with} places with people");
    // A pack without COMPONENTS has no populations, and that is not an error.
    let plain = synthetic(900, 5);
    let e = engine(plain.encode(), None);
    let place = e.get(plain.ids[0]).unwrap().unwrap();
    assert_eq!(e.population(&place).unwrap(), None);
}

#[test]
fn a_layer_without_the_extra_component_says_it_is_missing() {
    let (model, _) = with_people(7);
    let installed = Components::all().without(Components::of(&[Component::Extra]));
    let e = engine(model.encode(), Some(installed));
    let place = e.get(model.ids[0]).unwrap().unwrap();
    match e.population(&place) {
        Err(Error::MissingComponent { component, .. }) => assert_eq!(component, Component::Extra),
        other => panic!("{other:?}"),
    }
}

fn write(sections: &[extract::Body]) -> Vec<u8> {
    let refs: Vec<(Kind, Encoding, u64, &[u8])> = sections
        .iter()
        .map(|(k, e, n, b)| (*k, *e, *n, b.as_slice()))
        .collect();
    Writer::new("np-slice", "places", "2026.11.0")
        .unwrap()
        .pack(&refs, None)
        .unwrap()
}

#[test]
fn a_slice_and_a_merge_keep_each_rows_population() {
    let (model, p) = with_people(9);
    let bytes = model.encode();
    let pack = Pack::open(&bytes).unwrap();
    let n = model.ids.len() as u32;
    // A slice of every other row.
    let keep: Vec<u32> = (0..n).filter(|r| r % 2 == 0).collect();
    let slice = engine(
        write(&extract::rows(&pack, &keep, 0, &Options::default()).unwrap()),
        None,
    );
    for &r in &keep {
        let place = slice.get(model.ids[r as usize]).unwrap().unwrap();
        assert_eq!(read(&slice, &place), expect(p[r as usize]), "row {r}");
    }
    // Two packs, one with populations and one without, merged: the second owns the IDs it
    // shares with the first, so those rows lose their people.
    let plain_bytes = model_without_people(&model).encode();
    let plain = Pack::open(&plain_bytes).unwrap();
    let (first, second): (Vec<u32>, Vec<u32>) =
        ((0..n).collect(), (0..n).filter(|r| r % 5 == 0).collect());
    let part = |pack, keep| extract::Part {
        pack,
        keep,
        lod: 0,
        names: None,
        arcs: None,
    };
    let merged = engine(
        write(
            &extract::merge(
                &[part(&pack, &first), part(&plain, &second)],
                &Options::default(),
            )
            .unwrap(),
        ),
        None,
    );
    for r in 0..n {
        let place = merged.get(model.ids[r as usize]).unwrap().unwrap();
        let want = if r % 5 == 0 {
            None
        } else {
            expect(p[r as usize])
        };
        assert_eq!(read(&merged, &place), want, "row {r}");
    }
}

fn model_without_people(model: &Model) -> Model {
    let mut m = Model::from_pack(&model.encode());
    m.raw.retain(|s| s.0 != Kind::COMPONENTS);
    m
}
