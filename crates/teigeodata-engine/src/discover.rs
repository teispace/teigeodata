//! The engine as the catalog's [`Resolver`] (D-164): a name to candidate places, each with the
//! countries whose packs could hold it.

use teigeodata_sync::catalog::{Candidate, Resolver};

use crate::{Engine, Error, Hit, LOCATOR_RADIUS_M, Place, Scratch};

impl Resolver for Engine {
    type Error = Error;

    /// The best `k` places the engine's search finds for `query` over every loaded layer (the
    /// baseline, and the gazetteer pack when installed), each with its own country (its ID's
    /// namespace) and every country the locator finds within [`LOCATOR_RADIUS_M`] of its point,
    /// so a border town names both sides.
    fn resolve(&self, query: &str, k: usize, out: &mut Vec<Candidate>) -> Result<(), Error> {
        out.clear();
        let mut scratch = Scratch::default();
        let mut hits: Vec<Hit> = Vec::new();
        self.search(query, k, &mut scratch, &mut hits)?;
        let mut near: Vec<Place> = Vec::new();
        for h in hits {
            let point = h.place.point.to_e7();
            let mut countries: Vec<[u8; 2]> =
                h.place.id.namespace().country().into_iter().collect();
            self.countries_near(point, LOCATOR_RADIUS_M, &mut near)?;
            for c in near.iter().filter_map(|c| c.id.namespace().country()) {
                if !countries.contains(&c) {
                    countries.push(c);
                }
            }
            out.push(Candidate {
                id: h.place.id.get(),
                point,
                score: h.score,
                countries,
            });
        }
        Ok(())
    }
}
