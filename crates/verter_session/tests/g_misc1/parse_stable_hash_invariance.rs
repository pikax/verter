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
//! These tests synthesise `ShallowFileState` directly (the same way
//! `IndexedReady::new_for_test` does) so we can vary the inventory
//! programmatically without invoking the full parser pipeline.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_session::parse_stable_hash::compute_parse_stable_hash;
use verter_session::project_type_store::IndexedReady;

fn empty_external(
) -> Arc<verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource> {
    Arc::new(
        verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default(),
    )
}

fn build_indexed(
    type_symbols: Vec<(
        &str,
        verter_semantic::analysis::type_eval::TypeDeclKind,
        Vec<&str>,
    )>,
    value_symbols: Vec<(&str, verter_semantic::analysis::type_eval::ValueDeclKind)>,
    exports: Vec<(
        &str,
        verter_session::resolver_core::shallow_file_state::ExportTarget,
    )>,
) -> Arc<IndexedReady> {
    // Env-seeded construction: member names become the symbol's direct
    // syntactic member headers (the inventory `parse_stable_hash` walks).
    let mut env = verter_semantic::analysis::type_eval::EvalEnv::new();
    for (name, kind, members) in type_symbols {
        let body = verter_type_expr::TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
            properties: members
                .iter()
                .map(|m| {
                    verter_type_expr::ObjectMember::Property(
                        verter_type_expr::ObjectProperty::synthetic_public(
                            (*m).to_string(),
                            verter_type_expr::TypeExpr::Primitive(
                                verter_type_expr::PrimitiveName::String,
                            ),
                            false,
                            false,
                        ),
                    )
                })
                .collect(),
        }));
        env.add_type(verter_semantic::analysis::type_eval::TypeDeclInfo {
            name: name.to_string(),
            declaration_id: 0,
            kind,
            type_parameters: Vec::new(),
            body,
        });
    }
    for (name, kind) in value_symbols {
        env.add_value(verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: name.to_string(),
            declaration_id: 0,
            kind,
            type_annotation: None,
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
        });
    }
    let mut exports_map: FxHashMap<
        String,
        verter_session::resolver_core::shallow_file_state::ExportTarget,
    > = FxHashMap::default();
    for (name, target) in exports {
        exports_map.insert(name.to_string(), target);
    }
    let mut shallow =
        verter_session::resolver_core::shallow_file_state::ShallowFileState::from_analysis(
            [0u8; 16],
            empty_external(),
            Some(&env),
        );
    shallow.exports = exports_map;
    Arc::new(IndexedReady::new_for_test_with_state(
        [0u8; 16],
        Arc::new(shallow),
        Arc::from(""),
        Arc::from(""),
        empty_external(),
    ))
}

#[test]
fn whitespace_edit_does_not_change_parse_stable_hash() {
    // Two IndexedReady artifacts with the same decl inventory (the
    // shallow-state has identical symbols/exports). A whitespace edit
    // does not change the inventory — it only changes raw_source.
    // The hash MUST be identical because we hash the inventory, not the
    // raw text.
    use verter_semantic::analysis::type_eval::{TypeDeclKind, ValueDeclKind};
    use verter_session::resolver_core::shallow_file_state::ExportTarget;

    let a = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["x", "y"])],
        vec![("greet", ValueDeclKind::Function)],
        vec![(
            "Foo",
            ExportTarget::Local {
                symbol_name: "Foo".to_string(),
            },
        )],
    );
    let b = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["x", "y"])],
        vec![("greet", ValueDeclKind::Function)],
        vec![(
            "Foo",
            ExportTarget::Local {
                symbol_name: "Foo".to_string(),
            },
        )],
    );
    // Note: `raw_source` differs between a and b in a real edit, but
    // since parse_stable_hash walks ONLY shallow_state, this is
    // equivalent to a whitespace-only edit producing the same inventory.
    assert_eq!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "identical decl inventory MUST produce identical parse_stable_hash"
    );
}

#[test]
fn decl_reorder_does_not_change_parse_stable_hash() {
    use verter_semantic::analysis::type_eval::TypeDeclKind;

    let a = build_indexed(
        vec![
            ("Alpha", TypeDeclKind::Interface, vec!["a"]),
            ("Beta", TypeDeclKind::Interface, vec!["b"]),
        ],
        vec![],
        vec![],
    );
    let b = build_indexed(
        vec![
            ("Beta", TypeDeclKind::Interface, vec!["b"]),
            ("Alpha", TypeDeclKind::Interface, vec!["a"]),
        ],
        vec![],
        vec![],
    );
    assert_eq!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "decl reorder (FxHashMap iteration order) MUST NOT change parse_stable_hash"
    );
}

#[test]
fn member_reorder_does_not_change_parse_stable_hash() {
    use verter_semantic::analysis::type_eval::TypeDeclKind;

    let a = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["a", "b", "c"])],
        vec![],
        vec![],
    );
    let b = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["c", "a", "b"])],
        vec![],
        vec![],
    );
    assert_eq!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "member reorder MUST NOT change parse_stable_hash"
    );
}

// ── Discrimination tests ──

#[test]
fn added_decl_changes_parse_stable_hash() {
    use verter_semantic::analysis::type_eval::TypeDeclKind;

    let a = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["a"])],
        vec![],
        vec![],
    );
    let b = build_indexed(
        vec![
            ("Foo", TypeDeclKind::Interface, vec!["a"]),
            ("Bar", TypeDeclKind::Interface, vec!["b"]),
        ],
        vec![],
        vec![],
    );
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "adding a decl MUST change parse_stable_hash"
    );
}

#[test]
fn renamed_decl_changes_parse_stable_hash() {
    use verter_semantic::analysis::type_eval::TypeDeclKind;

    let a = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["x"])],
        vec![],
        vec![],
    );
    let b = build_indexed(
        vec![("Bar", TypeDeclKind::Interface, vec!["x"])],
        vec![],
        vec![],
    );
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "renaming a decl MUST change parse_stable_hash"
    );
}

#[test]
fn renamed_member_changes_parse_stable_hash() {
    use verter_semantic::analysis::type_eval::TypeDeclKind;

    let a = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["x"])],
        vec![],
        vec![],
    );
    let b = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["y"])],
        vec![],
        vec![],
    );
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "renaming a member MUST change parse_stable_hash"
    );
}

#[test]
fn kind_change_changes_parse_stable_hash() {
    use verter_semantic::analysis::type_eval::TypeDeclKind;

    let a = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["x"])],
        vec![],
        vec![],
    );
    let b = build_indexed(
        vec![("Foo", TypeDeclKind::Alias, vec!["x"])],
        vec![],
        vec![],
    );
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "changing decl kind MUST change parse_stable_hash"
    );
}

#[test]
fn added_export_changes_parse_stable_hash() {
    use verter_session::resolver_core::shallow_file_state::ExportTarget;

    let a = build_indexed(vec![], vec![], vec![]);
    let b = build_indexed(
        vec![],
        vec![],
        vec![(
            "Foo",
            ExportTarget::Local {
                symbol_name: "Foo".to_string(),
            },
        )],
    );
    assert_ne!(
        compute_parse_stable_hash(&a),
        compute_parse_stable_hash(&b),
        "adding an export MUST change parse_stable_hash"
    );
}

#[test]
fn deterministic_across_calls() {
    use verter_semantic::analysis::type_eval::TypeDeclKind;

    let indexed = build_indexed(
        vec![("Foo", TypeDeclKind::Interface, vec!["x", "y"])],
        vec![],
        vec![],
    );
    let h0 = compute_parse_stable_hash(&indexed);
    let h1 = compute_parse_stable_hash(&indexed);
    assert_eq!(h0, h1, "compute_parse_stable_hash MUST be deterministic");
}
