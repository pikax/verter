use std::collections::BTreeSet;

use rustc_hash::FxHashSet;
use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;
use verter_semantic::analysis::component_meta::ResolvedTypeAnalysis;
use verter_semantic::analysis::types::{
    AnalyzedImport, AnalyzedMacro, AnalyzedMacroKind, MacroTypeDep,
};

use crate::resolver_core::{
    resolve_type_declaration, surface_projector::ProjectedMacroSurfaces,
    DeclarationMetadataResolver, FactVersionRef, ResolvedNativeProp, ResolvedTypeDeclaration,
};

mod cold_resolver;
mod direct_macro;
mod projected_type_expr;

#[cfg(test)]
mod tests;

pub use cold_resolver::resolve_component_meta_parts;
pub use projected_type_expr::resolved_elements_to_type_expr_via_type_text;
// Re-export `projected_macro_surfaces_to_type_expr` so the original
// `crate::resolver_core::component_meta::projected_macro_surfaces_to_type_expr`
// path keeps resolving (legacy public path; no in-tree consumers, but
// preserved for downstream callers per Tier 2 W5d "public API
// unchanged" acceptance gate).
pub(crate) use direct_macro::{
    imported_declaration_surface_is_authoritative, imported_registry_seed_can_skip_refresh,
};
pub(crate) use projected_type_expr::project_macro_surfaces_from_expanded_shape;
#[allow(unused_imports)]
pub use projected_type_expr::projected_macro_surfaces_to_type_expr;

/// Collect the set of binding names exposed by macros (e.g., `defineExpose` fields).
/// Used as a filter for which `env.value_symbols` entries to expand as bindings
/// during `expand_macro_types`.
pub fn collect_requested_binding_names(macros: &[AnalyzedMacro]) -> FxHashSet<String> {
    macros
        .iter()
        .flat_map(|mac| mac.expose_fields.iter().map(|field| field.name.clone()))
        .collect()
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

#[derive(Debug, Clone)]
pub struct ResolvedMacroMeta {
    pub macro_index: usize,
    pub macro_kind: AnalyzedMacroKind,
    pub type_name: String,
    pub import_source: String,
    pub surface_is_authoritative: bool,
    pub declaration: ResolvedTypeDeclaration,
    pub native_props: Vec<ResolvedNativeProp>,
    pub props: Vec<verter_semantic::analysis::AnalyzedPropField>,
    pub emits: Vec<verter_semantic::analysis::AnalyzedEmitField>,
    pub slots: Vec<verter_semantic::analysis::AnalyzedSlotField>,
    pub jsdoc: Option<ResolvedJsdocBlock>,
}

#[derive(Debug, Clone)]
pub struct ResolvedImportedMacroSurface {
    pub declaration: ResolvedTypeDeclaration,
    pub elements: ResolvedElements,
}

#[derive(Debug, Clone)]
pub struct ResolvedJsdocBlock {
    pub description: Option<String>,
    pub tags: Vec<ResolvedJsdocTag>,
}

#[derive(Debug, Clone)]
pub struct ResolvedJsdocTag {
    pub name: String,
    pub text: Option<String>,
    pub raw_type: Option<String>,
    pub subject_name: Option<String>,
    pub resolved_type: Option<verter_semantic::analysis::type_expr::TypeExpr>,
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

pub fn component_meta_resolved_macros(
    snapshot_macros: &[AnalyzedMacro],
    resolved_macros: &[ResolvedMacroMeta],
) -> Vec<verter_semantic::analysis::component_meta::ResolvedMacroInput> {
    resolved_macros
        .iter()
        .filter(|resolved| {
            snapshot_macros
                .get(resolved.macro_index)
                .is_none_or(|mac| !raw_macro_surface_is_authoritative(mac))
        })
        .map(
            |resolved| verter_semantic::analysis::component_meta::ResolvedMacroInput {
                macro_index: resolved.macro_index,
                props: resolved.props.clone(),
                emits: resolved.emits.clone(),
                slots: resolved.slots.clone(),
            },
        )
        .collect()
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
        requested_name: &str,
    ) -> ResolvedTypeDeclaration
    where
        Self: Sized,
    {
        resolve_type_declaration(self, dep_canonical, requested_name)
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

    fn has_projectable_owner_local_macro_surface(
        &self,
        owner_canonical: &str,
        mac: &AnalyzedMacro,
    ) -> bool {
        !self
            .projectable_owner_local_macro_roots(owner_canonical, mac)
            .is_empty()
    }

    fn resolve_owner_local_macro_surface(
        &self,
        _owner_canonical: &str,
        _root_name: &str,
        _macro_kind: AnalyzedMacroKind,
    ) -> Option<ProjectedMacroSurfaces> {
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedElements>;

    #[allow(clippy::too_many_arguments)]
    fn resolve_imported_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedImportedMacroSurface>
    where
        Self: Sized,
    {
        let dep_canonical =
            self.resolve_type_dependency_canonical(owner_canonical, import_source)?;
        let declaration = self.resolve_type_declaration(dep_canonical.as_str(), exported_name);
        let elements = self.resolve_macro_elements(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
        )?;
        Some(ResolvedImportedMacroSurface {
            declaration,
            elements,
        })
    }

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
    ) -> Option<ResolvedJsdocBlock>;

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
        AnalyzedMacroKind::DefineExpose => !mac.expose_fields.is_empty(),
        AnalyzedMacroKind::DefineOptions => false,
    }
}

fn skip_macro_declaration_metadata_for_purpose(purpose: ComponentMetaResolutionPurpose) -> bool {
    purpose == ComponentMetaResolutionPurpose::Fallthrough
}

fn placeholder_type_declaration(
    requested_name: &str,
    resolved_name: &str,
) -> ResolvedTypeDeclaration {
    ResolvedTypeDeclaration {
        requested_name: requested_name.to_string(),
        declaration_id: None,
        resolved_name: resolved_name.to_string(),
        canonical_source: String::new(),
        span: verter_span::Span::default(),
        kind: crate::resolver_core::ResolvedDeclarationKind::Unknown,
        text: None,
    }
}

/// Reconstruct the original `JsdocTag.text` payload from the
/// post-parse `(text, raw_type, subject_name)` triple stored on
/// `ResolvedJsdocTag`.
///
/// `parse_jsdoc_tag_payload` (in `meta_resolve.rs`) splits a tag's raw
/// trailing text into three pieces: the `{Type}` block (`raw_type`),
/// an optional subject name (only for `@param`/`@arg`/`@argument`),
/// and the remaining trailing description text (`text`). The simple
/// `JsdocTag.text` form keeps everything as one string. To round-trip
/// through `host.resolve_jsdoc_block` we reassemble the three pieces
/// in the order the parser took them apart.
fn jsdoc_tag_text_from_resolved(tag: &ResolvedJsdocTag) -> Option<String> {
    if let Some(raw_type) = &tag.raw_type {
        let mut out = String::new();
        out.push('{');
        out.push_str(raw_type);
        out.push('}');
        if let Some(subject) = &tag.subject_name {
            out.push(' ');
            out.push_str(subject);
        }
        if let Some(text) = &tag.text {
            out.push(' ');
            out.push_str(text);
        }
        Some(out)
    } else {
        tag.text.clone()
    }
}

fn jsdoc_tags_from_resolved(
    resolved: &[ResolvedJsdocTag],
) -> Vec<verter_semantic::analysis::types::JsdocTag> {
    resolved
        .iter()
        .map(|tag| verter_semantic::analysis::types::JsdocTag {
            name: tag.name.clone(),
            text: jsdoc_tag_text_from_resolved(tag),
        })
        .collect()
}

/// graph-native per-member JSDoc enrichment.
///
/// Walks the resolved cross-file `elements` parallel to the way
/// `project_macro_surfaces` projected them, and asks
/// `host.resolve_jsdoc_block` (an existing host API) for the JSDoc
/// block sitting near each source-element span. Found descriptions
/// and tags overwrite the empty `description = None` / `tags = vec![]`
/// values that the graph-native projection (which
/// receives `source = None`) wrote.
///
/// the resolver passed the imported declaration's raw
/// source text into `project_macro_surfaces`, which then called
/// `member_jsdoc(source, prop.span)` per element. deleted
/// the host source-text reader from this resolver and the fallback
/// paths it fed; the source-text inputs are gone. This helper reuses the
/// shared `host.resolve_jsdoc_block` helper (the same path the
/// declaration-level JSDoc already uses), which reads from the
/// host-owned analysis source cache rather than re-parsing raw
/// text — preserving the cache-owned recovery rule (CLAUDE.md
/// "Component-Meta Native Vs Compat" + Macro Type Traversal Rule).
#[allow(clippy::too_many_arguments)]
fn enrich_projected_jsdoc<H>(
    host: &H,
    declaration: &ResolvedTypeDeclaration,
    elements: &ResolvedElements,
    macro_kind: AnalyzedMacroKind,
    projected: &mut ProjectedMacroSurfaces,
    expanded: bool,
    tracked_deps: &mut BTreeSet<String>,
    cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    visiting: &mut FxHashSet<(String, String)>,
) where
    H: ComponentMetaResolverHost,
{
    if declaration.canonical_source.is_empty() {
        return;
    }

    let mut apply_jsdoc =
        |span: verter_span::Span,
         description_slot: &mut Option<String>,
         tags_slot: &mut Vec<verter_semantic::analysis::types::JsdocTag>| {
            if span.start == 0 && span.end == 0 {
                return;
            }
            if let Some(block) = host.resolve_jsdoc_block(
                declaration.canonical_source.as_str(),
                span,
                expanded,
                tracked_deps,
                cache,
                visiting,
            ) {
                if block.description.is_some() {
                    *description_slot = block.description;
                }
                if !block.tags.is_empty() {
                    *tags_slot = jsdoc_tags_from_resolved(&block.tags);
                }
            }
        };

    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => {
            // `project_macro_surfaces` collects public props in the
            // same iteration order; walk the same filter to align
            // source spans 1:1 with `projected.props[i]`.
            let public_props: Vec<&_> = elements
                .props
                .iter()
                .filter(|prop| prop.visibility.is_public())
                .collect();
            for (proj, src) in projected.props.iter_mut().zip(public_props.iter()) {
                apply_jsdoc(src.span, &mut proj.description, &mut proj.tags);
            }
        }
        AnalyzedMacroKind::DefineEmits => {
            if !elements.emits.is_empty() {
                for (proj, src) in projected.emits.iter_mut().zip(elements.emits.iter()) {
                    apply_jsdoc(src.span, &mut proj.description, &mut proj.tags);
                }
            } else {
                // Property-style emits: `project_macro_surfaces`
                // walks `elements.props` filtering public + dedup'd
                // by name. Reproduce the same filter to align spans.
                let mut seen = FxHashSet::default();
                let mut prop_iter = elements
                    .props
                    .iter()
                    .filter(|prop| prop.visibility.is_public())
                    .filter_map(|prop| {
                        let name = prop.key_name.clone()?;
                        if !seen.insert(name) {
                            return None;
                        }
                        Some(prop)
                    });
                for proj in projected.emits.iter_mut() {
                    if let Some(src) = prop_iter.next() {
                        apply_jsdoc(src.span, &mut proj.description, &mut proj.tags);
                    } else {
                        break;
                    }
                }
            }
        }
        AnalyzedMacroKind::DefineSlots => {
            // `project_macro_surfaces` walks public props and keeps
            // those whose type either yields slot bindings, a return
            // type, or resolves as a function. Match by name to align
            // (slots' projected order is the public-prop order with
            // non-slot entries dropped).
            let mut by_name: std::collections::HashMap<&str, &_> = elements
                .props
                .iter()
                .filter(|prop| prop.visibility.is_public())
                .filter_map(|prop| prop.key_name.as_deref().map(|name| (name, prop)))
                .collect();
            for proj in projected.slots.iter_mut() {
                if let Some(src) = by_name.remove(proj.name.as_str()) {
                    apply_jsdoc(src.span, &mut proj.description, &mut proj.tags);
                }
            }
        }
        AnalyzedMacroKind::DefineExpose | AnalyzedMacroKind::DefineOptions => {}
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
