//! Verter-native `TypeExpander` implementation.
//!
//! Wraps the existing OXC-based cross-file resolver behind the [`TypeExpander`]
//! trait. No external processes — pure Rust.
//!
//! The async boundary exists at host/source-loading edges. Core semantic work
//! stays synchronous.

use std::sync::Arc;

use verter_semantic::analysis::type_expr::{PrimitiveName, TypeExpr};
#[cfg(test)]
use verter_span::Span;

use crate::resolver_core::type_expansion::{
    ExpandedMember, ExpanderFuture, ExpansionCompleteness, TypeExpander, TypeExpansionError,
    TypeExpansionRequest, TypeExpansionResult,
};
use crate::resolver_core::ResolvedMacroMeta;

// ---------------------------------------------------------------------------
// VerterComponentMetaProvider trait
// ---------------------------------------------------------------------------

/// Trait for hosts that can provide component-meta results.
///
/// Implemented by `ComponentMetaHost` in `verter_session`. This allows
/// `VerterTypeExpander` to call into the existing resolution pipeline
/// without depending on `verter_session` directly.
pub trait VerterComponentMetaProvider: Send + Sync {
    /// Get full component metadata for a file (uses Verter's native resolver).
    fn get_component_meta(
        &self,
        canonical_id: &str,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>;

    /// Get the resolved macro metadata for a file (for type expansion).
    fn get_resolved_macros(&self, canonical_id: &str) -> Vec<ResolvedMacroMeta> {
        // Default: not available. Implementations override.
        let _ = canonical_id;
        vec![]
    }
}

// ---------------------------------------------------------------------------
// VerterTypeExpander
// ---------------------------------------------------------------------------

/// Native Verter `TypeExpander` — wraps the existing OXC-based resolver.
///
/// Resolves types using Verter's cross-file import graph traversal and
/// type evaluation. Does not spawn external processes.
///
/// Calls `VerterComponentMetaProvider::get_component_meta()` and extracts
/// the matching macro's resolved data for the requested span.
pub struct VerterTypeExpander {
    provider: Arc<dyn VerterComponentMetaProvider>,
}

impl VerterTypeExpander {
    pub fn new(provider: Arc<dyn VerterComponentMetaProvider>) -> Self {
        Self { provider }
    }
}

impl TypeExpander for VerterTypeExpander {
    fn expand_type<'a>(
        &'a self,
        request: &'a TypeExpansionRequest,
    ) -> ExpanderFuture<'a, TypeExpansionResult> {
        Box::pin(async move {
            let meta = self
                .provider
                .get_component_meta(&request.canonical_id)
                .ok_or(TypeExpansionError::SourceUnavailable)?;

            // Return all props as expanded members (the Verter backend
            // resolves the full component surface, not a single type).
            let members: Vec<ExpandedMember> = meta
                .props
                .iter()
                .map(|p| ExpandedMember {
                    name: p.name.clone(),
                    type_expr: p.type_expr.clone(),
                    raw_type: p.raw_type.clone(),
                    optional: !p.required,
                    description: p.description.clone(),
                })
                .collect();

            let completeness = if members.is_empty() {
                ExpansionCompleteness::LowerBound
            } else {
                ExpansionCompleteness::Exact
            };

            Ok(TypeExpansionResult {
                type_expr: TypeExpr::primitive(PrimitiveName::Unknown),
                members,
                completeness,
            })
        })
    }
}

/// Convert a resolved macro's props to `ExpandedMember` entries.
pub fn resolved_macro_to_members(macro_meta: &ResolvedMacroMeta) -> Vec<ExpandedMember> {
    let mut members = Vec::new();

    for prop in &macro_meta.props {
        let type_expr = prop
            .type_annotation
            .as_deref()
            .map(crate::resolver_core::type_text_parser::parse_type_text)
            .unwrap_or_else(|| TypeExpr::primitive(PrimitiveName::Unknown));
        members.push(ExpandedMember {
            name: prop.name.clone(),
            type_expr,
            raw_type: prop.type_annotation.clone(),
            optional: prop.is_optional,
            description: prop.description.clone(),
        });
    }

    for emit in &macro_meta.emits {
        let type_expr = emit
            .payload_type
            .as_deref()
            .map(crate::resolver_core::type_text_parser::parse_type_text)
            .unwrap_or_else(|| TypeExpr::primitive(PrimitiveName::Unknown));
        members.push(ExpandedMember {
            name: emit.name.clone(),
            type_expr,
            raw_type: emit.payload_type.clone(),
            optional: false,
            description: emit.description.clone(),
        });
    }

    for slot in &macro_meta.slots {
        members.push(ExpandedMember {
            name: slot.name.clone(),
            type_expr: TypeExpr::primitive(PrimitiveName::Unknown),
            raw_type: slot.return_type.clone(),
            optional: !slot.is_required,
            description: slot.description.clone(),
        });
    }

    members
}

/// Build a `TypeExpansionResult` from a resolved macro.
pub fn resolved_macro_to_expansion(macro_meta: &ResolvedMacroMeta) -> TypeExpansionResult {
    let members = resolved_macro_to_members(macro_meta);
    let completeness = if members.is_empty() {
        ExpansionCompleteness::LowerBound
    } else {
        ExpansionCompleteness::Exact
    };

    let type_expr = if !macro_meta.type_name.is_empty() {
        TypeExpr::named(&macro_meta.type_name)
    } else {
        TypeExpr::primitive(PrimitiveName::Unknown)
    };

    TypeExpansionResult {
        type_expr,
        members,
        completeness,
    }
}

// ---------------------------------------------------------------------------
// Solver-backed type expansion (M6 cutover path)
// ---------------------------------------------------------------------------

/// Expand a resolved macro's type annotations through the new native solver.
///
/// Takes the same `ResolvedMacroMeta` as `resolved_macro_to_expansion` but
/// runs each prop/emit type_annotation through `solve_type` for deeper
/// resolution of generics, utilities, and cross-file types.
///
/// Returns the expansion result and a list of external declarations visited
/// during solving (for registry publishing).
pub fn resolved_macro_to_expansion_via_solver(
    macro_meta: &ResolvedMacroMeta,
    host: &crate::VerterHost,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> (
    TypeExpansionResult,
    Vec<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity>,
) {
    use verter_semantic::analysis::type_solver::audit::{NoopAudit, RecordingAudit};
    use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
    use verter_semantic::analysis::type_solver::query_engine::TypeQueryEngine;

    enum SolverEngine<'a> {
        Noop(Box<TypeQueryEngine<'a, NoopAudit>>),
        Recording(Box<TypeQueryEngine<'a, RecordingAudit>>),
    }

    impl<'a> SolverEngine<'a> {
        fn solve_with_trace(
            &mut self,
            expr: &TypeExpr,
        ) -> (
            verter_semantic::analysis::type_solver::result::SolverResult<TypeExpr>,
            Vec<ResolvedRootIdentity>,
        ) {
            match self {
                Self::Noop(engine) => engine.solve_with_trace(expr),
                Self::Recording(engine) => engine.solve_with_trace(expr),
            }
        }
    }

    let solver_host = if macro_meta.declaration.canonical_source.is_empty() {
        crate::resolver_core::SessionSolverHost::new(host, store_view)
    } else {
        crate::resolver_core::SessionSolverHost::with_declaration_scope(
            host,
            store_view,
            macro_meta.declaration.canonical_source.as_str(),
        )
    };
    let mut engine = if host.config.audit_enabled {
        SolverEngine::Recording(Box::new(TypeQueryEngine::new_with_recording(&solver_host)))
    } else {
        SolverEngine::Noop(Box::new(TypeQueryEngine::new(&solver_host)))
    };
    let mut all_visited = Vec::new();

    let mut members = Vec::new();

    for prop in &macro_meta.props {
        let type_expr = prop
            .type_annotation
            .as_deref()
            .map(|text| {
                let parsed = crate::resolver_core::type_text_parser::parse_type_text(text);
                let (result, trace) = engine.solve_with_trace(&parsed);
                all_visited.extend(trace);
                result.value
            })
            .unwrap_or_else(|| TypeExpr::primitive(PrimitiveName::Unknown));

        members.push(ExpandedMember {
            name: prop.name.clone(),
            type_expr,
            raw_type: prop.type_annotation.clone(),
            optional: prop.is_optional,
            description: prop.description.clone(),
        });
    }

    for emit in &macro_meta.emits {
        let type_expr = emit
            .payload_type
            .as_deref()
            .map(|text| {
                let parsed = crate::resolver_core::type_text_parser::parse_type_text(text);
                let (result, trace) = engine.solve_with_trace(&parsed);
                all_visited.extend(trace);
                result.value
            })
            .unwrap_or_else(|| TypeExpr::primitive(PrimitiveName::Unknown));

        members.push(ExpandedMember {
            name: emit.name.clone(),
            type_expr,
            raw_type: emit.payload_type.clone(),
            optional: false,
            description: emit.description.clone(),
        });
    }

    for slot in &macro_meta.slots {
        members.push(ExpandedMember {
            name: slot.name.clone(),
            type_expr: TypeExpr::primitive(PrimitiveName::Unknown),
            raw_type: slot.return_type.clone(),
            optional: !slot.is_required,
            description: slot.description.clone(),
        });
    }

    let completeness = if members.is_empty() {
        ExpansionCompleteness::LowerBound
    } else {
        ExpansionCompleteness::Exact
    };

    let type_expr = if !macro_meta.type_name.is_empty() {
        let parsed = TypeExpr::named(&macro_meta.type_name);
        let (result, trace) = engine.solve_with_trace(&parsed);
        all_visited.extend(trace);
        result.value
    } else {
        TypeExpr::primitive(PrimitiveName::Unknown)
    };

    (
        TypeExpansionResult {
            type_expr,
            members,
            completeness,
        },
        all_visited,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::ResolvedTypeDeclaration;
    use verter_semantic::analysis::types::AnalyzedMacroKind;

    fn make_resolved_macro() -> ResolvedMacroMeta {
        ResolvedMacroMeta {
            macro_index: 0,
            macro_kind: AnalyzedMacroKind::DefineProps,
            type_name: "ButtonProps".to_string(),
            import_source: "./types".to_string(),
            declaration: ResolvedTypeDeclaration {
                requested_name: "ButtonProps".to_string(),
                declaration_id: None,
                resolved_name: "ButtonProps".to_string(),
                canonical_source: "/src/types.ts".to_string(),
                span: Span::new(10, 50),
                kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
                text: None,
            },
            native_props: vec![],
            props: vec![
                verter_semantic::analysis::AnalyzedPropField {
                    name: "msg".to_string(),
                    is_optional: false,
                    type_annotation: Some("string".to_string()),
                    span: Span::new(20, 30),
                    description: None,
                    tags: vec![],
                    resolution_source: Default::default(),
                    resolution_error: None,
                },
                verter_semantic::analysis::AnalyzedPropField {
                    name: "count".to_string(),
                    is_optional: true,
                    type_annotation: Some("number".to_string()),
                    span: Span::new(35, 45),
                    description: None,
                    tags: vec![],
                    resolution_source: Default::default(),
                    resolution_error: None,
                },
            ],
            emits: vec![],
            slots: vec![],
            jsdoc: None,
        }
    }

    #[test]
    fn resolved_macro_to_members_extracts_props() {
        let macro_meta = make_resolved_macro();
        let members = resolved_macro_to_members(&macro_meta);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "msg");
        assert!(!members[0].optional);
        assert_eq!(members[1].name, "count");
        assert!(members[1].optional);
    }

    #[test]
    fn resolved_macro_to_members_does_not_include_unrelated_fields() {
        let macro_meta = make_resolved_macro();
        let members = resolved_macro_to_members(&macro_meta);
        for m in &members {
            assert!(
                m.name == "msg" || m.name == "count",
                "unexpected member: {}",
                m.name
            );
        }
    }

    #[test]
    fn resolved_macro_to_expansion_has_correct_completeness() {
        let macro_meta = make_resolved_macro();
        let result = resolved_macro_to_expansion(&macro_meta);
        assert_eq!(result.completeness, ExpansionCompleteness::Exact);
        assert_eq!(result.members.len(), 2);
    }

    #[test]
    fn resolved_macro_to_expansion_empty_macro_is_lower_bound() {
        let mut macro_meta = make_resolved_macro();
        macro_meta.props.clear();
        let result = resolved_macro_to_expansion(&macro_meta);
        assert_eq!(result.completeness, ExpansionCompleteness::LowerBound);
    }

    #[test]
    fn resolved_macro_to_expansion_type_name_is_reference() {
        let macro_meta = make_resolved_macro();
        let result = resolved_macro_to_expansion(&macro_meta);
        match &result.type_expr {
            TypeExpr::Ref { name, .. } => assert_eq!(name.as_ref(), "ButtonProps"),
            other => panic!("expected Ref, got: {other:?}"),
        }
    }

    // -- M5: Comparison harness — old vs solver --

    #[test]
    fn solver_expansion_matches_old_path_for_simple_macro() {
        let macro_meta = make_resolved_macro();

        // Old path
        let old = resolved_macro_to_expansion(&macro_meta);

        // Solver path (standalone host — no files loaded, so solver can't
        // resolve cross-file refs, but primitive type_text should match)
        let host = crate::VerterHost::new_standalone(Default::default());
        let (solver, _trace) = resolved_macro_to_expansion_via_solver(&macro_meta, &host, None);

        // Same number of members
        assert_eq!(old.members.len(), solver.members.len());

        // Same member names and optionality
        for (o, s) in old.members.iter().zip(solver.members.iter()) {
            assert_eq!(o.name, s.name, "member name mismatch");
            assert_eq!(o.optional, s.optional, "optional mismatch for {}", o.name);
            assert_eq!(o.raw_type, s.raw_type, "raw_type mismatch for {}", o.name);
        }

        // Solver resolves primitive type_text directly
        for m in &solver.members {
            assert!(
                !matches!(m.type_expr, TypeExpr::Unknown { .. }),
                "solver should resolve primitive type text for {}",
                m.name
            );
        }
    }

    #[test]
    fn solver_expansion_preserves_completeness() {
        let macro_meta = make_resolved_macro();
        let host = crate::VerterHost::new_standalone(Default::default());
        let (result, _trace) = resolved_macro_to_expansion_via_solver(&macro_meta, &host, None);
        assert_eq!(result.completeness, ExpansionCompleteness::Exact);
    }

    #[test]
    fn solver_expansion_empty_macro_is_lower_bound() {
        let mut macro_meta = make_resolved_macro();
        macro_meta.props.clear();
        let host = crate::VerterHost::new_standalone(Default::default());
        let (result, _trace) = resolved_macro_to_expansion_via_solver(&macro_meta, &host, None);
        assert_eq!(result.completeness, ExpansionCompleteness::LowerBound);
    }

    #[test]
    fn solver_expansion_supports_audit_enabled_host() {
        let macro_meta = make_resolved_macro();
        let host = crate::VerterHost::new_standalone(crate::HostConfig {
            audit_enabled: true,
            ..Default::default()
        });
        let (result, _trace) = resolved_macro_to_expansion_via_solver(&macro_meta, &host, None);

        assert_eq!(result.completeness, ExpansionCompleteness::Exact);
        assert_eq!(result.members.len(), 2);
        assert!(
            result
                .members
                .iter()
                .all(|member| !matches!(member.type_expr, TypeExpr::Unknown { .. })),
            "audit-enabled hosts should still resolve primitive type text through the solver",
        );
    }

    #[test]
    fn solver_expansion_uses_declaration_scope_for_projected_macro_text() {
        use rustc_hash::FxHashMap;
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::type_eval_build::parse_and_build_env;
        use verter_semantic::analysis::Hash16;

        let source = r#"
export interface Button {
  variants: {
    size: 'sm' | 'lg'
  }
}
"#;
        let allocator = oxc_allocator::Allocator::new();
        let analysis = Arc::new(analyze_external_type_source(source, &allocator));
        let env = parse_and_build_env(source);
        let state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&analysis),
            Some(&env),
        ));
        let host = crate::VerterHost::new_standalone(Default::default());
        host.imported_dependency_cache.lock().insert(
            "/src/types.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/src/types.ts".into(),
                raw_source: Arc::<str>::from(source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(analysis),
                shallow_file_state: Some(Arc::clone(&state)),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(source)),
                required_owner_import_names: None,
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),

                dependency_resolutions: FxHashMap::default(),
            }),
        );

        let mut macro_meta = make_resolved_macro();
        macro_meta.declaration.canonical_source = "/src/types.ts".to_string();
        macro_meta.type_name.clear();
        macro_meta.props = vec![verter_semantic::analysis::AnalyzedPropField {
            name: "size".to_string(),
            is_optional: false,
            type_annotation: Some("Button".to_string()),
            span: Span::new(20, 30),
            description: None,
            tags: vec![],
            resolution_source: Default::default(),
            resolution_error: None,
        }];

        let (result, _trace) = resolved_macro_to_expansion_via_solver(&macro_meta, &host, None);

        assert_eq!(result.members.len(), 1);
        match &result.members[0].type_expr {
            TypeExpr::Object(shape) => assert!(shape.properties.iter().any(|member| matches!(
                member,
                verter_semantic::analysis::type_expr::ObjectMember::Property(prop)
                    if prop.name == "variants"
            ))),
            other => panic!("expected declaration-scoped object resolution, got {other:?}"),
        }
    }
}
