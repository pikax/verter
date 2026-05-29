//! Convert module tests. Uses `super::*` plus explicit submodule imports for
//! items that are `pub(super)` (not re-exported at the `convert` level).

use std::sync::Arc;

use verter_session as host;

use crate::types::*;

use super::error::*;
use super::offset::*;
use super::string_helpers::*;
use super::*;

fn empty_analysis() -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
        props: Vec::new(),
        events: Vec::new(),
        slots: Vec::new(),
        models: Vec::new(),
        exposed: Vec::new(),
        public_instance: None,
        sfc_blocks: None,
        type_registry: Vec::new(),
        components: Vec::new(),
        template_refs: Vec::new(),
        imports: Vec::new(),
        bindings: Vec::new(),
        vue_api_calls: Vec::new(),
        styles: Vec::new(),
        flags: verter_semantic::analysis::component_meta::ComponentMetaFlags::default(),
        root_reachability:
            verter_semantic::analysis::component_meta::RootReachability::NoFallthrough {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
            },
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness:
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: verter_semantic::analysis::component_meta::FallthroughSurface::None {
            reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
        },
        macro_expansion_diagnostics: Vec::new(),
        options_api: false,
        file_path: String::new(),
    }
}

// ── Error path tests ──────────────────────────────────────────

#[test]
fn invalid_compile_error_policy() {
    let config = FfiHostConfig {
        compile_error_policy: Some("banana".to_string()),
        ..Default::default()
    };
    let err = ffi_config_to_host(config).unwrap_err();
    assert!(matches!(
        err,
        FfiConversionError::InvalidCompileErrorPolicy(_)
    ));
    assert!(err.to_string().contains("banana"));
}

#[test]
fn invalid_analysis_level() {
    let config = FfiHostConfig {
        analysis_level: Some("turbo".to_string()),
        ..Default::default()
    };
    let err = ffi_config_to_host(config).unwrap_err();
    assert!(matches!(err, FfiConversionError::InvalidAnalysisLevel(_)));
}

#[test]
fn invalid_hmr_strategy() {
    let profile = FfiCompileProfile {
        hmr_strategy: Some("rspack".to_string()),
        ..Default::default()
    };
    let err = ffi_profile_to_host(Some(profile)).unwrap_err();
    assert!(matches!(err, FfiConversionError::InvalidHmrStrategy(_)));
}

#[test]
fn invalid_delimiters_count() {
    let profile = FfiCompileProfile {
        delimiters: Some(vec!["{{".to_string()]),
        ..Default::default()
    };
    let err = ffi_profile_to_host(Some(profile)).unwrap_err();
    assert!(matches!(err, FfiConversionError::InvalidDelimiters(1)));
}

#[test]
fn invalid_file_kind() {
    let err = ffi_file_kind_to_host(Some("binary")).unwrap_err();
    assert!(matches!(err, FfiConversionError::InvalidFileKind(_)));
}

#[test]
fn invalid_node_kind() {
    let kind = FfiVirtualNodeKind {
        kind: "fragment".to_string(),
        index: None,
    };
    let err = ffi_node_kind_to_host(kind).unwrap_err();
    assert!(matches!(err, FfiConversionError::InvalidNodeKind(_)));
}

// ── Happy path smoke tests ────────────────────────────────────

#[test]
fn config_defaults_are_valid() {
    let config = FfiHostConfig::default();
    let result = ffi_config_to_host(config).unwrap();
    assert!(result.dev_mode);
}

#[test]
fn profile_none_returns_default() {
    let result = ffi_profile_to_host(None).unwrap();
    assert!(!result.is_production);
}

#[test]
fn component_meta_type_registry_keeps_expanded_and_pre_expansion_type_information() {
    let analysis = verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
        props: Vec::new(),
        events: Vec::new(),
        slots: Vec::new(),
        models: Vec::new(),
        exposed: Vec::new(),
        public_instance: None,
        sfc_blocks: None,
        type_registry: vec![
            verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
                name: "Props".to_string(),
                type_expr: verter_type_expr::TypeExpr::Unknown {
                    raw: "{ label: string }".to_string(),
                },
                type_expansion: None,
            },
        ],
        components: Vec::new(),
        template_refs: Vec::new(),
        imports: Vec::new(),
        bindings: Vec::new(),
        vue_api_calls: Vec::new(),
        styles: Vec::new(),
        flags: verter_semantic::analysis::component_meta::ComponentMetaFlags::default(),
        root_reachability:
            verter_semantic::analysis::component_meta::RootReachability::NoFallthrough {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
            },
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness:
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: verter_semantic::analysis::component_meta::FallthroughSurface::None {
            reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
        },
        macro_expansion_diagnostics: Vec::new(),
        options_api: false,
        file_path: "/src/App.vue".to_string(),
    };
    let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
        snapshot: host::FileAnalysisSnapshot::default(),
        mode: host::ProjectionMode::Expanded,
        whole_hash: [0; 16],
        resolved_macros: Vec::new(),
        resolved_type_registry: Vec::new(),
        resolved_type_registry_meta: vec![host::meta_resolve::ResolvedTypeRegistryMeta {
            name: "Props".to_string(),
            declaration: host::meta_resolve::ResolvedTypeDeclaration {
                requested_name: "Props".to_string(),
                declaration_id: None,
                resolved_name: "Props".to_string(),
                canonical_source: "/src/types.ts".to_string(),
                span: verter_span::Span::new(10, 48),
                kind: host::meta_resolve::ResolvedDeclarationKind::Interface,
                text: Some("export interface Props { label: string }".to_string()),
            },
        }],
        evaluated_types: None,
        fact_versions: Vec::new(),
        compute_audit: None,
        origin_graph: None,
        request_id: 0,
        surface_identities: None,
        synthesis_diagnostics: Vec::new(),
        synthesis_should_suppress: false,
    };

    let ffi = component_meta_analysis_to_ffi_with_resolution(analysis, Some(&resolved_state));
    let entry = ffi
        .type_registry
        .first()
        .expect("type registry entry should be present");

    assert_eq!(entry.name, "Props");
    assert_eq!(
        entry.raw_type.as_deref(),
        Some("export interface Props { label: string }"),
        "native payload should expose the pre-expansion source form explicitly",
    );
    assert_eq!(
        entry
            .declaration
            .as_ref()
            .map(|declaration| declaration.canonical_source.as_str()),
        Some("/src/types.ts"),
        "native payload should also retain declaration provenance",
    );
}

#[test]
fn component_meta_type_registry_prefers_resolved_registry_type_expr_when_available() {
    let analysis = verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
        props: Vec::new(),
        events: Vec::new(),
        slots: Vec::new(),
        models: Vec::new(),
        exposed: Vec::new(),
        public_instance: None,
        sfc_blocks: None,
        type_registry: vec![
            verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
                name: "Button".to_string(),
                type_expr: verter_type_expr::TypeExpr::named("Button"),
                type_expansion: None,
            },
        ],
        components: Vec::new(),
        template_refs: Vec::new(),
        imports: Vec::new(),
        bindings: Vec::new(),
        vue_api_calls: Vec::new(),
        styles: Vec::new(),
        flags: verter_semantic::analysis::component_meta::ComponentMetaFlags::default(),
        root_reachability:
            verter_semantic::analysis::component_meta::RootReachability::NoFallthrough {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
            },
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness:
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: verter_semantic::analysis::component_meta::FallthroughSurface::None {
            reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
        },
        macro_expansion_diagnostics: Vec::new(),
        options_api: false,
        file_path: "/src/App.vue".to_string(),
    };
    let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
        snapshot: host::FileAnalysisSnapshot::default(),
        mode: host::ProjectionMode::Expanded,
        whole_hash: [0; 16],
        resolved_macros: Vec::new(),
        resolved_type_registry: vec![
            verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
                name: "Button".to_string(),
                type_expr: verter_type_expr::TypeExpr::Object(Arc::new(
                    verter_type_expr::ObjectExpr {
                        properties: vec![verter_type_expr::ObjectMember::Property(
                            verter_type_expr::ObjectProperty::synthetic(
                                "variants".to_string(),
                                verter_type_expr::TypeExpr::Object(Arc::new(
                                    verter_type_expr::ObjectExpr { properties: vec![] },
                                )),
                                false,
                                false,
                            ),
                        )],
                    },
                )),
                type_expansion: None,
            },
        ],
        resolved_type_registry_meta: vec![host::meta_resolve::ResolvedTypeRegistryMeta {
            name: "Button".to_string(),
            declaration: host::meta_resolve::ResolvedTypeDeclaration {
                requested_name: "Button".to_string(),
                declaration_id: None,
                resolved_name: "Button".to_string(),
                canonical_source: "/src/App.vue".to_string(),
                span: verter_span::Span::new(10, 52),
                kind: host::meta_resolve::ResolvedDeclarationKind::TypeAlias,
                text: Some(
                    "type Button = ComponentConfig<typeof theme, MissingAppConfig>".to_string(),
                ),
            },
        }],
        evaluated_types: None,
        fact_versions: Vec::new(),
        compute_audit: None,
        origin_graph: None,
        request_id: 0,
        surface_identities: None,
        synthesis_diagnostics: Vec::new(),
        synthesis_should_suppress: false,
    };

    let ffi = component_meta_analysis_to_ffi_with_resolution(analysis, Some(&resolved_state));
    let entry = ffi
        .type_registry
        .first()
        .expect("type registry entry should be present");

    assert!(
        matches!(entry.r#type, verter_type_expr::TypeExpr::Object(_)),
        "resolved registry entry should override the shallow analysis alias"
    );
    assert_eq!(
        entry.raw_type.as_deref(),
        Some("type Button = ComponentConfig<typeof theme, MissingAppConfig>"),
        "resolved registry should still keep the pre-expansion source text",
    );
}

#[test]
fn component_meta_ffi_exposes_root_info_summary() {
    let analysis = verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
        props: Vec::new(),
        events: Vec::new(),
        slots: Vec::new(),
        models: Vec::new(),
        exposed: Vec::new(),
        public_instance: None,
        sfc_blocks: None,
        type_registry: Vec::new(),
        components: Vec::new(),
        template_refs: Vec::new(),
        imports: Vec::new(),
        bindings: Vec::new(),
        vue_api_calls: Vec::new(),
        styles: Vec::new(),
        flags: verter_semantic::analysis::component_meta::ComponentMetaFlags::default(),
        root_reachability: verter_semantic::analysis::component_meta::RootReachability::Branches {
            branches: vec![
                verter_semantic::analysis::component_meta::RootBranch {
                    branch_index: 0,
                    condition_text: None,
                    target:
                        verter_semantic::analysis::component_meta::RootTargetRef::ComponentUsage {
                            element_index: 1,
                            usage_index: 0,
                            name: "PrimaryButton".to_string(),
                            import_source: Some("./PrimaryButton.vue".to_string()),
                        },
                    consumed: verter_semantic::analysis::component_meta::ConsumedRootBindings {
                        attrs: vec!["class".to_string()],
                        listeners: vec!["click".to_string()],
                        has_dynamic_attr_name: false,
                        has_dynamic_listener_name: false,
                    },
                    has_unknown_spread: false,
                },
                verter_semantic::analysis::component_meta::RootBranch {
                    branch_index: 1,
                    condition_text: Some("isFallback".to_string()),
                    target:
                        verter_semantic::analysis::component_meta::RootTargetRef::NativeElement {
                            element_index: 2,
                            tag: "button".to_string(),
                        },
                    consumed: verter_semantic::analysis::component_meta::ConsumedRootBindings {
                        attrs: Vec::new(),
                        listeners: Vec::new(),
                        has_dynamic_attr_name: false,
                        has_dynamic_listener_name: false,
                    },
                    has_unknown_spread: false,
                },
            ],
        },
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness:
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface:
            verter_semantic::analysis::component_meta::FallthroughSurface::Branches {
                branches: Vec::new(),
            },
        macro_expansion_diagnostics: Vec::new(),
        options_api: false,
        file_path: "/src/App.vue".to_string(),
    };

    let ffi = component_meta_analysis_to_ffi(analysis);
    match ffi.root_info {
        FfiRootInfo {
            kind: FfiRootInfoKind::Conditional,
            reason: None,
            targets,
        } => {
            assert_eq!(targets.len(), 2);
            assert!(matches!(
                targets.first(),
                Some(FfiRootTargetRef::ComponentUsage { name, .. }) if name == "PrimaryButton"
            ));
            assert!(matches!(
                targets.get(1),
                Some(FfiRootTargetRef::NativeElement { tag, .. }) if tag == "button"
            ));
        }
        other => panic!("unexpected root info payload: {other:?}"),
    }
}

#[test]
fn file_kind_vue_default() {
    let kind = ffi_file_kind_to_host(None).unwrap();
    assert_eq!(kind, host::FileKind::VueSfc);
}

#[test]
fn file_kind_non_sfc() {
    let kind = ffi_file_kind_to_host(Some("non_sfc")).unwrap();
    assert_eq!(kind, host::FileKind::NonSfc);
}

#[test]
fn node_kind_round_trip() {
    let kinds = [
        ("main", host::VirtualNodeKind::Main),
        ("script", host::VirtualNodeKind::Script),
        ("template", host::VirtualNodeKind::Template),
    ];
    for (s, expected) in &kinds {
        let ffi = FfiVirtualNodeKind {
            kind: s.to_string(),
            index: None,
        };
        assert_eq!(ffi_node_kind_to_host(ffi).unwrap(), *expected);
    }
}

#[test]
fn node_kind_style_with_index() {
    let ffi = FfiVirtualNodeKind {
        kind: "style".to_string(),
        index: Some(2),
    };
    assert_eq!(
        ffi_node_kind_to_host(ffi).unwrap(),
        host::VirtualNodeKind::Style { index: 2 }
    );
}

#[test]
fn config_case_insensitive_policy() {
    let config = FfiHostConfig {
        compile_error_policy: Some("STRICT".to_string()),
        ..Default::default()
    };
    let result = ffi_config_to_host(config).unwrap();
    assert_eq!(
        result.compile_error_policy,
        host::CompileErrorPolicy::StrictError
    );
}

#[test]
fn ffi_conversion_error_display() {
    let err = FfiConversionError::InvalidFileKind("xyz".to_string());
    assert_eq!(err.to_string(), "invalid file_kind 'xyz'");

    let err = FfiConversionError::InvalidDelimiters(3);
    assert_eq!(
        err.to_string(),
        "delimiters must have exactly 2 elements, got 3"
    );
}

#[test]
fn ffi_conversion_error_to_string_impl() {
    let err = FfiConversionError::InvalidHmrStrategy("rspack".to_string());
    let s: String = err.into();
    assert!(s.contains("rspack"));
}

// ── Config: all fields populated ─────────────────────────────────

#[test]
fn config_all_fields() {
    let config = FfiHostConfig {
        dev_mode: Some(false),
        compile_error_policy: Some("strict".to_string()),
        lsp_scheme: Some("my-scheme".to_string()),
        max_profiles_per_file: Some(4),
        resolve_extensions: Some(vec![".vue".to_string(), ".ts".to_string()]),
        analysis_level: Some("essential".to_string()),
        audit_enabled: None,
        footprint_capture: None,
        typeinfo_scratch_cache_capacity: None,
    };
    let result = ffi_config_to_host(config).unwrap();
    assert!(!result.dev_mode);
    assert_eq!(
        result.compile_error_policy,
        host::CompileErrorPolicy::StrictError
    );
    assert_eq!(result.lsp_scheme, "my-scheme");
    assert_eq!(result.max_profiles_per_file, 4);
    assert_eq!(result.resolve_extensions, vec![".vue", ".ts"]);
    assert_eq!(result.analysis_level, host::AnalysisLevel::Essential);
}

#[test]
fn expansion_metadata_to_ffi_preserves_exactness_and_execution_status() {
    let ffi = expansion_metadata_to_ffi(verter_semantic::analysis::type_expand::ExpansionMetadata {
        exactness: verter_semantic::analysis::type_solver::result::SolverExactness::ExactSymbolic,
        execution_status:
            verter_semantic::analysis::type_solver::result::ExecutionStatus::HardStop,
        diagnostics: vec![verter_semantic::analysis::type_expand::ExpansionDiagnostic {
            reason: verter_semantic::analysis::type_expand::ExpansionStopReason::UnsupportedOperator,
            context: "kept symbolic".to_string(),
            property_name: None,
        }],
    });

    assert_eq!(ffi.exactness, "exactSymbolic");
    assert_eq!(ffi.execution_status, "hardStop");
    assert_eq!(ffi.diagnostics.len(), 1);
}

// ── Config: all policy string variants ───────────────────────────

#[test]
fn config_policy_all_variants() {
    let strict_variants = ["strict", "strict_error", "strictError", "STRICT", "Strict"];
    for v in &strict_variants {
        let cfg = FfiHostConfig {
            compile_error_policy: Some(v.to_string()),
            ..Default::default()
        };
        assert_eq!(
            ffi_config_to_host(cfg).unwrap().compile_error_policy,
            host::CompileErrorPolicy::StrictError,
            "variant '{v}' should map to StrictError"
        );
    }

    let dev_variants = [
        "dev",
        "dev_serve_last_known_good",
        "devServeLastKnownGood",
        "DEV",
    ];
    for v in &dev_variants {
        let cfg = FfiHostConfig {
            compile_error_policy: Some(v.to_string()),
            ..Default::default()
        };
        assert_eq!(
            ffi_config_to_host(cfg).unwrap().compile_error_policy,
            host::CompileErrorPolicy::DevServeLastKnownGood,
            "variant '{v}' should map to DevServeLastKnownGood"
        );
    }
}

// ── Config: all analysis level variants ──────────────────────────

#[test]
fn config_analysis_level_all_variants() {
    let cases = [
        ("none", host::AnalysisLevel::None),
        ("NONE", host::AnalysisLevel::None),
        ("essential", host::AnalysisLevel::Essential),
        ("ESSENTIAL", host::AnalysisLevel::Essential),
        ("full", host::AnalysisLevel::Full),
        ("FULL", host::AnalysisLevel::Full),
    ];
    for (input, expected) in &cases {
        let cfg = FfiHostConfig {
            analysis_level: Some(input.to_string()),
            ..Default::default()
        };
        assert_eq!(
            ffi_config_to_host(cfg).unwrap().analysis_level,
            *expected,
            "analysis level '{input}' mismatch"
        );
    }
}

// ── Profile: all fields populated ────────────────────────────────

#[test]
fn profile_all_fields() {
    let profile = FfiCompileProfile {
        filename: Some("Comp.vue".to_string()),
        is_production: Some(true),
        ssr: Some(true),
        hmr_strategy: Some("vite".to_string()),
        component_id: Some("abc123".to_string()),
        delimiters: Some(vec!["<%".to_string(), "%>".to_string()]),
        custom_elements: Some(vec!["my-el".to_string()]),
        comments: Some(true),
        runtime_module_name: Some("vue/runtime".to_string()),
        types_module_name: Some("@custom/types".to_string()),
        force_vapor: Some(true),
        force_js: Some(true),
        source_map: Some(true),
        target: Some("ide".to_string()),
        strict_slots: Some(true),
        requested_mode: Some("content".to_string()),
    };
    let result = ffi_profile_to_host(Some(profile)).unwrap();
    assert_eq!(result.filename, Some("Comp.vue".to_string()));
    assert!(result.is_production);
    assert!(result.ssr);
    assert!(result.target.needs_tsx());
    assert!(result.strict_slots);
    assert_eq!(result.requested_mode, host::CompileCacheMode::Content);
    assert_eq!(result.hmr_strategy, host::HmrStrategy::Vite);
    assert_eq!(result.component_id, Some("abc123".to_string()));
    assert_eq!(
        result.delimiters,
        Some(("<%".to_string(), "%>".to_string()))
    );
    assert_eq!(result.custom_elements, Some(vec!["my-el".to_string()]));
    assert_eq!(result.comments, Some(true));
    assert_eq!(result.runtime_module_name, Some("vue/runtime".to_string()));
    assert_eq!(result.types_module_name, Some("@custom/types".to_string()));
    assert!(result.force_vapor);
    assert!(result.force_js);
    assert!(result.source_map);
}

// ── Profile: all HMR strategy variants ───────────────────────────

#[test]
fn profile_hmr_strategy_all_variants() {
    let cases = [
        ("vite", host::HmrStrategy::Vite),
        ("VITE", host::HmrStrategy::Vite),
        ("webpack", host::HmrStrategy::Webpack),
        ("WEBPACK", host::HmrStrategy::Webpack),
        ("none", host::HmrStrategy::None),
        ("NONE", host::HmrStrategy::None),
    ];
    for (input, expected) in &cases {
        let profile = FfiCompileProfile {
            hmr_strategy: Some(input.to_string()),
            ..Default::default()
        };
        assert_eq!(
            ffi_profile_to_host(Some(profile)).unwrap().hmr_strategy,
            *expected,
            "hmr strategy '{input}' mismatch"
        );
    }
}

// ── Profile: delimiters edge cases ───────────────────────────────

#[test]
fn profile_delimiters_three_elements() {
    let profile = FfiCompileProfile {
        delimiters: Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
        ..Default::default()
    };
    let err = ffi_profile_to_host(Some(profile)).unwrap_err();
    assert!(matches!(err, FfiConversionError::InvalidDelimiters(3)));
}

#[test]
fn profile_delimiters_empty_vec() {
    let profile = FfiCompileProfile {
        delimiters: Some(vec![]),
        ..Default::default()
    };
    let err = ffi_profile_to_host(Some(profile)).unwrap_err();
    assert!(matches!(err, FfiConversionError::InvalidDelimiters(0)));
}

// ── File kind: all accepted variants ─────────────────────────────

#[test]
fn file_kind_all_vue_variants() {
    for v in &["vue", "sfc", "vue_sfc", "VUE", "SFC", "Vue_Sfc"] {
        assert_eq!(
            ffi_file_kind_to_host(Some(v)).unwrap(),
            host::FileKind::VueSfc,
            "'{v}' should map to VueSfc"
        );
    }
}

#[test]
fn file_kind_all_non_sfc_variants() {
    for v in &["non_sfc", "text", "file", "NON_SFC", "TEXT", "FILE"] {
        assert_eq!(
            ffi_file_kind_to_host(Some(v)).unwrap(),
            host::FileKind::NonSfc,
            "'{v}' should map to NonSfc"
        );
    }
}

// ── Node kind: custom with index ─────────────────────────────────

#[test]
fn node_kind_custom_with_index() {
    let ffi = FfiVirtualNodeKind {
        kind: "custom".to_string(),
        index: Some(5),
    };
    assert_eq!(
        ffi_node_kind_to_host(ffi).unwrap(),
        host::VirtualNodeKind::Custom { index: 5 }
    );
}

#[test]
fn node_kind_style_default_index() {
    let ffi = FfiVirtualNodeKind {
        kind: "style".to_string(),
        index: None,
    };
    assert_eq!(
        ffi_node_kind_to_host(ffi).unwrap(),
        host::VirtualNodeKind::Style { index: 0 }
    );
}

#[test]
fn node_kind_case_insensitive() {
    for kind in &["MAIN", "Main", "SCRIPT", "Script", "TEMPLATE", "Template"] {
        let ffi = FfiVirtualNodeKind {
            kind: kind.to_string(),
            index: None,
        };
        assert!(
            ffi_node_kind_to_host(ffi).is_ok(),
            "'{kind}' should be accepted"
        );
    }
}

// ── Upsert conversion ────────────────────────────────────────────

#[test]
fn upsert_basic() {
    let ffi = FfiUpsertRequest {
        canonical_id: Some("/src/Comp.vue".to_string()),
        input_id: "src/Comp.vue".to_string(),
        source: "<template>hi</template>".to_string(),
        file_kind: None,
        aliases: None,
    };
    let result = ffi_upsert_to_host(ffi).unwrap();
    assert_eq!(result.canonical_id, Some("/src/Comp.vue".to_string()));
    assert_eq!(result.input_id, "src/Comp.vue");
    assert_eq!(&*result.source, "<template>hi</template>");
    assert_eq!(result.file_kind, host::FileKind::VueSfc);
    assert!(result.aliases.is_empty());
}

#[test]
fn upsert_with_aliases_and_non_sfc() {
    let ffi = FfiUpsertRequest {
        canonical_id: None,
        input_id: "/src/types.ts".to_string(),
        source: "export type Foo = string;".to_string(),
        file_kind: Some("non_sfc".to_string()),
        aliases: Some(vec!["@/types".to_string(), "~/types".to_string()]),
    };
    let result = ffi_upsert_to_host(ffi).unwrap();
    assert!(result.canonical_id.is_none());
    assert_eq!(result.file_kind, host::FileKind::NonSfc);
    assert_eq!(result.aliases, vec!["@/types", "~/types"]);
}

#[test]
fn upsert_source_is_arc_str() {
    let ffi = FfiUpsertRequest {
        canonical_id: None,
        input_id: "test.vue".to_string(),
        source: "hello".to_string(),
        file_kind: None,
        aliases: None,
    };
    let result = ffi_upsert_to_host(ffi).unwrap();
    // source should be Arc<str>, verify via reference counting
    let arc: Arc<str> = result.source;
    assert_eq!(&*arc, "hello");
}

#[test]
fn upsert_invalid_file_kind() {
    let ffi = FfiUpsertRequest {
        canonical_id: None,
        input_id: "test.vue".to_string(),
        source: "x".to_string(),
        file_kind: Some("binary".to_string()),
        aliases: None,
    };
    assert!(ffi_upsert_to_host(ffi).is_err());
}

// ── Virtual query conversion ─────────────────────────────────────

#[test]
fn virtual_query_with_raw_id() {
    let ffi = FfiVirtualQuery {
        raw_id: Some("Comp.vue?vue&type=style&index=0".to_string()),
        canonical_id: None,
        node_kind: None,
        compile_profile: None,
    };
    let result = ffi_virtual_query_to_host(ffi).unwrap();
    assert_eq!(
        result.raw_id,
        Some("Comp.vue?vue&type=style&index=0".to_string())
    );
    assert!(result.canonical_id.is_none());
    assert!(result.node_kind.is_none());
}

#[test]
fn virtual_query_with_explicit_kind() {
    let ffi = FfiVirtualQuery {
        raw_id: None,
        canonical_id: Some("/src/Comp.vue".to_string()),
        node_kind: Some(FfiVirtualNodeKind {
            kind: "template".to_string(),
            index: None,
        }),
        compile_profile: Some(FfiCompileProfile {
            ssr: Some(true),
            ..Default::default()
        }),
    };
    let result = ffi_virtual_query_to_host(ffi).unwrap();
    assert_eq!(result.canonical_id, Some("/src/Comp.vue".to_string()));
    assert_eq!(result.node_kind, Some(host::VirtualNodeKind::Template));
    assert!(result.compile_profile.ssr);
}

#[test]
fn virtual_query_invalid_node_kind_propagates() {
    let ffi = FfiVirtualQuery {
        raw_id: None,
        canonical_id: None,
        node_kind: Some(FfiVirtualNodeKind {
            kind: "banana".to_string(),
            index: None,
        }),
        compile_profile: None,
    };
    assert!(matches!(
        ffi_virtual_query_to_host(ffi).unwrap_err(),
        FfiConversionError::InvalidNodeKind(_)
    ));
}

// ── Output direction: host_node_kind_to_ffi ──────────────────────

#[test]
fn node_kind_to_ffi_all_variants() {
    let cases: &[(host::VirtualNodeKind, &str, Option<u32>)] = &[
        (host::VirtualNodeKind::Main, "main", None),
        (host::VirtualNodeKind::Script, "script", None),
        (host::VirtualNodeKind::Template, "template", None),
        (host::VirtualNodeKind::Style { index: 3 }, "style", Some(3)),
        (
            host::VirtualNodeKind::Custom { index: 7 },
            "custom",
            Some(7),
        ),
    ];
    for (input, expected_kind, expected_index) in cases {
        let ffi = host_node_kind_to_ffi(input);
        assert_eq!(ffi.kind, *expected_kind);
        assert_eq!(ffi.index, *expected_index);
    }
}

// ── Output direction: host_diagnostics_to_ffi ────────────────────

#[test]
fn diagnostics_all_severity_levels() {
    let snapshot = host::DiagnosticsSnapshot {
        diagnostics: vec![
            host::HostDiagnostic {
                severity: host::HostSeverity::Error,
                code: "E001".to_string(),
                message: "error msg".to_string(),
                span: Some(verter_span::Span::new(0, 10)),
            },
            host::HostDiagnostic {
                severity: host::HostSeverity::Warning,
                code: "W001".to_string(),
                message: "warning msg".to_string(),
                span: None,
            },
            host::HostDiagnostic {
                severity: host::HostSeverity::Info,
                code: "I001".to_string(),
                message: "info msg".to_string(),
                span: None,
            },
        ],
        has_errors: true,
    };
    let ffi = host_diagnostics_to_ffi(&snapshot, None);
    assert!(ffi.has_errors);
    assert_eq!(ffi.diagnostics.len(), 3);
    assert_eq!(ffi.diagnostics[0].severity, "error");
    assert_eq!(ffi.diagnostics[0].code, "E001");
    assert_eq!(ffi.diagnostics[0].span_start, Some(0));
    assert_eq!(ffi.diagnostics[0].span_end, Some(10));
    assert_eq!(ffi.diagnostics[1].severity, "warning");
    assert_eq!(ffi.diagnostics[1].span_start, None);
    assert_eq!(ffi.diagnostics[2].severity, "info");
    assert_eq!(ffi.diagnostics[2].span_start, None);
    assert_eq!(ffi.diagnostics[2].span_end, None);
}

#[test]
fn diagnostics_empty() {
    let snapshot = host::DiagnosticsSnapshot::default();
    let ffi = host_diagnostics_to_ffi(&snapshot, None);
    assert!(!ffi.has_errors);
    assert!(ffi.diagnostics.is_empty());
}

#[test]
fn host_diagnostics_to_ffi_converts_utf8_spans_to_utf16_with_unicode_source() {
    // "😀" is 4 UTF-8 bytes and 2 UTF-16 code units.
    let source = "a😀b";
    let snapshot = host::DiagnosticsSnapshot {
        diagnostics: vec![host::HostDiagnostic {
            severity: host::HostSeverity::Error,
            code: "E_UTF".to_string(),
            message: "unicode".to_string(),
            span: Some(verter_span::Span::new(1, 5)), // byte offset at 😀 start..right after
        }],
        has_errors: true,
    };

    let ffi = host_diagnostics_to_ffi(&snapshot, Some(source));
    assert_eq!(ffi.diagnostics.len(), 1);
    assert_eq!(ffi.diagnostics[0].span_start, Some(1));
    assert_eq!(ffi.diagnostics[0].span_end, Some(3));
}

#[test]
fn host_diagnostics_to_ffi_preserves_none_spans() {
    let snapshot = host::DiagnosticsSnapshot {
        diagnostics: vec![host::HostDiagnostic {
            severity: host::HostSeverity::Warning,
            code: "W_NONE".to_string(),
            message: "none".to_string(),
            span: None,
        }],
        has_errors: false,
    };
    let ffi = host_diagnostics_to_ffi(&snapshot, Some("abc"));
    assert_eq!(ffi.diagnostics.len(), 1);
    assert_eq!(ffi.diagnostics[0].span_start, None);
    assert_eq!(ffi.diagnostics[0].span_end, None);
}

#[test]
fn lint_diagnostics_to_utf16_converts_spans() {
    let source = "a😀b";
    let input = vec![verter_diagnostics::LintDiagnostic {
        rule: "r".to_string(),
        category: "c".to_string(),
        severity: verter_diagnostics::Severity::Error,
        message: "m".to_string(),
        span: verter_span::Span::new(1, 5),
        tags: vec![],
        span_kind: verter_diagnostics::DiagnosticSpanKind::ElementOpenTag,
        certainty: verter_diagnostics::Certainty::Definite,
        evidence: Vec::new(),
        related_files: Vec::new(),
    }];

    let out = lint_diagnostics_to_utf16(input, Some(source));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].span.start, 1);
    assert_eq!(out[0].span.end, 3);
    assert!(out[0].tags.is_empty(), "tags should be unchanged");
}

// ── Output direction: host_update_to_ffi ─────────────────────────

#[test]
fn update_result_full_round_trip() {
    let source = "a😀b";
    let host_result = host::HostUpdateResult {
        canonical_id: "/src/App.vue".to_string(),
        changed: true,
        slice_changes: host::SliceChanges {
            script_changed: true,
            template_changed: false,
            style_indices_changed: vec![0, 2],
            custom_indices_changed: vec![1],
            structure_changed: true,
            descriptor_changed: false,
        },
        changed_virtual_nodes: vec![
            host::VirtualNodeKind::Script,
            host::VirtualNodeKind::Style { index: 0 },
        ],
        removed_virtual_nodes: vec![host::VirtualNodeKind::Style { index: 2 }],
        changed_virtual_ids: vec!["App.vue?type=script".to_string()],
        removed_virtual_ids: vec!["App.vue?type=style&index=2".to_string()],
        changed_lsp_ids: vec!["App.vue._VERTER_.script.ts".to_string()],
        removed_lsp_ids: vec!["App.vue._VERTER_.style.2.css".to_string()],
        diagnostics: host::DiagnosticsSnapshot {
            diagnostics: vec![host::HostDiagnostic {
                severity: host::HostSeverity::Warning,
                code: "W002".to_string(),
                message: "unused var".to_string(),
                span: Some(verter_span::Span::new(42, 45)),
            }],
            has_errors: false,
        },
        external_source_requests: vec![host::ExternalSourceRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            block_kind: host::ExternalBlockKind::Script,
            index: 0,
            specifier: "./script.ts".to_string(),
            resolved_canonical_id: "/src/script.ts".to_string(),
        }],
        import_specifiers: vec![host::ScriptImportInfo {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec!["ref".to_string(), "computed".to_string()],
        }],
        module_references: host::VerterHost::new_standalone(host::HostConfig::default())
            .upsert(host::UpsertRequest {
                canonical_id: Some("/src/dynamic.ts".to_string()),
                input_id: "/src/dynamic.ts".to_string(),
                source: std::sync::Arc::from("const mod = import('./Foo.vue');"),
                file_kind: host::FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .expect("upsert should extract module references")
            .module_references,
        preprocessor_requests: Vec::new(),
        export_signatures: Vec::new(),
        parse_duration_ms: 1.5,
    };

    let ffi = host_update_to_ffi(host_result, Some(source));
    assert_eq!(ffi.canonical_id, "/src/App.vue");
    assert!(ffi.changed);

    // slice changes
    assert!(ffi.slice_changes.script_changed);
    assert!(!ffi.slice_changes.template_changed);
    assert_eq!(ffi.slice_changes.style_indices_changed, vec![0, 2]);
    assert_eq!(ffi.slice_changes.custom_indices_changed, vec![1]);
    assert!(ffi.slice_changes.structure_changed);
    assert!(!ffi.slice_changes.descriptor_changed);

    // virtual nodes (usize→u32 for indexed kinds)
    assert_eq!(ffi.changed_virtual_nodes.len(), 2);
    assert_eq!(ffi.changed_virtual_nodes[0].kind, "script");
    assert_eq!(ffi.changed_virtual_nodes[1].kind, "style");
    assert_eq!(ffi.changed_virtual_nodes[1].index, Some(0));
    assert_eq!(ffi.removed_virtual_nodes.len(), 1);
    assert_eq!(ffi.removed_virtual_nodes[0].kind, "style");
    assert_eq!(ffi.removed_virtual_nodes[0].index, Some(2));

    // IDs
    assert_eq!(ffi.changed_virtual_ids, vec!["App.vue?type=script"]);
    assert_eq!(ffi.removed_virtual_ids, vec!["App.vue?type=style&index=2"]);
    assert_eq!(ffi.changed_lsp_ids, vec!["App.vue._VERTER_.script.ts"]);
    assert_eq!(ffi.removed_lsp_ids, vec!["App.vue._VERTER_.style.2.css"]);

    // diagnostics
    assert!(!ffi.diagnostics.has_errors);
    assert_eq!(ffi.diagnostics.diagnostics.len(), 1);
    assert_eq!(ffi.diagnostics.diagnostics[0].severity, "warning");

    // external source requests
    assert_eq!(ffi.external_source_requests.len(), 1);
    assert_eq!(
        ffi.external_source_requests[0].owner_canonical_id,
        "/src/App.vue"
    );
    assert_eq!(ffi.external_source_requests[0].block_kind, "script");
    assert_eq!(ffi.external_source_requests[0].index, 0);
    assert_eq!(ffi.external_source_requests[0].specifier, "./script.ts");

    // import specifiers
    assert_eq!(ffi.import_specifiers.len(), 1);
    assert_eq!(ffi.import_specifiers[0].source, "vue");
    assert!(!ffi.import_specifiers[0].is_type_only);
    assert_eq!(ffi.import_specifiers[0].bindings, vec!["ref", "computed"]);

    assert_eq!(ffi.module_references.len(), 1);
    assert_eq!(ffi.module_references[0].syntax, "dynamicImport");
    assert_eq!(ffi.module_references[0].analyzability, "exact");
    assert_eq!(
        ffi.module_references[0].literal_specifier.as_deref(),
        Some("./Foo.vue")
    );
    assert_eq!(ffi.module_references[0].expr_span_start, 19);
    assert_eq!(ffi.module_references[0].expr_span_end, 30);

    assert_eq!(ffi.parse_duration_ms, 1.5);
}

#[test]
fn host_update_to_ffi_export_signatures() {
    // Use the host to produce real export signatures from a barrel file
    let h = host::VerterHost::new_standalone(host::HostConfig::default());
    let result = h
        .upsert(host::UpsertRequest {
            canonical_id: Some("/src/barrel.ts".to_string()),
            input_id: "/src/barrel.ts".to_string(),
            source: std::sync::Arc::from(
                "export { default as Button } from './Button.vue';\nexport type { Props } from './types';",
            ),
            file_kind: host::FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Verify host produced export signatures
    assert!(
        !result.export_signatures.is_empty(),
        "barrel file must produce export signatures"
    );

    let ffi = host_update_to_ffi(result, None);

    // Positive: re-export signatures are mapped correctly
    let button_sig = ffi.export_signatures.iter().find(|s| s.name == "Button");
    assert!(button_sig.is_some(), "Button re-export must be present");
    let button = button_sig.unwrap();
    assert!(!button.is_type);
    assert_eq!(button.reexport_source, Some("./Button.vue".to_string()));
    assert_eq!(button.reexport_local, Some("default".to_string()));

    let props_sig = ffi.export_signatures.iter().find(|s| s.name == "Props");
    assert!(props_sig.is_some(), "Props type re-export must be present");
    let props = props_sig.unwrap();
    assert!(props.is_type);
    assert_eq!(props.reexport_source, Some("./types".to_string()));
}

#[test]
fn host_update_to_ffi_export_signatures_local_exports() {
    let h = host::VerterHost::new_standalone(host::HostConfig::default());
    let result = h
        .upsert(host::UpsertRequest {
            canonical_id: Some("/src/utils.ts".to_string()),
            input_id: "/src/utils.ts".to_string(),
            source: std::sync::Arc::from("export function greet() {}\nexport type Color = string;"),
            file_kind: host::FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    let ffi = host_update_to_ffi(result, None);

    let greet_sig = ffi.export_signatures.iter().find(|s| s.name == "greet");
    assert!(greet_sig.is_some(), "local export must be present");
    // Negative: local exports must not have reexport fields
    assert_eq!(greet_sig.unwrap().reexport_source, None);
    assert_eq!(greet_sig.unwrap().reexport_local, None);

    let color_sig = ffi.export_signatures.iter().find(|s| s.name == "Color");
    assert!(color_sig.is_some(), "type export must be present");
    assert!(color_sig.unwrap().is_type);
}

#[test]
fn host_update_to_ffi_export_signatures_empty() {
    let result = host::HostUpdateResult::no_change("/src/Empty.vue".to_string());
    let ffi = host_update_to_ffi(result, None);
    assert!(
        ffi.export_signatures.is_empty(),
        "no-change result must have empty export_signatures"
    );
}

#[test]
fn host_update_to_ffi_uses_utf16_conversion_for_embedded_diagnostics() {
    let source = "a😀b";
    let result = host::HostUpdateResult {
        diagnostics: host::DiagnosticsSnapshot {
            diagnostics: vec![host::HostDiagnostic {
                severity: host::HostSeverity::Error,
                code: "E_UTF".to_string(),
                message: "unicode".to_string(),
                span: Some(verter_span::Span::new(1, 5)),
            }],
            has_errors: true,
        },
        ..host::HostUpdateResult::no_change("x".to_string())
    };

    let ffi = host_update_to_ffi(result, Some(source));
    assert_eq!(ffi.diagnostics.diagnostics.len(), 1);
    assert_eq!(ffi.diagnostics.diagnostics[0].span_start, Some(1));
    assert_eq!(ffi.diagnostics.diagnostics[0].span_end, Some(3));
}

#[test]
fn update_result_external_block_kinds() {
    let kinds = [
        (host::ExternalBlockKind::Script, "script"),
        (host::ExternalBlockKind::Template, "template"),
        (host::ExternalBlockKind::Style, "style"),
        (host::ExternalBlockKind::Custom, "custom"),
    ];
    for (host_kind, expected_str) in &kinds {
        let result = host::HostUpdateResult {
            external_source_requests: vec![host::ExternalSourceRequest {
                owner_canonical_id: "x".to_string(),
                block_kind: *host_kind,
                index: 0,
                specifier: "s".to_string(),
                resolved_canonical_id: "r".to_string(),
            }],
            ..host::HostUpdateResult::no_change("x".to_string())
        };
        let ffi = host_update_to_ffi(result, Some("source"));
        assert_eq!(
            ffi.external_source_requests[0].block_kind, *expected_str,
            "block kind mismatch"
        );
    }
}

// ── Output direction: host_virtual_file_to_ffi ───────────────────

#[test]
fn virtual_file_arc_to_string() {
    let source = "a😀b";
    let response = host::VirtualFileResponse {
        id: "Comp.vue._VERTER_.script.ts".to_string(),
        code: Arc::from("export default {}"),
        source_map: Some(Arc::from("{\"mappings\":\"\"}")),
        lang: Some("ts".to_string()),
        stale: true,
        diagnostics: host::DiagnosticsSnapshot {
            diagnostics: vec![host::HostDiagnostic {
                severity: host::HostSeverity::Warning,
                code: "W_UTF".to_string(),
                message: "unicode".to_string(),
                span: Some(verter_span::Span::new(1, 5)),
            }],
            has_errors: false,
        },
        meta: host::VirtualMeta {
            scope_id: Some("data-v-abc123".to_string()),
            block_type: None,
            style_index: Some(2),
            custom_index: None,
        },
        cache_hit: false,
        requested_mode: host::CompileCacheMode::Session,
        actual_mode: host::CompileCacheMode::Session,
        downgrade_reason: None,
    };
    let ffi = host_virtual_file_to_ffi(response, Some(source));
    assert_eq!(ffi.id, "Comp.vue._VERTER_.script.ts");
    assert_eq!(ffi.code, "export default {}");
    assert_eq!(ffi.source_map, Some("{\"mappings\":\"\"}".to_string()));
    assert_eq!(ffi.lang, Some("ts".to_string()));
    assert!(ffi.stale);
    assert_eq!(ffi.meta.scope_id, Some("data-v-abc123".to_string()));
    assert!(ffi.meta.block_type.is_none());
    assert_eq!(ffi.meta.style_index, Some(2));
    assert!(ffi.meta.custom_index.is_none());
    assert_eq!(ffi.diagnostics.diagnostics.len(), 1);
    assert_eq!(ffi.diagnostics.diagnostics[0].span_start, Some(1));
    assert_eq!(ffi.diagnostics.diagnostics[0].span_end, Some(3));
}

#[test]
fn host_virtual_file_to_ffi_uses_utf16_conversion_for_embedded_diagnostics() {
    let source = "a😀b";
    let response = host::VirtualFileResponse {
        id: "x".to_string(),
        code: Arc::from(""),
        source_map: None,
        lang: None,
        stale: false,
        diagnostics: host::DiagnosticsSnapshot {
            diagnostics: vec![host::HostDiagnostic {
                severity: host::HostSeverity::Error,
                code: "E_UTF".to_string(),
                message: "unicode".to_string(),
                span: Some(verter_span::Span::new(1, 5)),
            }],
            has_errors: true,
        },
        meta: host::VirtualMeta::default(),
        cache_hit: false,
        requested_mode: host::CompileCacheMode::Session,
        actual_mode: host::CompileCacheMode::Session,
        downgrade_reason: None,
    };

    let ffi = host_virtual_file_to_ffi(response, Some(source));
    assert_eq!(ffi.diagnostics.diagnostics.len(), 1);
    assert_eq!(ffi.diagnostics.diagnostics[0].span_start, Some(1));
    assert_eq!(ffi.diagnostics.diagnostics[0].span_end, Some(3));
}

#[test]
fn virtual_file_no_source_map() {
    let response = host::VirtualFileResponse {
        id: "x".to_string(),
        code: Arc::from(""),
        source_map: None,
        lang: None,
        stale: false,
        diagnostics: host::DiagnosticsSnapshot::default(),
        meta: host::VirtualMeta::default(),
        cache_hit: false,
        requested_mode: host::CompileCacheMode::Session,
        actual_mode: host::CompileCacheMode::Session,
        downgrade_reason: None,
    };
    let ffi = host_virtual_file_to_ffi(response, Some("source"));
    assert!(ffi.source_map.is_none());
    assert!(ffi.lang.is_none());
    assert!(!ffi.stale);
}

// ── Output direction: host_resolved_id_to_ffi ────────────────────

#[test]
fn resolved_id_conversion() {
    let resolved = host::ResolvedId {
        canonical_id: "/src/Comp.vue".to_string(),
        node_kind: host::VirtualNodeKind::Style { index: 1 },
        exists_in_host: true,
        bundler_id: "Comp.vue?vue&type=style&index=1&lang.css".to_string(),
        lsp_id: "Comp.vue._VERTER_.style.1.css".to_string(),
    };
    let ffi = host_resolved_id_to_ffi(resolved);
    assert_eq!(ffi.canonical_id, "/src/Comp.vue");
    assert_eq!(ffi.node_kind.kind, "style");
    assert_eq!(ffi.node_kind.index, Some(1));
    assert!(ffi.exists_in_host);
    assert_eq!(ffi.bundler_id, "Comp.vue?vue&type=style&index=1&lang.css");
    assert_eq!(ffi.lsp_id, "Comp.vue._VERTER_.style.1.css");
}

// ── Output direction: host_remove_to_ffi ─────────────────────────

#[test]
fn remove_result_conversion() {
    let remove = host::HostRemoveResult {
        canonical_id: "/src/Old.vue".to_string(),
    };
    let ffi = host_remove_to_ffi(remove);
    assert_eq!(ffi.canonical_id, "/src/Old.vue");
}

// ── host_error_to_string: all 4 variants ─────────────────────────

#[test]
fn host_error_missing_source() {
    let err = host::HostError::MissingSource {
        canonical_id: "/src/X.vue".to_string(),
    };
    let s = host_error_to_string(&err);
    assert!(s.contains("MissingSource"));
    assert!(s.contains("/src/X.vue"));
}

#[test]
fn host_error_invalid_query() {
    let s = host_error_to_string(&host::HostError::InvalidQuery);
    assert!(s.contains("InvalidQuery"));
}

#[test]
fn host_error_missing_virtual_node() {
    let err = host::HostError::MissingVirtualNode {
        canonical_id: "/src/Y.vue".to_string(),
    };
    let s = host_error_to_string(&err);
    assert!(s.contains("MissingVirtualNode"));
    assert!(s.contains("/src/Y.vue"));
}

#[test]
fn host_error_compile_error_with_diagnostics() {
    let err = host::HostError::CompileError(host::CompileFailure {
        diagnostics: host::DiagnosticsSnapshot {
            diagnostics: vec![
                host::HostDiagnostic {
                    severity: host::HostSeverity::Error,
                    code: "PARSE_ERR".to_string(),
                    message: "unexpected token".to_string(),
                    span: None,
                },
                host::HostDiagnostic {
                    severity: host::HostSeverity::Warning,
                    code: "WARN_01".to_string(),
                    message: "unused import".to_string(),
                    span: None,
                },
            ],
            has_errors: true,
        },
        requested_mode: host::CompileCacheMode::Session,
        actual_mode: host::CompileCacheMode::Session,
        downgrade_reason: None,
    });
    let s = host_error_to_string(&err);
    assert!(s.contains("CompileError"));
    assert!(s.contains("[PARSE_ERR] unexpected token"));
    assert!(s.contains("[WARN_01] unused import"));
    // Both diagnostics joined by "; "
    assert!(s.contains("; "));
}

// ── FfiConversionError Display: all variants ─────────────────────

#[test]
fn ffi_conversion_error_display_all_variants() {
    let cases: Vec<(FfiConversionError, &str)> = vec![
        (
            FfiConversionError::InvalidCompileErrorPolicy("x".to_string()),
            "invalid compileErrorPolicy 'x' (expected 'strict' or 'dev')",
        ),
        (
            FfiConversionError::InvalidAnalysisLevel("y".to_string()),
            "invalid analysisLevel 'y' (expected 'none', 'essential', or 'full')",
        ),
        (
            FfiConversionError::InvalidHmrStrategy("z".to_string()),
            "invalid hmrStrategy 'z' (expected 'vite', 'webpack', or 'none')",
        ),
        (
            FfiConversionError::InvalidDelimiters(5),
            "delimiters must have exactly 2 elements, got 5",
        ),
        (
            FfiConversionError::InvalidFileKind("bin".to_string()),
            "invalid file_kind 'bin'",
        ),
        (
            FfiConversionError::InvalidNodeKind("frag".to_string()),
            "invalid virtual node kind 'frag'",
        ),
    ];
    for (err, expected) in &cases {
        assert_eq!(err.to_string(), *expected);
    }
}

// ── byte_offset_to_utf16: FFI boundary edge cases ─────────────────

/// Empty source: byte_offset 0 → UTF-16 offset 0.
#[test]
fn utf16_empty_source() {
    assert_eq!(byte_offset_to_utf16("", 0), 0);
}

/// Out-of-bounds offset: an offset beyond the end of the source clamps
/// to the end, returning the total UTF-16 length rather than panicking.
#[test]
fn utf16_out_of_bounds_clamps_to_end() {
    let source = "hello"; // 5 bytes, 5 UTF-16 code units
                          // offset 999 is way past the end
    assert_eq!(byte_offset_to_utf16(source, 999), 5);
    // offset exactly one past the end also clamps
    assert_eq!(byte_offset_to_utf16(source, 6), 5);
}

/// Mid-character clamping for a 2-byte UTF-8 sequence (U+00E9, "é").
///
/// "é" encodes as `[0xC3, 0xA9]` (2 bytes).  A byte offset that lands on
/// the continuation byte (offset 1 inside "é") must clamp backward to the
/// start of the character (offset 0) rather than producing an invalid
/// UTF-8 slice.  The resulting UTF-16 offset is 0 (nothing before "é").
#[test]
fn utf16_mid_char_2byte_clamps_to_char_start() {
    // "é" = U+00E9, UTF-8: [0xC3, 0xA9] (2 bytes), 1 UTF-16 code unit
    let source = "é"; // byte length == 2
    assert_eq!(source.len(), 2, "sanity: é is 2 bytes");

    // byte offset 0: before "é" → 0 UTF-16 code units
    assert_eq!(byte_offset_to_utf16(source, 0), 0);

    // byte offset 1 falls on the continuation byte → clamps to 0 → 0 UTF-16 CUs
    assert_eq!(
        byte_offset_to_utf16(source, 1),
        0,
        "mid-character offset must clamp to char start"
    );

    // byte offset 2: at/after end → 1 UTF-16 code unit (the full "é")
    assert_eq!(byte_offset_to_utf16(source, 2), 1);
}

/// Mid-character clamping for a 4-byte UTF-8 sequence (U+1F600, "😀").
///
/// "😀" encodes as 4 bytes and requires 2 UTF-16 code units (a surrogate
/// pair).  Any byte offset landing inside the 4-byte sequence must clamp
/// backward to byte 0 of the character, yielding 0 UTF-16 code units
/// (nothing before the emoji).
#[test]
fn utf16_mid_char_4byte_surrogate_pair_clamps_to_char_start() {
    // "😀" = U+1F600, UTF-8: 4 bytes, UTF-16: 2 code units (surrogate pair)
    let source = "😀";
    assert_eq!(source.len(), 4, "sanity: 😀 is 4 bytes");

    // offsets 1, 2, 3 all land inside the 4-byte sequence
    for mid in 1u32..=3 {
        assert_eq!(
            byte_offset_to_utf16(source, mid),
            0,
            "byte offset {mid} inside 😀 must clamp to 0 UTF-16 CUs"
        );
    }

    // byte offset 4 (past the char) → 2 UTF-16 code units
    assert_eq!(byte_offset_to_utf16(source, 4), 2);
}

/// UTF-16 offsets inside a surrogate pair clamp to the scalar start.
#[test]
fn utf16_to_byte_offset_clamps_inside_surrogate_pair() {
    let _source = "a\u{1F600}b";
    let source = "aðŸ˜€b";
    let _ = source;
    let source = "a\u{1F600}b";
    assert_eq!(utf16_to_byte_offset(source, 0), 0);
    assert_eq!(utf16_to_byte_offset(source, 1), 1);
    assert_eq!(
        utf16_to_byte_offset(source, 2),
        1,
        "offset inside the emoji surrogate pair should clamp to the emoji start"
    );
    assert_eq!(utf16_to_byte_offset(source, 3), 5);
    assert_eq!(utf16_to_byte_offset(source, 4), 6);
}

/// Verify that ASCII text is a 1:1 mapping (byte offset == UTF-16 offset).
#[test]
fn utf16_ascii_identity() {
    let source = "hello world";
    for i in 0..=(source.len() as u32) {
        assert_eq!(
            byte_offset_to_utf16(source, i),
            i,
            "ASCII byte offset {i} should equal its UTF-16 offset"
        );
    }
}

/// Mixed ASCII + multibyte: offset after a 2-byte char produces the
/// correct UTF-16 value (prior ASCII chars + 1 CU for the 2-byte char).
#[test]
fn utf16_mixed_ascii_and_multibyte() {
    // "aé" = 'a' (1 byte) + 'é' (2 bytes) = 3 bytes total, 2 UTF-16 CUs
    let source = "aé";
    assert_eq!(source.len(), 3);

    assert_eq!(byte_offset_to_utf16(source, 0), 0); // before 'a'
    assert_eq!(byte_offset_to_utf16(source, 1), 1); // after 'a', before 'é'
                                                    // byte offset 2 is the continuation byte of 'é' → clamps to byte 1 → 1 UTF-16 CU
    assert_eq!(
        byte_offset_to_utf16(source, 2),
        1,
        "continuation byte of é clamps to its char start"
    );
    assert_eq!(byte_offset_to_utf16(source, 3), 2); // after 'é'
}

// ── Offset encoding conversion tests ────────────────────────

#[test]
fn utf8_to_utf16_ascii_identity() {
    assert_eq!(utf8_to_utf16_offset("hello world", 5), 5);
}

#[test]
fn utf8_to_utf16_cjk() {
    // "日本" = 2 CJK chars, 3 bytes each = 6 bytes
    let text = "日本abc";
    assert_eq!(utf8_to_utf16_offset(text, 0), 0);
    assert_eq!(utf8_to_utf16_offset(text, 3), 1); // after first CJK
    assert_eq!(utf8_to_utf16_offset(text, 6), 2); // after second CJK
    assert_eq!(utf8_to_utf16_offset(text, 7), 3); // after 'a'
}

#[test]
fn utf8_to_utf16_emoji_surrogate() {
    // 😀 = 4 bytes UTF-8, 2 code units UTF-16
    let text = "a😀b";
    assert_eq!(utf8_to_utf16_offset(text, 0), 0);
    assert_eq!(utf8_to_utf16_offset(text, 1), 1); // after 'a'
    assert_eq!(utf8_to_utf16_offset(text, 5), 3); // after emoji (1+2)
    assert_eq!(utf8_to_utf16_offset(text, 6), 4); // after 'b'
}

#[test]
fn convert_offset_utf8_passthrough() {
    assert_eq!(convert_offset("hello", 3, OffsetEncoding::Utf8), 3);
}

#[test]
fn utf8_to_utf32_basic() {
    let text = "a😀b";
    assert_eq!(utf8_to_utf32_offset(text, 0), 0);
    assert_eq!(utf8_to_utf32_offset(text, 1), 1); // after 'a'
    assert_eq!(utf8_to_utf32_offset(text, 5), 2); // after emoji (1 codepoint)
    assert_eq!(utf8_to_utf32_offset(text, 6), 3); // after 'b'
}

// ── E1 origin graph tests ────────────────────────────────────────

#[test]
fn ffi_payload_contains_origin_field_when_resolved_state_has_origin_graph() {
    use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

    let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
        snapshot: host::FileAnalysisSnapshot::default(),
        mode: host::ProjectionMode::Expanded,
        whole_hash: [0; 16],
        resolved_macros: Vec::new(),
        resolved_type_registry: Vec::new(),
        resolved_type_registry_meta: Vec::new(),
        evaluated_types: None,
        fact_versions: Vec::new(),
        compute_audit: None,
        origin_graph: Some(OriginGraphDto {
            nodes: vec![
                OriginNodeDto {
                    id: 0,
                    kind: "Object".to_string(),
                    label: None,
                },
                OriginNodeDto {
                    id: 1,
                    kind: "Primitive".to_string(),
                    label: None,
                },
            ],
            edges: vec![OriginEdgeDto {
                source: 1,
                target: 0,
                kind: "instantiate".to_string(),
                meta_index: None,
            }],
            meta_strings: Vec::new(),
        }),
        request_id: 0,
        surface_identities: None,
        synthesis_diagnostics: Vec::new(),
        synthesis_should_suppress: false,
    };

    let ffi =
        component_meta_analysis_to_ffi_with_resolution(empty_analysis(), Some(&resolved_state));
    assert!(
        !ffi.origin.edges.is_empty(),
        "FfiComponentMeta.origin must contain edges when resolved state has origin graph"
    );
    assert_eq!(ffi.origin.edges[0].kind, "instantiate");
    assert_eq!(ffi.origin.nodes.len(), 2);
}

#[test]
fn ffi_origin_subgraph_is_empty_when_resolved_state_has_no_origin_graph() {
    let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
        snapshot: host::FileAnalysisSnapshot::default(),
        mode: host::ProjectionMode::Expanded,
        whole_hash: [0; 16],
        resolved_macros: Vec::new(),
        resolved_type_registry: Vec::new(),
        resolved_type_registry_meta: Vec::new(),
        evaluated_types: None,
        fact_versions: Vec::new(),
        compute_audit: None,
        origin_graph: None,
        request_id: 0,
        surface_identities: None,
        synthesis_diagnostics: Vec::new(),
        synthesis_should_suppress: false,
    };

    let ffi =
        component_meta_analysis_to_ffi_with_resolution(empty_analysis(), Some(&resolved_state));
    assert!(
        ffi.origin.edges.is_empty(),
        "FfiComponentMeta.origin must be empty when no origin graph"
    );
}

#[test]
fn ffi_edge_meta_strings_deduplicated() {
    use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

    let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
        snapshot: host::FileAnalysisSnapshot::default(),
        mode: host::ProjectionMode::Expanded,
        whole_hash: [0; 16],
        resolved_macros: Vec::new(),
        resolved_type_registry: Vec::new(),
        resolved_type_registry_meta: Vec::new(),
        evaluated_types: None,
        fact_versions: Vec::new(),
        compute_audit: None,
        origin_graph: Some(OriginGraphDto {
            nodes: vec![
                OriginNodeDto {
                    id: 0,
                    kind: "Object".to_string(),
                    label: None,
                },
                OriginNodeDto {
                    id: 1,
                    kind: "Primitive".to_string(),
                    label: None,
                },
            ],
            edges: vec![
                OriginEdgeDto {
                    source: 0,
                    target: 1,
                    kind: "substituteTypeParam".to_string(),
                    meta_index: Some(0),
                },
                OriginEdgeDto {
                    source: 1,
                    target: 0,
                    kind: "substituteTypeParam".to_string(),
                    meta_index: Some(0),
                },
            ],
            meta_strings: vec!["SubstitutedParam(\"T\")".to_string()],
        }),
        request_id: 0,
        surface_identities: None,
        synthesis_diagnostics: Vec::new(),
        synthesis_should_suppress: false,
    };

    let ffi =
        component_meta_analysis_to_ffi_with_resolution(empty_analysis(), Some(&resolved_state));
    assert_eq!(
        ffi.origin.meta_strings.len(),
        1,
        "meta strings must be deduplicated"
    );
    assert_eq!(ffi.origin.edges.len(), 2);
    assert_eq!(
        ffi.origin.edges[0].meta_index, ffi.origin.edges[1].meta_index,
        "both edges reference the same meta string"
    );
}

#[test]
fn ffi_projection_mode_wire_format() {
    let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
        snapshot: host::FileAnalysisSnapshot::default(),
        mode: host::ProjectionMode::Expanded,
        whole_hash: [0; 16],
        resolved_macros: Vec::new(),
        resolved_type_registry: Vec::new(),
        resolved_type_registry_meta: Vec::new(),
        evaluated_types: None,
        fact_versions: Vec::new(),
        compute_audit: None,
        origin_graph: None,
        request_id: 0,
        surface_identities: None,
        synthesis_diagnostics: Vec::new(),
        synthesis_should_suppress: false,
    };

    let ffi = resolved_component_meta_to_ffi(&resolved_state);
    assert_eq!(
        ffi.mode, "expanded",
        "ProjectionMode::Expanded wire format must be 'expanded'"
    );
}

#[test]
fn ffi_payload_contains_instantiate_edge_for_generic_component() {
    use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

    let resolved_state = host::meta_resolve::ResolvedComponentMetaState {
        snapshot: host::FileAnalysisSnapshot::default(),
        mode: host::ProjectionMode::Expanded,
        whole_hash: [0; 16],
        resolved_macros: Vec::new(),
        resolved_type_registry: Vec::new(),
        resolved_type_registry_meta: Vec::new(),
        evaluated_types: None,
        fact_versions: Vec::new(),
        compute_audit: None,
        origin_graph: Some(OriginGraphDto {
            nodes: vec![
                OriginNodeDto {
                    id: 0,
                    kind: "Object".to_string(),
                    label: Some("{...}".to_string()),
                },
                OriginNodeDto {
                    id: 1,
                    kind: "Primitive".to_string(),
                    label: Some("string".to_string()),
                },
                OriginNodeDto {
                    id: 2,
                    kind: "TypeParam".to_string(),
                    label: Some("T".to_string()),
                },
            ],
            edges: vec![
                OriginEdgeDto {
                    source: 1,
                    target: 0,
                    kind: "instantiate".to_string(),
                    meta_index: None,
                },
                OriginEdgeDto {
                    source: 2,
                    target: 0,
                    kind: "substituteTypeParam".to_string(),
                    meta_index: Some(0),
                },
            ],
            meta_strings: vec!["SubstitutedParam(\"T\")".to_string()],
        }),
        request_id: 0,
        surface_identities: None,
        synthesis_diagnostics: Vec::new(),
        synthesis_should_suppress: false,
    };

    let ffi =
        component_meta_analysis_to_ffi_with_resolution(empty_analysis(), Some(&resolved_state));

    assert_eq!(ffi.origin.nodes.len(), 3, "all 3 origin nodes survive FFI");
    assert_eq!(ffi.origin.edges.len(), 2, "both origin edges survive FFI");

    let has_instantiate = ffi.origin.edges.iter().any(|e| e.kind == "instantiate");
    let has_substitute = ffi
        .origin
        .edges
        .iter()
        .any(|e| e.kind == "substituteTypeParam");
    assert!(has_instantiate, "instantiate edge must survive FFI");
    assert!(has_substitute, "substituteTypeParam edge must survive FFI");

    let type_param_node = ffi.origin.nodes.iter().find(|n| n.kind == "TypeParam");
    assert!(type_param_node.is_some(), "TypeParam node must survive FFI");
    assert_eq!(
        type_param_node.unwrap().label.as_deref(),
        Some("T"),
        "TypeParam label must survive FFI"
    );

    assert_eq!(ffi.origin.meta_strings.len(), 1, "meta strings survive FFI");
    assert_eq!(
        ffi.origin.meta_strings[0], "SubstitutedParam(\"T\")",
        "meta string content survives FFI"
    );

    let proto_bytes = verter_protocol::component_meta::encode_component_meta_payload(&ffi);
    assert!(
        !proto_bytes.is_empty(),
        "proto encoding of origin graph must produce non-empty bytes"
    );
}

// ─────────────────────────────────────────────────────────────────────
// CompileCacheMode round-trips cleanly across the FFI seam that BOTH the
// NAPI and WASM bindings funnel through. NAPI converts
// `NapiCompileProfile -> FfiCompileProfile -> ffi_profile_to_host`, and
// WASM deserialises `FfiCompileProfile` directly; both then serialise
// `FfiVirtualFileResponse`. These tests pin the shared seam: the
// requested-mode string parses into the host profile, an invalid string
// is rejected, and the host response's mode fields serialise back out.
//
// Discrimination: before the compile-mode FFI seam existed,
// `FfiCompileProfile.requested_mode`, the three `FfiVirtualFileResponse`
// mode fields, and `FfiConversionError::InvalidCompileCacheMode` did not
// exist — the test would not compile.
// ─────────────────────────────────────────────────────────────────────

#[test]
fn ffi_seam_requested_mode_parses_each_variant() {
    for (s, expected) in [
        ("stateless", host::CompileCacheMode::Stateless),
        ("content", host::CompileCacheMode::Content),
        ("session", host::CompileCacheMode::Session),
        // case-insensitive
        ("Content", host::CompileCacheMode::Content),
    ] {
        let profile = FfiCompileProfile {
            requested_mode: Some(s.to_string()),
            ..Default::default()
        };
        let host_profile = ffi_profile_to_host(Some(profile)).expect("parse mode");
        assert_eq!(
            host_profile.requested_mode, expected,
            "requestedMode '{s}' must parse to {expected:?}"
        );
    }

    // A missing requestedMode keeps the host default (Session).
    let default_profile = ffi_profile_to_host(Some(FfiCompileProfile::default())).unwrap();
    assert_eq!(
        default_profile.requested_mode,
        host::CompileCacheMode::Session,
        "an absent requestedMode must default to Session"
    );
}

#[test]
fn ffi_seam_invalid_requested_mode_is_rejected() {
    let profile = FfiCompileProfile {
        requested_mode: Some("turbo".to_string()),
        ..Default::default()
    };
    let err = ffi_profile_to_host(Some(profile)).expect_err("invalid mode must error");
    assert!(
        matches!(err, FfiConversionError::InvalidCompileCacheMode(ref s) if s == "turbo"),
        "an unknown requestedMode must produce InvalidCompileCacheMode, got {err:?}"
    );
}

#[test]
fn ffi_seam_response_serialises_mode_fields() {
    // Build a host response carrying a Content->Stateless downgrade on a
    // cold miss and confirm the FFI conversion surfaces all three mode
    // fields plus `cache_hit` (NAPI/WASM serialise this same shape out to
    // JS). The downgrade case is a cold miss (`cache_hit == false`).
    let response = host::VirtualFileResponse {
        id: "Comp.vue".to_string(),
        code: Arc::from("export default {}"),
        source_map: None,
        lang: Some("ts".to_string()),
        stale: false,
        diagnostics: host::DiagnosticsSnapshot::default(),
        meta: host::VirtualMeta::default(),
        cache_hit: false,
        requested_mode: host::CompileCacheMode::Content,
        actual_mode: host::CompileCacheMode::Stateless,
        downgrade_reason: Some(host::DowngradeReason::HasMacroTypeDeps),
    };
    let ffi = host_virtual_file_to_ffi(response, None);
    assert_eq!(ffi.requested_mode, "content");
    assert_eq!(ffi.actual_mode, "stateless");
    assert_eq!(ffi.downgrade_reason, Some("HasMacroTypeDeps".to_string()));
    assert!(
        !ffi.cache_hit,
        "a cold-miss response must serialise cache_hit == false through the FFI seam"
    );

    // No-downgrade warm-hit case: a Session response served from a warm
    // slot reports session/session/None and `cache_hit == true`.
    let session_response = host::VirtualFileResponse {
        id: "Comp.vue".to_string(),
        code: Arc::from(""),
        source_map: None,
        lang: None,
        stale: false,
        diagnostics: host::DiagnosticsSnapshot::default(),
        meta: host::VirtualMeta::default(),
        cache_hit: true,
        requested_mode: host::CompileCacheMode::Session,
        actual_mode: host::CompileCacheMode::Session,
        downgrade_reason: None,
    };
    let ffi2 = host_virtual_file_to_ffi(session_response, None);
    assert_eq!(ffi2.requested_mode, "session");
    assert_eq!(ffi2.actual_mode, "session");
    assert_eq!(ffi2.downgrade_reason, None);
    assert!(
        ffi2.cache_hit,
        "a warm-hit response must serialise cache_hit == true through the FFI seam"
    );
}
