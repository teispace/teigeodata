//! Transliteration profiles, `data/xlit/<lang>-<source>-<Script>.yaml` (schema
//! `teigeodata/xlit/v1`, [note](../../docs/02-research/notes/nepali-names.md), D-222), and
//! their gates, `data/xlit/<id>.gate.yaml`.
//!
//! Each profile must parse, be named after its ID and compile, alone and with every country
//! lexicon's accepted terms in its script, as the forge uses it. Each gate must name an
//! existing profile. Its examples need a trained model, so `forge xlit-eval` checks them.

use std::fs;
use std::path::Path;

use serde::Deserialize;
use teigeodata_names::xlit::ProfileFile;

use crate::lexicon;

/// The part of a gate file checked here; the forge reads the rest.
#[derive(Deserialize)]
struct GateHead {
    schema: String,
    profile: String,
}

/// Checks every profile and gate against the lexicons `(country, text)`. Returns the number of
/// profiles and the problems found.
pub(crate) fn check(root: &Path, lexicons: &[(String, String)]) -> (usize, Vec<String>) {
    let dir = root.join("data/xlit");
    let Ok(entries) = fs::read_dir(&dir) else {
        return (0, Vec::new());
    };
    let mut files: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    files.sort();
    let mut problems = Vec::new();
    let mut n = 0;
    for path in &files {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let shown = format!("data/xlit/{name}");
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                problems.push(format!("{shown}: {e}"));
                continue;
            }
        };
        if let Some(stem) = name.strip_suffix(".gate.yaml") {
            match serde_saphyr::from_str::<GateHead>(&text) {
                Ok(g) if g.schema != "teigeodata/xlit-gate/v1" => {
                    problems.push(format!("{shown}: schema must be teigeodata/xlit-gate/v1"));
                }
                Ok(g) if g.profile != stem || !dir.join(format!("{stem}.yaml")).is_file() => {
                    problems.push(format!(
                        "{shown}: names profile {}, expected {stem}",
                        g.profile
                    ));
                }
                Ok(_) => {}
                Err(e) => problems.push(format!("{shown}: not a valid gate: {e}")),
            }
            continue;
        }
        let Some(stem) = name.strip_suffix(".yaml") else {
            problems.push(format!("{shown}: only .yaml files belong here"));
            continue;
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
            let terms = lexicon::xlit_terms(lex, &file.script);
            if let Err(e) = file.compile(stem, &terms) {
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
    fn a_lexicon_term_outside_the_script_is_reported() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let lex = std::fs::read_to_string(root.join("data/countries/np/lexicon.yaml"))
            .unwrap_or_default()
            .replacen("text: \"चोक\"", "text: \"চোক\"", 1);
        let (_, problems) = super::check(&root, &[("np".into(), lex)]);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("not in the target script")),
            "{problems:?}"
        );
    }
}
