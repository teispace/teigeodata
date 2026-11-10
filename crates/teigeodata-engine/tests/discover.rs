//! Discovery (D-164): the engine as the catalog's `Resolver`. Over a synthetic pack, which has no
//! country outlines, each candidate is a search hit in the same order with its own country; the
//! locator's neighbours are gated on the world part in `tests/world.rs`.

#![allow(
    clippy::unwrap_used,
    reason = "tests: failures panic with their message"
)]

mod common;

use common::synthetic;
use teigeodata_engine::{Engine, Layer, LayerOptions, Scratch};
use teigeodata_sync::catalog::{Candidate, Resolver};

#[test]
fn each_candidate_is_a_search_hit_with_its_own_country() {
    let model = synthetic(600, 41);
    let mut e = Engine::new();
    e.add(Layer::from_bytes(model.encode(), LayerOptions::default()).unwrap());
    let (mut s, mut hits, mut out) = (Scratch::default(), Vec::new(), Vec::new());
    // A name several places share, one of a single place, a prefix, and nothing.
    let shared = model.names[model.names.len() / 2][0].text.clone();
    let mut asked = 0;
    for q in [shared.as_str(), "Synthland", &shared[..3], "zzqx"] {
        for k in [1, 5, 50] {
            // `out` is cleared first: a stale candidate never survives.
            out.push(Candidate {
                id: 0,
                point: model.points[0].to_e7(),
                score: 0,
                countries: Vec::new(),
            });
            e.resolve(q, k, &mut out).unwrap();
            e.search(q, k, &mut s, &mut hits).unwrap();
            assert_eq!(out.len(), hits.len(), "{q} {k}");
            for (c, h) in out.iter().zip(&hits) {
                assert_eq!(c.id, h.place.id.get(), "{q}");
                assert_eq!(c.point, h.place.point.to_e7(), "{q}");
                assert_eq!(c.score, h.score, "{q}");
                assert_eq!(c.countries, [*b"np"], "{q}");
            }
            asked += usize::from(!hits.is_empty());
        }
    }
    assert!(asked >= 6, "the queries found places");
}
