//! DTO-boundary architecture guard.
//!
//! `verter_compiler` must never depend on `verter_session`. The macro-surface
//! codegen DTOs live in the dependency-neutral `verter_macro_dto` leaf crate
//! (re-exported at `verter_compiler::compile`): the resolution/session side
//! *produces* that hand-off shape and the compiler *consumes* it, and the two
//! sides share ONLY the neutral leaf — so the compiler never needs (and must
//! never grow) a `verter_session` dependency to reach its macro surface. A
//! `verter_session` dependency here would invert the boundary and let
//! semantic/host concerns leak into the codegen crate.
//!
//! This guard parses `verter_compiler`'s own `Cargo.toml` and asserts no
//! dependency entry names `verter_session` in any dependency table
//! (`[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`, and their
//! `[target.*.…]` variants). It is discriminating: it fails the instant a
//! `verter_session` dependency line is added in any form
//! (`verter_session = ...` or `verter_session = { path = ... }`).

use std::path::PathBuf;

/// Path to `verter_compiler`'s `Cargo.toml`.
fn compiler_cargo_toml_path() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is the crate root (`crates/verter_compiler`) at test
    // build time — the manifest sits directly inside it.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

/// Collect the dependency *names* declared across every dependency table in a
/// `Cargo.toml`, using a minimal line scanner (no toml crate needed, and the
/// guard must not itself pull in a new dependency).
///
/// A dependency entry is a line of the form `name = <value>` that appears while
/// the scanner is inside a dependency table. We track the current table header
/// (`[...]`) and only collect entries while inside a table whose name ends in
/// `dependencies` (covers `[dependencies]`, `[dev-dependencies]`,
/// `[build-dependencies]`, and `[target.'cfg(...)'.dependencies]` etc.).
fn declared_dependency_names(cargo_toml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_dependency_table = false;

    for raw_line in cargo_toml.lines() {
        let line = raw_line.trim();

        // Table header: update the current-section flag.
        if line.starts_with('[') {
            // Strip the surrounding brackets (handles both `[deps]` and the
            // array-of-tables `[[deps]]` form defensively).
            let header = line.trim_start_matches('[').trim_end_matches(']').trim();
            in_dependency_table = header.ends_with("dependencies");
            continue;
        }

        if !in_dependency_table {
            continue;
        }

        // Skip comments and blank lines.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // A dependency entry is `name = <value>`. The name is everything before
        // the first `=`, trimmed (and stripped of optional surrounding quotes,
        // which Cargo allows for keys).
        if let Some((key, _value)) = line.split_once('=') {
            let name = key.trim().trim_matches('"').trim();
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }

    names
}

#[test]
fn verter_compiler_has_no_verter_session_dependency() {
    let path = compiler_cargo_toml_path();
    let cargo_toml = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    let names = declared_dependency_names(&cargo_toml);

    assert!(
        !names.iter().any(|n| n == "verter_session"),
        "DTO-boundary violation: `verter_compiler` declares a `verter_session` dependency in \
         {}. The compiler-owned macro-surface DTOs are produced by the session/host and consumed \
         by the compiler; the dependency arrow must point session → compiler, never the reverse. \
         Remove the `verter_session` dependency. Declared dependencies: {names:?}",
        path.display(),
    );
}

/// Sanity check that the line scanner actually recognises a `verter_session`
/// dependency when one is present — proves the guard above can fail (it is not
/// vacuously true because the parser never matches anything).
#[test]
fn dependency_scanner_detects_verter_session_when_present() {
    let synthetic = "\
[package]
name = \"x\"

[dependencies]
verter_audit = { path = \"../verter_audit\" }
verter_session = { path = \"../verter_session\" }

[dev-dependencies]
dhat = \"0.3.3\"
";
    let names = declared_dependency_names(synthetic);
    assert!(
        names.iter().any(|n| n == "verter_session"),
        "scanner failed to detect a present `verter_session` dependency — the guard would be \
         vacuous. Parsed names: {names:?}"
    );
    // And it must still see the legitimate deps around it.
    assert!(names.iter().any(|n| n == "verter_audit"));
    assert!(names.iter().any(|n| n == "dhat"));
}
