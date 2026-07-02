//! `check-four-mode-terminology` — verifies the workspace is free of the
//! retired `two-mode` terminology. Wired into CI through the thin wrapper
//! `tools/check-four-mode-terminology.sh`.
//!
//! Patterns rejected:
//!   1. `\btwo[-_ ]?modes?\b` (case-insensitive)
//!   2. `\b(Type|Expanded)\s+modes?\b`
//!   3. `"(Type|Expanded) (mode|MODE)"` (string literal form)
//!   4. `\bTypeMode\b`
//!
//! Backticked spans (`` `...` ``) are stripped from each line before the
//! retired regexes apply, so prose written as `` `ResolverMode::Type` `` does
//! NOT fire.
//!
//! Allowlist (whole-line scan; if it appears on the RAW line, the line is
//! exempt): `ProjectionMode::{Identity,Navigate,Shallow,Expanded}`.
//!
//! Adding a bare prose phrase to the allowlist is forbidden — rewrite the
//! prose to backticked code form instead.
//!
//! Scans every tracked file whose extension is in the include set, minus
//! the exclude lists below. The scan is cwd-independent: the repo root is
//! resolved via `git rev-parse --show-toplevel` and files are enumerated
//! root-relative (`git -C <root> ls-files -z --full-name`), so invoking
//! the bin from any subdirectory still scans the WHOLE tree. Prints each
//! offending root-relative `file:line: content` triple to stdout and exits
//! 1 when any are found; exits 0 on a clean tree.

use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use regex::Regex;

const INCLUDE_EXT: &[&str] = &[
    ".rs", ".ts", ".tsx", ".js", ".mjs", ".cjs", ".md", ".toml", ".yml", ".yaml", ".json",
];

const EXCLUDE_FULL_PATHS: &[&str] = &["tools/check-four-mode-terminology.sh", "tmp-plan.md"];

const EXCLUDE_PATH_PARTS: &[&str] = &[
    "test-results/",
    "playwright-report/",
    "/snapshots/",
    "test-output/",
];

const EXCLUDE_SUFFIXES: &[&str] = &[".pb.ts", "_pb.ts", ".snap", ".lock", "pnpm-lock.yaml"];

/// The compiled scan rule set: the whole-line allowlist, the backtick-span
/// stripper, and the four retired-terminology patterns.
struct Rules {
    allowed: Regex,
    backtick_span: Regex,
    retired: Vec<Regex>,
}

impl Rules {
    fn new() -> Self {
        Self {
            allowed: Regex::new(r"ProjectionMode::(?:Identity|Navigate|Shallow|Expanded)")
                .expect("allowed regex"),
            backtick_span: Regex::new(r"`[^`]*`").expect("backtick regex"),
            retired: vec![
                Regex::new(r"(?i)\btwo[-_ ]?modes?\b").expect("retired regex 1"),
                Regex::new(r"\b(?:Type|Expanded)\s+modes?\b").expect("retired regex 2"),
                Regex::new(r#""(?:Type|Expanded) (?:mode|MODE)""#).expect("retired regex 3"),
                Regex::new(r"\bTypeMode\b").expect("retired regex 4"),
            ],
        }
    }
}

/// Is this tracked path in scope for the terminology scan?
fn is_scanned_path(path: &str) -> bool {
    if EXCLUDE_FULL_PATHS.contains(&path) {
        return false;
    }
    if EXCLUDE_SUFFIXES.iter().any(|s| path.ends_with(s)) {
        return false;
    }
    if EXCLUDE_PATH_PARTS.iter().any(|p| path.contains(p)) {
        return false;
    }
    INCLUDE_EXT.contains(&file_ext_lower(path).as_str())
}

/// Lowercased extension of the path's final component, INCLUDING the dot
/// (empty when the component has no extension; leading dots are not
/// extension separators, so `.bashrc` has no extension).
fn file_ext_lower(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    let trimmed = base.trim_start_matches('.');
    match trimmed.rfind('.') {
        Some(i) => trimmed[i..].to_ascii_lowercase(),
        None => String::new(),
    }
}

/// Scan one file's content. Returns `(line_number, line)` for every line
/// carrying retired terminology: allowlisted lines are exempt wholesale,
/// backticked spans are stripped before the retired patterns apply.
fn scan_content(content: &str, rules: &Rules) -> Vec<(usize, String)> {
    let mut bad = Vec::new();
    for (idx, raw) in content.split('\n').enumerate() {
        let raw = raw.trim_end_matches('\r');
        if rules.allowed.is_match(raw) {
            continue;
        }
        let stripped = rules.backtick_span.replace_all(raw, "");
        if rules.retired.iter().any(|rx| rx.is_match(&stripped)) {
            bad.push((idx + 1, raw.to_string()));
        }
    }
    bad
}

/// Repository root resolved from `dir` via `git rev-parse --show-toplevel`,
/// so the scan covers the whole tree no matter which subdirectory the bin
/// is invoked from.
fn repo_root_from(dir: &Path) -> PathBuf {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| {
            eprintln!("ERROR: failed to run `git rev-parse --show-toplevel`: {e}");
            exit(2)
        });
    if !output.status.success() {
        eprintln!(
            "ERROR: `git rev-parse --show-toplevel` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        exit(2);
    }
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
}

/// Every tracked path, root-relative, via
/// `git -C <root> ls-files -z --full-name` (NUL-separated so no path byte
/// can split an entry; `--full-name` keeps entries root-relative even when
/// the enumeration runs from a subdirectory).
fn tracked_files(root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--full-name"])
        .output()
        .unwrap_or_else(|e| {
            eprintln!("ERROR: failed to run `git ls-files`: {e}");
            exit(2)
        });
    if !output.status.success() {
        eprintln!(
            "ERROR: `git ls-files` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        exit(2);
    }
    output
        .stdout
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect()
}

fn main() {
    let rules = Rules::new();
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        eprintln!("ERROR: failed to resolve the current directory: {e}");
        exit(2)
    });
    let root = repo_root_from(&cwd);

    let mut bad_lines: Vec<(String, usize, String)> = Vec::new();
    for file in tracked_files(&root) {
        if !is_scanned_path(&file) {
            continue;
        }
        // Skip unreadable or non-UTF-8 files, mirroring the scan's
        // decode-or-skip file policy. Reads resolve against the repo root;
        // findings keep the root-relative path.
        let Ok(bytes) = std::fs::read(root.join(&file)) else {
            continue;
        };
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        for (line_no, line) in scan_content(&content, &rules) {
            bad_lines.push((file.clone(), line_no, line));
        }
    }

    for (file, line_no, line) in &bad_lines {
        println!("{file}:{line_no}: {line}");
    }
    if !bad_lines.is_empty() {
        eprintln!();
        eprintln!(
            "Retired terminology found in {} location(s).",
            bad_lines.len()
        );
        eprintln!("Allowlist (always): ProjectionMode::{{Identity,Navigate,Shallow,Expanded}}.");
        eprintln!("No transitional allowlist entries remain (E1 retired ResolverMode).");
        eprintln!(
            "Backticked spans (`...`) are stripped before regex application; \
             rewrite prose to code-form instead of allowlisting prose."
        );
        exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Known-bad inputs are built by RUNTIME concatenation so this test file
    // never contains a contiguous retired term — the live repo scan covers
    // this tracked source file too.
    fn bad_type_mode_ident() -> String {
        ["Type", "Mode"].concat()
    }

    fn scan(line: &str) -> usize {
        scan_content(line, &Rules::new()).len()
    }

    #[test]
    fn flags_retired_type_mode_identifier() {
        let line = format!("let x = {};", bad_type_mode_ident());
        assert_eq!(scan(&line), 1, "bare retired identifier must be flagged");
    }

    #[test]
    fn flags_two_modes_prose_case_insensitively() {
        for sep in ["-", "_", " ", ""] {
            let line = format!("we keep {}{}{} here", "Two", sep, "modes");
            assert_eq!(scan(&line), 1, "sep {sep:?} variant must be flagged");
        }
        let line = format!("ONLY {} {} REMAIN", "TWO", "MODES");
        assert_eq!(scan(&line), 1, "all-caps variant must be flagged");
    }

    #[test]
    fn flags_spaced_mode_prose_and_quoted_literal_form() {
        let spaced = format!("uses {} {} now", "Expanded", "mode");
        assert_eq!(scan(&spaced), 1, "spaced prose form must be flagged");
        let quoted = format!("\"{} {}\"", "Type", "MODE");
        assert_eq!(scan(&quoted), 1, "quoted literal form must be flagged");
    }

    #[test]
    fn backticked_spans_are_exempt() {
        let line = format!("prose about `{}` stays fine", bad_type_mode_ident());
        assert_eq!(scan(&line), 0, "backticked code span must be stripped");
    }

    #[test]
    fn allowlisted_projection_mode_line_is_exempt_wholesale() {
        let line = format!(
            "ProjectionMode::Expanded replaced {}",
            bad_type_mode_ident()
        );
        assert_eq!(scan(&line), 0, "allowlisted line must be exempt wholesale");
    }

    #[test]
    fn clean_lines_and_line_numbers() {
        assert_eq!(scan("ProjectionMode::Shallow is one of four modes"), 0);
        assert_eq!(scan("fn resolve() {}"), 0);
        let content = format!("ok\n{}\nok", bad_type_mode_ident());
        let findings = scan_content(&content, &Rules::new());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].0, 2, "line numbers are 1-based");
    }

    #[test]
    fn subdir_invocation_scans_the_whole_tree() {
        // `xtask/` is a SUBDIR of the repo root: resolving the root from it
        // must land on the same root as resolving from the root itself, and
        // the enumeration from that root is full-tree and root-relative
        // (`--full-name`), independent of the invocation directory.
        let subdir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = repo_root_from(&subdir);
        assert_ne!(
            root, subdir,
            "xtask/ must resolve to its PARENT repo root, not itself"
        );
        assert_eq!(
            root,
            repo_root_from(&root),
            "root resolution must be invocation-dir independent"
        );
        let files = tracked_files(&root);
        assert!(
            files
                .iter()
                .any(|f| f == "xtask/src/bin/check_four_mode_terminology.rs"),
            "tracked enumeration must be root-relative (xtask-prefixed), not cwd-relative"
        );
        assert!(
            files
                .iter()
                .any(|f| f == "crates/verter_session/Cargo.toml"),
            "tracked enumeration must cover files OUTSIDE the invocation subdir"
        );
        assert!(
            !files.iter().any(|f| f.starts_with("src/")),
            "no entry may be relative to the xtask subdir itself"
        );
    }

    #[test]
    fn path_filter_matches_include_and_exclude_lists() {
        assert!(is_scanned_path("crates/verter_session/src/lib.rs"));
        assert!(is_scanned_path("docs/arch/fact-based-cache.md"));
        assert!(is_scanned_path("package.json"));
        assert!(!is_scanned_path("scripts/foo.py"), "extension not included");
        assert!(!is_scanned_path("tools/check-four-mode-terminology.sh"));
        assert!(
            !is_scanned_path("pnpm-lock.yaml"),
            "lockfile suffix excluded"
        );
        assert!(!is_scanned_path("packages/x/test-results/report.md"));
        assert!(!is_scanned_path("proto/gen/typeinfo.pb.ts"));
        assert!(!is_scanned_path("fixtures/a.snap"));
        assert!(!is_scanned_path(".bashrc"), "no extension");
    }
}
