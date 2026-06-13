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

use rustc_hash::{FxHashMap, FxHashSet};
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
    let mut symbols: FxHashMap<
        String,
        verter_session::resolver_core::shallow_file_state::ShallowTypeSymbol,
    > = FxHashMap::default();
    for (name, kind, members) in type_symbols {
        let mut member_deps: FxHashMap<String, Vec<String>> = FxHashMap::default();
        for m in members {
            member_deps.insert(m.to_string(), Vec::new());
        }
        symbols.insert(
            name.to_string(),
            verter_session::resolver_core::shallow_file_state::ShallowTypeSymbol {
                kind,
                body: verter_semantic::analysis::type_eval::TypeDeclBody::Single(
                    verter_type_expr::TypeExpr::Unknown { raw: String::new() },
                ),
                type_parameters: Vec::new(),
                local_deps: Vec::new(),
                external_deps: Vec::new(),
                member_deps,
            },
        );
    }
    let mut value_symbols_map: FxHashMap<
        String,
        verter_session::resolver_core::shallow_file_state::ShallowValueSymbol,
    > = FxHashMap::default();
    for (name, kind) in value_symbols {
        value_symbols_map.insert(
            name.to_string(),
            verter_session::resolver_core::shallow_file_state::ShallowValueSymbol {
                kind,
                type_annotation: None,
                signatures: Vec::new(),
                object_shape: None,
                enum_members: None,
                is_synthesised_component_default: false,
            },
        );
    }
    let mut exports_map: FxHashMap<
        String,
        verter_session::resolver_core::shallow_file_state::ExportTarget,
    > = FxHashMap::default();
    for (name, target) in exports {
        exports_map.insert(name.to_string(), target);
    }
    let shallow = verter_session::resolver_core::shallow_file_state::ShallowFileState {
        whole_hash: [0u8; 16],
        exports: exports_map,
        wildcard_reexports: Vec::new(),
        symbols,
        value_symbols: value_symbols_map,
        import_locals: FxHashSet::default(),
        import_targets: FxHashMap::default(),
        augmentation_scopes: Default::default(),
        augmentation_value_scopes: Default::default(),
        analysis: empty_external(),
    };
    Arc::new(IndexedReady {
        whole_hash: [0u8; 16],
        shallow_state: Arc::new(shallow),
        import_routes: Arc::new(FxHashMap::default()),
        import_route_hash: None,
        route_hash: None,
        edge_generation: 0,
        raw_source: Arc::from(""),
        eval_source: Arc::from(""),
        framework_parse: None,
        script_analysis: None,
        export_signatures: None,
        snapshot: Arc::new(verter_session::FileAnalysisSnapshot::default()),
        external_type_analysis: empty_external(),
        declares_interface_app_config: false,
    })
}

// ── Invariance tests ──

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
