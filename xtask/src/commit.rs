//! `cargo xtask check-commit <file>`: the commit-message rules of
//! `docs/09-guidelines/commits-and-branches.md`, run by the `commit-msg` hook.

use std::fs;
use std::path::Path;

const TYPES: [&str; 11] = [
    "feat", "fix", "perf", "refactor", "test", "docs", "build", "ci", "chore", "data", "revert",
];
const MAX_SUBJECT: usize = 72;

/// Phrases that attribute a commit to an AI tool (AGENTS.md §8). Matched case-insensitively;
/// `CLAUDE.md`, the agent-instructions file, is not an attribution.
const ATTRIBUTION: [&str; 7] = [
    "co-authored-by: claude",
    "generated with",
    "anthropic",
    "claude code",
    "ai-generated",
    "ai assistant",
    "noreply@anthropic.com",
];

/// Checks the commit message in `file` against the scopes in `<root>/.commit-scopes`.
pub(crate) fn check_file(root: &Path, file: &Path) -> Result<String, Vec<String>> {
    let message = fs::read_to_string(file).map_err(|e| vec![format!("{}: {e}", file.display())])?;
    let scopes = fs::read_to_string(root.join(".commit-scopes")).unwrap_or_default();
    check(&message, &scopes).map(|()| "commit message ok".into())
}

/// Checks one message. Git comment lines (`#`) are ignored, as git strips them.
pub(crate) fn check(message: &str, scopes: &str) -> Result<(), Vec<String>> {
    let lines: Vec<&str> = message.lines().filter(|l| !l.starts_with('#')).collect();
    let mut problems = Vec::new();
    let subject = lines.first().copied().unwrap_or_default();

    // Merges and reverts written by git keep their own form.
    if subject.starts_with("Merge ") {
        return Ok(());
    }

    match parse_subject(subject) {
        Ok((kind, scope, text)) => {
            if !TYPES.contains(&kind) {
                problems.push(format!(
                    "unknown type `{kind}` (one of: {})",
                    TYPES.join(", ")
                ));
            }
            if let Some(scope) = scope {
                let known = scopes
                    .lines()
                    .map(str::trim)
                    .any(|s| s == scope && !s.is_empty());
                if !known {
                    problems.push(format!("scope `{scope}` is not in .commit-scopes"));
                }
            }
            if text.chars().next().is_some_and(char::is_uppercase) {
                problems.push("subject must start lowercase".into());
            }
            if text.ends_with('.') {
                problems.push("subject must not end with a period".into());
            }
        }
        Err(e) => problems.push(e),
    }
    if subject.chars().count() > MAX_SUBJECT {
        problems.push(format!("subject is longer than {MAX_SUBJECT} characters"));
    }
    if lines.get(1).is_some_and(|l| !l.trim().is_empty()) {
        problems.push("leave a blank line after the subject".into());
    }
    if !lines.iter().any(|l| l.starts_with("Signed-off-by: ")) {
        problems.push("missing `Signed-off-by:` (commit with `git commit -s`, see DCO)".into());
    }
    let lowered = message
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase()
        .replace("claude.md", "");
    for phrase in ATTRIBUTION {
        if lowered.contains(phrase) {
            problems.push(format!(
                "AI attribution is not allowed (`{phrase}`), AGENTS.md §8"
            ));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Splits `type(scope)!: text` into its parts.
fn parse_subject(subject: &str) -> Result<(&str, Option<&str>, &str), String> {
    let format_hint = "subject must look like `type(scope): text` or `type: text`";
    let (head, text) = subject
        .split_once(": ")
        .ok_or_else(|| format_hint.to_string())?;
    let head = head.strip_suffix('!').unwrap_or(head);
    let (kind, scope) = match head.split_once('(') {
        Some((kind, rest)) => {
            let scope = rest
                .strip_suffix(')')
                .ok_or_else(|| format_hint.to_string())?;
            (kind, Some(scope))
        }
        None => (head, None),
    };
    if kind.is_empty() || text.trim().is_empty() {
        return Err(format_hint.into());
    }
    Ok((kind, scope, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCOPES: &str = "docs\nindex\n";
    const SIGN: &str = "\n\nSigned-off-by: A Person <a@example.com>\n";

    fn ok(msg: &str) -> bool {
        check(msg, SCOPES).is_ok()
    }

    #[test]
    fn a_good_message_passes() {
        assert!(ok(&format!("feat(index): add packed hilbert r-tree{SIGN}")));
        assert!(ok(&format!("docs: record the pack format{SIGN}")));
        assert!(ok(&format!("feat(index)!: change the leaf layout{SIGN}")));
        assert!(ok(&format!("docs: mention CLAUDE.md in the map{SIGN}")));
    }

    #[test]
    fn bad_messages_fail() {
        assert!(!ok(&format!("Add a thing{SIGN}")));
        assert!(!ok(&format!("wip(index): add a thing{SIGN}")));
        assert!(!ok(&format!("feat(nope): add a thing{SIGN}")));
        assert!(!ok(&format!("feat(index): Add a thing{SIGN}")));
        assert!(!ok(&format!("feat(index): add a thing.{SIGN}")));
        assert!(!ok("feat(index): add a thing\n"));
        assert!(!ok(&format!(
            "feat(index): add a thing\nbody right after{SIGN}"
        )));
        assert!(!ok(&format!("feat(index): {}{SIGN}", "x".repeat(80))));
    }

    #[test]
    fn ai_attribution_fails() {
        let msg = format!(
            "feat(index): add a thing{SIGN}Co-Authored-By: Claude <noreply@anthropic.com>\n"
        );
        let problems = check(&msg, SCOPES).unwrap_err();
        assert!(
            problems.iter().any(|p| p.contains("AI attribution")),
            "{problems:?}"
        );
        assert!(!ok(&format!("docs: x\n\nGenerated with a tool{SIGN}")));
    }

    #[test]
    fn git_comments_and_merges_are_ignored() {
        assert!(ok(&format!(
            "docs: x{SIGN}# Please enter the commit message\n"
        )));
        assert!(ok("Merge branch 'main' into feat/x\n"));
    }
}
