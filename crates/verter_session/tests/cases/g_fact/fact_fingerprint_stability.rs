//! Fact-fingerprint stability discrimination tests.
//!
//! Invariants:
//!
//! - Bullet 1 — Cosmetic edits: `semantic_hash` unchanged across all
//!   parse-domain facts; `display_hash` may change.
//! - Bullet 2 — Comment-only edit: NO `semantic_hash` change anywhere;
//!   `MemberSemanticFactStore` not re-keyed (parse_stable_hash invariant).
//! - Bullet 4 — Single body edit: exactly one `Member` `semantic_hash`
//!   changes; corresponding `MemberPresence` UNCHANGED.
//! - Bullet 5 — Adding a non-selected member: new `MemberPresence` +
//!   new `Member` entries; existing `MemberPresence` UNCHANGED.
//! - Bullet 7 — Adding an export bumps `SyntacticExportSet.semantic_hash`
//!   and adds a new `Export(...)` entry.
//! - Bullet 8 — Type/value namespace coexistence: `type Foo` + `const Foo`
//!   produce two distinct `Export` facts (R11).
//! - Bullet 11 — Topological-order test: same file rewritten with
//!   decls in reverse declaration order produces byte-identical
//!   fact set.
//!
//! Tests build a real `IndexedReady` from AUTHORED source through the
//! production-shaped service-backed construction path and run
//! `emit_parse_facts` to inspect the emitted `FileFacts.registry()`.
//! Without the fact emitter, these tests would fail because every
//! variant lookup would return `None`.
//!
//! Architectural rules bound: R10, R11, R12, R13, R14, R16, R28.

use std::sync::Arc;

use verter_semantic::facts::{FactKey, FactRegistry, SymbolSpace};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::InternedName;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;

fn empty_external(
) -> Arc<verter_parser::utils::oxc::script::type_inventory::AnalyzedExternalTypeSource> {
    Arc::new(
        verter_parser::utils::oxc::script::type_inventory::AnalyzedExternalTypeSource::default(),
    )
}

/// Build a real `IndexedReady` from authored source through the
/// production-shaped service-backed construction path (parse → header
/// index → lazy decl-body memo): the header inventory + exports come
/// from the real walk; declaration bodies lower lazily on the fact
/// path's first demand.
fn indexed_from_source(source: &str) -> Arc<IndexedReady> {
    let shallow =
        ShallowFileState::service_backed_for_test_with_hash("/facts.ts", source, [0u8; 16]);
    Arc::new(IndexedReady::new_for_test_with_state(
        [0u8; 16],
        shallow,
        Arc::from(source),
        Arc::from(source),
        empty_external(),
    ))
}

fn registry_of(indexed: &IndexedReady) -> FactRegistry {
    let emission = emit_parse_facts(indexed);
    emission.facts.registry().clone()
}

/// The full `FileFacts` (eager registry + lazy body-fact source) —
/// body-sensitive `Export` / `LocalDecl` facts are observed through
/// `lookup_or_compute`, never the eager registry.
fn facts_of(indexed: &IndexedReady) -> verter_session::file_artifact_store::FileFacts {
    emit_parse_facts(indexed).facts
}

// ── Bullet 1 — Cosmetic edits: semantic_hash unchanged ──

#[test]
fn raw_source_only_change_does_not_change_semantic_hash() {
    // Two files with identical shallow inventory but different
    // `raw_source` MUST produce identical parse-domain
    // semantic_hashes. The `raw_source` carries comments,
    // whitespace, and JSDoc; the shallow walk has already
    // extracted the structural shape so semantic emission is
    // invariant.
    let a = indexed_from_source("// comment A\nexport interface Foo { a: string }");
    let b = indexed_from_source("// comment B (cosmetic edit)\nexport interface Foo { a: string }");
    let facts_a = facts_of(&a);
    let facts_b = facts_of(&b);
    let key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let fact_a = facts_a
        .lookup_or_compute(&key)
        .expect("Foo export computed on observation");
    let fact_b = facts_b
        .lookup_or_compute(&key)
        .expect("Foo export computed on observation");
    assert_eq!(
        fact_a.semantic_hash, fact_b.semantic_hash,
        "cosmetic edits (different raw_source) MUST NOT change semantic_hash"
    );
}

// ── Bullet 5 / R28 — Adding a non-selected member ──

#[test]
fn adding_member_adds_new_presence_keeps_existing_unchanged() {
    // R28: `MemberPresence(Foo, "a")` MUST be invariant under adding
    // sibling `b`. `MemberShape` MUST change (whole-surface
    // observation); existing per-member presence MUST NOT.
    let only_a = indexed_from_source("export interface Foo { a: string }");
    let a_and_b = indexed_from_source("export interface Foo { a: string; b: number }");
    let reg_a = registry_of(&only_a);
    let reg_ab = registry_of(&a_and_b);

    let presence_a_key = FactKey::MemberPresence {
        exporter: InternedName::from("Foo"),
        name: InternedName::from("a"),
        space: SymbolSpace::Type,
    };
    let presence_a_before = reg_a
        .get(&presence_a_key)
        .expect("MemberPresence(Foo, a) emitted pre-add");
    let presence_a_after = reg_ab
        .get(&presence_a_key)
        .expect("MemberPresence(Foo, a) emitted post-add");
    assert_eq!(
        presence_a_before.semantic_hash, presence_a_after.semantic_hash,
        "R28: adding sibling MUST NOT shift MemberPresence(a)"
    );

    let presence_b_key = FactKey::MemberPresence {
        exporter: InternedName::from("Foo"),
        name: InternedName::from("b"),
        space: SymbolSpace::Type,
    };
    assert!(
        reg_a.get(&presence_b_key).is_none(),
        "pre-add: MemberPresence(Foo, b) MUST NOT exist"
    );
    assert!(
        reg_ab.get(&presence_b_key).is_some(),
        "post-add: MemberPresence(Foo, b) MUST exist"
    );

    let shape_key = FactKey::MemberShape {
        exporter: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let shape_before = reg_a.get(&shape_key).expect("shape emitted pre-add");
    let shape_after = reg_ab.get(&shape_key).expect("shape emitted post-add");
    assert_ne!(
        shape_before.semantic_hash, shape_after.semantic_hash,
        "R28: MemberShape MUST change when membership shifts"
    );
}

// ── Bullet 7 — Adding an export ──

#[test]
fn adding_export_bumps_syntactic_export_set_and_adds_export_fact() {
    let one_export = indexed_from_source("export interface Foo { a: string }");
    let two_exports = indexed_from_source(
        "export interface Foo { a: string }\nexport interface Bar { b: number }",
    );
    let facts_one = facts_of(&one_export);
    let facts_two = facts_of(&two_exports);

    let bar_key = FactKey::Export {
        name: InternedName::from("Bar"),
        space: SymbolSpace::Type,
    };
    assert!(
        facts_one.lookup_or_compute(&bar_key).is_none(),
        "Bar must NOT exist before adding the export"
    );
    assert!(
        facts_two.lookup_or_compute(&bar_key).is_some(),
        "Bar MUST exist post-add"
    );

    let set_one = facts_one
        .lookup(&FactKey::SyntacticExportSet)
        .cloned()
        .expect("SyntacticExportSet emitted");
    let set_two = facts_two
        .lookup(&FactKey::SyntacticExportSet)
        .cloned()
        .expect("SyntacticExportSet emitted");
    assert_ne!(
        set_one.semantic_hash, set_two.semantic_hash,
        "adding an export MUST bump SyntacticExportSet.semantic_hash"
    );
}

// ── Bullet 8 / R11 — Type + Value namespace coexistence ──

#[test]
fn type_and_value_namespace_keys_coexist_for_same_name() {
    // R11: `class Foo` occupies both `Type` and `Value`. We
    // approximate this with `type Foo` + `const Foo` — distinct
    // declarations sharing the name. The emitter MUST produce two
    // facts under different SymbolSpace keys.
    let indexed = indexed_from_source("export type Foo = { a: string }\nexport const Foo = 1");
    let facts = facts_of(&indexed);

    let type_key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let value_key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Value,
    };
    let type_fact = facts
        .lookup_or_compute(&type_key)
        .expect("type-space Export");
    let value_fact = facts
        .lookup_or_compute(&value_key)
        .expect("value-space Export");
    assert_ne!(
        type_fact.semantic_hash, value_fact.semantic_hash,
        "type-space and value-space facts have distinct identities (R11)"
    );
}

// ── Bullet 11 — Topological-order test ──

#[test]
fn decl_reorder_does_not_change_emitted_fact_set() {
    // R10 + R27 canonical visit order. The fact emitter sorts
    // type symbols and value symbols by name before emission, so
    // the same file rewritten with decls in reverse declaration
    // order produces a byte-identical fact set.
    let in_order = indexed_from_source(
        "export interface Alpha { a: string }\n\
         export interface Bravo { b: number }\n\
         export interface Charlie { c: boolean }\n",
    );
    let reversed = indexed_from_source(
        "export interface Charlie { c: boolean }\n\
         export interface Bravo { b: number }\n\
         export interface Alpha { a: string }\n",
    );
    let reg_a = registry_of(&in_order);
    let reg_b = registry_of(&reversed);
    // Every fact key in `reg_a` MUST exist in `reg_b` with the
    // same semantic_hash.
    for (key, fact_a) in reg_a.iter() {
        let fact_b = reg_b
            .get(key)
            .unwrap_or_else(|| panic!("declaration reorder dropped key: {key:?}"));
        assert_eq!(
            fact_a.semantic_hash, fact_b.semantic_hash,
            "decl reorder MUST be byte-identical: {key:?}"
        );
    }
    assert_eq!(
        reg_a.len(),
        reg_b.len(),
        "decl reorder MUST emit the same number of facts"
    );
}

// ── Bullet 7 / R10 — Removing an export drops the fact key ──

#[test]
fn removing_export_drops_export_fact_key() {
    // R10: removed facts validate as misses (registry returns
    // None). This is the cache-invalidation semantics — the
    // consumer's signature includes the export key; the post-
    // removal registry returns None at that key; validation fails.
    let with_bar = indexed_from_source(
        "export interface Foo { a: string }\nexport interface Bar { b: number }",
    );
    let without_bar = indexed_from_source("export interface Foo { a: string }");
    let facts_with = facts_of(&with_bar);
    let facts_without = facts_of(&without_bar);
    let bar_key = FactKey::Export {
        name: InternedName::from("Bar"),
        space: SymbolSpace::Type,
    };
    assert!(facts_with.lookup_or_compute(&bar_key).is_some());
    assert!(
        facts_without.lookup_or_compute(&bar_key).is_none(),
        "removed exports MUST validate as observation misses (R10)"
    );
}

/// Corpus-anchored binding: load the `cosmetic_edit_comment.ts`
/// fixture and verify the documented invariant — that the file
/// declares an `interface Foo` with members `a` and `b` plus a
/// top-of-file standalone comment. The fact-emitter discriminator
/// asserts that re-running the fact emitter on a comment-edited
/// variant of this file produces the same semantic_hashes.
#[test]
fn cosmetic_edit_comment_fixture_declares_documented_shape() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join("fixtures")
        .join("path_precise")
        .join("cosmetic_edit_comment.ts");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    assert!(src.contains("export interface Foo"));
    assert!(src.contains("a: number"));
    assert!(src.contains("b: string"));
    // The standalone comment that gets edited in the
    // characterisation pass.
    assert!(
        src.contains("This is a top-of-file standalone comment"),
        "cosmetic_edit_comment.ts MUST contain the documented standalone comment"
    );
}

// ── enum member values fold into the value-body fingerprint ──

// (enum fixtures below author the `export enum Color { ... }` source
// directly — the real binder folds the member values.)

#[test]
fn enum_member_edit_moves_value_body_fingerprint() {
    // The value-space `Export(Color, Value)` fact's `semantic_hash` is the
    // enum's value-body fingerprint. It folds the FOLDABLE member set into a
    // synthetic object, so a foldable member value / name / count edit moves the
    // hash — a constant body would leave a warm `typeof Color` / `Color.Red`
    // consumer serving STALE after an edit. It MUST discriminate every foldable
    // member edit.
    let key = FactKey::Export {
        name: InternedName::from("Color"),
        space: SymbolSpace::Value,
    };
    let hash = |indexed: &IndexedReady| {
        facts_of(indexed)
            .lookup_or_compute(&key)
            .expect("enum value-body fact computes on observation")
            .semantic_hash
    };

    let red0 = indexed_from_source("export enum Color { Red = 0 }");
    let red0_again = indexed_from_source("export enum Color { Red = 0 }");
    let red1 = indexed_from_source("export enum Color { Red = 1 }");
    let crimson0 = indexed_from_source("export enum Color { Crimson = 0 }");
    let red_green = indexed_from_source("export enum Color { Red, Green }");

    // Identical enums → identical fingerprint (no spurious churn).
    assert_eq!(
        hash(&red0),
        hash(&red0_again),
        "identical enums MUST produce the same value-body fingerprint"
    );
    // A member VALUE edit (`Red = 0` → `Red = 1`).
    assert_ne!(
        hash(&red0),
        hash(&red1),
        "a member value edit MUST move the value-body fingerprint"
    );
    // A member NAME edit (`Red` → `Crimson`), same value.
    assert_ne!(
        hash(&red0),
        hash(&crimson0),
        "a member name edit MUST move the value-body fingerprint"
    );
    // A member COUNT change (add `Green`).
    assert_ne!(
        hash(&red0),
        hash(&red_green),
        "adding a member MUST move the value-body fingerprint"
    );
}
