//! Architecture guard — lifecycle-aware post-cutover deferral scan.
//!
//! Scans `crates/*/src/**/*.rs` (production source only) for two kinds
//! of cutover-block deferral annotations:
//!
//! 1. `// active-block: <token>` — a comment marker noting that a
//!    site is in-flight for a specific cutover block.
//! 2. `// TODO(block-<N>)` — a follow-up reminder gated on a specific
//!    cutover block.
//!
//! The guard reads `.cutover-state` from the repo root to determine
//! the lifecycle policy:
//!
//! - If `.cutover-state` is ABSENT (post-Block-10 state — the
//!   cutover-state file is deleted when the last block lands), the
//!   guard fails on ANY active-block or `TODO(block-<N>)` token in
//!   production source. After all blocks land, no deferral
//!   annotations should remain.
//!
//! - If `.cutover-state` is PRESENT and `active_block` is non-empty,
//!   the guard only accepts annotations whose `<token>` equals the
//!   current `active_block`. Annotations for any OTHER block
//!   (including blocks already in `landed_blocks`) are violations.
//!   Per `landed_blocks`, those should have been removed in the
//!   commit that landed the block.
//!
//! - If `.cutover-state` is PRESENT but `active_block` is empty (the
//!   cutover-state xtask clears `active_block` immediately after a
//!   `land`, before dispatching the next block), the guard fails on
//!   any deferral annotation — no block is currently in flight, so
//!   any remaining annotation is dead text.
//!
//! Tests/benches/examples and `_tests.rs` siblings are exempt: a
//! deferral annotation in a test file may be a characterisation of
//! "this test must run when block-N lands" rather than dead text.
//!
//! Reference patterns: Block 0's `cutover_state_arch_guard.rs` for
//! the `.cutover-state` parse helpers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

/// `// active-block: <token>` — accept `active-block` or `active_block`.
/// Compiled once (hoisted out of the per-file `scan_source` loop).
static ACTIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"//\s*active[-_]block\s*:\s*([A-Za-z0-9._-]+)").unwrap());

/// `// TODO(block-<token>)` and `// TODO(block-<token>:...)`.
/// Compiled once (hoisted out of the per-file `scan_source` loop).
static TODO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"//\s*TODO\s*\(\s*block-([A-Za-z0-9._-]+)").unwrap());

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

#[derive(Debug)]
struct Hit {
    file: PathBuf,
    line: usize,
    kind: &'static str,
    token: String,
    source_line: String,
}

impl std::fmt::Display for Hit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: [{}] token=`{}`\n    {}",
            self.file.display(),
            self.line,
            self.kind,
            self.token,
            self.source_line.trim()
        )
    }
}

/// Extract the active_block string value from a `.cutover-state` body.
/// Mirrors `cutover_state_arch_guard::parse_active_block`.
fn parse_active_block(content: &str) -> Option<String> {
    content
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .find(|l| l.trim_start().starts_with("active_block"))
        .and_then(|l| l.split('=').nth(1))
        .map(|rhs| rhs.trim().trim_matches('"').trim_matches('\'').to_string())
}

/// Extract `landed_blocks = ["a", "b", ...]` as a Vec<String>.
/// Mirrors `cutover_state_arch_guard::parse_landed_blocks`. Handles
/// the multi-line form actually present at HEAD (TOML arrays may span
/// multiple lines).
fn parse_landed_blocks(content: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    let start_idx = lines
        .iter()
        .position(|l| l.trim_start().starts_with("landed_blocks"))?;
    let mut body = String::new();
    for line in lines.iter().skip(start_idx) {
        body.push_str(line);
        body.push('\n');
        if line.contains(']') {
            break;
        }
    }
    let after_eq = body.split('=').nth(1)?.trim();
    let inner = after_eq
        .trim_start_matches('[')
        .trim_end_matches(|c: char| c == ']' || c.is_whitespace() || c == '\n');
    let inner = inner.trim_end_matches(']');
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    Some(
        inner
            .split(',')
            .map(|s| {
                s.trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .trim()
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

/// Walk every `crates/*/src` directory (production source only).
fn production_src_dirs() -> Vec<PathBuf> {
    let crates_root = workspace_root().join("crates");
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&crates_root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let src = p.join("src");
            if src.is_dir() {
                out.push(src);
            }
        }
    }
    out
}

fn collect_production_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            collect_production_rs(&path, out);
        } else if path.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("rs")
            && !is_test_file(&path)
        {
            out.push(path);
        }
    }
}

fn is_test_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name == "tests.rs" || name.ends_with("_tests.rs") {
        return true;
    }
    path.components()
        .any(|c| c.as_os_str().to_str() == Some("tests"))
}

/// Scan a single file's source text for `active-block:` / `TODO(block-...)`
/// annotations and return matched `Hit`s.
fn scan_source(file: &Path, src: &str, hits: &mut Vec<Hit>) {
    let active_re = &*ACTIVE_RE;
    let todo_re = &*TODO_RE;

    for (line_idx, line) in src.lines().enumerate() {
        if let Some(cap) = active_re.captures(line) {
            hits.push(Hit {
                file: file.to_path_buf(),
                line: line_idx + 1,
                kind: "active-block",
                token: cap[1].to_string(),
                source_line: line.to_string(),
            });
        }
        if let Some(cap) = todo_re.captures(line) {
            hits.push(Hit {
                file: file.to_path_buf(),
                line: line_idx + 1,
                kind: "TODO(block-...)",
                token: cap[1].to_string(),
                source_line: line.to_string(),
            });
        }
    }
}

fn collect_all_production_hits() -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut files = Vec::new();
    for d in production_src_dirs() {
        collect_production_rs(&d, &mut files);
    }
    for f in files {
        let Ok(src) = std::fs::read_to_string(&f) else {
            continue;
        };
        // Necessary-condition pre-filter: an `// active-block:` /
        // `// active_block:` annotation always contains the substring
        // `active`, and a `// TODO(block-<N>)` annotation always contains
        // `block-`. A file with neither substring cannot match either
        // regex, so skip the (per-file) regex scan there. Both substrings
        // are strict prerequisites for the two annotation forms this
        // guard searches for, so filtering cannot hide a stale deferral.
        if !src.contains("active") && !src.contains("block-") {
            continue;
        }
        scan_source(&f, &src, &mut hits);
    }
    hits
}

fn format_hits(hits: &[Hit]) -> String {
    let mut by_file: BTreeMap<&Path, Vec<&Hit>> = BTreeMap::new();
    for h in hits {
        by_file.entry(h.file.as_path()).or_default().push(h);
    }
    let mut lines = Vec::new();
    for (file, fhs) in by_file {
        lines.push(format!("  {}", file.display()));
        for h in fhs {
            lines.push(format!(
                "    L{}: [{}] token=`{}` -- {}",
                h.line,
                h.kind,
                h.token,
                h.source_line.trim()
            ));
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Lifecycle-aware production-tree guard.
// ---------------------------------------------------------------------------

/// Lifecycle-aware deferral scan. Walks the entire `crates/*/src` tree
/// and applies one of three policies depending on `.cutover-state`:
///
/// - `.cutover-state` missing (post-Block-10 final state): zero
///   tolerance. Any `// active-block:` or `// TODO(block-<N>)` is a
///   violation.
/// - `active_block = "<X>"` (mid-cutover): only `<X>` tokens are
///   accepted. Tokens for landed blocks are dead text and flagged.
/// - `active_block = ""` (between blocks): zero tolerance — there's
///   no block in flight to justify the annotation.
#[test]
fn no_post_cutover_deferrals_in_production_source() {
    let state_path = workspace_root().join(".cutover-state");
    let state_content = std::fs::read_to_string(&state_path).ok();

    let (active, landed) = match &state_content {
        None => {
            // `.cutover-state` absent → final state, every block has
            // landed and the file was deleted. Zero tolerance.
            (None, Vec::new())
        }
        Some(content) => {
            let active = parse_active_block(content).filter(|s| !s.is_empty());
            let landed = parse_landed_blocks(content).unwrap_or_default();
            (active, landed)
        }
    };

    let all_hits = collect_all_production_hits();

    let mut violations = Vec::new();
    for h in all_hits {
        let accept = match &active {
            Some(token) => &h.token == token,
            None => false,
        };
        if accept {
            continue;
        }
        violations.push(h);
    }

    let policy_note = match (&state_content, &active) {
        (None, _) => "`.cutover-state` is absent (post-Block-10 final state). \
             No deferral annotations are permitted in production source."
            .to_string(),
        (Some(_), None) => "`.cutover-state` is present but `active_block` is empty. \
             No block is currently in flight; deferral annotations \
             must be resolved before the next `dispatch`."
            .to_string(),
        (Some(_), Some(active_block)) => format!(
            "`.cutover-state.active_block = \"{active_block}\"`. Only \
             `active-block: {active_block}` / `TODO(block-{active_block})` \
             tokens are permitted. Tokens for landed blocks (`{}`) or \
             unrelated blocks must be removed in the commit that lands \
             that block.",
            landed.join(", ")
        ),
    };

    assert!(
        violations.is_empty(),
        "Block 5 `no_post_cutover_deferrals` violation:\n\n{policy_note}\n\n\
         found {} stale deferral annotation(s):\n{}",
        violations.len(),
        format_hits(&violations)
    );
}

// ---------------------------------------------------------------------------
// Sentinel: `.cutover-state` parser self-test.
// ---------------------------------------------------------------------------

/// Confirm the parsing helpers match the on-disk shape. Without this,
/// the lifecycle-aware test could pass trivially if the parser
/// silently returned `None` for both keys.
#[test]
fn cutover_state_parser_reads_current_active_block() {
    let state_path = workspace_root().join(".cutover-state");
    let Ok(content) = std::fs::read_to_string(&state_path) else {
        // The file is absent — Block-10 post-cutover state. The
        // production-tree guard handles this correctly; nothing more
        // to verify here.
        return;
    };

    let active = parse_active_block(&content);
    assert!(
        active.is_some(),
        ".cutover-state must contain a parseable `active_block` line; \
         the deferral guard depends on this. Content:\n{content}"
    );

    let landed = parse_landed_blocks(&content);
    assert!(
        landed.is_some(),
        ".cutover-state must contain a parseable `landed_blocks` line; \
         the deferral guard depends on this. Content:\n{content}"
    );

    let landed = landed.unwrap();
    // The active block must not also appear in landed_blocks (mirrors
    // `cutover_state_arch_guard::active_block_and_landed_blocks_are_disjoint`).
    if let Some(active_str) = active.as_deref() {
        if !active_str.is_empty() {
            assert!(
                !landed.iter().any(|b| b == active_str),
                "active_block `{active_str}` must not appear in landed_blocks {landed:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Sentinel: scanner discriminating-property fixtures.
// ---------------------------------------------------------------------------

/// Confirm the scanner classifies fixtures correctly. The
/// production-tree guard could pass trivially if `scan_source` never
/// recognised any annotation.
#[test]
fn scanner_recognises_deferral_annotations() {
    // Fixture A: `// active-block: 5` — matched.
    let fixture_a = "fn foo() {\n    // active-block: 5\n}\n";
    let mut hits = Vec::new();
    scan_source(Path::new("<fixture>"), fixture_a, &mut hits);
    assert_eq!(hits.len(), 1, "scanner failed to match `active-block: 5`");
    assert_eq!(hits[0].token, "5");
    assert_eq!(hits[0].kind, "active-block");

    // Fixture B: `// TODO(block-6.B): ...` — matched.
    let fixture_b = "fn foo() {\n    // TODO(block-6.B): close after retire\n}\n";
    let mut hits = Vec::new();
    scan_source(Path::new("<fixture>"), fixture_b, &mut hits);
    assert_eq!(hits.len(), 1, "scanner failed to match `TODO(block-6.B)`");
    assert_eq!(hits[0].token, "6.B");
    assert_eq!(hits[0].kind, "TODO(block-...)");

    // Fixture C: `// active_block: 5` (underscore variant) — matched.
    let fixture_c = "fn foo() {\n    // active_block: 5\n}\n";
    let mut hits = Vec::new();
    scan_source(Path::new("<fixture>"), fixture_c, &mut hits);
    assert_eq!(
        hits.len(),
        1,
        "scanner failed to match underscore `active_block: 5` variant"
    );

    // Fixture D: a totally unrelated comment — NOT matched.
    let fixture_d = "fn foo() {\n    // active-helper: 5\n}\n";
    let mut hits = Vec::new();
    scan_source(Path::new("<fixture>"), fixture_d, &mut hits);
    assert!(
        hits.is_empty(),
        "scanner incorrectly matched a benign comment: {:?}",
        hits
    );

    // Fixture E: a doc comment without the marker — NOT matched.
    let fixture_e = "/// Block 5 cutover landed.\nfn foo() {}\n";
    let mut hits = Vec::new();
    scan_source(Path::new("<fixture>"), fixture_e, &mut hits);
    assert!(
        hits.is_empty(),
        "scanner incorrectly matched a doc comment without an active-block marker: {:?}",
        hits
    );

    // Fixture F: `// TODO(NOT-block-...)` — NOT matched.
    let fixture_f = "fn foo() {\n    // TODO(general): fix this\n}\n";
    let mut hits = Vec::new();
    scan_source(Path::new("<fixture>"), fixture_f, &mut hits);
    assert!(
        hits.is_empty(),
        "scanner incorrectly matched a generic TODO: {:?}",
        hits
    );
}

// ---------------------------------------------------------------------------
// Sentinel: `.cutover-state` multi-line landed_blocks parser.
// ---------------------------------------------------------------------------

/// The repo's `.cutover-state` uses a multi-line `landed_blocks` array.
/// This fixture pins the parser shape down so a single-line refactor
/// (or a multi-line refactor) does not silently break the deferral
/// guard.
#[test]
fn landed_blocks_parser_handles_multi_line_array() {
    let content = "# header\n\
                   active_block = \"5\"\n\
                   landed_blocks = [\n\
                   \"0\",\n\
                   \"1.6\",\n\
                   \"1.5\",\n\
                   ]\n";
    let active = parse_active_block(content);
    let landed = parse_landed_blocks(content);
    assert_eq!(active.as_deref(), Some("5"));
    assert_eq!(
        landed,
        Some(vec!["0".to_string(), "1.6".to_string(), "1.5".to_string()])
    );

    // Single-line form must also work.
    let content2 = "active_block = \"\"\nlanded_blocks = [\"0\", \"1.6\"]\n";
    let landed2 = parse_landed_blocks(content2);
    assert_eq!(landed2, Some(vec!["0".to_string(), "1.6".to_string()]));
}
