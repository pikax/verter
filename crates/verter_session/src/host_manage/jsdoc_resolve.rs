//! `HostComponentMetaResolver` adapter + JSDoc resolution helpers.
//!
//! domain 14 + 15 — owns:
//! - `HostComponentMetaResolver<'a>` struct + `impl
//!   DeclarationMetadataResolver` + `impl ComponentMetaResolverHost`
//!   (the adapter that wraps `&VerterHost` for the
//!   `resolver_core::resolve_type_declaration` entry point and the
//!   `ComponentMetaResolverHost` trait used by the surface projector).
//! - `pub(crate) fn resolve_type_declaration` (the host-method
//!   wrapper that constructs a `HostComponentMetaResolver`, calls
//!   `resolver_core::resolve_type_declaration`, and threads the
//!   resolved-shape through).
//! - The JSDoc helpers
//!   (`read_full_source`, `resolve_jsdoc_block`, `map_jsdoc_tag`,
//!   `parse_jsdoc_tag_payload`, `resolve_jsdoc_tag_type`).
//!
//! Visibility: the previously-private `struct HostComponentMetaResolver`
//! and the JSDoc free fns are exposed at `pub(crate)` so the
//! `host_methods.rs` impl block keeps calling them via the
//! shell's `pub(crate) use jsdoc_resolve::*;` re-export.

use crate::host_manage::{component_meta_debug, component_meta_debug_enabled};
use crate::resolver_core::ComponentMetaEvalOutputs;
use crate::types::{FileAnalysisSnapshot, ProjectionMode};
use crate::VerterHost;

use crate::instant::Instant;

// file moved from `meta_resolve/jsdoc_resolve.rs` to
// `host_manage/jsdoc_resolve.rs`. The original `super::dispatch_helpers`
// import resolved through `meta_resolve` private siblings; after the
// move, `super` is `host_manage`, so the rewrite goes via the
// `crate::meta_resolve` re-export surface.
use crate::host_manage::component_meta_request_impl::{
    CapturedComponentMetaInputs, ResolvedJsdocBlock, ResolvedJsdocTag, ResolvedTypeDeclaration,
};
use crate::meta_resolve::project_expr_class_a_via_dispatch;

pub(crate) struct HostComponentMetaResolver<'a> {
    pub(crate) host: &'a VerterHost,
    pub(crate) ctx: &'a dyn crate::resolver_core::resolver_context::ResolverContext,
}

impl crate::resolver_core::DeclarationMetadataResolver for HostComponentMetaResolver<'_> {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<crate::resolver_core::ResolvedExportTarget> {
        self.ctx
            .resolve_named_type_export_target(dep_canonical, requested_name)
            .map(
                |(canonical, name)| crate::resolver_core::ResolvedExportTarget {
                    source_canonical_id: (canonical != dep_canonical).then_some(canonical),
                    source_name: name,
                },
            )
    }

    fn get_export_span_follow_reexports(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<verter_span::Span> {
        self.host
            .get_export_span_follow_reexports(dep_canonical, requested_name)
            .map(|(_, start, end)| verter_span::Span::new(start, end))
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        self.host
            .local_type_declaration_id(canonical_source, resolved_name)
    }

    fn resolve_type_dependency_canonical(
        &self,
        from_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.ctx
            .resolve_type_dependency_canonical(from_canonical, import_source)
    }

    fn resolve_direct_type_reexport_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        self.host
            .resolve_direct_type_reexport_target(dep_canonical, requested_name)
    }

    fn resolve_local_import_symbol_target(
        &self,
        dep_canonical: &str,
        resolved_name: &str,
    ) -> Option<(String, String)> {
        // Overlay-aware: the owner file (and its import surface) may
        // exist only in a session overlay, so the shallow import
        // lookup must read through the resolver context's view — the
        // base host's shallow state has no entry for overlay-only
        // files and would mis-classify an imported helper as
        // owner-local.
        let shallow = self.ctx.shallow_file_state(dep_canonical)?;
        let import_target = shallow.import_target(resolved_name)?;
        let next_canonical = if import_target.canonical_id.is_empty() {
            self.ctx
                .resolve_type_dependency_canonical(dep_canonical, &import_target.source_specifier)?
        } else {
            import_target.canonical_id.clone()
        };
        Some((next_canonical, import_target.imported_name.clone()))
    }

    fn resolve_local_export_symbol_target(
        &self,
        canonical_source: &str,
        exported_name: &str,
    ) -> Option<String> {
        self.host
            .resolve_local_export_symbol_target(canonical_source, exported_name)
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<crate::resolver_core::ResolvedLocalTypeSymbolMetadata> {
        let analysis = self.ctx.external_type_analysis(canonical_source)?;
        let symbol = analysis.local_type_symbol(resolved_name)?;
        let kind = match symbol.kind {
            verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSymbolKind::TypeAlias => {
                crate::resolver_core::ResolvedDeclarationKind::TypeAlias
            }
            verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSymbolKind::Interface => {
                crate::resolver_core::ResolvedDeclarationKind::Interface
            }
            verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSymbolKind::Class => {
                crate::resolver_core::ResolvedDeclarationKind::Class
            }
        };
        Some(crate::resolver_core::ResolvedLocalTypeSymbolMetadata {
            kind,
            span: symbol.span,
        })
    }
}

impl HostComponentMetaResolver<'_> {
    /// Shared owner-local macro-root presence gate, decided in NODE DOMAIN.
    ///
    /// Lowers the bare root reference at `Navigate` and resolves its one-level
    /// `SurfaceView` through the SOLE query-time dispatch at `Published(Shallow)`
    /// (the same demand the prior `ExpandedObjectShape` bridge resolved), then
    /// decides per macro kind whether the root carries a non-empty macro surface
    /// directly off the `SurfaceView` — never materialising it to a `TypeExpr` /
    /// `ExpandedObjectShape`.
    ///
    /// Construct signatures and index signatures live on dedicated `SurfaceView`
    /// fields; the materialised `ExpandedObjectShape` form FOLDS construct
    /// signatures into `call_signatures` and surfaces an open index domain through
    /// `has_index_signature`, so the props/model/slots gate ORs `call_signatures`,
    /// `construct_signatures`, `index_signatures` AND `has_index_signature` to keep
    /// the presence semantics identical to the materialised reader it replaces.
    fn owner_local_macro_root_surface_presence(
        &self,
        owner_canonical: &str,
        root_name: &str,
        macro_kind: verter_semantic::analysis::types::AnalyzedMacroKind,
    ) -> bool {
        use verter_semantic::analysis::AnalyzedMacroKind;

        let root_ref = verter_type_expr::TypeExpr::Ref {
            name: std::sync::Arc::from(root_name),
            type_arguments: std::sync::Arc::from(Vec::<verter_type_expr::TypeExpr>::new()),
        };
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
        let Some(base) = dispatch.lower_type_expr_in_scope_with_mode(
            owner_canonical,
            &root_ref,
            crate::semantic_query::ProjectionMode::Navigate,
        ) else {
            return false;
        };
        let Some(view) = dispatch.resolve_typeinfo_surface_view(
            base,
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Shallow,
            ),
        ) else {
            return false;
        };
        match macro_kind {
            // Props / model / slots gate on any member surface: named members,
            // call signatures, construct signatures, a concrete index signature,
            // OR an open index domain.
            AnalyzedMacroKind::DefineProps
            | AnalyzedMacroKind::WithDefaults
            | AnalyzedMacroKind::DefineModel
            | AnalyzedMacroKind::DefineSlots => {
                !view.members.is_empty()
                    || !view.call_signatures.is_empty()
                    || !view.construct_signatures.is_empty()
                    || !view.index_signatures.is_empty()
                    || view.has_index_signature
            }
            // Emits surface comes from property-style members or callable events
            // (call signatures, or construct signatures folded alongside them).
            AnalyzedMacroKind::DefineEmits => {
                !view.members.is_empty()
                    || !view.call_signatures.is_empty()
                    || !view.construct_signatures.is_empty()
            }
            // The exposed surface publishes named members only
            // (`exposed_from_typeinfo_surface`), so the presence gate is the
            // named-property surface.
            AnalyzedMacroKind::DefineExpose => !view.members.is_empty(),
            AnalyzedMacroKind::DefineOptions => false,
        }
    }
}

impl crate::resolver_core::ComponentMetaResolverHost for HostComponentMetaResolver<'_> {
    type Snapshot = FileAnalysisSnapshot;
    type EvalContext = CapturedComponentMetaInputs;

    fn resolve_type_declaration(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        resolve_type_declaration_with_context(self.host, self.ctx, dep_canonical, requested_name)
    }

    fn snapshot_imports<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::AnalyzedImport] {
        snapshot.imports.as_slice()
    }

    fn snapshot_macros<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::AnalyzedMacro] {
        snapshot.macros.as_slice()
    }

    fn snapshot_macro_type_deps<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::MacroTypeDep] {
        snapshot.macro_type_deps.as_slice()
    }

    fn build_eval_outputs(
        &self,
        owner_canonical: &str,
        snapshot: &Self::Snapshot,
        eval_context: Option<&Self::EvalContext>,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
    ) -> ComponentMetaEvalOutputs {
        let eval_started = component_meta_debug_enabled().then(Instant::now);
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} step=evaluated_types:start imports={} macro_type_deps={}",
                owner_canonical,
                ProjectionMode::Expanded,
                snapshot.imports.len(),
                snapshot.macro_type_deps.len(),
            ));
        }
        // Tracked dependencies: snapshot-level candidates + solver-discovered deps.
        let mut tracked_dependencies = std::collections::BTreeSet::new();
        tracked_dependencies.extend(
            eval_context
                .map(|captured| captured.direct_dependency_candidates.clone())
                .unwrap_or_else(|| {
                    self.host
                        .cache_dependency_candidates_from_snapshot(owner_canonical, snapshot)
                }),
        );
        let compute_eval_start = component_meta_debug_enabled().then(Instant::now);
        // the retired `shared_owner_engine` path
        // is gone; all callers go through
        // `compute_evaluated_types_with_tracking_from_owner_context_with_ctx`
        // which threads the resolver context so overlay-bearing
        // sessions observe overlay candidates for cross-file types.
        let computed_eval_types = self
            .host
            .compute_evaluated_types_with_tracking_from_owner_context_with_ctx(
                self.ctx,
                owner_canonical,
                snapshot,
                eval_context.and_then(|captured| captured.owner_eval_source.as_deref()),
                purpose,
            );
        if let Some(compute_eval_start) = compute_eval_start {
            let elapsed = compute_eval_start.elapsed();
            component_meta_debug(format!(
                "EVAL_TYPES owner={} elapsed_ms={:.1} has_result={}",
                owner_canonical,
                elapsed.as_secs_f64() * 1000.0,
                computed_eval_types.is_some(),
            ));
        }
        if let Some(computed) = computed_eval_types.as_ref() {
            tracked_dependencies.extend(computed.discovered_dependencies.iter().cloned());
        }
        let (evaluated_types, surface_identities) = computed_eval_types
            .map(|computed| (computed.evaluated_types, computed.surface_identities))
            .unwrap_or((None, None));
        if let Some(eval_started) = eval_started {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} evaluated_types took {:?} has_output={}",
                owner_canonical,
                ProjectionMode::Expanded,
                eval_started.elapsed(),
                evaluated_types
                    .as_ref()
                    .is_some_and(|types| !types.is_empty()),
            ));
        }
        ComponentMetaEvalOutputs {
            evaluated_types,
            tracked_dependencies,
            surface_identities,
        }
    }

    fn macro_type_arg_has_direct_reference(
        &self,
        owner_canonical: &str,
        mac: &verter_semantic::analysis::types::AnalyzedMacro,
        type_name: &str,
    ) -> Option<bool> {
        let locator = mac.parsed_type_argument.as_ref()?;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
        let payload = dispatch.raise_authored_locator_to_hot(
            &verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(
                absolutize_macro_payload_locator(locator, owner_canonical),
            ),
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        )?;
        Some(node_has_direct_macro_reference(
            self.ctx,
            payload.node(),
            type_name,
        ))
    }

    fn projectable_owner_local_macro_roots(
        &self,
        owner_canonical: &str,
        mac: &verter_semantic::analysis::types::AnalyzedMacro,
    ) -> Vec<String> {
        fn macro_lacks_direct_local_surface(
            mac: &verter_semantic::analysis::types::AnalyzedMacro,
        ) -> bool {
            match mac.kind {
                verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                | verter_semantic::analysis::AnalyzedMacroKind::WithDefaults
                | verter_semantic::analysis::AnalyzedMacroKind::DefineModel => {
                    mac.prop_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                    mac.emit_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                    mac.slot_fields.is_empty()
                }
                // `expose_fields` holds only the object-literal fields and
                // never the type-argument surface, so EVERY type-based
                // `defineExpose<LocalApi>(...)` — bare or with an object
                // literal alongside — lacks a direct local surface and must
                // discover its owner-local root. Presence gate on the type
                // argument (mirrors `raw_macro_surface_is_authoritative`),
                // not on the literal's absence.
                verter_semantic::analysis::AnalyzedMacroKind::DefineExpose => mac.is_type_based,
                verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => false,
            }
        }

        let mut candidate_roots = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        for resolved in &mac.resolved_local_types {
            let is_direct_local = mac
                .type_references
                .iter()
                .any(|type_name| type_name == &resolved.name);
            if is_direct_local && seen.insert(resolved.name.as_str()) {
                candidate_roots.push(resolved.name.as_str());
            }
        }

        if candidate_roots.is_empty() && macro_lacks_direct_local_surface(mac) {
            let owner_has_symbol = self.ctx.routed_shallow_state(owner_canonical);
            for type_name in &mac.type_references {
                if type_name.contains('.') || !seen.insert(type_name.as_str()) {
                    continue;
                }
                let owner_local_decl = owner_has_symbol
                    .as_ref()
                    .is_some_and(|state| state.symbol(type_name).is_some())
                    || self
                        .resolve_type_declaration(owner_canonical, type_name)
                        .canonical_source
                        == owner_canonical;
                if owner_local_decl {
                    candidate_roots.push(type_name.as_str());
                }
            }
        }

        if candidate_roots.is_empty() {
            return Vec::new();
        }

        // Projectability is decided through the SOLE query-time resolver: each
        // candidate root name lowers to a bare `TypeExpr::Ref` and resolves its
        // one-level `SurfaceView` in NODE DOMAIN (dispatch `lower_type_expr_in_scope`
        // at `Navigate` + empty-path `Published(Shallow)`), the SAME dispatch
        // surface route the owner-local authority gate
        // `owner_local_macro_root_has_surface` uses — both share the single
        // `owner_local_macro_root_surface_presence` helper, so the pre-filter and
        // the authority gate agree by construction on what "projectable" means.
        candidate_roots
            .into_iter()
            .filter(|root_name| {
                self.owner_local_macro_root_surface_presence(owner_canonical, root_name, mac.kind)
            })
            .map(str::to_string)
            .collect()
    }

    fn owner_local_macro_root_has_surface(
        &self,
        owner_canonical: &str,
        root_name: &str,
        macro_kind: verter_semantic::analysis::types::AnalyzedMacroKind,
    ) -> bool {
        // Authority gate for the cold resolver's owner-local arm: does the
        // owner-local type `root_name` resolve to a non-empty macro surface?
        // The actual props/emits/slots/exposed are NOT materialised here —
        // they are owned by the typeinfo macro-surface path (`vue_macro_dtos`);
        // this is purely a presence gate.
        //
        // Resolution routes through the SOLE query-time resolver via the shared
        // `owner_local_macro_root_surface_presence` helper (dispatch
        // `lower_type_expr_in_scope` at `Navigate` + empty-path
        // `Published(Shallow)` one-level `SurfaceView`), decided in node domain —
        // no `ExpandedObjectShape` materialisation, NOT the retired prepared-decl
        // walker.
        self.owner_local_macro_root_surface_presence(owner_canonical, root_name, macro_kind)
    }

    fn resolve_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<crate::resolver_core::ResolvedMacroElements> {
        let _ = visiting;
        // Route through the view-aware variant so the resolved-type cache
        // slot, dep-source reads, and the route-frontier closure observe
        // the active session overlay (when one is present). Base
        // (non-session) compute paths surface `view = None` and the
        // helper collapses to the historical behaviour.
        self.host.resolve_component_meta_macro_elements_with_view(
            self.ctx,
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
            self.ctx.active_session_view(),
        )
    }

    fn resolve_imported_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<crate::resolver_core::ResolvedImportedMacroSurface> {
        let _ = visiting;
        self.host.resolve_component_meta_macro_surface_with_view(
            self.ctx,
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
            self.ctx.active_session_view(),
        )
    }

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<ResolvedJsdocBlock> {
        resolve_jsdoc_block(
            self.host,
            self.ctx,
            canonical_source,
            span,
            if expanded {
                ProjectionMode::Expanded
            } else {
                ProjectionMode::Identity
            },
            tracked_deps,
            cache,
            visiting,
            verter_workspace::ResolveRequestKind::TypeImport,
        )
    }

    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        crate::resolver_core::ResolverContext::workspace_is_package_backed(self.host, canonical_id)
    }

    fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) {
        self.host
            .sync_transitive_macro_type_dependencies(canonical_id, tracked_deps);
    }

    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        self.host
            .current_dependency_fact_versions(canonical, tracked_deps)
    }
}

/// Absolutize a producer-local (empty-anchored) macro-payload locator against
/// the owning canonical: the analyzer's local-file convention stamps
/// `canonical_id: ""`; every payload deref requires the producing canonical.
fn absolutize_macro_payload_locator(
    locator: &verter_type_expr::locators::MacroPayloadLocator,
    owner_canonical: &str,
) -> verter_type_expr::locators::MacroPayloadLocator {
    if !locator.anchor.canonical_id.is_empty() {
        return locator.clone();
    }
    verter_type_expr::locators::MacroPayloadLocator {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: std::sync::Arc::from(owner_canonical),
            symbol: std::sync::Arc::clone(&locator.anchor.symbol),
            space: locator.anchor.space,
        },
        macro_index: locator.macro_index,
        payload: locator.payload,
    }
}

/// Node-domain "direct macro reference" walk: whether the raised macro
/// payload node carries a top-level reference to `needle`, reachable through
/// reference heads / arrays / tuples / unions / intersections / conditionals
/// / mapped / keyof / indexed-access / function signatures — never through
/// Object MEMBERS, which encode "nested" deps. Visited-guarded (graph nodes
/// may be shared or cyclic).
fn node_has_direct_macro_reference(
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
    needle: &str,
) -> bool {
    use crate::semantic_query::{IndexKey, SemanticNodeData, SemanticNodeId};

    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<SemanticNodeId> = vec![node];
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some((name, args)) =
            crate::resolver_core::component_meta_registry::component_meta_registry_node_ref_head(
                ctx, node,
            )
        {
            if name == needle {
                return true;
            }
            worklist.extend(args);
            continue;
        }
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::TypeOf(_) => {
                if let Some((value_root, path)) = data.typeof_head() {
                    if value_root.name.as_ref() == needle
                        || path.iter().any(|segment| segment.as_ref() == needle)
                    {
                        return true;
                    }
                }
                worklist.extend(data.carrier_type_args().iter().copied());
            }
            SemanticNodeData::Alias(target) => worklist.push(*target),
            SemanticNodeData::Array { element, .. } | SemanticNodeData::KeyOf { base: element } => {
                worklist.push(*element)
            }
            SemanticNodeData::Tuple { elements, .. } => {
                worklist.extend(elements.iter().map(|element| element.value));
            }
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                worklist.extend(arms.iter().copied());
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                worklist.extend(expressions.iter().copied());
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                worklist.push(*object);
                if let IndexKey::TypeNode(index_node) = index {
                    worklist.push(*index_node);
                }
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                worklist.push(*check);
                worklist.push(*extends);
                worklist.push(*true_branch_ref);
                worklist.push(*false_branch_ref);
            }
            SemanticNodeData::Mapped { source, .. } => worklist.push(*source),
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
                ..
            } => {
                worklist.extend(params.iter().map(|param| param.ty));
                worklist.push(*return_type);
                for param in type_parameters.iter() {
                    worklist.extend(param.constraint);
                    worklist.extend(param.default);
                }
            }
            SemanticNodeData::ConstructorType { signature } => worklist.push(*signature),
            // Object MEMBERS encode "nested" deps — never walked.
            _ => {}
        }
    }
    false
}

/// Test-only bare wrapper. Production callers go through
/// [`resolve_type_declaration_with_context`] with a request-bound
/// `HostResolverContext` / `SessionResolverContext` so the
/// `HostComponentMetaResolver`'s `ctx.resolve_named_type_export_target`
/// reads route through the overlay-aware view rather than the
/// panic-shimmed bare-host trait impl.
#[cfg(any(test, debug_assertions))]
#[allow(dead_code)]
pub(crate) fn resolve_type_declaration(
    host: &VerterHost,
    dep_canonical: &str,
    requested_name: &str,
) -> ResolvedTypeDeclaration {
    let base_ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = host;
    resolve_type_declaration_with_context(host, base_ctx, dep_canonical, requested_name)
}

/// Context-aware variant of [`resolve_type_declaration`]. Routes the
/// resolver-host construction through the supplied `ResolverContext`
/// so session-bearing entries observe overlay-aware reads when the
/// declaration walker dereferences cross-file types.
pub(crate) fn resolve_type_declaration_with_context(
    host: &VerterHost,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    dep_canonical: &str,
    requested_name: &str,
) -> ResolvedTypeDeclaration {
    let resolver = HostComponentMetaResolver { host, ctx };
    let key =
        crate::resolver_core::symbol_resolver::declaration_node_key(dep_canonical, requested_name);
    let mut resolve_ctx = crate::resolver_core::symbol_resolver::ResolveContext::new();
    let permissive_view = crate::resolver_core::PermissiveStoreView;
    let result = host.resolver_runtime().symbol.resolve_node(
        key,
        &permissive_view,
        &mut resolve_ctx,
        |_| {
            let declaration = crate::resolver_core::resolve_type_declaration(
                &resolver,
                dep_canonical,
                requested_name,
            );
            let mut tracked_deps = std::collections::BTreeSet::new();
            if !declaration.canonical_source.is_empty()
                && declaration.canonical_source != dep_canonical
            {
                tracked_deps.insert(declaration.canonical_source.clone());
            }

            crate::resolver_core::symbol_resolver::SymbolNodeResult {
                value: crate::resolver_core::symbol_resolver::SymbolNodeValue::Declaration(
                    declaration,
                ),
                facts: host.current_dependency_fact_versions(dep_canonical, &tracked_deps),
                diagnostics: Vec::new(),
            }
        },
    );

    match result.value {
        crate::resolver_core::symbol_resolver::SymbolNodeValue::Declaration(declaration) => {
            declaration
        }
        _ => unreachable!("declaration resolution must return a declaration node result"),
    }
}

pub(crate) fn read_full_source(host: &VerterHost, canonical_source: &str) -> Option<String> {
    host.read_analysis_source(canonical_source)
        .map(|source| source.to_string())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_jsdoc_block(
    host: &VerterHost,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    canonical_source: &str,
    span: verter_span::Span,
    mode: ProjectionMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_workspace::ResolveRequestKind,
) -> Option<ResolvedJsdocBlock> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    let source = read_full_source(host, canonical_source)?;
    let (description, tags) =
        verter_semantic::analysis::jsdoc::extract_jsdoc_near_offset(&source, span.start);
    if description.is_none() && tags.is_empty() {
        return None;
    }

    Some(ResolvedJsdocBlock {
        description,
        tags: tags
            .into_iter()
            .map(|tag| {
                map_jsdoc_tag(
                    host,
                    ctx,
                    canonical_source,
                    mode,
                    tracked_deps,
                    cache,
                    visiting,
                    kind,
                    tag,
                )
            })
            .collect(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn map_jsdoc_tag(
    host: &VerterHost,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    canonical_source: &str,
    mode: ProjectionMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    _visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    _kind: verter_workspace::ResolveRequestKind,
    tag: verter_semantic::analysis::types::JsdocTag,
) -> ResolvedJsdocTag {
    let (text, raw_type, subject_name) = parse_jsdoc_tag_payload(tag.name.as_str(), tag.text);
    let resolved_type = if mode == ProjectionMode::Expanded {
        raw_type.as_deref().and_then(|raw_type| {
            resolve_jsdoc_tag_type(host, ctx, canonical_source, raw_type, tracked_deps)
        })
    } else {
        None
    };
    ResolvedJsdocTag {
        name: tag.name,
        text,
        raw_type,
        subject_name,
        resolved_type,
    }
}

pub(crate) fn parse_jsdoc_tag_payload(
    tag_name: &str,
    text: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(text) = text else {
        return (None, None, None);
    };
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('{') else {
        return (Some(text), None, None);
    };
    // Depth-aware brace matching: find the closing `}` that matches the
    // opening `{`, handling nested braces like `{Record<string, {nested: true}>}`.
    let end = {
        let mut depth = 0u32;
        let mut found = None;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        found = Some(i);
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        found
    };
    let Some(end) = end else {
        return (Some(text), None, None);
    };

    let raw_type = Some(rest[..end].trim().to_string());
    let trailing = rest[end + 1..].trim();
    if trailing.is_empty() {
        return (None, raw_type, None);
    }

    if matches!(tag_name, "param" | "arg" | "argument") {
        let mut parts = trailing.splitn(2, char::is_whitespace);
        let subject_name = parts.next().map(str::to_string);
        let text = parts
            .next()
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
            .map(str::to_string);
        (text, raw_type, subject_name)
    } else {
        (Some(trailing.to_string()), raw_type, None)
    }
}

/// Sanitize a JSDoc display-fallback `raw` string so an unresolved user-authored
/// payload can never be mistaken for an internal materialisation sentinel.
///
/// When a JSDoc tag payload fails to parse/resolve, its verbatim text is stored
/// in `TypeExpr::Unknown { raw }` purely as a display fallback. The raw
/// classifier [`raw_is_unmaterialized_sentinel`] recognises a fixed family of
/// sentinel spellings (exact strings plus prefixes such as `budgetExceeded(`); a
/// user-authored payload that happens to spell one of those would otherwise be
/// misread as dispatch control flow. Only sentinel-looking payloads are wrapped
/// (with a non-sentinel `jsdoc:` marker that preserves the human-readable text);
/// every ordinary payload passes through byte-for-byte unchanged.
///
/// [`raw_is_unmaterialized_sentinel`]: crate::project_semantic_dispatch::raise_sentinel::raw_is_unmaterialized_sentinel
fn sanitize_jsdoc_unknown_raw(raw_type: &str) -> String {
    if crate::project_semantic_dispatch::raise_sentinel::raw_is_unmaterialized_sentinel(raw_type) {
        format!("jsdoc:{raw_type}")
    } else {
        raw_type.to_string()
    }
}

pub(crate) fn resolve_jsdoc_tag_type(
    host: &VerterHost,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    canonical_source: &str,
    raw_type: &str,
    tracked_deps: &mut std::collections::BTreeSet<String>,
) -> Option<verter_protocol::graph::snapshot::ResolvedJsdocTypeOutput> {
    // `raw_type` is the display string reconstructed by `parse_jsdoc_tag_payload`
    // (the JSDoc comment text line-joined and re-trimmed), NOT a contiguous slice
    // of the source file — there is no honest file position for its members.
    // Lower with `None` so the resulting type's spans are cleared (this `TypeExpr`
    // is consumed only to resolve the referenced type through the shared dispatch;
    // its member spans are never sliced against the file — `raw_type` carries the
    // display text separately on `ResolvedJsdocTag.raw_type`).
    let parsed = verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(raw_type, None);
    let parsed = if parsed.is_unknown() {
        verter_type_expr::TypeExpr::Unknown {
            raw: sanitize_jsdoc_unknown_raw(raw_type),
        }
    } else {
        parsed
    };

    // Ensure module facts are materialized so the dispatch path can
    // resolve imports through host-owned caches.
    let _facts = host.ensure_indexed_ready_serve(canonical_source)?;
    tracked_deps.extend(
        host.imported_symbol_dependencies_for_expr(ctx, canonical_source, &parsed)
            .into_iter()
            .map(|dependency| dependency.canonical_id),
    );
    // route directly through the shared
    // dispatch ProjectPath helper. Falls back to the raw parsed
    // annotation when projection misses so the caller still receives
    // the unresolved payload rather than `None`.
    //
    // Route the dispatch helper through the
    // request-bound `ctx` rather than `host: &VerterHost`. Passing
    // `host` here coerced into the bare-host
    // `<&VerterHost as ResolverContext>` impl, which panics under
    // `cfg(not(any(test, debug_assertions)))` (release) once
    // `project_expr_class_a_via_dispatch` reaches
    // `ctx.prepared_decl_bundle(...)` deeper in the call graph.
    let resolved =
        project_expr_class_a_via_dispatch(ctx, canonical_source, &parsed).unwrap_or(parsed);

    // OUTPUT-BOUNDARY materialisation: the resolved symbolic IR is TRANSIENT
    // producer-local state. Render its display string, capture its wire-node
    // graph snapshot through the shared `GraphBuilder` (the SAME builder every
    // proto graph rides — the proto conversion later re-interns this snapshot
    // wire-identically), and DISCARD the `TypeExpr`. No raw symbolic IR
    // survives past this point.
    let display = crate::resolver_core::surface_projector::render_type_expr_display(&resolved);
    let mut builder = verter_protocol::graph::GraphBuilder::new();
    let root_node_id = builder.node_id(&resolved);
    // Validated capture is fail-closed: `.ok()?` maps `SnapshotCaptureError`
    // to `None` (no resolved-type output) DELIBERATELY — never a partial
    // snapshot admitted. Both error arms are unreachable from this producer,
    // so no `Result` is threaded through a dead path: a resolved JSDoc
    // `{Type}` payload can never legitimately contain a `SyntheticSlotBinding`
    // carrier (`NonPersistableNode` unreachable), and a `GraphBuilder`-
    // captured snapshot is well-formed by construction (the malformed-table
    // arms guard hand-built tables, never builder captures). The error arm is
    // defense-in-depth only.
    let graph = verter_protocol::graph::snapshot::ResolvedTypeGraphSnapshot::from_builder(
        builder,
        root_node_id,
    )
    .ok()?;
    Some(verter_protocol::graph::snapshot::ResolvedJsdocTypeOutput { display, graph })
}

#[cfg(test)]
mod sanitizer_tests {
    use super::sanitize_jsdoc_unknown_raw;
    use crate::project_semantic_dispatch::raise_sentinel::raw_is_unmaterialized_sentinel;

    /// A JSDoc tag payload that happens to SPELL an internal materialisation
    /// sentinel must be wrapped (`jsdoc:`-prefixed) so the shared raw classifier
    /// `raw_is_unmaterialized_sentinel` can NEVER read user JSDoc text as dispatch
    /// control flow. Discriminating: it asserts the RAW payload classifies as a
    /// sentinel (so the sanitiser is non-vacuous) AND the sanitised payload does
    /// NOT.
    #[test]
    fn sanitizes_sentinel_spelling_jsdoc_payloads() {
        for sentinel in [
            "semanticMiss",
            "budgetExceeded(7)",
            "unsupportedIntrinsic(Foo)",
            "aliasCycle(Bar)",
            "materialize:x",
        ] {
            assert!(
                raw_is_unmaterialized_sentinel(sentinel),
                "precondition: `{sentinel}` must classify as a raw sentinel (so the test is \
                 non-vacuous)"
            );
            let sanitized = sanitize_jsdoc_unknown_raw(sentinel);
            assert_eq!(
                sanitized,
                format!("jsdoc:{sentinel}"),
                "a sentinel-spelling JSDoc payload must be `jsdoc:`-prefixed"
            );
            assert!(
                !raw_is_unmaterialized_sentinel(&sanitized),
                "the sanitised payload `{sanitized}` must NOT classify as a materialisation \
                 sentinel — user JSDoc text can never be read as dispatch control flow"
            );
        }
    }

    /// A benign (non-sentinel) JSDoc payload passes through BYTE-IDENTICAL — the
    /// sanitiser only rewrites sentinel-shaped strings, never ordinary text.
    #[test]
    fn benign_jsdoc_payloads_pass_through_byte_identical() {
        for benign in [
            "import('vue').PropType<string>",
            "Record<string, unknown>",
            "() => void",
            "budgetExceeded", // bare verb (no `(`) — NOT a sentinel
            "MyComponentProps",
            "{ a: number }",
        ] {
            assert!(
                !raw_is_unmaterialized_sentinel(benign),
                "precondition: `{benign}` must NOT be a sentinel"
            );
            assert_eq!(
                sanitize_jsdoc_unknown_raw(benign),
                benign,
                "a benign JSDoc payload must pass through byte-identical (no over-sanitisation)"
            );
        }
    }
}
