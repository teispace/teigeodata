//! Romanisation profiles, `data/romanise/<lang>-<Script>.yaml` (schema
//! `teigeodata/romanise/v1`, [note](../../docs/02-research/notes/names-canonical.md)).
//!
//! Each profile must parse, be named after its ID, compile and pass its own examples. It is then
//! compiled again with every country lexicon's accepted terms in its script, which is how the
//! forge uses it, so a lexicon term the engine refuses (an empty form, a canonical spelling that
//! is not ASCII) fails here and not in a build.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use teigeodata_names::romanise::ProfileFile;

use crate::lexicon;

/// The IDs of the profiles in `data/romanise/` (file names without `.yaml`).
pub(crate) fn ids(root: &Path) -> BTreeSet<String> {
    fs::read_dir(root.join("data/romanise"))
        .map(|d| {
            d.filter_map(Result::ok)
                .filter_map(|e| {
                    e.file_name()
                        .to_str()?
                        .strip_suffix(".yaml")
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Checks every profile against the lexicons `(country, text)`. Returns the number of profiles
/// and the problems found.
pub(crate) fn check(root: &Path, lexicons: &[(String, String)]) -> (usize, Vec<String>) {
    let dir = root.join("data/romanise");
    let Ok(entries) = fs::read_dir(&dir) else {
        return (0, Vec::new());
    };
    let mut files: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    files.sort();
    let mut problems = Vec::new();
    let mut n = 0;
    for path in files {
        let shown = format!(
            "data/romanise/{}",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
        );
        let Some(stem) = path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_suffix(".yaml"))
        else {
            problems.push(format!("{shown}: only .yaml files belong here"));
            continue;
        };
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                problems.push(format!("{shown}: {e}"));
                continue;
            }
        };
        let file: ProfileFile = match serde_saphyr::from_str(&text) {
            Ok(f) => f,
            Err(e) => {
                problems.push(format!("{shown}: not a valid profile: {e}"));
                continue;
            }
        };
        n += 1;
        if let Err(e) = file.compile(stem, &[]) {
            problems.push(format!("{shown}: {e}"));
            continue;
        }
        for (cc, lex) in lexicons {
            let morphemes = lexicon::morphemes(lex, &file.script);
            if let Err(e) = file.compile(stem, &morphemes) {
                problems.push(format!(
                    "{shown} with data/countries/{cc}/lexicon.yaml: {e}"
                ));
            }
        }
    }
    (n, problems)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn the_checked_in_profiles_pass() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let lex = std::fs::read_to_string(root.join("data/countries/np/lexicon.yaml"))
            .unwrap_or_default();
        let (n, problems) = super::check(&root, &[("np".into(), lex)]);
        assert!(n >= 1);
        assert_eq!(problems, Vec::<String>::new());
    }

    #[test]
    fn a_lexicon_term_the_engine_refuses_is_reported() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let lex = std::fs::read_to_string(root.join("data/countries/np/lexicon.yaml"))
            .unwrap_or_default()
            .replacen("canonical: \"Chowk\"", "canonical: \"Čowk\"", 1);
        let (_, problems) = super::check(&root, &[("np".into(), lex)]);
        assert!(
            problems.iter().any(|p| p.contains("not ASCII")),
            "{problems:?}"
        );
    }
}
