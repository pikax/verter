//! Guard: `provider_op_requires_resolved_project`.
//!
//! The external-TS contract makes a config-less / inferred-project operation for
//! a production carrier source UNREPRESENTABLE. The compiler enforces this via
//! the witness type-state (a production op takes a `BoundProject`, obtainable
//! only from a resolved `ProjectBinding`). This STATIC guard is the source-level
//! backstop: it walks the external-TS contract module and FAILS if a path-only
//! external-TS entry point reappears — a `fn` that takes a bare uri/path and
//! produces an external-TS result WITHOUT routing through a `ProjectBinding`
//! (e.g. `pub fn query_by_path(uri: &str)` / `fn open_tsx(` / `fn update_file(`).
//!
//! DISCRIMINATING: on a tree where someone adds `pub fn query_by_path(uri: &str)`
//! to the contract the scanner fires (proven by the self-test below); on the
//! type-state-only tree it is clean.

use std::fs;
use std::path::{Path, PathBuf};

/// Repo root (two parents up from `crates/verter_session`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Walk every `.rs` file rooted at `path` (recursively), collecting paths.
fn walk_rs_files(path: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(path) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// The forbidden path-only external-TS op-name fragments. A production external-
/// TS result producer keyed on a bare uri/path (not a `BoundProject`) is exactly
/// the inferred-fallthrough shape this contract bans.
const FORBIDDEN_OP_NAME_FRAGMENTS: &[&str] = &[
    "open_tsx",
    "update_file",
    "query_by_path",
    "query_by_uri",
    "query_uri",
    "diagnostics_by_path",
    "diagnostics_by_uri",
    "diagnostics_for_uri",
    "diagnostics_for_path",
    "publish_by_path",
];

/// Does a trimmed source line declare a `fn` whose name matches a forbidden
/// path-only external-TS op? Recognises `fn`, `pub fn`, `pub(...) fn`, `async fn`.
fn line_declares_forbidden_path_only_op(line: &str) -> bool {
    let trimmed = line.trim_start();
    // Strip leading `pub`/visibility/`async` qualifiers to reach `fn <name>`.
    let mut rest = trimmed;
    for prefix in ["pub(crate) ", "pub(super) ", "pub ", "async ", "const "] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    let Some(after_fn) = rest.strip_prefix("fn ") else {
        return false;
    };
    let name_end = after_fn
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after_fn.len());
    let name = &after_fn[..name_end];
    FORBIDDEN_OP_NAME_FRAGMENTS
        .iter()
        .any(|frag| name.contains(frag))
}

/// The contract module directory.
fn external_ts_dir() -> PathBuf {
    workspace_root().join("crates/verter_session/src/external_ts")
}

#[test]
fn provider_op_requires_resolved_project() {
    let dir = external_ts_dir();
    assert!(
        dir.is_dir(),
        "the external-TS contract module must exist at {}",
        dir.display()
    );

    let mut files = Vec::new();
    walk_rs_files(&dir, &mut files);
    // Exclude the test module: the scanner targets the production contract
    // surface (a path-only op in tests is harmless).
    files.retain(|f| {
        f.file_name()
            .and_then(|n| n.to_str())
            .map(|n| !n.ends_with("_tests.rs"))
            .unwrap_or(true)
    });
    assert!(
        !files.is_empty(),
        "the external-TS contract module must contain production source"
    );

    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        let src = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for (i, line) in src.lines().enumerate() {
            if line_declares_forbidden_path_only_op(line) {
                violations.push(format!(
                    "{}:{}: {}",
                    file.strip_prefix(workspace_root())
                        .unwrap_or(file)
                        .display(),
                    i + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "provider_op_requires_resolved_project — a PATH-ONLY external-TS entry \
         point was found in the contract module. Every external-TS result \
         producer (`publish_snapshot`/`query`/`diagnostics`) MUST route through a \
         `BoundProject` witness obtainable only from a resolved `ProjectBinding` \
         — a bare-uri/path op (e.g. `query_by_path(uri: &str)`) is the inferred- \
         fallthrough shape the contract bans. Offending declarations:\n  {}",
        violations.join("\n  ")
    );
}

/// Self-discriminator: the predicate MUST fire on the injected bad signature and
/// MUST NOT fire on the contract's legitimate ops. Without this the scanner could
/// silently pass a permissive implementation.
#[test]
fn provider_op_scanner_discriminates() {
    // Forbidden path-only ops are detected, regardless of visibility / async.
    assert!(line_declares_forbidden_path_only_op(
        "    pub fn query_by_path(uri: &str) -> QueryOutcome {"
    ));
    assert!(line_declares_forbidden_path_only_op(
        "pub fn open_tsx(path: &str) {"
    ));
    assert!(line_declares_forbidden_path_only_op(
        "    async fn update_file(uri: &str, content: &str) {"
    ));
    assert!(line_declares_forbidden_path_only_op(
        "fn diagnostics_for_uri(uri: &str) {"
    ));

    // The contract's real, witness-gated ops are NOT flagged.
    assert!(!line_declares_forbidden_path_only_op(
        "    fn query(&self, project: &BoundProject, query: Query) -> Result<QueryOutcome, EngineError>;"
    ));
    assert!(!line_declares_forbidden_path_only_op(
        "    fn resolve(&self, source_uri: &str) -> CarrierOwnershipResolution;"
    ));
    assert!(!line_declares_forbidden_path_only_op(
        "    fn ensure_project(&self, request: EnsureProject) -> Result<BoundProject, EngineError>;"
    ));
    // A non-fn line is never a declaration.
    assert!(!line_declares_forbidden_path_only_op(
        "// query_by_path is the forbidden shape this guard bans"
    ));
}
