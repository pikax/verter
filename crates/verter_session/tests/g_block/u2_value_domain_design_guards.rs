//! Design-gate architecture guards for the U2 query-value-domain
//! design (`docs/arch/u2-query-value-domain-design.md`).
//!
//! These guards land AT the design gate — before the STAGE-B
//! implementation — and are discriminating TODAY (each can FAIL if
//! the corresponding invariant is violated on the current tree).
//!
//! Pinned invariants:
//!
//! 1. `no_envless_semantic_query_env_key_envelope` — the superseded
//!    env-less "U2a" uniform-envelope KEY types (`SemanticQueryEnvKey`,
//!    `TypeLibEnvKey`) must NOT appear in ANY production
//!    `crates/*/src/**` file, and the env-less query-identity wire slot
//!    `GraphDeclSlotRef` must NOT appear in ANY production source. The
//!    env-bearing `GraphResolvedDeclSlotIdentity`
//!    (`GraphQueryIdentity.resolved_roots`, tag 18) is the query-identity
//!    roots carrier; the retired slot's tag (2) and name (`roots`) are
//!    reserved at message scope. Env stays ON the key via per-key
//!    `*Context` (two-tier env model); the env-less envelope is
//!    FORBIDDEN.
//!
//!    The env-less wire slot was deleted from the proto and the
//!    `verter_protocol` typed surface; this scanner bans the symbol
//!    across ALL `crates/*/src/**` so it can never be reintroduced.
//!    The companion `typeinfo_proto_retires_envless_decl_slot_ref` guard
//!    pins the proto surface itself (no `message GraphDeclSlotRef`,
//!    `roots` tag/name reserved, `resolved_roots = 18` present).
//! 2. `error_rides_opaque_no_new_error_type_wire_arm` — the error
//!    type rides the existing `SemanticNodeData::Opaque(QueryError)`
//!    carrier; NO `ErrorType` arm/variant/field may exist on
//!    `SemanticNodeData`, `GraphTypeNode`, or the typeinfo proto.
//! 3. `u2_value_domain_design_doc_locks_invariants` — the design doc
//!    pins the load-bearing locked decisions; this guards against
//!    silent drift of the locked text before STAGE B.
//!
//! Each scanner ships a discriminator self-test that injects the
//! forbidden token / a deliberately-absent phrase into a local
//! string and asserts the scanner verdict flips — mirroring
//! `every_registry_guard_name_validity_scanner_discriminates_against_fake`
//! in `g_misc0/critical_rules_have_guards.rs`.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Recursively collect every `*.rs` file under `dir`.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip build artefacts.
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Exclude extracted `#[cfg(test)]` modules and `tests.rs` — they are
/// not production behaviour.
fn is_production_src(path: &Path) -> bool {
    !path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with("_tests.rs") || n == "tests.rs")
}

/// Collect every production-source `.rs` file under `crates/*/src/`.
fn collect_production_src_files() -> Vec<PathBuf> {
    let crates_dir = workspace_root().join("crates");
    let mut crate_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&crates_dir) {
        for entry in read_dir.flatten() {
            if entry.path().is_dir() {
                crate_dirs.push(entry.path());
            }
        }
    }
    let mut files = Vec::new();
    for crate_dir in crate_dirs {
        let src_dir = crate_dir.join("src");
        if src_dir.is_dir() {
            collect_rs_files(&src_dir, &mut files);
        }
    }
    files.retain(|p| is_production_src(p));
    files
}

/// The superseded env-less "U2a" uniform-envelope KEY types. These
/// must appear NOWHERE in production source — env stays ON the key via
/// per-key `*Context`.
const FORBIDDEN_ENVLESS_KEY_TYPES: &[&str] = &["SemanticQueryEnvKey", "TypeLibEnvKey"];

/// The env-less query-identity wire slot. It was RETIRED (deleted from
/// the proto and the `verter_protocol` typed surface) in favour of the
/// env-bearing `GraphResolvedDeclSlotIdentity`. The symbol must NOT
/// appear in ANY production source (`crates/*/src/**`) — it can never
/// be reintroduced.
const FORBIDDEN_ENVLESS_WIRE_SLOT: &str = "GraphDeclSlotRef";

/// Pure predicate: does `content` contain any forbidden env-less KEY
/// type? Returns the first match for diagnostics.
fn first_forbidden_envless_key_type(content: &str) -> Option<&'static str> {
    FORBIDDEN_ENVLESS_KEY_TYPES
        .iter()
        .copied()
        .find(|sym| content.contains(sym))
}

// ---------------------------------------------------------------------------
// Guard 1 — no env-LESS uniform-envelope symbols in production src.
// ---------------------------------------------------------------------------
#[test]
fn no_envless_semantic_query_env_key_envelope() {
    // (a) The env-less KEY types must appear NOWHERE in production src.
    let all_files = collect_production_src_files();
    assert!(
        !all_files.is_empty(),
        "guard self-check: walked crates/*/src and found no .rs files \
         — the file walker is broken, not the invariant."
    );
    let mut key_offenders: Vec<String> = Vec::new();
    for path in &all_files {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(sym) = first_forbidden_envless_key_type(&content) {
            key_offenders.push(format!("{} -> `{sym}`", path.display()));
        }
    }
    assert!(
        key_offenders.is_empty(),
        "U2 VALUE-DOMAIN KEY IDENTITY (CRITICAL): the env-LESS uniform \
         envelope is FORBIDDEN. The superseded U2a KEY types \
         {FORBIDDEN_ENVLESS_KEY_TYPES:?} must not appear in production \
         `crates/*/src/**`. Env stays ON the key via per-key `*Context` \
         (two-tier env model). Offending sites:\n  {}",
        key_offenders.join("\n  "),
    );

    // (b) The env-less wire slot was RETIRED — it must NOT appear in
    // ANY production source. Its env-bearing replacement is
    // `GraphResolvedDeclSlotIdentity` (`GraphQueryIdentity.resolved_roots`).
    let mut slot_offenders: Vec<String> = Vec::new();
    for path in &all_files {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        if content.contains(FORBIDDEN_ENVLESS_WIRE_SLOT) {
            slot_offenders.push(path.display().to_string());
        }
    }
    assert!(
        slot_offenders.is_empty(),
        "U2 VALUE-DOMAIN KEY IDENTITY (CRITICAL): the retired env-less \
         query-identity wire slot `{FORBIDDEN_ENVLESS_WIRE_SLOT}` must \
         NOT appear in ANY production source (`crates/*/src/**`). Query \
         identity uses the env-bearing `GraphResolvedDeclSlotIdentity` \
         (`GraphQueryIdentity.resolved_roots`), not the env-less wire \
         slot. Offending sites:\n  {}",
        slot_offenders.join("\n  "),
    );
}

/// Discriminator self-test for guard 1: injected forbidden tokens MUST
/// be caught; clean / legal samples must NOT be.
#[test]
fn no_envless_semantic_query_env_key_envelope_discriminator_self_test() {
    let dirty = "struct Foo { key: SemanticQueryEnvKey }";
    assert_eq!(
        first_forbidden_envless_key_type(dirty),
        Some("SemanticQueryEnvKey"),
        "scanner self-test: an injected forbidden env-less KEY type was \
         NOT caught — the scanner is too permissive."
    );
    let dirty2 = "fn f(k: TypeLibEnvKey) {}";
    assert_eq!(
        first_forbidden_envless_key_type(dirty2),
        Some("TypeLibEnvKey"),
        "scanner self-test: `TypeLibEnvKey` was not caught."
    );
    let clean = "struct ResolvedDeclSlotIdentity { type_env_hash: u64 }";
    assert_eq!(
        first_forbidden_envless_key_type(clean),
        None,
        "scanner self-test: a clean sample (the LEGAL `*Context` / \
         slot-identity surface) was flagged — the scanner is too strict."
    );
    // The wire-slot sub-check is a plain substring contains; verify it
    // discriminates a wired-in slot from a clean session sample.
    let dirty_slot = "let r: GraphDeclSlotRef = build_query_identity();";
    assert!(
        dirty_slot.contains(FORBIDDEN_ENVLESS_WIRE_SLOT),
        "scanner self-test: the wire-slot substring check failed to \
         catch a wired-in `GraphDeclSlotRef`."
    );
    let clean_slot = "let r: ResolvedDeclSlotIdentity = build_query_identity();";
    assert!(
        !clean_slot.contains(FORBIDDEN_ENVLESS_WIRE_SLOT),
        "scanner self-test: the wire-slot substring check flagged a \
         clean `ResolvedDeclSlotIdentity` sample."
    );
}

// ---------------------------------------------------------------------------
// Guard 2 — error rides Opaque(QueryError); no new `ErrorType` arm.
// ---------------------------------------------------------------------------

/// Pure predicate: does `content` introduce an `ErrorType`
/// enum-arm / variant / field token? We match the bare identifier
/// `ErrorType` (word-bounded) because the design forbids the symbol
/// entirely on these three carriers.
fn mentions_error_type_arm(content: &str) -> bool {
    // Word-boundary check so e.g. `ErrorTypeFoo` is still flagged
    // (a recycled name is equally forbidden) but a substring inside
    // an unrelated word like `MirrorTypeX` is not.
    content.match_indices("ErrorType").any(|(idx, _)| {
        let before_ok = idx == 0
            || !content[..idx]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        before_ok
    })
}

#[test]
fn error_rides_opaque_no_new_error_type_wire_arm() {
    let root = workspace_root();
    let semantic_query = root.join("crates/verter_session/src/semantic_query.rs");
    let graph = root.join("crates/verter_protocol/src/typeinfo/graph.rs");
    let proto = root.join("crates/verter_protocol/proto/verter/v1/typeinfo.proto");

    // Sanity: the carriers must exist, else the guard is vacuous.
    for p in [&semantic_query, &graph, &proto] {
        assert!(
            p.is_file(),
            "guard self-check: expected carrier file `{}` to exist; \
             the guard cannot validate the no-ErrorType invariant \
             against a missing file.",
            p.display()
        );
    }

    // Anchor: the EXISTING error carrier must still be present in
    // `semantic_query.rs`. If `Opaque(QueryError)` ever disappears the
    // error type lost its home and the guard's premise is broken —
    // surface that loudly rather than passing silently.
    let semantic_src = fs::read_to_string(&semantic_query).unwrap();
    assert!(
        semantic_src.contains("Opaque(QueryError)"),
        "guard premise: the error type rides the EXISTING \
         `SemanticNodeData::Opaque(QueryError)` carrier, but \
         `Opaque(QueryError)` is no longer present in \
         `semantic_query.rs` — the carrier was removed."
    );

    let mut offenders: Vec<String> = Vec::new();
    for p in [&semantic_query, &graph, &proto] {
        let content = fs::read_to_string(p).unwrap();
        if mentions_error_type_arm(&content) {
            offenders.push(p.display().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "TYPED VALUE DOMAIN (CRITICAL): the error type MUST ride the \
         existing `SemanticNodeData::Opaque(QueryError)` carrier. NO \
         `ErrorType` arm/variant/field may be introduced on \
         `SemanticNodeData`, `GraphTypeNode`, or the typeinfo proto \
         (wire-purity closure). Offending carrier(s):\n  {}",
        offenders.join("\n  "),
    );
}

/// Discriminator self-test for guard 2: an injected `ErrorType` arm
/// MUST be caught; clean carrier text must NOT be.
#[test]
fn error_rides_opaque_no_new_error_type_wire_arm_discriminator_self_test() {
    assert!(
        mentions_error_type_arm("enum GraphTypeNode { ErrorType(QueryError) }"),
        "scanner self-test: an injected `ErrorType` arm was NOT caught \
         — the scanner is too permissive."
    );
    assert!(
        mentions_error_type_arm("    ErrorType = 9;"),
        "scanner self-test: an injected proto `ErrorType` field was not caught."
    );
    assert!(
        !mentions_error_type_arm("enum SemanticNodeData { Opaque(QueryError) }"),
        "scanner self-test: the LEGAL `Opaque(QueryError)` carrier was \
         flagged — the scanner is too strict."
    );
    assert!(
        !mentions_error_type_arm("let x = MirrorTypeNode;"),
        "scanner self-test: an unrelated `MirrorType*` identifier was \
         flagged — the word-boundary check is broken."
    );
}

// ---------------------------------------------------------------------------
// Guard 3 — the design doc locks its load-bearing invariants.
// ---------------------------------------------------------------------------

/// The load-bearing locked phrases that MUST remain verbatim in the
/// design doc. Each was confirmed present by grep before landing.
const LOCKED_DESIGN_PHRASES: &[&str] = &[
    // The locked `Instantiate`/`ResolveMacroPayload` slot-keying decision
    // (env-bearing `ResolvedDeclSlotIdentity` base/owner). Pinned by a
    // final-state phrase, not the former plan-fork label.
    "key on the env-bearing slot",
    "Partial join is ACCEPTABLE",
    "two-tier env model",
    "MaterializedSet",
    // The no-ErrorType-wire-arm statement.
    "GraphTypeNode::ErrorType",
];

fn design_doc_path() -> PathBuf {
    workspace_root().join("docs/arch/u2-query-value-domain-design.md")
}

/// Pure predicate: which locked phrases are MISSING from `body`?
fn missing_locked_phrases(body: &str) -> Vec<&'static str> {
    LOCKED_DESIGN_PHRASES
        .iter()
        .copied()
        .filter(|phrase| !body.contains(phrase))
        .collect()
}

#[test]
fn u2_value_domain_design_doc_locks_invariants() {
    let path = design_doc_path();
    let body = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read design doc `{}`: {e}", path.display()));

    let missing = missing_locked_phrases(&body);
    assert!(
        missing.is_empty(),
        "U2 design doc `{}` has DRIFTED: the following locked phrases \
         are no longer present verbatim — the locked decision was \
         silently weakened or reworded. Restore the phrasing or update \
         this guard in the same change that re-locks the design:\n  {}",
        path.display(),
        missing.join("\n  "),
    );
}

/// Discriminator self-test for guard 3: a deliberately-absent phrase
/// must be reported missing; the real phrases must be found in a
/// sample that contains them.
#[test]
fn u2_value_domain_design_doc_locks_invariants_discriminator_self_test() {
    // A sample missing every locked phrase must report them all missing.
    let empty_sample = "this text contains none of the locked phrases";
    assert_eq!(
        missing_locked_phrases(empty_sample).len(),
        LOCKED_DESIGN_PHRASES.len(),
        "scanner self-test: a sample missing every locked phrase did \
         not report them all missing — the scanner is too permissive."
    );
    // A deliberately-absent phrase that is NOT one of the locked
    // phrases must never be reported (the predicate keys on the
    // registered phrases, not on arbitrary absence).
    let absent = "FORK-Z = DELETE_EVERYTHING_AND_REPARSE";
    assert!(
        !LOCKED_DESIGN_PHRASES.contains(&absent),
        "scanner self-test: a deliberately-absent phrase unexpectedly \
         appears in the locked-phrase set."
    );
    // A sample that DOES contain all locked phrases must report none missing.
    let full_sample = LOCKED_DESIGN_PHRASES.join(" ... ");
    assert!(
        missing_locked_phrases(&full_sample).is_empty(),
        "scanner self-test: a sample containing every locked phrase \
         reported some missing — the scanner is too strict."
    );
}

// ---------------------------------------------------------------------------
// Guard 4 — the typeinfo proto retired the env-less decl-slot roots slot.
// ---------------------------------------------------------------------------

fn typeinfo_proto_path() -> PathBuf {
    workspace_root().join("crates/verter_protocol/proto/verter/v1/typeinfo.proto")
}

/// Strip line comments (`// …`) from proto source so a symbol mentioned
/// only in prose does not register as a live wire referent. Block
/// comments are not used in this schema.
fn strip_proto_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The env-less decl-slot roots slot was retired on the typeinfo wire
/// (schema_version 2). This guard pins the proto surface itself — the
/// Rust-src scanner in guard 1 cannot see the proto. It asserts:
///
/// 1. NO `message GraphDeclSlotRef` definition and no field typed as the
///    retired slot survive (checked against comment-stripped source so
///    an explanatory mention in prose is not a false positive).
/// 2. The retired `GraphQueryIdentity.roots` tag (`2`) and name
///    (`roots`) are reserved at message scope.
/// 3. The env-bearing replacement `resolved_roots = 18` carrying
///    `GraphResolvedDeclSlotIdentity` is present.
#[test]
fn typeinfo_proto_retires_envless_decl_slot_ref() {
    let path = typeinfo_proto_path();
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read typeinfo proto `{}`: {e}", path.display()));
    let code = strip_proto_line_comments(&raw);

    // (1) The retired message + any field typed as the env-less slot
    // must be gone from the live (comment-stripped) wire surface.
    assert!(
        !code.contains("message GraphDeclSlotRef"),
        "TYPEINFO WIRE (CRITICAL): `message GraphDeclSlotRef` must be \
         DELETED from `{}` — the env-less decl-slot roots carrier was \
         retired in favour of `GraphResolvedDeclSlotIdentity`.",
        path.display(),
    );
    assert!(
        !code.contains("GraphDeclSlotRef"),
        "TYPEINFO WIRE (CRITICAL): the retired `GraphDeclSlotRef` symbol \
         must not appear as a live referent anywhere in `{}` (only \
         comment prose may mention the retirement).",
        path.display(),
    );

    // (2) The retired tag + name are reserved at message scope.
    assert!(
        code.contains("reserved 2;"),
        "TYPEINFO WIRE (CRITICAL): `GraphQueryIdentity` must `reserved 2;` \
         — the retired `roots` field number must never be reused.",
    );
    assert!(
        code.contains("reserved \"roots\";"),
        "TYPEINFO WIRE (CRITICAL): `GraphQueryIdentity` must \
         `reserved \"roots\";` — the retired field name must never be \
         reused.",
    );

    // (3) The env-bearing replacement carrier is present on the next
    // free tag (18), typed as `GraphResolvedDeclSlotIdentity`.
    assert!(
        code.contains("repeated GraphResolvedDeclSlotIdentity resolved_roots = 18;"),
        "TYPEINFO WIRE (CRITICAL): `GraphQueryIdentity` must carry \
         `repeated GraphResolvedDeclSlotIdentity resolved_roots = 18;` — \
         the env-bearing query-identity roots carrier.",
    );
}

/// Discriminator self-test for guard 4: the comment-stripper must hide a
/// prose mention but expose a live referent, and the three structural
/// needles must be absence/presence discriminating.
#[test]
fn typeinfo_proto_retires_envless_decl_slot_ref_discriminator_self_test() {
    // A prose-only mention is stripped → not a live referent.
    let prose = "  // GraphDeclSlotRef was retired here\n  uint32 ok = 1;";
    assert!(
        !strip_proto_line_comments(prose).contains("GraphDeclSlotRef"),
        "self-test: the comment-stripper failed to remove a prose-only \
         mention of the retired symbol."
    );
    // A live field referent survives stripping → caught.
    let live = "  repeated GraphDeclSlotRef roots = 2; // carrier";
    assert!(
        strip_proto_line_comments(live).contains("GraphDeclSlotRef"),
        "self-test: the comment-stripper wrongly removed a live \
         `GraphDeclSlotRef` field referent."
    );
    // The reserved + replacement needles discriminate present vs absent.
    let clean = "reserved 2;\nreserved \"roots\";\n\
                 repeated GraphResolvedDeclSlotIdentity resolved_roots = 18;";
    assert!(
        clean.contains("reserved 2;")
            && clean.contains("reserved \"roots\";")
            && clean.contains("repeated GraphResolvedDeclSlotIdentity resolved_roots = 18;"),
        "self-test: the structural needles failed to match a clean sample."
    );
    let dirty = "repeated GraphResolvedDeclSlotIdentity resolved_roots = 7;";
    assert!(
        !dirty.contains("resolved_roots = 18;"),
        "self-test: the tag-18 needle matched a wrong-tag sample."
    );
}
