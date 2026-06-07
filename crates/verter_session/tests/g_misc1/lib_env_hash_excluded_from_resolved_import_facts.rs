//! ARCH GUARD — `ResolvedImportFacts*` MUST NOT carry a
//! `lib_env_hash` field.
//!
//! Per R21 / the Cache Architecture rule: `ResolvedImportFacts` does NOT
//! include `lib_env_hash`; `RouteDb`, typed-IR resolve, `MaterializeStructureDb`,
//! `RefCycleResultDb`, `SemanticGraphStore`, `ComponentMetaResultDb` DO
//! include `lib_env_hash`. This guard locks the scoping decision so a
//! future refactor cannot silently add `lib_env_hash` to `ResolvedImportFactsKey`
//! and re-conflate the syntactic-resolution layer with the lib-data
//! domain.
//!
//! The guard scans the on-disk source of
//! `crates/verter_session/src/resolved_import_facts.rs` for every
//! `pub struct ResolvedImportFacts*` definition and asserts none of
//! them name a `lib_env_hash` field. The guard FAILS if a future
//! refactor introduces such a field; it PASSES against the current
//! tree.
//!
//! The guard is intentionally text-based (not `syn`-based) so it
//! catches the simplest plausible violation: a developer typing
//! `pub lib_env_hash: Hash16,` inside one of the struct definitions.
//! A token-based scanner is the right granularity — semantic
//! equivalence of "this struct depends on lib data" is the higher-
//! level architectural rule the cache substrate enforces elsewhere.

use std::fs;
use std::path::PathBuf;

const RESOLVED_IMPORT_FACTS_PATH: &str = "src/resolved_import_facts.rs";

fn read_resolved_import_facts_source() -> String {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = crate_root.join(RESOLVED_IMPORT_FACTS_PATH);
    fs::read_to_string(&target).unwrap_or_else(|err| {
        panic!(
            "arch guard MUST be able to read {} from the crate's `src/`: {err}",
            target.display(),
        )
    })
}

/// Walk struct definitions and yield each `pub struct ResolvedImportFacts*`
/// body as a `(name, body_text)` pair. Body is everything between the
/// opening `{` and the matching closing `}`.
fn extract_resolved_import_facts_struct_bodies(source: &str) -> Vec<(String, String)> {
    let mut bodies: Vec<(String, String)> = Vec::new();
    let mut cursor = 0usize;
    while let Some(struct_pos) = source[cursor..].find("pub struct ResolvedImportFacts") {
        let abs = cursor + struct_pos;
        let after = &source[abs..];
        // Capture the struct name up to whitespace, `<`, or `{`.
        let name_end = after
            .find(|c: char| c == '{' || c == '<' || c.is_whitespace())
            .unwrap_or(after.len());
        let header = &after[..name_end];
        let name = header.trim_start_matches("pub struct ").trim().to_string();

        let Some(open_rel) = after.find('{') else {
            break;
        };
        let open_abs = abs + open_rel;
        let body_start = open_abs + 1;
        let mut depth: i32 = 1;
        let bytes = source.as_bytes();
        let mut idx = body_start;
        while idx < bytes.len() && depth > 0 {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            idx += 1;
        }
        if depth == 0 {
            // `idx` now points one past the matching `}`.
            let body = &source[body_start..(idx - 1)];
            bodies.push((name, body.to_string()));
            cursor = idx;
        } else {
            break;
        }
    }
    bodies
}

#[test]
fn resolved_import_facts_structs_do_not_carry_lib_env_hash_field() {
    let source = read_resolved_import_facts_source();
    let bodies = extract_resolved_import_facts_struct_bodies(&source);
    assert!(
        !bodies.is_empty(),
        "fixture invariant: at least one `pub struct ResolvedImportFacts*` MUST exist; the \
         scanner found none — either the file moved or the naming convention drifted",
    );

    for (name, body) in &bodies {
        assert!(
            !body.contains("lib_env_hash"),
            "R21 scoping rule: `{name}` MUST NOT carry a `lib_env_hash` field. \
             ResolvedImportFacts captures base syntactic resolution; lib data lives in \
             `RouteDb`, typed-IR resolve, `MaterializeStructureDb`, etc. \
             Found the token `lib_env_hash` inside `{name}`'s struct body.",
        );
    }
}
