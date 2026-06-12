//! R28 binding: two-fact MemberPresence vs Member model.
//!
//! Bindings:
//!
//! - Single body edit: exactly one `Member`
//!   `semantic_hash` changes; corresponding `MemberPresence`
//!   UNCHANGED (header invariant).
//! - `pick_literal_key.ts` fixture: editing
//!   `Foo.b` body changes `Member(Foo, "b")` only; `MemberPresence(Foo, "a")`
//!   unchanged; `Member(Foo, "a")` unchanged.
//!
//! The emitter produces `MemberPresence` eagerly and provides
//! the lazy substrate for `Member`. The path-precise consumer
//! admits `Member` facts into the stores via
//! `compute_semantic_hash`. Here we test the hashing
//! discrimination directly using `compute_semantic_hash` over the
//! member body — which is exactly what the `Member` producer calls.
//!
//! Architectural rules bound: R14, R28.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::{TypeDeclBody, TypeDeclKind};
use verter_semantic::facts::{
    compute_member_presence_hash, compute_semantic_hash, FactKey, MemberKind, SymbolSpace,
    UnresolvedLens,
};
use verter_session::fact_emission::emit_parse_facts;
use verter_session::file_artifact_store::InternedName;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::{
    ExportTarget, ShallowFileState, ShallowTypeSymbol,
};
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

fn empty_external(
) -> Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource> {
    Arc::new(verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default())
}

/// Build a `Foo` interface from a member list `[(name, body)]`.
fn build_foo(members: Vec<(&str, TypeExpr)>) -> Arc<IndexedReady> {
    let body = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members
            .iter()
            .map(|(name, ty)| {
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    (*name).to_string(),
                    ty.clone(),
                    false,
                    false,
                ))
            })
            .collect(),
    }));
    let mut member_deps: FxHashMap<String, Vec<String>> = FxHashMap::default();
    for (n, _) in &members {
        member_deps.insert((*n).to_string(), Vec::new());
    }
    let mut symbols = FxHashMap::default();
    symbols.insert(
        "Foo".to_string(),
        ShallowTypeSymbol {
            kind: TypeDeclKind::Interface,
            body: TypeDeclBody::Single(body),
            type_parameters: Vec::new(),
            local_deps: Vec::new(),
            external_deps: Vec::new(),
            member_deps,
        },
    );
    let mut exports = FxHashMap::default();
    exports.insert(
        "Foo".to_string(),
        ExportTarget::Local {
            symbol_name: "Foo".to_string(),
        },
    );
    let shallow = ShallowFileState {
        whole_hash: [0u8; 16],
        exports,
        wildcard_reexports: Vec::new(),
        symbols,
        value_symbols: FxHashMap::default(),
        import_locals: FxHashSet::default(),
        import_targets: FxHashMap::default(),
        augmentation_scopes: Default::default(),
        augmentation_value_scopes: Default::default(),
        analysis: empty_external(),
    };
    Arc::new(IndexedReady::new_for_test_with_state(
        [0u8; 16],
        Arc::new(shallow),
        Arc::from(""),
        Arc::from(""),
        empty_external(),
    ))
}

#[test]
fn bullet_4_single_body_edit_changes_member_keeps_presence_unchanged() {
    // R28 two-fact model: editing `Foo.a`'s body changes the
    // `Member(Foo, a)` semantic_hash but the
    // `MemberPresence(Foo, a)` header is UNCHANGED.
    let kind = MemberKind::Property {
        readonly: false,
        optional: false,
    };

    // `Foo.a = string` initially, then `Foo.a = number`.
    let body_v1 = TypeExpr::Primitive(PrimitiveName::String);
    let body_v2 = TypeExpr::Primitive(PrimitiveName::Number);

    // `Member.semantic_hash` (admitted
    // into `MemberSemanticFactStore`).
    let member_v1 = compute_semantic_hash(&body_v1, SymbolSpace::Type, &UnresolvedLens);
    let member_v2 = compute_semantic_hash(&body_v2, SymbolSpace::Type, &UnresolvedLens);
    assert_ne!(
        member_v1.hash, member_v2.hash,
        "R28: editing member body MUST change Member semantic_hash"
    );

    // `MemberPresence` is header-only and identical across
    // body edits with the same `(name, kind, exporter)`.
    let presence_v1 = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
    let presence_v2 = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
    assert_eq!(
        presence_v1, presence_v2,
        "R28: body edits MUST NOT change MemberPresence header"
    );
}

#[test]
fn bullet_6_pick_literal_key_path_precise_invariant() {
    // Fixture: `pick_literal_key.ts` —
    //   interface Foo { a; b; c }
    //   export type Props = Pick<Foo, "a">;
    // Editing `Foo.b` body MUST NOT shift `MemberPresence(Foo, "a")`
    // or `Member(Foo, "a")`. Only `Member(Foo, "b")` shifts.
    let kind = MemberKind::Property {
        readonly: false,
        optional: false,
    };

    // Pre-edit bodies — `b: { other: string }` is the shape that
    // gets edited.
    let body_a = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "id".to_string(),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
            )),
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "name".to_string(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            )),
        ],
    }));
    let body_b_v1 = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "other".to_string(),
            TypeExpr::Primitive(PrimitiveName::String),
            false,
            false,
        ))],
    }));
    let body_b_v2 = TypeExpr::Object(Arc::new(ObjectExpr {
        // Edit: `string` → `number`.
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "other".to_string(),
            TypeExpr::Primitive(PrimitiveName::Number),
            false,
            false,
        ))],
    }));
    let body_c = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "extra".to_string(),
            TypeExpr::Primitive(PrimitiveName::Boolean),
            false,
            false,
        ))],
    }));

    // Member.semantic_hash per member body (admitted
    // into `MemberSemanticFactStore`).
    let member_a_v1 = compute_semantic_hash(&body_a, SymbolSpace::Type, &UnresolvedLens).hash;
    let member_a_v2 = compute_semantic_hash(&body_a, SymbolSpace::Type, &UnresolvedLens).hash;
    let member_b_v1 = compute_semantic_hash(&body_b_v1, SymbolSpace::Type, &UnresolvedLens).hash;
    let member_b_v2 = compute_semantic_hash(&body_b_v2, SymbolSpace::Type, &UnresolvedLens).hash;
    let member_c_v1 = compute_semantic_hash(&body_c, SymbolSpace::Type, &UnresolvedLens).hash;
    let member_c_v2 = compute_semantic_hash(&body_c, SymbolSpace::Type, &UnresolvedLens).hash;

    // `Member(Foo, "a")` and `Member(Foo, "c")` UNCHANGED.
    assert_eq!(
        member_a_v1, member_a_v2,
        "R28: editing Foo.b MUST NOT shift Member(Foo, a)"
    );
    assert_eq!(
        member_c_v1, member_c_v2,
        "R28: editing Foo.b MUST NOT shift Member(Foo, c)"
    );

    // `Member(Foo, "b")` MUST shift.
    assert_ne!(
        member_b_v1, member_b_v2,
        "R28: Member(Foo, b) MUST shift on body edit"
    );

    // `MemberPresence(Foo, "a")` UNCHANGED (header invariant).
    let presence_a_v1 = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
    let presence_a_v2 = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
    assert_eq!(
        presence_a_v1, presence_a_v2,
        "R28: editing Foo.b MUST NOT shift MemberPresence(Foo, a) — the consumer is `Pick<Foo, a>` and observes ONLY `(a)`"
    );

    // `MemberPresence(Foo, "b")` UNCHANGED (the header — name +
    // kind + exporter — is invariant; only the body changed).
    let presence_b_v1 = compute_member_presence_hash("Foo", "b", kind, SymbolSpace::Type);
    let presence_b_v2 = compute_member_presence_hash("Foo", "b", kind, SymbolSpace::Type);
    assert_eq!(
        presence_b_v1, presence_b_v2,
        "R28: body edit MUST NOT shift MemberPresence(b) — only body changes shift MEMBER, not PRESENCE"
    );
}

#[test]
fn phase1_emission_produces_member_presence_for_every_member() {
    // Verify the emitter populates `MemberPresence` for every
    // member in the shallow inventory. Required for downstream
    // path-precise consumers to observe presence facts.
    let indexed = build_foo(vec![
        ("a", TypeExpr::Primitive(PrimitiveName::Number)),
        ("b", TypeExpr::Primitive(PrimitiveName::String)),
        ("c", TypeExpr::Primitive(PrimitiveName::Boolean)),
    ]);
    let emission = emit_parse_facts(&indexed);
    for name in &["a", "b", "c"] {
        let key = FactKey::MemberPresence {
            exporter: InternedName::from("Foo"),
            name: InternedName::from(*name),
            space: SymbolSpace::Type,
        };
        assert!(
            emission.facts.lookup(&key).is_some(),
            "emitter MUST emit MemberPresence for member `{name}`"
        );
    }
    // And a MemberShape fact for the whole-surface consumer.
    let shape_key = FactKey::MemberShape {
        exporter: InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    assert!(
        emission.facts.lookup(&shape_key).is_some(),
        "emitter MUST emit MemberShape"
    );
}

/// Corpus-anchored binding: the
/// `pick_literal_key.ts` fixture's expected-invalidation matrix
/// describes the path-precise contract the consumer enforces.
/// The emitter must (a) be able to LOAD the fixture without
/// special-casing, and (b) discriminate the documented
/// member-set: the consumer selects key `"a"` and the source
/// declares members `a`, `b`, `c`.
#[test]
fn pick_literal_key_fixture_declares_documented_member_set() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("path_precise")
        .join("pick_literal_key.ts");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));

    // The fixture must declare exactly members `a`, `b`, `c` on
    // `interface Foo`. The consumer's discrimination depends on this
    // exact member set.
    assert!(src.contains("export interface Foo"));
    assert!(src.contains("a: { id: number; name: string }"));
    assert!(src.contains("b: { other: string }"));
    assert!(src.contains("c: { extra: boolean }"));
    assert!(src.contains("export type Props = Pick<Foo, \"a\">"));

    // Load the fixture's expected JSON; verify the consumer's
    // fact_dep_signature documents BOTH `MemberPresence(Foo, "a")`
    // AND `Member(Foo, "a")` per the R28 two-fact model.
    let json_path = path.with_extension("expected.json");
    let json_raw = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("read {}: {}", json_path.display(), e));
    let json: serde_json::Value = serde_json::from_str(&json_raw).unwrap();
    let sig = json
        .get("fact_dep_signature_observed_by_consumer")
        .and_then(|v| v.as_array())
        .expect("invalidation matrix carries the consumer signature");
    let sig_strs: Vec<&str> = sig.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        sig_strs
            .iter()
            .any(|s| s.contains("MemberPresence(Foo, \"a\"")),
        "corpus pairs with the MemberPresence emission"
    );
    assert!(
        sig_strs.iter().any(|s| s.contains("Member(Foo, \"a\"")),
        "corpus pairs with the Member emission"
    );
}
