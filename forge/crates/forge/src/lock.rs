//! Reading `data/sources.lock`. `cargo xtask check-data` validates the schema; the forge reads
//! only what it needs and trusts nothing it downloads until the bytes match the pin.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The lock file.
#[derive(Debug, Deserialize)]
pub(crate) struct Lock {
    #[serde(default, rename = "source")]
    pub(crate) sources: Vec<Source>,
}

/// One pinned source.
#[derive(Debug, Deserialize)]
pub(crate) struct Source {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) mirror_url: Option<String>,
    #[serde(default)]
    pub(crate) moving: bool,
    pub(crate) sha256: String,
    pub(crate) size: u64,
    pub(crate) licence_class: String,
    /// The notice a pack built from it carries (D-126).
    pub(crate) attribution: String,
    #[serde(default)]
    pub(crate) query: Option<String>,
    #[serde(default)]
    pub(crate) query_params: BTreeMap<String, String>,
    /// `tileset` only: the manifest that pins each tile; `sha256` and `size` pin the manifest.
    #[serde(default)]
    pub(crate) tiles: Option<String>,
}

impl Lock {
    pub(crate) fn read(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub(crate) fn get(&self, id: &str) -> Result<&Source, String> {
        self.sources
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("no source `{id}` in data/sources.lock"))
    }

    /// Where source `id`'s pinned file is stored.
    pub(crate) fn path(&self, root: &Path, id: &str) -> Result<PathBuf, String> {
        self.get(id).map(|s| crate::fetch::store_path(root, s))
    }
}

/// Rewrites the pin (`sha256`, `size`, `retrieved`) of one source in the lock text, keeping
/// every comment and the layout. Returns `None` when the source block is not found.
pub(crate) fn repin(text: &str, id: &str, sha256: &str, size: u64, date: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    let mut found = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("[[source]]") {
            in_block = false;
        }
        if t == format!("id = \"{id}\"") {
            in_block = true;
            found = true;
        }
        let replaced = if in_block && t.starts_with("sha256 = ") {
            format!("sha256 = \"{sha256}\"")
        } else if in_block && t.starts_with("size = ") {
            format!("size = {size}")
        } else if in_block && t.starts_with("retrieved = ") {
            format!("retrieved = \"{date}\"")
        } else {
            line.to_string()
        };
        out.push(replaced);
    }
    found.then(|| {
        let mut s = out.join("\n");
        s.push('\n');
        s
    })
}

#[cfg(test)]
mod tests {
    use super::repin;

    const TEXT: &str = "# c\n[[source]]\nid = \"a\"\nsha256 = \"x\"\nsize = 1\nretrieved = \"2026-01-01\"\n\n[[source]]\nid = \"b\"\nsha256 = \"y\"\nsize = 2\nretrieved = \"2026-01-01\"\n";

    #[test]
    fn repins_only_the_named_source() {
        let out = repin(TEXT, "b", "zz", 9, "2026-11-06").unwrap_or_default();
        assert!(out.contains("id = \"a\"\nsha256 = \"x\"\nsize = 1"));
        assert!(out.contains("id = \"b\"\nsha256 = \"zz\"\nsize = 9\nretrieved = \"2026-11-06\""));
        assert!(out.starts_with("# c\n"));
        assert!(repin(TEXT, "c", "zz", 9, "2026-11-06").is_none());
    }
}
