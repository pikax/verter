use super::*;
use crate::resolver_core::declaration_metadata::ResolvedExportTarget;
use std::collections::BTreeMap;
use verter_compiler::utils::oxc::vue::resolve_type::{
    ResolvedEmit, ResolvedEmitSignature, ResolvedMemberVisibility, ResolvedProp, RuntimeType,
};
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::types::{
    AnalyzedImport, AnalyzedImportBinding, AnalyzedMacro, AnalyzedMacroKind, ImportBindingKind,
    ResolvedLocalType,
};
use verter_span::Span;
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

#[derive(Clone)]
struct TestSnapshot {
    imports: Vec<AnalyzedImport>,
    macros: Vec<AnalyzedMacro>,
    macro_type_deps: Vec<verter_semantic::analysis::types::MacroTypeDep>,
}

struct TestHost {
    external_macro_elements: BTreeMap<(String, String), ResolvedElements>,
    eval_outputs: ComponentMetaEvalOutputs,
    projectable_owner_local_roots: BTreeSet<String>,
    // Owner-local roots that project to a NON-EMPTY prepared surface. The
    // cold resolver gates the owner-local
    // authority entry on a boolean ("does this root have a surface?") via
    // `owner_local_macro_root_has_surface` (covering `DefineExpose` alongside
    // the other macro kinds); the published props/emits/slots/exposed
    // surface itself is owned by the typeinfo path. This set mirrors that
    // gate: a root present here returns `true`.
    owner_local_roots_with_surface: BTreeSet<String>,
}

impl crate::resolver_core::DeclarationMetadataResolver for TestHost {
    fn resolve_export_target(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<ResolvedExportTarget> {
        None
    }

    fn get_export_span_follow_reexports(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<Span> {
        None
    }

    fn type_declaration_id(
        &self,
        _canonical_source: &str,
        _resolved_name: &str,
    ) -> Option<DeclarationId> {
        None
    }

    fn resolve_type_dependency_canonical(
        &self,
        _from_canonical: &str,
        _import_source: &str,
    ) -> Option<String> {
        None
    }
}

impl ComponentMetaResolverHost for TestHost {
    type Snapshot = TestSnapshot;
    type EvalContext = ();

    fn snapshot_imports<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedImport] {
        &snapshot.imports
    }

    fn snapshot_macros<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedMacro] {
        &snapshot.macros
    }

    fn snapshot_macro_type_deps<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::MacroTypeDep] {
        &snapshot.macro_type_deps
    }

    fn build_eval_outputs(
        &self,
        _owner_canonical: &str,
        _snapshot: &Self::Snapshot,
        _eval_context: Option<&Self::EvalContext>,
        _purpose: ComponentMetaResolutionPurpose,
    ) -> ComponentMetaEvalOutputs {
        self.eval_outputs.clone()
    }

    fn projectable_owner_local_macro_roots(
        &self,
        _owner_canonical: &str,
        mac: &AnalyzedMacro,
    ) -> Vec<String> {
        self.projectable_owner_local_roots
            .iter()
            .filter(|root_name| mac.type_references.iter().any(|name| name == *root_name))
            .cloned()
            .collect()
    }

    fn owner_local_macro_root_has_surface(
        &self,
        _owner_canonical: &str,
        root_name: &str,
        _macro_kind: AnalyzedMacroKind,
    ) -> bool {
        self.owner_local_roots_with_surface.contains(root_name)
    }

    fn resolve_macro_elements(
        &self,
        _owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        _tracked_deps: &mut BTreeSet<String>,
        _resolution_deps: &mut BTreeSet<String>,
        _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        _visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedElements> {
        self.external_macro_elements
            .get(&(import_source.to_string(), exported_name.to_string()))
            .cloned()
    }

    fn resolve_jsdoc_block(
        &self,
        _canonical_source: &str,
        _span: Span,
        _expanded: bool,
        _tracked_deps: &mut BTreeSet<String>,
        _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        _visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedJsdocBlock> {
        None
    }

    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        canonical_id.contains("/node_modules/")
    }

    fn sync_transitive_macro_type_dependencies(
        &self,
        _canonical_id: &str,
        _tracked_deps: &BTreeSet<String>,
    ) {
    }

    fn current_dependency_fact_versions(
        &self,
        _canonical: &str,
        _tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef> {
        Vec::new()
    }
}

struct CombinedSurfaceTestHost {
    imported_surface_calls: std::cell::Cell<usize>,
    eval_outputs: ComponentMetaEvalOutputs,
}

impl crate::resolver_core::DeclarationMetadataResolver for CombinedSurfaceTestHost {
    fn resolve_export_target(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<ResolvedExportTarget> {
        None
    }

    fn get_export_span_follow_reexports(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<Span> {
        None
    }

    fn type_declaration_id(
        &self,
        _canonical_source: &str,
        _resolved_name: &str,
    ) -> Option<DeclarationId> {
        None
    }

    fn resolve_type_dependency_canonical(
        &self,
        _from_canonical: &str,
        _import_source: &str,
    ) -> Option<String> {
        Some("/dep.ts".to_string())
    }
}

impl ComponentMetaResolverHost for CombinedSurfaceTestHost {
    type Snapshot = TestSnapshot;
    type EvalContext = ();

    fn resolve_type_declaration(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        panic!("resolve_component_meta_parts should use the combined imported-macro surface path");
    }

    fn snapshot_imports<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedImport] {
        &snapshot.imports
    }

    fn snapshot_macros<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedMacro] {
        &snapshot.macros
    }

    fn snapshot_macro_type_deps<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::MacroTypeDep] {
        &snapshot.macro_type_deps
    }

    fn build_eval_outputs(
        &self,
        _owner_canonical: &str,
        _snapshot: &Self::Snapshot,
        _eval_context: Option<&Self::EvalContext>,
        _purpose: ComponentMetaResolutionPurpose,
    ) -> ComponentMetaEvalOutputs {
        self.eval_outputs.clone()
    }

    fn resolve_macro_elements(
        &self,
        _owner_canonical: &str,
        _import_source: &str,
        _exported_name: &str,
        _tracked_deps: &mut BTreeSet<String>,
        _resolution_deps: &mut BTreeSet<String>,
        _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        _visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedElements> {
        panic!(
            "resolve_component_meta_parts should not separately ask for imported macro elements"
        );
    }

    fn resolve_imported_macro_surface(
        &self,
        _owner_canonical: &str,
        _import_source: &str,
        exported_name: &str,
        _tracked_deps: &mut BTreeSet<String>,
        _resolution_deps: &mut BTreeSet<String>,
        _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        _visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedImportedMacroSurface> {
        self.imported_surface_calls
            .set(self.imported_surface_calls.get() + 1);
        Some(ResolvedImportedMacroSurface {
            declaration: ResolvedTypeDeclaration {
                requested_name: exported_name.to_string(),
                declaration_id: None,
                resolved_name: "Props".to_string(),
                canonical_source: "/dep.ts".to_string(),
                span: Span::new(0, 29),
                kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
                text: Some("export interface Props { label: string }".to_string()),
            },
            elements: ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 29),
                    key: Span::new(24, 29),
                    key_name: Some("label".to_string()),
                    optional: false,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: Some(Span::new(31, 37)),
                    type_text: Some("string".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                    declared_in_macro_type_arg: false,
                }],
                emits: vec![ResolvedEmit {
                    span: Span::new(0, 24),
                    name: "save".to_string(),
                    name_span: None,
                    signature: ResolvedEmitSignature::Tuple {
                        tuple_text: "[value: string]".to_string(),
                    },
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: Some(TypeExpr::Unknown {
                        raw: "[value: string]".to_string(),
                    }),
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                }],
                ..ResolvedElements::default()
            },
        })
    }

    fn resolve_jsdoc_block(
        &self,
        _canonical_source: &str,
        _span: Span,
        _expanded: bool,
        _tracked_deps: &mut BTreeSet<String>,
        _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        _visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedJsdocBlock> {
        None
    }

    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        canonical_id.contains("/node_modules/")
    }

    fn sync_transitive_macro_type_dependencies(
        &self,
        _canonical_id: &str,
        _tracked_deps: &BTreeSet<String>,
    ) {
    }

    fn current_dependency_fact_versions(
        &self,
        _canonical: &str,
        _tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef> {
        Vec::new()
    }
}

#[test]
fn resolve_component_meta_parts_prefers_combined_imported_macro_surface() {
    let host = CombinedSurfaceTestHost {
        imported_surface_calls: std::cell::Cell::new(0),
        eval_outputs: ComponentMetaEvalOutputs::default(),
    };
    let snapshot = TestSnapshot {
        imports: vec![AnalyzedImport {
            source: "./dep".to_string(),
            is_type_only: true,
            bindings: vec![AnalyzedImportBinding {
                name: "Props".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: Some("Props".to_string()),
                is_type_only: true,
                vue_api: None,
                span: Span::new(0, 5),
            }],
            span: Span::new(0, 26),
            resolved_canonical_id: Some("/dep.ts".to_string()),
        }],
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 20),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            macro_index: 0,
            import_source: "./dep".to_string(),
            type_name: "Props".to_string(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_span: Span::new(0, 20),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert_eq!(
        host.imported_surface_calls.get(),
        1,
        "expanded imported-macro resolution should use the combined surface path exactly once",
    );
    assert_eq!(resolved.resolved_macros.len(), 1);
    // The combined imported-macro surface still feeds `native_props` (the
    // class-member visibility carrier). The published props surface is owned
    // by the typeinfo path and is not carried on `ResolvedMacroMeta`.
    assert_eq!(
        resolved.resolved_macros[0]
            .native_props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["label"],
    );
    assert_eq!(
        resolved.resolved_macros[0].declaration.canonical_source, "/dep.ts",
        "combined imported-macro resolution should still preserve declaration ownership",
    );
}

#[test]
fn resolve_component_meta_parts_fallthrough_reuses_combined_imported_macro_surface() {
    let host = CombinedSurfaceTestHost {
        imported_surface_calls: std::cell::Cell::new(0),
        eval_outputs: ComponentMetaEvalOutputs::default(),
    };
    let snapshot = TestSnapshot {
        imports: vec![AnalyzedImport {
            source: "./dep".to_string(),
            is_type_only: true,
            bindings: vec![AnalyzedImportBinding {
                name: "Emits".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: Some("Emits".to_string()),
                is_type_only: true,
                vue_api: None,
                span: Span::new(0, 5),
            }],
            span: Span::new(0, 26),
            resolved_canonical_id: Some("/dep.ts".to_string()),
        }],
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["Emits".to_string()],
            binding_name: Some("emit".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 20),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            macro_index: 0,
            import_source: "./dep".to_string(),
            type_name: "Emits".to_string(),
            macro_kind: AnalyzedMacroKind::DefineEmits,
            macro_span: Span::new(0, 20),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Fallthrough,
    );

    assert_eq!(
        host.imported_surface_calls.get(),
        1,
        "fallthrough imported-macro resolution should still use the combined surface path",
    );
    assert_eq!(resolved.resolved_macros.len(), 1);
    assert_eq!(
        resolved.resolved_macros[0].macro_kind,
        AnalyzedMacroKind::DefineEmits
    );
    assert_eq!(resolved.resolved_macros[0].type_name, "Emits");
    assert_eq!(
        resolved.resolved_macros[0].declaration.canonical_source, "",
        "fallthrough should still skip declaration ownership materialization",
    );
}

#[test]
fn resolve_component_meta_parts_fallthrough_skips_imported_define_emits_when_eval_shape_exists() {
    let host = CombinedSurfaceTestHost {
        imported_surface_calls: std::cell::Cell::new(0),
        eval_outputs: ComponentMetaEvalOutputs {
            evaluated_types: Some(
                verter_semantic::analysis::type_expand::ExpandedComponentTypes {
                    props: Vec::new(),
                    define_props: Vec::new(),
                    define_emits: vec![
                        verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                            macro_index: 0,
                            result: verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                verter_semantic::analysis::type_expand::ExpandedObjectShape {
                                    properties: vec![
                                        verter_semantic::analysis::type_expand::ExpandedProperty {
                                            name: "save".to_string(),
                                            ty: verter_type_expr::TypeExpr::Tuple {
                                                elements: std::sync::Arc::from(vec![
                                                    verter_type_expr::TupleElement {
                                                        label: Some("value".to_string()),
                                                        ty: verter_type_expr::TypeExpr::Primitive(
                                                            verter_type_expr::PrimitiveName::String,
                                                        ),
                                                        optional: false,
                                                        rest: false,
                                                    },
                                                ]),
                                                readonly: false,
                                            },
                                            optional: false,
                                            readonly: false,
                                            visibility: verter_type_expr::MemberVisibility::Public,
                                            declared_in_macro_type_arg: false,
                                        },
                                    ],
                                    index_signatures: Vec::new(),
                                    call_signatures: Vec::new(),
                                },
                            ),
                        },
                    ],
                    emits: Vec::new(),
                    define_slots: Vec::new(),
                    slot_bindings: Vec::new(),
                    bindings: Vec::new(),
                },
            ),
            tracked_dependencies: BTreeSet::new(),
            surface_identities: None,
        },
    };
    let snapshot = TestSnapshot {
        imports: vec![AnalyzedImport {
            source: "./dep".to_string(),
            is_type_only: true,
            bindings: vec![AnalyzedImportBinding {
                name: "Emits".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: Some("Emits".to_string()),
                is_type_only: true,
                vue_api: None,
                span: Span::new(0, 5),
            }],
            span: Span::new(0, 26),
            resolved_canonical_id: Some("/dep.ts".to_string()),
        }],
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["Emits".to_string()],
            binding_name: Some("emit".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 20),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            macro_index: 0,
            import_source: "./dep".to_string(),
            type_name: "Emits".to_string(),
            macro_kind: AnalyzedMacroKind::DefineEmits,
            macro_span: Span::new(0, 20),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Fallthrough,
    );

    assert_eq!(
        host.imported_surface_calls.get(),
        0,
        "fallthrough should keep type-based imported defineEmits on the evaluated shape path when that shape is already authoritative",
    );
    assert!(
        resolved
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "Emits"),
        "fallthrough should not materialize an imported defineEmits surface when evaluated_types already provide the declared events"
    );
    let evaluated = resolved
        .evaluated_types
        .as_ref()
        .expect("the authoritative evaluated defineEmits shape should be preserved");
    assert_eq!(
        evaluated.define_emits.len(),
        1,
        "the evaluated defineEmits shape should still drive downstream extraction",
    );
}

#[test]
fn resolve_component_meta_parts_fallthrough_skips_imported_define_emits_for_local_wrapper_root() {
    let source = r#"
import type { RootEmits } from './dep'

interface Emits extends RootEmits {}

defineEmits<Emits>()
"#;
    let host = CombinedSurfaceTestHost {
        imported_surface_calls: std::cell::Cell::new(0),
        eval_outputs: ComponentMetaEvalOutputs::default(),
    };
    let snapshot = TestSnapshot {
        imports: vec![AnalyzedImport {
            source: "./dep".to_string(),
            is_type_only: true,
            bindings: vec![AnalyzedImportBinding {
                name: "RootEmits".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: Some("RootEmits".to_string()),
                is_type_only: true,
                vue_api: None,
                span: Span::new(0, 9),
            }],
            span: Span::new(0, 34),
            resolved_canonical_id: Some("/dep.ts".to_string()),
        }],
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["Emits".to_string()],
            binding_name: Some("emit".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "Emits".to_string(),
                expanded: "interface Emits extends RootEmits {}".to_string(),
                type_expr: Some(
                    verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(
                        "interface Emits extends RootEmits {}",
                        None,
                    ),
                ),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            macro_index: 0,
            import_source: "./dep".to_string(),
            type_name: "RootEmits".to_string(),
            macro_kind: AnalyzedMacroKind::DefineEmits,
            macro_span: Span::new(0, source.len() as u32),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Fallthrough,
    );

    assert_eq!(
        host.imported_surface_calls.get(),
        0,
        "fallthrough should leave transitive imported defineEmits deps lazy when an owner-local wrapper root will drive object-shape projection later",
    );
    assert!(
        resolved
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "RootEmits"),
        "fallthrough should keep the imported defineEmits root off resolved_macros when the owner-local wrapper is the requested route",
    );
}

#[test]
fn local_resolved_macro_types_push_authoritative_owner_local_entry() {
    // An owner-local emit root that projects to a non-empty surface produces
    // an authoritative `ResolvedMacroMeta` entry (the cold resolver's
    // contract). The published emit payload itself is NOT carried on
    // `ResolvedMacroMeta` — it is owned by the typeinfo
    // macro-surface path (covered in `typeinfo_tests::vue_adapter`); this
    // test pins the entry identity + authority the materialiser gates on.
    let host = TestHost {
        external_macro_elements: BTreeMap::new(),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["AccordionEmits".to_string()]),
        owner_local_roots_with_surface: BTreeSet::from(["AccordionEmits".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["AccordionEmits".to_string()],
            binding_name: Some("emit".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "AccordionEmits".to_string(),
                expanded:
                    "{ 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined] }"
                        .to_string(),
                type_expr: Some(verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(
                    "{ 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined] }", None)),
                span: Span::new(0, 1),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 1),
        }],
        macro_type_deps: Vec::new(),
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/Accordion.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert_eq!(resolved.resolved_macros.len(), 1);
    assert_eq!(resolved.resolved_macros[0].macro_index, 0);
    assert_eq!(
        resolved.resolved_macros[0].macro_kind,
        AnalyzedMacroKind::DefineEmits
    );
    assert_eq!(resolved.resolved_macros[0].type_name, "AccordionEmits");
    assert_eq!(
        resolved.resolved_macros[0].declaration.resolved_name,
        "AccordionEmits"
    );
    // Discriminator: the owner-local entry is authoritative. If
    // `owner_local_macro_root_has_surface` returned false (root absent from
    // `owner_local_roots_with_surface`), no entry would be pushed.
    assert!(resolved.resolved_macros[0].surface_is_authoritative);
}

#[test]
fn projectable_local_emit_roots_fill_resolved_macros_without_resolved_local_types() {
    let host = TestHost {
        external_macro_elements: BTreeMap::new(),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["AppEmits".to_string()]),
        owner_local_roots_with_surface: BTreeSet::from(["AppEmits".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["AppEmits".to_string()],
            binding_name: Some("emit".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 1),
        }],
        macro_type_deps: Vec::new(),
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert_eq!(resolved.resolved_macros.len(), 1);
    assert_eq!(resolved.resolved_macros[0].type_name, "AppEmits");
    assert_eq!(
        resolved.resolved_macros[0].macro_kind,
        AnalyzedMacroKind::DefineEmits
    );
    assert!(resolved.resolved_macros[0].surface_is_authoritative);
}

#[test]
fn local_resolved_slot_types_push_authoritative_owner_local_entry() {
    // An owner-local slot root that projects to a non-empty surface produces
    // an authoritative `ResolvedMacroMeta` entry. The slot binding
    // type-resolution (e.g. `Pick<X,'day'>` → leaf `Date`) is owned by the
    // typeinfo macro-surface path and characterized in
    // `typeinfo_tests::vue_adapter::define_slots_normalizer_extracts_pick_bindings`;
    // here we pin the cold resolver's entry-identity contract only.
    let source = r#"
interface CalendarCellTriggerProps {
  day: Date
  month: number
}

interface CalendarSlots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}

defineSlots<CalendarSlots>()
"#;
    let host = TestHost {
        external_macro_elements: BTreeMap::new(),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["CalendarSlots".to_string()]),
        owner_local_roots_with_surface: BTreeSet::from(["CalendarSlots".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            is_type_based: true,
            type_references: vec!["CalendarSlots".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "CalendarSlots".to_string(),
                expanded: "{ day?: (props: { day: Date }) => any }".to_string(),
                type_expr: Some(
                    verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(
                        "{ day?: (props: { day: Date }) => any }",
                        None,
                    ),
                ),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: Vec::new(),
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert_eq!(resolved.resolved_macros.len(), 1);
    assert_eq!(resolved.resolved_macros[0].type_name, "CalendarSlots");
    assert_eq!(
        resolved.resolved_macros[0].macro_kind,
        AnalyzedMacroKind::DefineSlots
    );
    assert!(resolved.resolved_macros[0].surface_is_authoritative);
}

#[test]
fn resolve_component_meta_parts_does_not_preseed_transitive_imported_macro_deps() {
    let source = r#"
type Props = {
  item?: LocalItem
}

type LocalItem = {
  label?: string
}
"#;
    let mut external_macro_elements = BTreeMap::new();
    external_macro_elements.insert(
        ("./types".to_string(), "ImportedBase".to_string()),
        ResolvedElements {
            props: vec![ResolvedProp {
                span: Span::new(0, 0),
                key: Span::new(0, 0),
                key_name: Some("href".to_string()),
                optional: true,
                types: vec![RuntimeType::String],
                visibility: ResolvedMemberVisibility::Public,
                type_span: None,
                type_text: Some("string".to_string()),
                map_local: false,
                span_is_absolute: true,
                type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                declared_in_macro_type_arg: false,
            }],
            ..ResolvedElements::default()
        },
    );
    external_macro_elements.insert(
        ("./types".to_string(), "ImportedKeys".to_string()),
        ResolvedElements {
            props: vec![ResolvedProp {
                span: Span::new(0, 0),
                key: Span::new(0, 0),
                key_name: Some("value".to_string()),
                optional: true,
                types: vec![RuntimeType::String],
                visibility: ResolvedMemberVisibility::Public,
                type_span: None,
                type_text: Some("'href' | 'target'".to_string()),
                map_local: false,
                span_is_absolute: true,
                type_expr: Some(TypeExpr::Unknown {
                    raw: "'href' | 'target'".to_string(),
                }),
                type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                declared_in_macro_type_arg: false,
            }],
            ..ResolvedElements::default()
        },
    );
    let host = TestHost {
        external_macro_elements,
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::new(),
        owner_local_roots_with_surface: BTreeSet::new(),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "Props".to_string(),
                expanded: "{ item?: LocalItem }".to_string(),
                type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                        "item".to_string(),
                        TypeExpr::Ref {
                            name: "LocalItem".into(),
                            type_arguments: Vec::new().into(),
                        },
                        true,
                        false,
                    ))],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: vec![
            verter_semantic::analysis::types::MacroTypeDep {
                type_name: "ImportedBase".to_string(),
                import_source: "./types".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::new(0, source.len() as u32),
            },
            verter_semantic::analysis::types::MacroTypeDep {
                type_name: "ImportedKeys".to_string(),
                import_source: "./types".to_string(),
                macro_kind: AnalyzedMacroKind::DefineProps,
                macro_index: 0,
                macro_span: Span::new(0, source.len() as u32),
            },
        ],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );
    let registry_names: Vec<&str> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert_eq!(
        registry_names,
        vec!["Props"],
        "transitive imported macro deps should stay out of the initial registry seed; later append can publish only what the owner surface actually requests"
    );
}

#[test]
fn resolve_component_meta_parts_skips_transitive_imported_macro_resolution_when_eval_surface_is_authoritative(
) {
    // `authoritative_resolved_local` is now
    // `projectable_owner_local`; configuring `Props` as a
    // projectable owner-local root makes the transitive
    // imported `ImportedBase` dep suppressible alongside the
    // eval-surface authority signal.
    let source = r#"
type Props = Pick<ImportedBase, 'href'>
"#;
    let host = TestHost {
        external_macro_elements: BTreeMap::from([(
            ("./types".to_string(), "ImportedBase".to_string()),
            ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 0),
                    key: Span::new(0, 0),
                    key_name: Some("href".to_string()),
                    optional: true,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: None,
                    type_text: Some("string".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                    declared_in_macro_type_arg: false,
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs {
            evaluated_types: Some(verter_semantic::analysis::type_expand::ExpandedComponentTypes {
                define_props: vec![
                    verter_semantic::analysis::type_expand::ExpandedMacroProps {
                        macro_index: 0,
                        result: verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                            verter_semantic::analysis::type_expand::ExpandedObjectShape {
                                properties: vec![
                                    verter_semantic::analysis::type_expand::ExpandedProperty {
                                        name: "href".to_string(),
                                        ty: TypeExpr::Primitive(PrimitiveName::String),
                                        optional: true,
                                        readonly: false,
                                        visibility: verter_type_expr::MemberVisibility::Public,
                                        declared_in_macro_type_arg: false,
                                    },
                                ],
                                index_signatures: Vec::new(),
                                call_signatures: Vec::new(),
                            },
                        ),
                    },
                ],
                ..Default::default()
}),
            tracked_dependencies: BTreeSet::new(),
            surface_identities: None,
        },
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_roots_with_surface: BTreeSet::from(["Props".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "Props".to_string(),
                expanded: "{ href?: string }".to_string(),
                type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                        "href".to_string(),
                        TypeExpr::Primitive(PrimitiveName::String),
                        true,
                        false,
                    ))],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            type_name: "ImportedBase".to_string(),
            import_source: "./types".to_string(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: Span::new(0, source.len() as u32),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert!(
        resolved
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "ImportedBase"),
        "authoritative owner-evaluated surfaces should keep transitive imported macro deps off resolved_macros: {:?}",
        resolved
            .resolved_macros
            .iter()
            .map(|entry| entry.type_name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn resolve_component_meta_parts_skips_transitive_imported_macro_resolution_when_raw_surface_is_authoritative(
) {
    // `authoritative_resolved_local` is now
    // `projectable_owner_local`; configuring `Props` as a
    // projectable owner-local root makes the owner-local
    // surface authoritative and suppresses the transitive
    // imported `ImportedBase` dep.
    let source = r#"
type Props = Pick<ImportedBase, 'href'>
"#;
    let host = TestHost {
        external_macro_elements: BTreeMap::from([(
            ("./types".to_string(), "ImportedBase".to_string()),
            ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 0),
                    key: Span::new(0, 0),
                    key_name: Some("href".to_string()),
                    optional: true,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: None,
                    type_text: Some("string".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                    declared_in_macro_type_arg: false,
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_roots_with_surface: BTreeSet::from(["Props".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![verter_semantic::analysis::AnalyzedPropField {
                name: "href".to_string(),
                is_optional: true,
                span: Span::new(0, 0),
                type_annotation: Some("string".to_string()),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
                type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                declared_in_macro_type_arg: false,
            }],
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "Props".to_string(),
                expanded: "{ href?: string }".to_string(),
                type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                        "href".to_string(),
                        TypeExpr::Primitive(PrimitiveName::String),
                        true,
                        false,
                    ))],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            type_name: "ImportedBase".to_string(),
            import_source: "./types".to_string(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: Span::new(0, source.len() as u32),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert!(
        resolved
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "ImportedBase"),
        "authoritative owner-local raw fields should keep transitive imported macro deps off resolved_macros: {:?}",
        resolved
            .resolved_macros
            .iter()
            .map(|entry| entry.type_name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn resolve_component_meta_parts_skips_transitive_imported_macro_resolution_when_resolved_local_surface_is_authoritative(
) {
    // `authoritative_resolved_local` is now
    // `projectable_owner_local`; configuring `Props` as a
    // projectable owner-local root drives the suppression.
    let source = r#"
type Props = {
  href?: ImportedBase['href']
}
"#;
    let host = TestHost {
        external_macro_elements: BTreeMap::from([(
            ("./types".to_string(), "ImportedBase".to_string()),
            ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 0),
                    key: Span::new(0, 0),
                    key_name: Some("href".to_string()),
                    optional: true,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: None,
                    type_text: Some("string".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                    declared_in_macro_type_arg: false,
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_roots_with_surface: BTreeSet::from(["Props".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "Props".to_string(),
                expanded: "{ href?: ImportedBase['href'] }".to_string(),
                type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                        "href".to_string(),
                        TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(TypeExpr::Ref {
                                name: "ImportedBase".into(),
                                type_arguments: Vec::new().into(),
                            }),
                            index: std::sync::Arc::new(TypeExpr::string_literal("href")),
                        },
                        true,
                        false,
                    ))],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            type_name: "ImportedBase".to_string(),
            import_source: "./types".to_string(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: Span::new(0, source.len() as u32),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert!(
        resolved
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "ImportedBase"),
        "authoritative resolved-local surfaces should keep transitive imported macro deps off resolved_macros: {:?}",
        resolved
            .resolved_macros
            .iter()
            .map(|entry| entry.type_name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn resolve_component_meta_parts_skips_direct_imported_macro_resolution_when_resolved_local_surface_is_authoritative(
) {
    // `authoritative_resolved_local` is now
    // `projectable_owner_local`. The macro's
    // `type_references = ["Props", "ImportedBase"]` lists
    // ImportedBase as an indirect reference (used inside
    // `Props`'s body). Configuring `Props` as the projectable
    // root makes the owner-local surface authoritative, which
    // suppresses the indirect ImportedBase dep.
    let source = r#"
type Props = {
  tooltip?: ImportedBase
}
"#;
    let host = TestHost {
        external_macro_elements: BTreeMap::from([(
            ("./types".to_string(), "ImportedBase".to_string()),
            ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 0),
                    key: Span::new(0, 0),
                    key_name: Some("label".to_string()),
                    optional: true,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: None,
                    type_text: Some("string".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                    declared_in_macro_type_arg: false,
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_roots_with_surface: BTreeSet::from(["Props".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string(), "ImportedBase".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "Props".to_string(),
                expanded: "{ tooltip?: ImportedBase }".to_string(),
                type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                        "tooltip".to_string(),
                        TypeExpr::Ref {
                            name: "ImportedBase".into(),
                            type_arguments: Vec::new().into(),
                        },
                        true,
                        false,
                    ))],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            type_name: "ImportedBase".to_string(),
            import_source: "./types".to_string(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: Span::new(0, source.len() as u32),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert!(
        resolved
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "ImportedBase"),
        "authoritative resolved-local surfaces should keep nested direct imported macro deps off resolved_macros: {:?}",
        resolved
            .resolved_macros
            .iter()
            .map(|entry| entry.type_name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn resolve_component_meta_parts_skips_direct_imported_macro_resolution_when_local_wrapper_type_expr_is_available(
) {
    // `authoritative_resolved_local` is now
    // `projectable_owner_local`; configuring `Props` as a
    // projectable owner-local root drives the suppression of
    // the indirect ImportedBase dep.
    let source = r#"
type Props = Omit<ImportedBase, 'hidden'>
"#;
    let host = TestHost {
        external_macro_elements: BTreeMap::from([(
            ("./types".to_string(), "ImportedBase".to_string()),
            ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 0),
                    key: Span::new(0, 0),
                    key_name: Some("label".to_string()),
                    optional: true,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: None,
                    type_text: Some("string".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                    declared_in_macro_type_arg: false,
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_roots_with_surface: BTreeSet::from(["Props".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string(), "ImportedBase".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "Props".to_string(),
                expanded: "Omit<ImportedBase, 'hidden'>".to_string(),
                type_expr: Some(
                    verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(
                        "Omit<ImportedBase, 'hidden'>",
                        None,
                    ),
                ),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            type_name: "ImportedBase".to_string(),
            import_source: "./types".to_string(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: Span::new(0, source.len() as u32),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert!(
        resolved
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "ImportedBase"),
        "local wrapper type_expr should keep direct imported macro deps off resolved_macros: {:?}",
        resolved
            .resolved_macros
            .iter()
            .map(|entry| entry.type_name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn resolve_component_meta_parts_skips_direct_imported_macro_resolution_when_owner_local_prepared_surface_is_available(
) {
    let source = r#"
type Props = Omit<ImportedBase, 'hidden'>
"#;
    let host = TestHost {
        external_macro_elements: BTreeMap::from([(
            ("./types".to_string(), "ImportedBase".to_string()),
            ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 0),
                    key: Span::new(0, 0),
                    key_name: Some("label".to_string()),
                    optional: true,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: None,
                    type_text: Some("string".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                    declared_in_macro_type_arg: false,
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        // graph-only: precomputed owner-local
        // surface for `Props` (the prepared shape resolves
        // `Omit<ImportedBase, 'hidden'>` graph-natively in
        // production via `host.resolve_owner_local_macro_surface`;
        // the test seeds the result directly).
        owner_local_roots_with_surface: BTreeSet::from(["Props".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string(), "ImportedBase".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![ResolvedLocalType {
                name: "Props".to_string(),
                expanded: "{}".to_string(),
                type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                    properties: Vec::new(),
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            type_name: "ImportedBase".to_string(),
            import_source: "./types".to_string(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: Span::new(0, source.len() as u32),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );

    assert!(
        resolved
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "ImportedBase"),
        "prepared owner-local wrapper surfaces should keep direct imported macro deps off resolved_macros: {:?}",
        resolved
            .resolved_macros
            .iter()
            .map(|entry| entry.type_name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn resolve_component_meta_parts_keeps_direct_imported_macro_root_seeded() {
    let host = TestHost {
        external_macro_elements: BTreeMap::from([(
            ("./types".to_string(), "Props".to_string()),
            ResolvedElements {
                props: vec![ResolvedProp {
                    span: Span::new(0, 0),
                    key: Span::new(0, 0),
                    key_name: Some("label".to_string()),
                    optional: false,
                    types: vec![RuntimeType::String],
                    visibility: ResolvedMemberVisibility::Public,
                    type_span: None,
                    type_text: Some("string".to_string()),
                    map_local: false,
                    span_is_absolute: true,
                    type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                    declared_in_macro_type_arg: false,
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::new(),
        owner_local_roots_with_surface: BTreeSet::new(),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 1),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            type_name: "Props".to_string(),
            import_source: "./types".to_string(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: Span::new(0, 1),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );
    let registry_names: Vec<&str> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert_eq!(
        registry_names,
        vec!["Props"],
        "the direct imported macro root should still seed the initial registry"
    );
}

#[test]
fn resolve_component_meta_parts_seeds_imported_macro_root_when_graph_metadata_unknown() {
    // the prior assertion ("direct non-object
    // imported aliases stay out of the initial registry seed")
    // depended on the source-text reparse path: the
    // resolver derived `kind=TypeAlias` + body-text from the
    // host source-text reader, parsed the body, and skipped the
    // seed when the alias was non-object
    // (`string | VNode | (() => VNode)`).
    //
    // that source-text reparse path is gone. The
    // host returns whatever graph metadata the
    // `local_type_symbol_metadata` index carries; for an imported
    // alias with no metadata seeded, `kind` is `Unknown`. The
    // graph-only `should_seed_direct_macro_registry_entry` then
    // seeds the entry (kind != TypeAlias short-circuits the
    // body-text inspection).
    //
    // This is the architecturally correct contract under the
    // graph-only resolver: registry seeding is governed by the
    // direct-macro-reference predicate plus the graph-typed
    // declaration, NOT by a substring scan of the underlying
    // alias body.
    let host = TestHost {
        external_macro_elements: BTreeMap::from([(
            ("./types".to_string(), "StringOrVNode".to_string()),
            ResolvedElements {
                props: vec![
                    ResolvedProp {
                        span: Span::new(0, 0),
                        key: Span::new(0, 0),
                        key_name: Some("component".to_string()),
                        optional: true,
                        types: vec![RuntimeType::Object],
                        visibility: ResolvedMemberVisibility::Public,
                        type_span: None,
                        type_text: Some("object".to_string()),
                        map_local: false,
                        span_is_absolute: true,
                        type_expr: Some(TypeExpr::Primitive(PrimitiveName::Object)),
                        type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                        declared_in_macro_type_arg: false,
                    },
                    ResolvedProp {
                        span: Span::new(0, 0),
                        key: Span::new(0, 0),
                        key_name: Some("children".to_string()),
                        optional: true,
                        types: vec![RuntimeType::String],
                        visibility: ResolvedMemberVisibility::Public,
                        type_span: None,
                        type_text: Some("string".to_string()),
                        map_local: false,
                        span_is_absolute: true,
                        type_expr: Some(TypeExpr::Primitive(PrimitiveName::String)),
                        type_expr_scope: Some(verter_type_expr::TypeExprScope::new("/test.ts")),
                        declared_in_macro_type_arg: false,
                    },
                ],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::new(),
        owner_local_roots_with_surface: BTreeSet::new(),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["StringOrVNode".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 1),
        }],
        macro_type_deps: vec![verter_semantic::analysis::types::MacroTypeDep {
            type_name: "StringOrVNode".to_string(),
            import_source: "./types".to_string(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: Span::new(0, 1),
        }],
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );
    let registry_names: Vec<&str> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(
        registry_names,
        vec!["StringOrVNode"],
        "graph-only registry seeding: imported macro root with non-empty surface seeds the registry regardless of underlying alias body"
    );
}

#[test]
fn resolve_component_meta_parts_keeps_non_root_local_helpers_off_resolved_macros() {
    // under the graph-only resolver only roots
    // the host's `projectable_owner_local_macro_roots` returns
    // are eligible for owner-local projection. The TestHost
    // models this by configuring `projectable_owner_local_roots`
    // to contain only `Props` (the direct macro reference) and
    // not `Helper` (the alias body's referent).
    let source = r#"
type Props = Helper

interface Helper {
  label?: string
}
"#;
    let host = TestHost {
        external_macro_elements: BTreeMap::new(),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_roots_with_surface: BTreeSet::from(["Props".to_string()]),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: Some("props".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: vec![
                ResolvedLocalType {
                    name: "Props".to_string(),
                    expanded: "{ label?: string }".to_string(),
                    type_expr: Some(TypeExpr::Ref {
                        name: "Helper".into(),
                        type_arguments: Vec::new().into(),
                    }),
                    span: Span::new(0, "type Props = Helper".len() as u32),
                },
                ResolvedLocalType {
                    name: "Helper".to_string(),
                    expanded: "{ label?: string }".to_string(),
                    type_expr: Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                            "label".to_string(),
                            TypeExpr::Primitive(PrimitiveName::String),
                            true,
                            false,
                        ))],
                    }))),
                    span: Span::new(0, source.len() as u32),
                },
            ],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, source.len() as u32),
        }],
        macro_type_deps: Vec::new(),
    };

    let resolved = resolve_component_meta_parts(
        &host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );
    let resolved_names: Vec<&str> = resolved
        .resolved_macros
        .iter()
        .map(|entry| entry.type_name.as_str())
        .collect();

    assert_eq!(
        resolved_names,
        vec!["Props"],
        "only the direct local macro root should project into resolved_macros; helper companions should stay lazy",
    );
}

/// Characterisation: pin the invariant that
/// `ResolvedLocalType.type_expr` MUST be populated whenever
/// `expanded` is non-empty. The cold resolver consumes the typed
/// form via `.expect(...)`; a violation panics with a stable
/// message.
///
/// Pre-W0.2 the consumer fell back through
/// text-mode reparse of `resolved.expanded` and silently
/// recovered, so this fixture would not panic and `should_panic`
/// would FAIL. Post-W0.2 the consumer asserts the invariant via
/// `.expect(...)`, the panic fires, and `should_panic` PASSES.
#[test]
#[should_panic(expected = "ResolvedLocalType.type_expr populated by analyzer")]
fn cold_resolver_panics_when_type_expr_missing_for_non_empty_expanded() {
    let host = TestHost {
        external_macro_elements: BTreeMap::new(),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::new(),
        owner_local_roots_with_surface: BTreeSet::new(),
    };
    let snapshot = TestSnapshot {
        imports: Vec::new(),
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["ViolatingEmits".to_string()],
            binding_name: Some("emit".to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            // Synthetic violation: `expanded` is non-empty but
            // `type_expr` is `None`. This shape is unreachable
            // from production producers (both constructor sites in
            // `verter_semantic::analysis::macros` populate
            // `type_expr` via `build_expanded_type_expr`), but the
            // fixture mints it directly to discriminate the
            // consumer's invariant assertion.
            resolved_local_types: vec![ResolvedLocalType {
                name: "ViolatingEmits".to_string(),
                expanded: "{ change: [value: string] }".to_string(),
                type_expr: None,
                span: Span::new(0, 1),
            }],
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 1),
        }],
        macro_type_deps: Vec::new(),
    };

    // Routing through `resolve_component_meta_parts` is the same
    // entry point production consumers use. The direct-local
    // resolved-type registry seeding block at the consumer's
    // `.expect(...)` site fires for `(resolved_index == 0,
    // direct_named_reference, first-seen registry name)`, which
    // this fixture satisfies.
    let _ = resolve_component_meta_parts(
        &host,
        "/src/Violator.vue",
        &snapshot,
        true,
        None,
        ComponentMetaResolutionPurpose::Full,
    );
}
