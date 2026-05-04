use super::*;
use crate::resolver_core::declaration_metadata::ResolvedExportTarget;
use std::collections::BTreeMap;
use verter_compiler::utils::oxc::vue::resolve_type::{
    ResolvedEmit, ResolvedEmitSignature, ResolvedMemberVisibility, ResolvedProp, RuntimeType,
};
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_expr::{
    ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr,
};
use verter_semantic::analysis::types::{
    AnalyzedImport, AnalyzedImportBinding, AnalyzedMacro, AnalyzedMacroKind, ImportBindingKind,
    ResolvedLocalType,
};
use verter_span::Span;

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
    // precomputed owner-local macro surfaces
    // keyed by root name. The legacy TestHost derived these
    // surfaces by re-parsing `source` text via the (now-deleted)
    // source-typed projector; the graph-only resolver demands
    // the surface up front, mirroring how production
    // `host.resolve_owner_local_macro_surface` returns a
    // graph-projected `ProjectedMacroSurfaces` value.
    owner_local_macro_surfaces: BTreeMap<String, ProjectedMacroSurfaces>,
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

    fn resolve_owner_local_macro_surface(
        &self,
        _owner_canonical: &str,
        root_name: &str,
        _macro_kind: AnalyzedMacroKind,
    ) -> Option<ProjectedMacroSurfaces> {
        self.owner_local_macro_surfaces.get(root_name).cloned()
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
    assert_eq!(resolved.resolved_macros[0].props[0].name, "label");
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
    assert_eq!(resolved.resolved_macros[0].emits.len(), 1);
    assert_eq!(resolved.resolved_macros[0].emits[0].name, "save");
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
                                            ty: verter_semantic::analysis::type_expr::TypeExpr::Tuple {
                                                elements: std::sync::Arc::from(vec![
                                                    verter_semantic::analysis::type_expr::TupleElement {
                                                        label: Some("value".to_string()),
                                                        ty: verter_semantic::analysis::type_expr::TypeExpr::Primitive(
                                                            verter_semantic::analysis::type_expr::PrimitiveName::String,
                                                        ),
                                                        optional: false,
                                                        rest: false,
                                                    },
                                                ]),
                                                readonly: false,
                                            },
                                            optional: false,
                                            readonly: false,
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
                type_expr: None,
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
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
fn local_resolved_macro_types_project_into_resolved_macro_surfaces() {
    // the legacy text-projection first pass
    // would derive the projected surface by parsing
    // `mac.resolved_local_types[i].expanded` text. Under the
    // graph-only resolver the surface comes from
    // `host.resolve_owner_local_macro_surface`, which the
    // TestHost backs via the precomputed
    // `owner_local_macro_surfaces` map.
    let host = TestHost {
        external_macro_elements: BTreeMap::new(),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["AccordionEmits".to_string()]),
        owner_local_macro_surfaces: BTreeMap::from([(
            "AccordionEmits".to_string(),
            ProjectedMacroSurfaces {
                props: Vec::new(),
                emits: vec![verter_semantic::analysis::AnalyzedEmitField {
                    name: "update:modelValue".to_string(),
                    span: Span::default(),
                    payload_type: Some(
                        "[value: (T extends 'single' ? string : string[]) | undefined]".to_string(),
                    ),
                    description: None,
                    tags: Vec::new(),
                }],
                slots: Vec::new(),
                native_props: Vec::new(),
            },
        )]),
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
                type_expr: None,
                span: Span::new(0, 1),
            }],
            parsed_type_argument: None,
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
    assert_eq!(resolved.resolved_macros[0].emits.len(), 1);
    assert_eq!(
        resolved.resolved_macros[0].emits[0].name,
        "update:modelValue"
    );
    assert_eq!(
        resolved.resolved_macros[0].emits[0].payload_type.as_deref(),
        Some("[value: (T extends 'single' ? string : string[]) | undefined]")
    );
}

#[test]
fn projectable_local_emit_roots_fill_resolved_macros_without_resolved_local_types() {
    let host = TestHost {
        external_macro_elements: BTreeMap::new(),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["AppEmits".to_string()]),
        // graph-only: precomputed surface for
        // `AppEmits` derived once and seeded directly.
        // the TestHost re-derived this from `source` text.
        owner_local_macro_surfaces: BTreeMap::from([(
            "AppEmits".to_string(),
            ProjectedMacroSurfaces {
                props: Vec::new(),
                emits: vec![verter_semantic::analysis::AnalyzedEmitField {
                    name: "change".to_string(),
                    span: Span::default(),
                    payload_type: Some("[value: string]".to_string()),
                    description: None,
                    tags: Vec::new(),
                }],
                slots: Vec::new(),
                native_props: Vec::new(),
            },
        )]),
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
    assert_eq!(resolved.resolved_macros[0].emits.len(), 1);
    assert_eq!(resolved.resolved_macros[0].emits[0].name, "change");
}

#[test]
fn local_resolved_slot_types_project_resolved_pick_bindings() {
    // slot binding type-annotations are now the resolved
    // leaf, not the symbolic indexed-access form.
    //
    // + : this test asserted the symbolic
    // form `Some("CalendarCellTriggerProps['day']")`. The pre-
    // change pipeline read the owner source via the host source-
    // text reader and ran the source-typed projector against it;
    // that walked the owner source (where
    // `Pick<CalendarCellTriggerProps, 'day'>` is still symbolic)
    // and `extract_slot_info_from_type_text` reduced
    // `Pick<X, K>` down to the symbolic `X[K]` form
    // `CalendarCellTriggerProps['day']`.
    //
    // + + : the
    // source-text reparse path is gone (both the host source-
    // text reader and the source-typed projector are deleted).
    // Owner-local resolved-type projection runs through the
    // graph-native owner-local macro surface API, which produces
    // the leaf `Date` for the binding's type-annotation
    // (`Pick` is already resolved to `{ day: Date }` in the
    // expanded shape).
    //
    // The resolved leaf is the architecturally correct contract
    // for post-engine component-meta: `Pick<X,K>` is a source-
    // text construct that the type system reduces; surfacing
    // the reduction is what consumers (LSP, MCP, codegen) want.
    // The symbolic form was an artefact of the old source-text
    // reparse pathway, not a property the architecture targets.
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
        // graph-only: the owner-local slot
        // surface is precomputed (production derives the same
        // shape via `host.resolve_owner_local_macro_surface` from
        // the prepared graph projection).
        owner_local_macro_surfaces: BTreeMap::from([(
            "CalendarSlots".to_string(),
            ProjectedMacroSurfaces {
                props: Vec::new(),
                emits: Vec::new(),
                slots: vec![verter_semantic::analysis::AnalyzedSlotField {
                    name: "day".to_string(),
                    is_required: false,
                    span: Span::default(),
                    bindings: vec![verter_semantic::analysis::AnalyzedSlotFieldBinding {
                        name: "day".to_string(),
                        type_annotation: Some("Date".to_string()),
                        span: Span::default(),
                    }],
                    return_type: Some("any".to_string()),
                    description: None,
                    tags: Vec::new(),
                }],
                native_props: Vec::new(),
            },
        )]),
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
                type_expr: None,
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
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
    assert_eq!(resolved.resolved_macros[0].slots.len(), 1);
    assert_eq!(resolved.resolved_macros[0].slots[0].bindings.len(), 1);
    assert_eq!(
        resolved.resolved_macros[0].slots[0].bindings[0]
            .type_annotation
            .as_deref(),
        Some("Date")
    );
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
            }],
            ..ResolvedElements::default()
        },
    );
    let host = TestHost {
        external_macro_elements,
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::new(),
        owner_local_macro_surfaces: BTreeMap::new(),
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
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "item".to_string(),
                        ty: TypeExpr::Ref {
                            name: "LocalItem".into(),
                            type_arguments: Vec::new().into(),
                        },
                        optional: true,
                        readonly: false,
                    })],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
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
        owner_local_macro_surfaces: BTreeMap::from([(
            "Props".to_string(),
            ProjectedMacroSurfaces {
                props: vec![verter_semantic::analysis::AnalyzedPropField {
                    name: "href".to_string(),
                    is_optional: true,
                    span: Span::default(),
                    type_annotation: Some("string".to_string()),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: verter_semantic::analysis::TypeResolutionSource::Rust,
                    resolution_error: None,
                }],
                emits: Vec::new(),
                slots: Vec::new(),
                native_props: Vec::new(),
            },
        )]),
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
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "href".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: true,
                        readonly: false,
                    })],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
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
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_macro_surfaces: BTreeMap::from([(
            "Props".to_string(),
            ProjectedMacroSurfaces {
                props: vec![verter_semantic::analysis::AnalyzedPropField {
                    name: "href".to_string(),
                    is_optional: true,
                    span: Span::default(),
                    type_annotation: Some("string".to_string()),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: verter_semantic::analysis::TypeResolutionSource::Rust,
                    resolution_error: None,
                }],
                emits: Vec::new(),
                slots: Vec::new(),
                native_props: Vec::new(),
            },
        )]),
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
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "href".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: true,
                        readonly: false,
                    })],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
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
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_macro_surfaces: BTreeMap::from([(
            "Props".to_string(),
            ProjectedMacroSurfaces {
                props: vec![verter_semantic::analysis::AnalyzedPropField {
                    name: "href".to_string(),
                    is_optional: true,
                    span: Span::default(),
                    type_annotation: Some("ImportedBase['href']".to_string()),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: verter_semantic::analysis::TypeResolutionSource::Rust,
                    resolution_error: None,
                }],
                emits: Vec::new(),
                slots: Vec::new(),
                native_props: Vec::new(),
            },
        )]),
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
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "href".to_string(),
                        ty: TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(TypeExpr::Ref {
                                name: "ImportedBase".into(),
                                type_arguments: Vec::new().into(),
                            }),
                            index: std::sync::Arc::new(TypeExpr::string_literal("href")),
                        },
                        optional: true,
                        readonly: false,
                    })],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
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
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_macro_surfaces: BTreeMap::from([(
            "Props".to_string(),
            ProjectedMacroSurfaces {
                props: vec![verter_semantic::analysis::AnalyzedPropField {
                    name: "tooltip".to_string(),
                    is_optional: true,
                    span: Span::default(),
                    type_annotation: Some("ImportedBase".to_string()),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: verter_semantic::analysis::TypeResolutionSource::Rust,
                    resolution_error: None,
                }],
                emits: Vec::new(),
                slots: Vec::new(),
                native_props: Vec::new(),
            },
        )]),
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
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "tooltip".to_string(),
                        ty: TypeExpr::Ref {
                            name: "ImportedBase".into(),
                            type_arguments: Vec::new().into(),
                        },
                        optional: true,
                        readonly: false,
                    })],
                }))),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
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
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::from(["Props".to_string()]),
        owner_local_macro_surfaces: BTreeMap::from([(
            "Props".to_string(),
            ProjectedMacroSurfaces {
                props: vec![verter_semantic::analysis::AnalyzedPropField {
                    name: "label".to_string(),
                    is_optional: true,
                    span: Span::default(),
                    type_annotation: Some("string".to_string()),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: verter_semantic::analysis::TypeResolutionSource::Rust,
                    resolution_error: None,
                }],
                emits: Vec::new(),
                slots: Vec::new(),
                native_props: Vec::new(),
            },
        )]),
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
                    verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                        "Omit<ImportedBase, 'hidden'>",
                    ),
                ),
                span: Span::new(0, source.len() as u32),
            }],
            parsed_type_argument: None,
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
        owner_local_macro_surfaces: BTreeMap::from([(
            "Props".to_string(),
            ProjectedMacroSurfaces {
                props: vec![verter_semantic::analysis::AnalyzedPropField {
                    name: "label".to_string(),
                    is_optional: true,
                    span: Span::default(),
                    type_annotation: Some("string".to_string()),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: verter_semantic::analysis::TypeResolutionSource::Rust,
                    resolution_error: None,
                }],
                emits: Vec::new(),
                slots: Vec::new(),
                native_props: Vec::new(),
            },
        )]),
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
                }],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::new(),
        owner_local_macro_surfaces: BTreeMap::new(),
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
                    },
                ],
                ..ResolvedElements::default()
            },
        )]),
        eval_outputs: ComponentMetaEvalOutputs::default(),
        projectable_owner_local_roots: BTreeSet::new(),
        owner_local_macro_surfaces: BTreeMap::new(),
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
        owner_local_macro_surfaces: BTreeMap::from([(
            "Props".to_string(),
            ProjectedMacroSurfaces {
                props: vec![verter_semantic::analysis::AnalyzedPropField {
                    name: "label".to_string(),
                    is_optional: true,
                    span: Span::default(),
                    type_annotation: Some("string".to_string()),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: verter_semantic::analysis::TypeResolutionSource::Rust,
                    resolution_error: None,
                }],
                emits: Vec::new(),
                slots: Vec::new(),
                native_props: Vec::new(),
            },
        )]),
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
                        properties: vec![ObjectMember::Property(ObjectProperty {
                            name: "label".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: true,
                            readonly: false,
                        })],
                    }))),
                    span: Span::new(0, source.len() as u32),
                },
            ],
            parsed_type_argument: None,
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
