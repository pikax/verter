//! Guard: `no_inferred_project_knobs_on_tsserver`.
//!
//! The tsserver engine is project-bound: a framework carrier is a member of its
//! REAL configured project (the `@verter/typescript-plugin` makes it one via
//! `getExternalFiles` + `extraFileExtensions`), so the tsserver transport injects
//! NO inferred-project compiler options — there is no config-less inferred carrier
//! to configure. This STATIC guard is the source-level backstop for the
//! inferred-path deletion: it walks the tsserver transport PRODUCTION source
//! (excluding `*_tests.rs`) and FAILS if any inferred-project knob reappears:
//!
//! 1. a `compilerOptionsForInferredProjects` request (the inferred-project
//!    compiler-options injection), AND
//! 2. a `TsserverTypeProvider::configure_paths` impl (the inferred `paths`/
//!    `baseUrl` injection method — its trait DEFAULT no-op stays; only the
//!    tsserver-specific override is forbidden), AND
//! 3. a synthetic inferred-project carrier construction (a rootUri-only /
//!    `inferredProjectCompilerOptions` synthetic open).
//!
//! DISCRIMINATING: the self-test below proves the scanner FIRES when the inferred
//! knob string is present (i.e. it would have failed PRE-deletion) and is CLEAN on
//! the post-deletion tree.

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

/// Walk every `.rs` file rooted at `path` (recursively).
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

/// The tsserver transport PRODUCTION source files (excludes the `*_tests.rs`
/// siblings, which legitimately ASSERT the absence of the inferred knob).
fn tsserver_production_sources() -> Vec<PathBuf> {
    let dir = workspace_root()
        .join("crates")
        .join("verter_type_runtime")
        .join("src")
        .join("tsserver");
    let mut files = Vec::new();
    walk_rs_files(&dir, &mut files);
    files
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !n.ends_with("_tests.rs"))
                .unwrap_or(true)
        })
        .collect()
}

/// The forbidden inferred-project knob fragments. Each is a string that exists
/// ONLY in the deleted inferred-project construction path.
const FORBIDDEN_INFERRED_KNOBS: &[&str] = &[
    // The inferred-project compiler-options injection request.
    "compilerOptionsForInferredProjects",
    // The synthetic inferred-project options key (the alternate spelling tsserver
    // accepts for the same construction).
    "inferredProjectCompilerOptions",
];

#[test]
fn no_inferred_project_knobs_on_tsserver() {
    let mut violations: Vec<String> = Vec::new();

    for file in tsserver_production_sources() {
        let content = match fs::read_to_string(&file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = file
            .strip_prefix(workspace_root())
            .unwrap_or(&file)
            .display()
            .to_string();

        for (lineno, line) in content.lines().enumerate() {
            // Skip comment lines: a doc-comment explaining WHY the knob is gone is
            // allowed (final-state prose), but a live request/impl is not.
            let trimmed = line.trim_start();
            let is_comment = trimmed.starts_with("//") || trimmed.starts_with("/*");

            for knob in FORBIDDEN_INFERRED_KNOBS {
                if line.contains(knob) && !is_comment {
                    violations.push(format!("{rel}:{}: `{knob}`: {}", lineno + 1, line.trim()));
                }
            }

            // A `TsserverTypeProvider::configure_paths` IMPL is forbidden (the
            // trait default no-op stays; only the tsserver override is the
            // inferred `paths`/`baseUrl` injection). A `fn configure_paths(` in the
            // tsserver production source IS that override.
            if trimmed.starts_with("fn configure_paths") && !is_comment {
                violations.push(format!(
                    "{rel}:{}: tsserver must NOT define a `configure_paths` impl \
                     (the inferred paths/baseUrl injection) — it inherits the trait \
                     default no-op: {}",
                    lineno + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "tsserver inferred-project knobs found (the project-bound contract forbids them — \
         the carrier is a real configured-project member via the plugin):\n{}",
        violations.join("\n")
    );
}

/// DISCRIMINATING self-test: the scanner's predicate FIRES on a line that
/// contains an inferred-project knob and is NOT a comment, and stays clean on a
/// comment / a benign line. This proves the guard would have FAILED on the
/// pre-deletion tree (which contained a live `compilerOptionsForInferredProjects`
/// request) and is non-vacuous.
#[test]
fn no_inferred_project_knobs_self_test_discriminates() {
    let fire = |line: &str| -> bool {
        let trimmed = line.trim_start();
        let is_comment = trimmed.starts_with("//") || trimmed.starts_with("/*");
        FORBIDDEN_INFERRED_KNOBS
            .iter()
            .any(|knob| line.contains(knob) && !is_comment)
            || (trimmed.starts_with("fn configure_paths") && !is_comment)
    };

    // A live inferred-knob request line FIRES (this is exactly the deleted line).
    assert!(
        fire("            .request(\"compilerOptionsForInferredProjects\", json!({}))"),
        "a live compilerOptionsForInferredProjects request must trip the guard"
    );
    // A tsserver configure_paths impl FIRES.
    assert!(
        fire("    fn configure_paths(&self, base_url: &str, paths: Value) -> Fut {"),
        "a tsserver configure_paths impl must trip the guard"
    );
    // A doc-comment explaining the knob is GONE is allowed (final-state prose).
    assert!(
        !fire("    // The session injects NO compilerOptionsForInferredProjects."),
        "a comment must NOT trip the guard"
    );
    // A benign unrelated line is clean.
    assert!(
        !fire("        transport.request(\"configure\", json!({}))"),
        "the `configure` handshake (not the inferred-options request) must be clean"
    );
}
