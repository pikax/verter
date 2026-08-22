use std::collections::BTreeSet;

use rustc_hash::FxHashSet;
use verter_semantic::analysis::component_meta::ResolvedTypeAnalysis;
use verter_semantic::analysis::types::{
    AnalyzedImport, AnalyzedMacro, AnalyzedMacroKind, MacroTypeDep,
};

use crate::resolver_core::{
    resolve_type_declaration, DeclarationMetadataResolver, FactVersionRef, ResolvedTypeDeclaration,
};

mod cold_resolver;
mod direct_macro;
mod native_props;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod native_props_rehome_contract_tests;

pub(crate) use native_props::named_native_props_outcome;
pub use native_props::{NativePropProjectionCache, ResolvedNativeProp, ResolvedNativePropsOutcome};

pub use cold_resolver::resolve_component_meta_parts;
pub(crate) use direct_macro::imported_registry_seed_can_skip_refresh;

/// Collect owner-qualified lexical demands for bindings exposed by macros.
/// The owner is the `defineExpose` use scope; admission resolves it to the
/// exact visible declaration owner before expansion.
///
/// Demands the field's REFERENCED LOCAL BINDING
/// (`AnalyzedExposeField::resolved_binding_name`), never the exposed
/// property key: `defineExpose({ public: local })` must demand `local` (the
/// value expression's identifier), which is the only one of the two that
/// can actually resolve to a visible declaration — `public` is not itself a
/// local binding. A field with NO referenced binding (a method, or any
/// other non-identifier value expression) demands nothing at all — never a
/// demand keyed on the property key, which could accidentally collide with
/// an unrelated same-named binding elsewhere in scope.
pub fn collect_requested_binding_demands(
    macros: &[AnalyzedMacro],
) -> BTreeSet<verter_type_expr::DeclBindingKey> {
    macros
        .iter()
        .flat_map(|mac| {
            mac.expose_fields.iter().filter_map(|field| {
                field.resolved_binding_name().map(|binding_name| {
                    verter_type_expr::DeclBindingKey::new(mac.owner, binding_name)
                })
            })
        })
        .collect()
}

/// Collect the local-value-declaration keys a runtime-constructor position
/// resolved to (`ConstructorBindingOutcome::Local`) across every macro's
/// `prop_fields` and, when present, the Options-API `props:` object.
///
/// Unlike [`collect_requested_binding_demands`], these keys are ALREADY
/// PROVEN local by the owner-aware `RootBindingIndex`
/// (`verter_semantic::analysis::root_binding_index`) — no second
/// name-visibility re-derivation applies to them (see
/// `docs/arch/refactor/rev11/evidence/CM1/binding-index-design.md`,
/// "Consumer wiring"). The caller feeds the result directly into
/// `expand_macro_types_impl_with_expander`'s `binding_entries` — the same
/// shared local-value-type-expansion primitive `defineExpose` bindings
/// resolve through — never through `component_meta_binding_type_entries`,
/// which re-derives Local-vs-Import visibility for a DIFFERENT field kind
/// and would be a second, potentially-diverging binding-resolution engine
/// for a question this index already answered authoritatively.
pub fn collect_local_constructor_binding_keys(
    macros: &[AnalyzedMacro],
    options_api: Option<&verter_semantic::analysis::AnalyzedOptionsApi>,
) -> BTreeSet<verter_type_expr::DeclBindingKey> {
    fn local_key(
        entry: &verter_type_expr::ConstructorBindingEntry,
    ) -> Option<verter_type_expr::DeclBindingKey> {
        match &entry.resolution {
            verter_type_expr::ConstructorBindingOutcome::Local(key) => Some(key.clone()),
            _ => None,
        }
    }

    let from_macros = macros.iter().flat_map(|mac| {
        mac.prop_fields
            .iter()
            .flat_map(|field| field.constructor_bindings.iter().filter_map(local_key))
    });
    let from_options = options_api.into_iter().flat_map(|opts| {
        opts.props
            .iter()
            .flat_map(|prop| prop.constructor_bindings.iter().filter_map(local_key))
    });
    from_macros.chain(from_options).collect()
}

#[cfg(test)]
mod collect_local_constructor_binding_keys_tests {
    use super::collect_local_constructor_binding_keys;
    use std::collections::BTreeSet;
    use verter_semantic::analysis::types::{
        AnalyzedMacro, AnalyzedMacroKind, AnalyzedOptionsApi, AnalyzedOptionsProp,
        AnalyzedPropField, TypeResolutionSource,
    };
    use verter_type_expr::{ConstructorBindingEntry, ConstructorBindingOutcome, DeclBindingKey};

    fn constructor_prop_field(
        name: &str,
        constructor_bindings: Vec<ConstructorBindingEntry>,
    ) -> AnalyzedPropField {
        AnalyzedPropField {
            name: name.to_string(),
            is_optional: true,
            span: verter_span::Span::new(0, 0),
            type_annotation: None,
            payload: None,
            type_expr_scope: None,
            description: None,
            tags: Vec::new(),
            resolution_source: TypeResolutionSource::Rust,
            resolution_error: None,
            declared_in_macro_type_arg: true,
            constructor_bindings,
        }
    }

    fn define_props_macro(prop_fields: Vec<AnalyzedPropField>) -> AnalyzedMacro {
        AnalyzedMacro {
            edit_anchors: Default::default(),
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: false,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields,
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: verter_span::Span::new(0, 0),
        }
    }

    /// The "bridge" this test exercises: `RootBindingIndex`-proven `Local`
    /// constructor keys collected from a macro's `prop_fields`, threaded
    /// (by the caller, `compute_evaluated_types_from_owner_context_with_ctx`)
    /// directly into `expand_macro_types_impl_with_expander`'s
    /// `BindingExpansionEntry` demand list. The full session/host-backed
    /// integration is out of scope for a pure-function unit test; this
    /// pins the collection half of that bridge.
    #[test]
    fn collects_local_keys_from_macro_prop_fields_only() {
        let owner = verter_type_expr::TopLevelOwnerId::module(0);
        let key = DeclBindingKey::new(owner, "String");
        let macros = vec![define_props_macro(vec![constructor_prop_field(
            "label",
            vec![ConstructorBindingEntry {
                spelling: std::sync::Arc::from("String"),
                resolution: ConstructorBindingOutcome::Local(key.clone()),
            }],
        )])];
        let collected = collect_local_constructor_binding_keys(&macros, None);
        assert_eq!(collected, BTreeSet::from([key]));
    }

    #[test]
    fn global_and_indeterminate_entries_are_excluded() {
        let macros = vec![define_props_macro(vec![constructor_prop_field(
            "label",
            vec![
                ConstructorBindingEntry {
                    spelling: std::sync::Arc::from("String"),
                    resolution: ConstructorBindingOutcome::Global,
                },
                ConstructorBindingEntry {
                    spelling: std::sync::Arc::from("Number"),
                    resolution: ConstructorBindingOutcome::Indeterminate,
                },
            ],
        )])];
        assert!(collect_local_constructor_binding_keys(&macros, None).is_empty());
    }

    #[test]
    fn collects_local_keys_from_options_api_props_too() {
        let owner = verter_type_expr::TopLevelOwnerId::module(0);
        let key = DeclBindingKey::new(owner, "Array");
        let options_api = AnalyzedOptionsApi {
            props: vec![AnalyzedOptionsProp {
                name: "items".to_string(),
                span: verter_span::Span::new(0, 0),
                type_constructor: None,
                is_required: false,
                has_default: false,
                default_value: None,
                type_annotation: None,
                payload: None,
                type_expr_scope: None,
                description: None,
                tags: Vec::new(),
                constructor_bindings: vec![ConstructorBindingEntry {
                    spelling: std::sync::Arc::from("Array"),
                    resolution: ConstructorBindingOutcome::Local(key.clone()),
                }],
            }],
            ..Default::default()
        };
        let collected = collect_local_constructor_binding_keys(&[], Some(&options_api));
        assert_eq!(collected, BTreeSet::from([key]));
    }

    #[test]
    fn no_macros_no_options_api_is_empty() {
        assert!(collect_local_constructor_binding_keys(&[], None).is_empty());
    }
}

#[derive(Debug, Clone, Default)]
pub struct ComponentMetaEvalOutputs {
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    pub tracked_dependencies: BTreeSet<String>,
    /// Step 9.1 / D32: surface-id sidecar captured during the
    /// `expand_macro_types_impl_with_expander` closure run. None when
    /// audit is off; populated in lock-step with `evaluated_types`'s
    /// per-FieldKind output vectors when audit is on. Consumed by
    /// `compute_component_meta_state_inner` and stored on
    /// `ResolvedComponentMetaState.surface_identities`.
    pub surface_identities: Option<crate::meta_resolve::SurfaceNodeIdentities>,
}

#[derive(Debug, Clone)]
pub struct ResolvedTypeRegistryMeta {
    pub name: String,
    pub declaration: ResolvedTypeDeclaration,
}

/// Resolved per-macro metadata: declaration identity, authority/provenance
/// gating, and the native-only `native_props` carrier.
///
/// The published props/emits/slots/exposed surface is NOT carried here. Those
/// derive solely from the typeinfo macro-surface path
/// ([`crate::VerterHost::vue_macro_dtos`]); [`component_meta_resolved_macros`]
/// sources `ResolvedMacroInput.{props,emits,slots,exposed}` from that path
/// keyed on the admitted macro index. `ResolvedMacroMeta` exists to (1) gate
/// which macro indices contribute (the `surface_is_authoritative` /
/// `type_references` filter consumed by the materialiser and the
/// `component_meta_resolved_macros` adapter), (2) carry declaration identity +
/// JSDoc for the registry seed, and (3) carry `native_props` (the
/// private/protected class-member visibility surface with a real FFI/proto/JS
/// consumer that the typeinfo surface does not cover).
#[derive(Debug, Clone)]
pub struct ResolvedMacroMeta {
    pub macro_index: usize,
    pub macro_kind: AnalyzedMacroKind,
    pub type_name: String,
    pub import_source: String,
    pub surface_is_authoritative: bool,
    pub declaration: ResolvedTypeDeclaration,
    pub native_props: Vec<ResolvedNativeProp>,
    pub jsdoc: Option<ResolvedJsdocBlock>,
}

/// The combined imported-macro resolution: declaration identity plus the
/// component-meta-owned native visibility rows.
#[derive(Debug, Clone)]
pub struct ResolvedImportedMacroSurface {
    pub declaration: ResolvedTypeDeclaration,
    pub native_props: Vec<ResolvedNativeProp>,
}

#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct ResolvedJsdocBlock {
    pub description: Option<String>,
    pub tags: Vec<ResolvedJsdocTag>,
}

#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct ResolvedJsdocTag {
    pub name: String,
    pub text: Option<String>,
    pub raw_type: Option<String>,
    pub subject_name: Option<String>,
    /// The SEALED resolved-type output snapshot (display + captured wire-node
    /// graph), materialised at the producer boundary
    /// (`host_manage::jsdoc_resolve`) — never a raw `TypeExpr`. Persisted on
    /// the `ComponentMetaResultDb` value and re-interned into the proto graph
    /// at conversion time.
    pub resolved_type: Option<verter_protocol::graph::snapshot::ResolvedJsdocTypeOutput>,
}

#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaParts {
    pub resolved_macros: Vec<ResolvedMacroMeta>,
    pub resolved_type_registry: Vec<ResolvedTypeAnalysis>,
    pub resolved_type_registry_meta: Vec<ResolvedTypeRegistryMeta>,
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    pub tracked_dependencies: BTreeSet<String>,
    pub fact_versions: Vec<FactVersionRef>,
    /// Step 9.1 / D32: surface-id sidecar. Populated when audit is on
    /// (vector-aligned with `evaluated_types`'s per-FieldKind output
    /// vectors); `None` when audit is off. Threaded down from
    /// `ComponentMetaEvalOutputs.surface_identities` so
    /// `compute_component_meta_state_inner` can store it on
    /// `ResolvedComponentMetaState.surface_identities` (Step 9.2's
    /// scoped origin export consumer).
    pub surface_identities: Option<crate::meta_resolve::SurfaceNodeIdentities>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentMetaResolutionPurpose {
    Full,
    Fallthrough,
}

/// Build the [`verter_semantic::analysis::component_meta::ResolvedMacroInput`]
/// set the `verter_semantic` component-meta extractor consumes, sourcing the
/// props/emits/slots/exposed from the SOLE typeinfo macro-surface authority
/// (`vue_macro_dtos`, reached through the resolver-context seam).
///
/// The snapshot macro inventory is the sole index/order/cardinality authority.
/// An object-literal-only `defineExpose` with fields stays analyzer-owned and is
/// excluded by `raw_macro_surface_is_authoritative`; every other snapshot macro
/// contributes exactly one input in source order. Field DATA comes from
/// `vue_macro_dtos`, keyed on `(owner, macro_index, kind)` — the same key the
/// materialiser's `synthesize_*_from_known_surface` path already uses.
///
/// `ResolvedMacroMeta` is intentionally absent from this API. Those rows carry
/// optional declaration identity/JSDoc enrichment for registry publication;
/// exact lexical ownership can legitimately produce no row for a module-script
/// declaration consumed by a setup-script macro. Optional metadata must never
/// gate the normalized DTO field authority.
///
/// Host state is reached through `&dyn ResolverContext` (the resolver-tier
/// seal): `vue_macro_dtos_with_ctx(ctx, …)` resolves the macro surface through
/// the ACTIVE context, so an overlay session reads its overlay content (an
/// overlay-added prop surfaces here; it never leaks into a base-view read,
/// which keys a distinct `whole_hash`). The DTO core validates its own cached
/// entry against `ctx.store_view()` and bubbles the entry's fact signature into
/// any active outer fact tracer (so an outer component-meta cold trace inherits
/// the DTO's cross-file carrier facts on a warm DTO hit), keeping the outer
/// component-meta cache entry correctly keyed — all inside the single
/// resolution engine.
pub(crate) fn component_meta_resolved_macros(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    snapshot_macros: &[AnalyzedMacro],
) -> Vec<verter_semantic::analysis::component_meta::ResolvedMacroInput> {
    let mut inputs = Vec::new();
    for (macro_index, mac) in snapshot_macros.iter().enumerate() {
        if raw_macro_surface_is_authoritative(mac) {
            continue;
        }
        let dtos_read = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
            ctx,
            &crate::typeinfo::types::VueMacroSurfaceRequest {
                owner_canonical: std::sync::Arc::from(owner_canonical),
                macro_index,
                macro_kind: mac.kind,
                root_identity: ctx.get_whole_hash(owner_canonical).unwrap_or([0u8; 16]),
                level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
            },
        );
        // Fold a genuine partial macro surface into the request-result
        // completeness so the enclosing component-meta result is refused warm
        // promotion (the no-poison invariant).
        dtos_read.observe_partial();
        let dtos = dtos_read.dtos;
        // The lower-crate input rows carry the session-resolved SOURCE
        // POSITIONS alongside the analysis fields — the normalized-surface
        // authority the extraction publishes (the flat evaluated lanes
        // contribute metadata only).
        inputs.push(
            verter_semantic::analysis::component_meta::ResolvedMacroInput {
                macro_index,
                props: dtos
                    .prop_fields()
                    .iter()
                    .map(
                        |row| verter_semantic::analysis::component_meta::ResolvedPropInput {
                            field: row.analysis.clone(),
                            authority: row.authority.clone(),
                            authored_evidence: row.authored_evidence.clone(),
                            callable_role: row.callable_role.clone(),
                        },
                    )
                    .collect(),
                emits: dtos
                    .emit_fields()
                    .iter()
                    .map(
                        |row| verter_semantic::analysis::component_meta::ResolvedEmitInput {
                            id: row.id.clone(),
                            name: row.name.clone(),
                            span: row.span,
                            payload_type: row.payload_type.clone(),
                            payload: row.payload.clone(),
                            payload_expr_scope: row.payload_expr_scope.clone(),
                            description: row.description.clone(),
                            tags: row.tags.clone(),
                            payload_source: row.payload_source.clone(),
                            payload_publication: row.payload_publication.clone(),
                            return_publication: row.return_publication.clone(),
                            return_publication_scope: row.return_publication_scope.clone(),
                        },
                    )
                    .collect(),
                slots: dtos.slot_fields().to_vec(),
                slot_return_publications: dtos.slot_return_publications().to_vec(),
                exposed: dtos
                    .expose_fields()
                    .iter()
                    .map(
                        |row| verter_semantic::analysis::component_meta::ResolvedExposeInput {
                            field: row.analysis.clone(),
                            type_source: row.type_source.clone(),
                        },
                    )
                    .collect(),
                default_keys: Vec::new(),
            },
        );
    }

    let host = ctx.host_for_fact_tracer_install();
    let is_svelte = host
        .scheduler
        .try_get_source(owner_canonical)
        .and_then(|snapshot| {
            snapshot
                .downcast_data::<crate::host_executor::HostSourceData>()
                .map(|data| data.file_language.clone())
        })
        .and_then(|language| language.adapter_id().cloned())
        .is_some_and(|adapter| adapter.is_svelte());
    if is_svelte {
        use crate::typeinfo::framework_surface::SvelteSurfaceSource;

        let native_index = snapshot_macros.len();
        let mut native = verter_semantic::analysis::component_meta::ResolvedMacroInput {
            macro_index: native_index,
            props: Vec::new(),
            emits: Vec::new(),
            slots: Vec::new(),
            slot_return_publications: Vec::new(),
            exposed: Vec::new(),
            default_keys: Vec::new(),
        };
        for source in [
            SvelteSurfaceSource::RunesProps,
            SvelteSurfaceSource::LegacyExportLet,
            SvelteSurfaceSource::CallbackPropEvents,
            SvelteSurfaceSource::LegacyDispatcher,
            SvelteSurfaceSource::SnippetProps,
            SvelteSurfaceSource::LegacySlotInventory,
        ] {
            let outcome = crate::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface(
                host,
                ctx,
                owner_canonical,
                source,
            );
            if matches!(
                outcome,
                crate::typeinfo::framework_surface::ResolvedOutcome::Partial { .. }
            ) {
                crate::request_context::mark_request_result_partial();
            }
            let Some(dtos) = outcome.value() else {
                continue;
            };
            native.props.extend(dtos.prop_fields().iter().map(|row| {
                verter_semantic::analysis::component_meta::ResolvedPropInput {
                    field: row.analysis.clone(),
                    authority: row.authority.clone(),
                    authored_evidence: row.authored_evidence.clone(),
                    callable_role: row.callable_role.clone(),
                }
            }));
            native.default_keys.extend(
                dtos.prop_defaults()
                    .iter()
                    .map(|default| default.key.clone()),
            );
            native.emits.extend(dtos.emit_fields().iter().map(|row| {
                verter_semantic::analysis::component_meta::ResolvedEmitInput {
                    id: row.id.clone(),
                    name: row.name.clone(),
                    span: row.span,
                    payload_type: row.payload_type.clone(),
                    payload: row.payload.clone(),
                    payload_expr_scope: row.payload_expr_scope.clone(),
                    description: row.description.clone(),
                    tags: row.tags.clone(),
                    payload_source: row.payload_source.clone(),
                    payload_publication: row.payload_publication.clone(),
                    return_publication: row.return_publication.clone(),
                    return_publication_scope: row.return_publication_scope.clone(),
                }
            }));
            native.slots.extend_from_slice(dtos.slot_fields());
            native
                .slot_return_publications
                .extend_from_slice(dtos.slot_return_publications());
        }
        inputs.push(native);
    }
    inputs
}

pub fn component_meta_type_registry(
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
) -> Vec<verter_semantic::analysis::component_meta::ResolvedTypeAnalysis> {
    let mut seen = FxHashSet::default();
    let mut registry = Vec::new();

    for entry in resolved_type_registry {
        if seen.insert(entry.name.clone()) {
            registry.push(entry.clone());
        }
    }

    registry
}

pub trait ComponentMetaResolverHost: DeclarationMetadataResolver {
    type Snapshot;
    type EvalContext;

    fn resolve_type_declaration(
        &self,
        dep_canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration
    where
        Self: Sized,
    {
        resolve_type_declaration(self, dep_canonical, owner, requested_name)
    }

    fn snapshot_imports<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedImport];
    fn snapshot_macros<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [AnalyzedMacro];
    fn snapshot_macro_type_deps<'a>(&self, snapshot: &'a Self::Snapshot) -> &'a [MacroTypeDep];

    fn build_eval_outputs(
        &self,
        owner_canonical: &str,
        snapshot: &Self::Snapshot,
        eval_context: Option<&Self::EvalContext>,
        purpose: ComponentMetaResolutionPurpose,
    ) -> ComponentMetaEvalOutputs;

    fn projectable_owner_local_macro_roots(
        &self,
        _owner_canonical: &str,
        _mac: &AnalyzedMacro,
    ) -> Vec<String> {
        Vec::new()
    }

    /// Whether the macro's parsed type-argument payload carries a top-level
    /// (non-object-member) reference to `type_name` — the "direct" macro-dep
    /// classification, decided NODE-DOMAIN off the authored payload locator
    /// by graph-backed hosts. `None` = no parsed payload or no live graph
    /// representation; the caller falls back to the macro's
    /// `type_references` membership fact. The default is `None` because a
    /// host without a semantic graph cannot classify payload shape.
    fn macro_type_arg_has_direct_reference(
        &self,
        _owner_canonical: &str,
        _mac: &AnalyzedMacro,
        _type_name: &str,
    ) -> Option<bool> {
        None
    }

    fn has_projectable_owner_local_macro_surface(
        &self,
        owner_canonical: &str,
        mac: &AnalyzedMacro,
    ) -> bool {
        !self
            .projectable_owner_local_macro_roots(owner_canonical, mac)
            .is_empty()
    }

    /// Whether the owner-local root `(owner, root_name)` projects to a
    /// NON-EMPTY prepared macro surface for `macro_kind`.
    ///
    /// This is the authority gate for the cold resolver's owner-local arm: it
    /// pushes an authoritative [`ResolvedMacroMeta`] entry for the root iff
    /// this returns `true`. The published props/emits/slots/exposed surface
    /// itself is NOT projected here — it is owned by the typeinfo macro-surface
    /// path (`vue_macro_dtos`); this method only signals "does this owner-local
    /// root have a real surface to gate on", folding the prior emptiness check
    /// that inspected the projected props/emits/slots/exposed.
    fn owner_local_macro_root_has_surface(
        &self,
        _owner_canonical: &str,
        _owner: verter_type_expr::TopLevelOwnerId,
        _root_name: &str,
        _macro_kind: AnalyzedMacroKind,
    ) -> bool {
        false
    }

    fn resolve_native_props(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut NativePropProjectionCache,
    ) -> Option<Vec<ResolvedNativeProp>>;

    fn resolve_imported_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut NativePropProjectionCache,
    ) -> Option<ResolvedImportedMacroSurface>
    where
        Self: Sized,
    {
        let dep_canonical =
            self.resolve_type_dependency_canonical(owner_canonical, import_source)?;
        let declaration = self.resolve_type_declaration(
            dep_canonical.as_str(),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            exported_name,
        );
        let native_props = self.resolve_native_props(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
        )?;
        Some(ResolvedImportedMacroSurface {
            declaration,
            native_props,
        })
    }

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut BTreeSet<String>,
    ) -> Option<ResolvedJsdocBlock>;

    /// Whether `canonical_id` is package-backed per the workspace's
    /// resolver classification (NOT a path-substring check on
    /// `node_modules`). Mirrors `ResolverContext::workspace_is_package_backed`
    /// at the component-meta-host trait layer so cold-resolver consumers
    /// can use the structural classifier without coupling to the
    /// `VerterHost`/`ResolverContext` surface.
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool;

    fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        tracked_deps: &BTreeSet<String>,
    );

    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef>;
}

fn raw_macro_surface_is_authoritative(mac: &AnalyzedMacro) -> bool {
    match mac.kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => false,
        AnalyzedMacroKind::DefineEmits => false,
        AnalyzedMacroKind::DefineSlots => false,
        // A type-based `defineExpose<T>({ ... })` carries doc supply on the
        // type-argument surface (span-sliced JSDoc), so the raw object-literal
        // field list alone is not authoritative; without a type argument the
        // analyzer's field list is the whole truth.
        AnalyzedMacroKind::DefineExpose => !mac.is_type_based && !mac.expose_fields.is_empty(),
        AnalyzedMacroKind::DefineOptions => false,
    }
}

fn skip_macro_declaration_metadata_for_purpose(purpose: ComponentMetaResolutionPurpose) -> bool {
    purpose == ComponentMetaResolutionPurpose::Fallthrough
}

fn placeholder_type_declaration(
    requested_name: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    resolved_name: &str,
) -> ResolvedTypeDeclaration {
    ResolvedTypeDeclaration {
        requested_name: requested_name.to_string(),
        declaration_id: None,
        resolved_name: resolved_name.to_string(),
        canonical_source: String::new(),
        owner,
        span: verter_span::Span::default(),
        kind: crate::resolver_core::ResolvedDeclarationKind::Unknown,
        text: None,
    }
}

fn macro_kind_needed_for_fallthrough(kind: AnalyzedMacroKind) -> bool {
    matches!(
        kind,
        AnalyzedMacroKind::DefineProps
            | AnalyzedMacroKind::WithDefaults
            | AnalyzedMacroKind::DefineModel
            | AnalyzedMacroKind::DefineEmits
    )
}
