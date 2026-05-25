//! Architecture guard for `MemberEdgeProvenance`.
//!
//! Every production emit site that records a `ProjectMember` edge
//! **must** construct an `OriginMeta::ProjectedMember { name,
//! provenance: MemberEdgeProvenance::X }` with an explicit provenance
//! variant. The exhaustive-match contract in the audit bridge
//! (`verter_session::component_meta_audit::footprint_miner::translate_meta`)
//! panics if a `ProjectMember` edge is emitted through any other
//! `OriginMeta` variant; the audit-validator's Rule-5 compliance
//! check (see
//! `packages/benchmark/audit-validator.ts::validateRule5Compliance`)
//! depends on the provenance being preserved through the bridge.
//!
//! This guard is the static check that backs the runtime contract:
//! it walks every `.rs` file under `crates/*/` and asserts:
//!
//! 1. No production file references the retired
//!    `OriginMeta::MemberName(...)` variant.
//! 2. No construction of `OriginMeta::ProjectedMember { ... }` uses
//!    `..Default::default()` to elide the `provenance:` field.
//! 3. Every construction of `OriginEdgeMetaDto::ProjectMember { ... }`
//!    names an explicit `provenance:` field.

use std::fs;
use std::path::{Path, PathBuf};

fn walk_rust(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rust(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolvable from CARGO_MANIFEST_DIR")
        .to_path_buf()
}

fn rel(path: &Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// True for lines that are pattern matches rather than struct
/// constructions. Patterns appear after `if let`, `let Some(...) =`,
/// `match X { ... =>`, `matches!`, or contain a `&edge.meta` reference
/// to the matched value.
fn looks_like_pattern_line(line: &str) -> bool {
    let l = line.trim_start();
    l.starts_with("if let ")
        || l.starts_with("else if let ")
        || l.starts_with("while let ")
        || line.contains("matches!(")
        || line.contains("=> ")
        || line.contains("&edge.meta")
        || line.contains("&e.meta")
        // Pattern destructure inside a `match X { ` arm: line starts
        // with `Variant {` (no leading let/assignment).
        || (l.starts_with("OriginEdgeMetaDto::ProjectMember {") && !line.contains("meta:"))
}

/// True for paths that are production source files (under `src/`,
/// not `tests/`, not `examples/`, not `benches/`, not `bin/`, and
/// not a test-named file like `*_tests.rs` or `tests.rs`).
fn is_production_source(path: &Path) -> bool {
    let s = rel(path);
    if !s.contains("/src/") {
        return false;
    }
    if s.contains("/tests/") || s.contains("/examples/") || s.contains("/benches/") {
        return false;
    }
    if s.contains("/bin/") {
        return false;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if stem == "tests" || stem.ends_with("_tests") || stem.ends_with("_test") {
        return false;
    }
    true
}

/// Files that DEFINE the substrate (enum/DTO/match-arms over all
/// kinds) — they reference `OriginMeta::MemberName` historically and
/// `OriginEdgeKind::ProjectMember` as part of the type system, not as
/// emit sites. Allowlisted for guard #1.
const SUBSTRATE_FILES: &[&str] = &[
    "crates/verter_audit/src/origin_graph.rs",
    "crates/verter_session/src/semantic_query.rs",
    "crates/verter_session/src/component_meta_audit/footprint_miner.rs",
    "crates/verter_session/src/component_meta_audit/accumulator.rs",
    "crates/verter_session/src/component_meta_audit/mod.rs",
    "crates/verter_session/src/semantic_query_memo/mod.rs",
    "crates/verter_session/src/semantic_query_memo/family.rs",
    "crates/verter_session/src/semantic_query_memo/derivation.rs",
    "crates/verter_session/src/capture_token.rs",
    "crates/verter_session/src/loop5_instrumentation.rs",
    "crates/verter_session/src/host_test_audit.rs",
    "crates/verter_session/src/host_resolve_type_audit.rs",
    "crates/verter_session/src/project_semantic_dispatch/mod.rs",
    "crates/verter_session/src/project_semantic_dispatch/raise.rs",
];

#[test]
fn retired_origin_meta_member_name_variant_is_not_referenced_in_production() {
    let root = workspace_root().join("crates");
    let mut files = Vec::new();
    walk_rust(&root, &mut files);

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        if !is_production_source(file) {
            continue;
        }
        let r = rel(file);
        if SUBSTRATE_FILES.iter().any(|s| r == *s) {
            continue;
        }
        let body = match fs::read_to_string(file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if body.contains("OriginMeta::MemberName") {
            violations.push(format!(
                "{r}: references retired `OriginMeta::MemberName` — every \
                 ProjectMember producer must construct \
                 `OriginMeta::ProjectedMember {{ name, provenance }}`."
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "MemberEdgeProvenance architecture guard violations:\n  - {}",
        violations.join("\n  - ")
    );
}

#[test]
fn projected_member_construction_never_uses_default_rest_spread() {
    let root = workspace_root().join("crates");
    let mut files = Vec::new();
    walk_rust(&root, &mut files);

    let mut violations: Vec<String> = Vec::new();
    let self_path = rel(Path::new(file!()));

    for file in &files {
        let r = rel(file);
        // Skip the guard test file itself (it contains forbidden-pattern
        // descriptions as strings).
        if r.ends_with(&self_path) || r.contains("member_edge_provenance_arch_guard.rs") {
            continue;
        }
        let body = match fs::read_to_string(file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        for (idx, line) in body.lines().enumerate() {
            if !line.contains("OriginMeta::ProjectedMember") {
                continue;
            }
            // The only forbidden construction shape is rest-spread of
            // a default: `OriginMeta::ProjectedMember { ..Default::default() }`.
            // Block any line that contains both the variant and a
            // `..Default::default()` clause.
            if line.contains("..Default::default()") {
                violations.push(format!(
                    "{r}:{}: `OriginMeta::ProjectedMember {{ ..Default::default() }}` is \
                     forbidden — name an explicit `provenance:` value.",
                    idx + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "ProjectedMember construction guard violations:\n  - {}",
        violations.join("\n  - ")
    );
}

#[test]
fn audit_dto_project_member_construction_carries_provenance() {
    let root = workspace_root().join("crates");
    let mut files = Vec::new();
    walk_rust(&root, &mut files);

    let mut violations: Vec<String> = Vec::new();
    let self_marker = "member_edge_provenance_arch_guard.rs";

    for file in &files {
        let r = rel(file);
        if r.contains(self_marker) {
            continue;
        }
        let body = match fs::read_to_string(file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let lines: Vec<&str> = body.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            if !line.contains("OriginEdgeMetaDto::ProjectMember {") {
                continue;
            }
            // Skip pattern-match lines.
            if looks_like_pattern_line(line) {
                continue;
            }
            // The line is a construction. Verify a `provenance:` field
            // appears within the next 8 lines (block scope ends with a
            // bare `}` line; rare to extend beyond 8).
            let mut found_provenance = false;
            let mut closed = false;
            for l in lines
                .iter()
                .take((idx + 10).min(lines.len()))
                .skip(idx + 1)
                .map(|s| s.trim())
            {
                if l.starts_with("provenance:") || l.starts_with("provenance ") {
                    found_provenance = true;
                    break;
                }
                // A closing brace at column 0 (or just `}` on its own)
                // signals the construction block ended without a
                // provenance: line — but only count it as closure if
                // the brace is balanced (no `,` continuation).
                if l == "}" || l == "}," {
                    closed = true;
                    break;
                }
            }
            if !found_provenance && closed {
                violations.push(format!(
                    "{r}:{}: `OriginEdgeMetaDto::ProjectMember {{ … }}` construction \
                     without explicit `provenance:` — every audit-DTO ProjectMember \
                     edge must carry a `MemberEdgeProvenance` variant.",
                    idx + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "OriginEdgeMetaDto::ProjectMember construction guard violations:\n  - {}",
        violations.join("\n  - ")
    );
}

#[test]
fn member_edge_provenance_enum_has_all_known_variants() {
    // Sanity guard: the enum's discriminator variants are stable. If
    // someone removes or renames a variant, this test catches it.
    use verter_audit::MemberEdgeProvenance;
    let _published = MemberEdgeProvenance::PublishedField;
    let _path = MemberEdgeProvenance::PathProjection;
    let _key_of = MemberEdgeProvenance::KeyOfEnumerated;
    let _mapped = MemberEdgeProvenance::MappedKeyEnumerated;
}
