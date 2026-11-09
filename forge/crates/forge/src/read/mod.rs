//! The `read/<source>` stages: each turns fetched bytes into the common record tables
//! ([`crate::record`]) under `data/build/read/<source-id>/`.

pub(crate) mod boundaries;
mod geonames;
mod osm;
mod wikidata;

use std::path::Path;
use std::time::Instant;

use crate::lock::Lock;
use crate::record::Output;

/// `forge read <reader> <source-id> [<extra-source-id>…]`. The output directory is named by
/// the first source. `GeoNames` takes optional alternate-name and hierarchy sources.
pub(crate) fn run(root: &Path, reader: &str, ids: &[String]) -> Result<String, String> {
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let first = ids.first().ok_or("name at least one source")?;
    let main = lock.get(first)?;
    let path_of = |id: &str| -> Result<std::path::PathBuf, String> {
        let p = lock.path(root, id)?;
        if p.is_file() {
            Ok(p)
        } else {
            Err(format!("{id} is not fetched yet: forge fetch {id}"))
        }
    };
    let t = Instant::now();
    let mut out: Output = match reader {
        "geonames" => {
            let extra = |prefix: &str| ids.iter().skip(1).find(|i| i.contains(prefix));
            let alt = extra("alternate").map(|i| path_of(i)).transpose()?;
            let hier = extra("hierarchy").map(|i| path_of(i)).transpose()?;
            geonames::read(&path_of(first)?, alt.as_deref(), hier.as_deref())?
        }
        "osm" => osm::read(&path_of(first)?)?,
        "wikidata" => wikidata::read(&path_of(first)?)?,
        r => return Err(format!("unknown reader `{r}` (geonames, osm, wikidata)")),
    };
    let dir = root.join("data/build/read").join(first);
    out.write(&dir, first, &main.licence_class)?;
    Ok(format!(
        "{first}: {} records, {} names, {} links in {:.1?} → {}",
        out.records.len(),
        out.names.len(),
        out.links.len(),
        t.elapsed(),
        dir.strip_prefix(root).unwrap_or(&dir).display()
    ))
}
