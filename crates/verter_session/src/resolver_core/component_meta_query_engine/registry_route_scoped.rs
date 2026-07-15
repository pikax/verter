//! Route-scoped registry publication encoding for
//! `ComponentMetaQueryEngine<'a>` — the SELECTED one-level topology of a
//! declaration under an accumulated [`RouteDemand`], published as a
//! `Projected(Surface)` fact (member payloads stay lazy `TypeBodySlot`
//! locators).
//!
//! Inherent methods in a sibling `impl` block (parent-private locality):
//! `route_scoped_surface_fact` is the publication-site entry;
//! `route_scoped_member_fact` is its declaring-contributor slot recovery.

use super::surface::compound_root_surface_view_via_dispatch;
use super::ComponentMetaQueryEngine;
use crate::semantic_query::{ProjectionMode, SemanticNodeId};

impl ComponentMetaQueryEngine<'_> {
    /// Route-scoped registry publication encoding: the SELECTED one-level
    /// topology of `symbol_name`'s declaration under the accumulated
    /// [`RouteDemand`], as a [`ProjectedSurfaceFact`] the publication site
    /// wraps in `SemanticTypeSource::Projected(ProjectedTypeFact::Surface)`.
    ///
    /// Built from the EXISTING memoized empty-path Shallow surface
    /// observation (the same shared walker the heritage encoder above
    /// drives), with the route applied to the observed TOP-LEVEL membership:
    ///
    /// - `MemberPath([m])` selects exactly `m`;
    /// - `Pick(keys)` selects the keys;
    /// - `Omit(keys)` selects the observed complement;
    /// - a DEEP `MemberPath` (len > 1) selects NO top-level member (the
    ///   consumer resolved that path inline, path-precisely) — the caller
    ///   skips publication for such routes;
    /// - `Whole` is not route-scoped (`None`; the whole-surface publication
    ///   paths serve it).
    ///
    /// Selected member payloads stay LAZY `TypeBodySlot` locators: the
    /// declaring contributor's prepared member slot by default; for a member
    /// whose declaring surface sits behind a generic SUBSTITUTION, the
    /// authored USE-SITE slot that expressed the indexed access (replaying it
    /// through the one dispatch re-derives navigation + substitution) — never
    /// a serialized post-substitution graph node. Declines (`None`) when the
    /// observed surface carries signatures or a selected member's payload
    /// slot is unrecoverable; the caller falls back to the whole authored
    /// publication.
    ///
    /// [`RouteDemand`]: crate::resolver_core::RouteDemand
    pub(crate) fn route_scoped_surface_fact(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        route: &crate::resolver_core::RouteDemand,
        member_use_sites: &[(String, verter_type_expr::locators::TypeBodySlot)],
    ) -> Option<verter_type_expr::facts::ProjectedSurfaceFact> {
        use verter_type_expr::facts::{ProjectedMemberFact, ProjectedSurfaceFact};

        if self.projection_op_budget_exhausted() {
            return None;
        }
        enum Selection {
            Named(std::collections::BTreeSet<String>),
            Complement(std::collections::BTreeSet<String>),
        }
        let selection = match route {
            crate::resolver_core::RouteDemand::Whole => return None,
            crate::resolver_core::RouteDemand::MemberPath(path) => {
                if path.len() != 1 {
                    return None;
                }
                Selection::Named(std::collections::BTreeSet::from([path[0].clone()]))
            }
            crate::resolver_core::RouteDemand::Pick(keys) => {
                Selection::Named(keys.iter().map(|key| key.to_string()).collect())
            }
            crate::resolver_core::RouteDemand::Omit(keys) => {
                Selection::Complement(keys.iter().map(|key| key.to_string()).collect())
            }
        };
        // The declaration's resolved root identity + raised body root — the
        // same resolution the heritage encoder performs.
        let scope_payload_arc = self.scope_payload_for_scope(scope_canonical_id);
        let (own_canonical, own_name) =
            crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                self.ctx,
                scope_canonical_id,
                scope_payload_arc.as_deref(),
                symbol_name,
            )
            .map(|root| (root.canonical_id, root.symbol_name))
            .unwrap_or_else(|| {
                let interner = self.ctx.project_type_store().identity_interner();
                (
                    interner.intern(scope_canonical_id),
                    interner.intern(symbol_name),
                )
            });
        let body_locator = self.named_decl_body(own_canonical.as_ref(), own_name.as_ref())?;
        let body_root = {
            let dispatch = self.semantic_dispatch();
            dispatch
                .raise_authored_locator_to_hot(
                    &body_locator,
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    ),
                )
                .map(|hot: crate::semantic_query::HotTypeRef| hot.node())?
        };
        // The one-level view through the shared empty-path Shallow surface
        // walker (member values stay shallow nodes).
        let (view, _surface_node) = compound_root_surface_view_via_dispatch(self.ctx, body_root)?;
        if !view.call_signatures.is_empty()
            || !view.construct_signatures.is_empty()
            || !view.index_signatures.is_empty()
            || view.has_index_signature
        {
            return None;
        }
        let selected = |member_name: &str| -> bool {
            match &selection {
                Selection::Named(names) => names.contains(member_name),
                Selection::Complement(names) => !names.contains(member_name),
            }
        };
        let mut members: Vec<ProjectedMemberFact> = Vec::new();
        for member in view.members.iter() {
            if !selected(member.name.as_ref()) {
                continue;
            }
            let (fact, crossed_substitution) = self.route_scoped_member_fact(
                own_canonical.as_ref(),
                own_name.as_ref(),
                member.name.as_ref(),
            )?;
            // A member reached through a generic SUBSTITUTION retains the
            // authored use-site slot when the discovery recorded one — the
            // declaring contributor's slot would replay unsubstituted.
            let ty = if crossed_substitution {
                member_use_sites
                    .iter()
                    .find(|(name, _)| name == member.name.as_ref())
                    .map(|(_, slot)| slot.clone())
                    .unwrap_or_else(|| fact.ty.clone())
            } else {
                fact.ty.clone()
            };
            members.push(ProjectedMemberFact {
                name: member.name.as_ref().to_string(),
                optional: fact.optional,
                readonly: fact.readonly,
                is_method: fact.is_method,
                visibility: fact.visibility,
                declared_in_macro_type_arg: false,
                declaration_origin: fact.declaration_origin.clone(),
                ty,
                span_origin: fact.span_origin.clone(),
            });
        }
        Some(ProjectedSurfaceFact {
            members: std::sync::Arc::from(members.into_boxed_slice()),
            call_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            has_index_signature: false,
        })
    }

    /// Recover the declaring contributor's prepared member slot for
    /// `member_name` starting at `(canonical, symbol)`, following the
    /// declaration's body head structurally (typed IR / node domain only):
    /// alias chains (`DeclRef`), generic applications (`InstantiationRef`
    /// bases), builtin object-filter sources (`Pick`/`Omit` first argument),
    /// and lone-`extends` heritage arms. Returns the fact plus whether the
    /// walk CROSSED a generic substitution boundary (a userland application
    /// carrying arguments) — in which case the fact's slot replays
    /// unsubstituted and the caller prefers the authored use-site slot.
    fn route_scoped_member_fact(
        &mut self,
        canonical: &str,
        symbol: &str,
        member_name: &str,
    ) -> Option<(verter_type_expr::facts::PreparedMemberFact, bool)> {
        use crate::project_semantic_dispatch::node_data_for;
        use crate::semantic_query::SemanticNodeData;

        let peel_alias = |mut node: SemanticNodeId| {
            while let Some(SemanticNodeData::Alias(inner)) =
                node_data_for(self.ctx, node).as_deref()
            {
                node = *inner;
            }
            node
        };
        let mut current: Vec<(String, String, bool)> =
            vec![(canonical.to_string(), symbol.to_string(), false)];
        for _ in 0..8 {
            let mut next: Vec<(String, String, bool)> = Vec::new();
            for (decl_canonical, decl_name, crossed) in current.drain(..) {
                let Some(prepared) =
                    self.prepared_type_decl(decl_canonical.as_str(), decl_name.as_str())
                else {
                    continue;
                };
                if let Some(fact) = prepared.member_index.get(member_name) {
                    return Some((fact.clone(), crossed));
                }
                let Some(root) =
                    crate::resolver_core::component_meta_registry::prepared_body_root_node(
                        self.ctx,
                        prepared.as_ref(),
                    )
                else {
                    continue;
                };
                match node_data_for(self.ctx, peel_alias(root)).as_deref() {
                    Some(SemanticNodeData::DeclRef { identity }) => {
                        next.push((
                            identity.canonical_id.as_ref().to_string(),
                            identity.decl_name.as_ref().to_string(),
                            crossed,
                        ));
                    }
                    Some(SemanticNodeData::InstantiationRef { base, args }) => {
                        if base.canonical_id.as_ref() == "__builtin__"
                            && matches!(base.decl_name.as_ref(), "Pick" | "Omit")
                        {
                            // Object-filter utilities keep member VALUE slots
                            // from the SOURCE type unchanged (no value
                            // substitution) — recurse into the source arg.
                            if let Some(SemanticNodeData::DeclRef { identity }) = args
                                .first()
                                .and_then(|arg| node_data_for(self.ctx, peel_alias(*arg)))
                                .as_deref()
                            {
                                next.push((
                                    identity.canonical_id.as_ref().to_string(),
                                    identity.decl_name.as_ref().to_string(),
                                    crossed,
                                ));
                            }
                        } else if base.canonical_id.as_ref() != "__builtin__" {
                            next.push((
                                base.canonical_id.as_ref().to_string(),
                                base.decl_name.as_ref().to_string(),
                                crossed || !args.is_empty(),
                            ));
                        }
                    }
                    Some(SemanticNodeData::Intersection(arms)) => {
                        for arm in arms.iter() {
                            if let Some(SemanticNodeData::DeclRef { identity }) =
                                node_data_for(self.ctx, peel_alias(*arm)).as_deref()
                            {
                                next.push((
                                    identity.canonical_id.as_ref().to_string(),
                                    identity.decl_name.as_ref().to_string(),
                                    crossed,
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
            if next.is_empty() {
                return None;
            }
            current = next;
        }
        None
    }
}
