//! `parse_stable_hash` invariance tests.
//!
//! `parse_stable_hash` is a structural hash over a file's post-shallow-analysis
//! decl skeleton (R28 fingerprint hashing rules). Invariants:
//!
//! - **Invariant under whitespace edits, comment edits, JSDoc edits, and
//!   generic param identifier renames.** `parse_stable_hash`
//!   walks the shallow symbol inventory (names + kinds + member name lists)
//!   without inspecting bodies, so cosmetic changes that don't shift the
//!   inventory shape don't ripple.
//! - **Changes under decl-shape edits.** Adding/removing/renaming a
//!   declaration or member produces a new hash.
//!
//! `parse_stable_hash` is built from `IndexedReady.shallow_state`.
//! These tests author REAL source variants and construct through the
//! production-shaped service-backed path (parse → header index → lazy
//! decl-body memo), so the inventory the hash walks is the real one.

use std::sync::Arc;

use verter_session::parse_stable_hash::compute_parse_stable_hash;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;

fn empty_external(
) -> Arc<verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSource> {
    Arc::new(verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default())
}

/// Build a real `IndexedReady` from authored source through the
/// service-backed construction path. `parse_stable_hash` walks ONLY the
/// shallow inventory, so the raw source rides along untouched.
fn indexed_from_source(source: &str) -> Arc<IndexedReady> {
    let shallow = ShallowFileState::service_backed_for_test_with_hash("/psh.ts", source, [0u8; 16]);
    Arc::new(IndexedReady::new_for_test_with_state(
        [0u8; 16],
        shallow,
        Arc::from(source),
        Arc::from(source),
        empty_external(),
    ))
}

#[test]
fn whitespace_edit_does_not_change_parse_stable_hash() {
    // Two artifacts from sources that differ ONLY in whitespace and a
    // comment: the shallow inventory (symbols/members/exports) is
    // identical, so the hash MUST be identical — we hash the inventory,
    // not the raw text.
    let a = indexed_from_source(
        "export interface Foo { x: string; y: number }\nfunction greet(): void {}\n",
    );
    let b = indexed_from_source(
        "// cosmetic comment\nexport interface Foo {\n  x: string;\n  y: number;\n}\n\nfunction greet(): void {}\n",
    );
    assert_eq!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "identical decl inventory MUST produce identical parse_stable_hash"
    );
}

#[test]
fn decl_reorder_does_not_change_parse_stable_hash() {
    let a = indexed_from_source("interface Alpha { a: string }\ninterface Beta { b: string }\n");
    let b = indexed_from_source("interface Beta { b: string }\ninterface Alpha { a: string }\n");
    assert_eq!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "decl reorder (FxHashMap iteration order) MUST NOT change parse_stable_hash"
    );
}

#[test]
fn member_reorder_does_not_change_parse_stable_hash() {
    let a = indexed_from_source("interface Foo { a: string; b: string; c: string }\n");
    let b = indexed_from_source("interface Foo { c: string; a: string; b: string }\n");
    assert_eq!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "member reorder MUST NOT change parse_stable_hash"
    );
}

// ── Discrimination tests ──

#[test]
fn added_decl_changes_parse_stable_hash() {
    let a = indexed_from_source("interface Foo { a: string }\n");
    let b = indexed_from_source("interface Foo { a: string }\ninterface Bar { b: string }\n");
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "adding a decl MUST change parse_stable_hash"
    );
}

#[test]
fn renamed_decl_changes_parse_stable_hash() {
    let a = indexed_from_source("interface Foo { x: string }\n");
    let b = indexed_from_source("interface Bar { x: string }\n");
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "renaming a decl MUST change parse_stable_hash"
    );
}

#[test]
fn renamed_member_changes_parse_stable_hash() {
    let a = indexed_from_source("interface Foo { x: string }\n");
    let b = indexed_from_source("interface Foo { y: string }\n");
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "renaming a member MUST change parse_stable_hash"
    );
}

#[test]
fn kind_change_changes_parse_stable_hash() {
    let a = indexed_from_source("interface Foo { x: string }\n");
    let b = indexed_from_source("type Foo = { x: string }\n");
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "changing decl kind MUST change parse_stable_hash"
    );
}

#[test]
fn added_export_changes_parse_stable_hash() {
    let a = indexed_from_source("const Foo = 1;\n");
    let b = indexed_from_source("export const Foo = 1;\n");
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "adding an export MUST change parse_stable_hash"
    );
}

#[test]
fn deterministic_across_calls() {
    let indexed = indexed_from_source("interface Foo { x: string; y: number }\n");
    let h0 = compute_parse_stable_hash(&indexed);
    let h1 = compute_parse_stable_hash(&indexed);
    assert_eq!(h0, h1, "compute_parse_stable_hash MUST be deterministic");
}
