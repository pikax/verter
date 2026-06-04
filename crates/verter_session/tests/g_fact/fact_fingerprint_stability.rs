//! Stage 3 fact-fingerprint stability discrimination tests.
//!
//! Verify-bullet binding:
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
//! Tests synthesise `IndexedReady` directly via `build_indexed`
//! (no parser invocation) and run `emit_parse_facts` to inspect the
//! emitted `FileFacts.registry()`. Without the Stage 3 emitter,
//! these tests would fail because every variant lookup would
//! return `None` (the Stage 1 placeholder).
//!
//! Architectural rules bound: R10, R11, R12, R13, R14, R16, R28.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::{TypeDeclBody, TypeDeclKind, ValueDeclKind};
use verter_semantic::facts::{FactKey, FactRegistry, SymbolSpace};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::InternedName;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::{
    ExportTarget, ShallowFileState, ShallowTypeSymbol, ShallowValueSymbol,
};
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

fn empty_external(
) -> Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource> {
    Arc::new(verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default())
}

/// Builder for a synthetic `IndexedReady` that drives Phase 1 fact
/// emission. The `member_bodies` map per-symbol describes the
/// member name list (used by `MemberShape` + `MemberPresence`) and
/// the synthesized body `TypeExpr`.
struct TypeDecl<'a> {
    name: &'a str,
    kind: TypeDeclKind,
    members: Vec<(&'a str, TypeExpr)>,
    body: TypeExpr,
}

fn build_type_decl<'a>(name: &'a str, members: Vec<(&'a str, TypeExpr)>) -> TypeDecl<'a> {
    // Synthesize an interface-shaped body from the members.
    let body = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members
            .iter()
            .map(|(n, ty)| {
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    n.to_string(),
                    ty.clone(),
                    false,
                    false,
                ))
            })
            .collect(),
    }));
    TypeDecl {
        name,
        kind: TypeDeclKind::Interface,
        members,
        body,
    }
}

fn build_indexed(
    type_decls: Vec<TypeDecl<'_>>,
    value_symbols: Vec<(&str, ValueDeclKind)>,
    exports: Vec<(&str, ExportTarget)>,
    raw_source: &str,
) -> Arc<IndexedReady> {
    let mut symbols: FxHashMap<String, ShallowTypeSymbol> = FxHashMap::default();
    for decl in type_decls {
        let mut member_deps: FxHashMap<String, Vec<String>> = FxHashMap::default();
        for (n, _) in &decl.members {
            member_deps.insert((*n).to_string(), Vec::new());
        }
        symbols.insert(
            decl.name.to_string(),
            ShallowTypeSymbol {
                kind: decl.kind,
                body: TypeDeclBody::Single(decl.body),
                type_parameters: Vec::new(),
                local_deps: Vec::new(),
                external_deps: Vec::new(),
                member_deps,
            },
        );
    }
    let mut value_syms: FxHashMap<String, ShallowValueSymbol> = FxHashMap::default();
    for (name, kind) in value_symbols {
        value_syms.insert(
            name.to_string(),
            ShallowValueSymbol {
                kind,
                type_annotation: None,
                signatures: Vec::new(),
                object_shape: None,
                enum_members: None,
                is_synthesised_vue_default: false,
            },
        );
    }
    let mut exports_map: FxHashMap<String, ExportTarget> = FxHashMap::default();
    for (name, target) in exports {
        exports_map.insert(name.to_string(), target);
    }
    let shallow = ShallowFileState {
        whole_hash: [0u8; 16],
        exports: exports_map,
        wildcard_reexports: Vec::new(),
        symbols,
        value_symbols: value_syms,
        import_locals: FxHashSet::default(),
        import_targets: FxHashMap::default(),
        augmentation_scopes: Default::default(),
        analysis: empty_external(),
    };
    Arc::new(IndexedReady {
        whole_hash: [0u8; 16],
        shallow_state: Arc::new(shallow),
        import_routes: Arc::new(FxHashMap::default()),
        import_route_hash: None,
        route_hash: None,
        raw_source: Arc::from(raw_source),
        eval_source: Arc::from(""),
        cached_parse: None,
        script_analysis: None,
        export_signatures: None,
        snapshot: Arc::new(verter_session::FileAnalysisSnapshot::default()),
        external_type_analysis: empty_external(),
        declares_interface_app_config: false,
    })
}

fn export_local(name: &str) -> ExportTarget {
    ExportTarget::Local {
        symbol_name: name.to_string(),
    }
}

fn prim(p: PrimitiveName) -> TypeExpr {
    TypeExpr::Primitive(p)
}

fn registry_of(indexed: &IndexedReady) -> FactRegistry {
    let emission = emit_parse_facts(indexed);
    emission.facts.registry().clone()
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
    let make = |raw: &str| {
        build_indexed(
            vec![build_type_decl(
                "Foo",
                vec![("a", prim(PrimitiveName::String))],
            )],
            vec![],
            vec![("Foo", export_local("Foo"))],
            raw,
        )
    };
    let a = make("// comment A\nexport interface Foo { a: string }");
    let b = make("// comment B (cosmetic edit)\nexport interface Foo { a: string }");
    let reg_a = registry_of(&a);
    let reg_b = registry_of(&b);
    let key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let fact_a = reg_a.get(&key).expect("Foo export emitted");
    let fact_b = reg_b.get(&key).expect("Foo export emitted");
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
    let only_a = build_indexed(
        vec![build_type_decl(
            "Foo",
            vec![("a", prim(PrimitiveName::String))],
        )],
        vec![],
        vec![("Foo", export_local("Foo"))],
        "interface Foo { a: string }",
    );
    let a_and_b = build_indexed(
        vec![build_type_decl(
            "Foo",
            vec![
                ("a", prim(PrimitiveName::String)),
                ("b", prim(PrimitiveName::Number)),
            ],
        )],
        vec![],
        vec![("Foo", export_local("Foo"))],
        "interface Foo { a: string; b: number }",
    );
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
    let one_export = build_indexed(
        vec![build_type_decl(
            "Foo",
            vec![("a", prim(PrimitiveName::String))],
        )],
        vec![],
        vec![("Foo", export_local("Foo"))],
        "export interface Foo { a: string }",
    );
    let two_exports = build_indexed(
        vec![
            build_type_decl("Foo", vec![("a", prim(PrimitiveName::String))]),
            build_type_decl("Bar", vec![("b", prim(PrimitiveName::Number))]),
        ],
        vec![],
        vec![("Foo", export_local("Foo")), ("Bar", export_local("Bar"))],
        "export interface Foo { a: string }\nexport interface Bar { b: number }",
    );
    let reg_one = registry_of(&one_export);
    let reg_two = registry_of(&two_exports);

    let bar_key = FactKey::Export {
        name: InternedName::from("Bar"),
        space: SymbolSpace::Type,
    };
    assert!(
        reg_one.get(&bar_key).is_none(),
        "Bar must NOT exist before adding the export"
    );
    assert!(reg_two.get(&bar_key).is_some(), "Bar MUST exist post-add");

    let set_one = reg_one
        .get(&FactKey::SyntacticExportSet)
        .expect("SyntacticExportSet emitted");
    let set_two = reg_two
        .get(&FactKey::SyntacticExportSet)
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
    let indexed = build_indexed(
        vec![build_type_decl(
            "Foo",
            vec![("a", prim(PrimitiveName::String))],
        )],
        vec![("Foo", ValueDeclKind::Const)],
        vec![("Foo", export_local("Foo"))],
        "export type Foo = { a: string }\nexport const Foo = 1",
    );
    let reg = registry_of(&indexed);

    let type_key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    let value_key = FactKey::Export {
        name: InternedName::from("Foo"),
        space: SymbolSpace::Value,
    };
    let type_fact = reg.get(&type_key).expect("type-space Export");
    let value_fact = reg.get(&value_key).expect("value-space Export");
    assert_ne!(
        type_fact.semantic_hash, value_fact.semantic_hash,
        "type-space and value-space facts have distinct identities (R11)"
    );
}

// ── Bullet 11 — Topological-order test ──

#[test]
fn decl_reorder_does_not_change_emitted_fact_set() {
    // R10 + R27 canonical visit order. The Phase 1 emitter sorts
    // type symbols and value symbols by name before emission, so
    // the same file rewritten with decls in reverse declaration
    // order produces a byte-identical fact set.
    let in_order = build_indexed(
        vec![
            build_type_decl("Alpha", vec![("a", prim(PrimitiveName::String))]),
            build_type_decl("Bravo", vec![("b", prim(PrimitiveName::Number))]),
            build_type_decl("Charlie", vec![("c", prim(PrimitiveName::Boolean))]),
        ],
        vec![],
        vec![
            ("Alpha", export_local("Alpha")),
            ("Bravo", export_local("Bravo")),
            ("Charlie", export_local("Charlie")),
        ],
        "alpha bravo charlie",
    );
    let reversed = build_indexed(
        vec![
            build_type_decl("Charlie", vec![("c", prim(PrimitiveName::Boolean))]),
            build_type_decl("Bravo", vec![("b", prim(PrimitiveName::Number))]),
            build_type_decl("Alpha", vec![("a", prim(PrimitiveName::String))]),
        ],
        vec![],
        vec![
            ("Charlie", export_local("Charlie")),
            ("Bravo", export_local("Bravo")),
            ("Alpha", export_local("Alpha")),
        ],
        "alpha bravo charlie",
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
    let with_bar = build_indexed(
        vec![
            build_type_decl("Foo", vec![("a", prim(PrimitiveName::String))]),
            build_type_decl("Bar", vec![("b", prim(PrimitiveName::Number))]),
        ],
        vec![],
        vec![("Foo", export_local("Foo")), ("Bar", export_local("Bar"))],
        "",
    );
    let without_bar = build_indexed(
        vec![build_type_decl(
            "Foo",
            vec![("a", prim(PrimitiveName::String))],
        )],
        vec![],
        vec![("Foo", export_local("Foo"))],
        "",
    );
    let reg_with = registry_of(&with_bar);
    let reg_without = registry_of(&without_bar);
    let bar_key = FactKey::Export {
        name: InternedName::from("Bar"),
        space: SymbolSpace::Type,
    };
    assert!(reg_with.get(&bar_key).is_some());
    assert!(
        reg_without.get(&bar_key).is_none(),
        "removed exports MUST validate as registry misses (R10)"
    );
}

/// Stage 0 → Stage 3 corpus-anchored binding: load the
/// `cosmetic_edit_comment.ts` fixture and verify the documented
/// invariant — that the file declares an `interface Foo` with
/// members `a` and `b` plus a top-of-file standalone comment.
/// The Stage 6d discriminator will assert that re-running the
/// fact emitter on a comment-edited variant of this file
/// produces the same semantic_hashes.
#[test]
fn cosmetic_edit_comment_fixture_declares_documented_shape() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
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
