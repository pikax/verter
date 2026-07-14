//! `host_manage::intrinsic_projection` — project-scoped HTML intrinsic
//! attribute / element projection pipeline.
//!
//! Domain E. Projects `JSX.IntrinsicElements` and `HTMLAttributes` shapes
//! against the project-resolved Vue / JSX companions, derives per-tag member
//! sets, and merges fallback attributes. Member types stay SHALLOW semantic
//! SOURCES end-to-end (`ExpandedObjectShape` members are
//! [`verter_type_expr::facts::SemanticTypeSource`]); consumers raise a source
//! to a graph handle on demand through the shared dispatch bridge — no member
//! surface is materialised at projection time. Public surface remains rooted
//! at `crate::host_manage::*`; this file contributes a private
//! `impl VerterHost { … }` block that continues the parent shell's impl chain.

use crate::resolver_core::{IntrinsicMemberTypeSource, IntrinsicSurfaceMember};
use crate::VerterHost;

impl VerterHost {
    /// The single owning project's ownership descriptor for a NON-carrier-ownership
    /// consumer (intrinsic-type projection + its cache anchor). Resolves through the
    /// published snapshot's EXACT configured-owner resolution (`Unique` → that
    /// project) and, only on an authoritative configured-`None`, the SEPARATE
    /// single-fallback resolution. A genuine configured overlap (`Ambiguous`) fails
    /// closed to `None` — never a fabricated winner. This is an explicit,
    /// purpose-scoped project lookup, NOT the carrier-ownership authority (that is
    /// `external_ts::CarrierOwnershipResolution`) and NOT a generic path→singleton
    /// selector on the workspace trait.
    fn owning_project_ownership(
        &self,
        canonical_id: &str,
    ) -> Option<verter_workspace::ProjectOwnership> {
        use verter_workspace::workspace_snapshot::ConfiguredOwnerResolution;
        let root = self.ws().published_root()?;
        let snapshot = &root.snapshot;
        let id = match snapshot.configured_owner_resolution_for_file(canonical_id) {
            ConfiguredOwnerResolution::Unique(id) => id,
            ConfiguredOwnerResolution::Ambiguous(_) => return None,
            ConfiguredOwnerResolution::None => {
                snapshot.single_fallback_owner_for_file(canonical_id)?
            }
        };
        let project = snapshot.project(id);
        Some(verter_workspace::ProjectOwnership {
            project_root: project.root.as_str().to_string(),
            tsconfig_path: snapshot.tsconfig_path(id).map(|p| p.as_str().to_string()),
        })
    }

    pub(super) fn project_intrinsic_cache_anchor(&self, canonical_id: &str) -> (String, u64) {
        let generation = self.ws().content_generation();
        let anchor = self
            .owning_project_ownership(canonical_id)
            .map(|owner| {
                format!(
                    "{}|{}",
                    owner.project_root,
                    owner.tsconfig_path.unwrap_or_default()
                )
            })
            .unwrap_or_else(|| format!("host:{}", self.instance_id));
        (anchor, generation)
    }

    pub(super) fn project_intrinsic_members_for_tag(
        &self,
        owner_canonical_id: &str,
        tag: &str,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<Vec<IntrinsicSurfaceMember>> {
        let vue_canonical = self.resolve_project_intrinsic_canonical(owner_canonical_id, "vue")?;
        let jsx_canonical =
            self.resolve_project_intrinsic_canonical(owner_canonical_id, "vue/jsx")?;

        // Ensure module facts are materialized for the resolved canonicals.
        let _ = self.ensure_indexed_ready_serve(&vue_canonical);
        let _ = self.ensure_indexed_ready_serve(&jsx_canonical);

        let fallback_members = self
            .expand_project_intrinsic_shape_for_canonical(&vue_canonical, "HTMLAttributes", ctx)
            .map(Self::intrinsic_members_from_shape);

        let tag_members =
            self.expand_project_intrinsic_tag_members_for_canonical(&jsx_canonical, tag, ctx);

        match (
            tag_members.filter(|members| !members.is_empty()),
            fallback_members.filter(|members| !members.is_empty()),
        ) {
            (Some(tag_members), Some(fallback_members)) => {
                Some(Self::merge_intrinsic_members(tag_members, fallback_members))
            }
            (Some(tag_members), None) => Some(tag_members),
            (None, Some(fallback_members)) => Some(fallback_members),
            (None, None) => None,
        }
    }

    fn resolve_project_intrinsic_canonical(
        &self,
        owner_canonical_id: &str,
        specifier: &str,
    ) -> Option<String> {
        let owner = self.owning_project_ownership(owner_canonical_id)?;
        let resolved = self.ws().resolve_import_for_project(
            &owner,
            specifier,
            verter_workspace::ResolutionContext {
                phase: verter_workspace::ResolvePhase::ProviderGraph,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
            },
        )?;
        let _ = self.ensure_indexed_ready_serve(&resolved.source_id);
        Some(resolved.source_id)
    }

    fn expand_project_intrinsic_shape_for_canonical(
        &self,
        canonical_id: &str,
        type_name: &str,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        // The root shape resolves in NODE DOMAIN through the query engine's
        // intrinsic rail (`project_intrinsic_root_shape`): the root-symbol
        // whole-surface PRIMARY, then the Class-A FALLBACK for re-exported /
        // namespace-qualified globals (e.g. `JSX.IntrinsicElements`). The engine
        // binds to the supplied request-bound `ctx` so cache validators inside
        // the engine inherit the overlay-aware view. Member values stay shallow
        // sources; consumers raise them on demand through the dispatch bridge.
        let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
        engine.project_intrinsic_root_shape(canonical_id, type_name)
    }

    fn expand_project_intrinsic_tag_members_for_canonical(
        &self,
        canonical_id: &str,
        tag: &str,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<Vec<IntrinsicSurfaceMember>> {
        let intrinsics_shape = self.expand_project_intrinsic_shape_for_canonical(
            canonical_id,
            "JSX.IntrinsicElements",
            ctx,
        )?;
        let tag_source = intrinsics_shape
            .properties
            .into_iter()
            .find(|property| property.name == tag)
            .and_then(|property| property.ty.into_present())?;
        // Resolve NativeElements scope for tag body expansion.
        let tag_scope_canonical = self
            .resolve_local_import_symbol_target(canonical_id, "NativeElements")
            .map(|(resolved_id, _)| resolved_id)
            .filter(|resolved_id| resolved_id != canonical_id);
        let scope = tag_scope_canonical.as_deref().unwrap_or(canonical_id);
        let _ = self.ensure_indexed_ready_serve(scope);
        // The tag's value SOURCE (`HTMLAttributes & { … }`) projects to its
        // `ExpandedObjectShape` in NODE DOMAIN through the query engine's
        // intrinsic tag-member rail, resolved in the `NativeElements` scope: the
        // source raises to a graph handle through the shared dispatch bridge and
        // the node-domain surface synthesiser composes the one-level surface,
        // merging the anonymous property-type intersection role-awarely
        // (Authored arms value-INTERSECT — `number & string` — never
        // last-arm-override), the TS-correct merge for `A & B`. The engine binds
        // to the supplied request-bound `ctx`.
        let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
        let tag_shape = engine.project_intrinsic_tag_member_shape(scope, &tag_source)?;
        Some(Self::intrinsic_members_from_shape(tag_shape))
    }

    fn intrinsic_members_from_shape(
        shape: verter_semantic::analysis::type_expand::ExpandedObjectShape,
    ) -> Vec<IntrinsicSurfaceMember> {
        let mut members = rustc_hash::FxHashMap::default();
        for property in shape.properties {
            // Intrinsic tag members are open-position successes by
            // construction (the intrinsic surface projector publishes
            // present sources only); a non-present position carries no
            // resolvable member source.
            let Some(source) = property.ty.into_present() else {
                continue;
            };
            if let Some(event_name) =
                verter_semantic::analysis::html_intrinsics::on_prop_to_event_name(
                    property.name.as_str(),
                )
            {
                members
                    .entry(format!("listener:{event_name}"))
                    .or_insert(IntrinsicSurfaceMember {
                    name: event_name,
                    kind: verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener,
                    source: IntrinsicMemberTypeSource::Resolved(source),
                });
                continue;
            }

            if !verter_semantic::analysis::html_intrinsics::should_expose_intrinsic_member(
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr,
                property.name.as_str(),
            ) {
                continue;
            }

            members
                .entry(format!("attr:{}", property.name))
                .or_insert(IntrinsicSurfaceMember {
                    name: property.name,
                    kind: verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr,
                    source: IntrinsicMemberTypeSource::Resolved(source),
                });
        }

        let mut members: Vec<_> = members.into_values().collect();
        Self::sort_intrinsic_members(&mut members);
        members
    }

    fn merge_intrinsic_members(
        primary: Vec<IntrinsicSurfaceMember>,
        fallback: Vec<IntrinsicSurfaceMember>,
    ) -> Vec<IntrinsicSurfaceMember> {
        let mut members = rustc_hash::FxHashMap::default();
        for member in fallback.into_iter().chain(primary) {
            members.insert(
                format!(
                    "{}:{}",
                    match member.kind {
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr =>
                            "attr",
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                            "listener"
                        }
                    },
                    member.name
                ),
                member,
            );
        }

        let mut members: Vec<_> = members.into_values().collect();
        Self::sort_intrinsic_members(&mut members);
        members
    }

    /// Canonical intrinsic member ordering: attrs before listeners, each group
    /// by name.
    fn sort_intrinsic_members(members: &mut [IntrinsicSurfaceMember]) {
        members.sort_by(|left, right| {
            let rank = |kind: verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind| {
                match kind {
                    verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                    verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
                }
            };
            rank(left.kind)
                .cmp(&rank(right.kind))
                .then_with(|| left.name.cmp(&right.name))
        });
    }
}
