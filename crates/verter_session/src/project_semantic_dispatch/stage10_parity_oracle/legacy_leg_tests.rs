//! The retained prepared-body body-lowering implementation — the
//! `BodyLeg::LegacyPreparedBody` oracle leg.
//!
//! A faithful copy of the prepared-body implementation of
//! `lower_decl_body_with_provenance` (the `&prepared.body` /
//! `prepared.merged_contributors` body source lowered through the
//! reducing `shallow_lower_type_expr_with_context` entry). Reached ONLY
//! through the `#[cfg(test)]` delegation at the top of
//! `lower_decl_body_with_provenance` while a
//! [`super::LegacyPreparedBodyLegGuard`] is active — never from
//! production code.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    MemberMergeRole, NodeScopeId, PrimitiveKind, SemanticNodeData, SemanticNodeId,
};

impl<'a> ProjectSemanticDispatch<'a> {
    /// Lower a prepared declaration's `body` from the PREPARED-BODY
    /// source, carrying the macro-surface provenance with
    /// own-body-vs-heritage discrimination — the retained oracle leg.
    ///
    /// Per-arm rule (identical to the production helper's published
    /// contract): own-body `Object` (and `Parenthesized(Object)`) arms
    /// keep the caller's provenance and carry `OwnBody`; reference arms
    /// decay to structural provenance and carry the declaration-kind
    /// merge role (`Heritage` for interface/class, `Authored` for alias);
    /// same-name merged contributors lower as own-body surfaces into the
    /// distinct `MergedDecl` carrier.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn legacy_lower_decl_body_from_prepared(
        &self,
        prepared: &PreparedTypeDecl,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        scope_payload: Option<&crate::resolver_core::bare_name_resolve::DeclarationScopePayload>,
        shadowing: &crate::resolver_core::scope_shadowing::ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        use verter_type_expr::TypeExpr;

        super::note_legacy_prepared_body_read();

        // Whether an intersection arm is an own-body member-bearing arm
        // (an inline `Object` literal, or a `Parenthesized` wrapper of
        // one) vs a reference carrier (`Ref` / anything else).
        fn arm_is_own_body(arm: &TypeExpr) -> bool {
            match arm {
                TypeExpr::Object(_) => true,
                TypeExpr::Parenthesized(inner) => arm_is_own_body(inner),
                _ => false,
            }
        }

        use verter_semantic::analysis::type_eval::TypeDeclKind;
        let reference_arm_role = match prepared.kind {
            TypeDeclKind::Interface | TypeDeclKind::Class => MemberMergeRole::Heritage,
            TypeDeclKind::Alias => MemberMergeRole::Authored,
        };

        // Same-name merged declaration: lower each contributor body as its
        // own OWN-body surface and intern the distinct `MergedDecl`
        // carrier (never a bare `Intersection`).
        if !prepared.merged_contributors.is_empty() {
            let contributor_ids: Vec<SemanticNodeId> = prepared
                .merged_contributors
                .iter()
                .map(|contributor| {
                    self.shallow_lower_type_expr_with_context(
                        contributor,
                        env,
                        scope,
                        &prepared.name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        context.with_merge_role(MemberMergeRole::OwnBody),
                    )
                })
                .collect();
            return self.graph().intern_node_with_scope(
                SemanticNodeData::MergedDecl {
                    contributors: Arc::from(contributor_ids.into_boxed_slice()),
                },
                scope.clone(),
            );
        }

        match &prepared.body {
            TypeExpr::Intersection(arms) => {
                let arm_ids: Vec<SemanticNodeId> = arms
                    .iter()
                    .map(|arm| {
                        let arm_context = if arm_is_own_body(arm) {
                            context.with_merge_role(MemberMergeRole::OwnBody)
                        } else {
                            context
                                .into_structural_provenance()
                                .with_merge_role(reference_arm_role)
                        };
                        self.shallow_lower_type_expr_with_context(
                            arm,
                            env,
                            scope,
                            &prepared.name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            arm_context,
                        )
                    })
                    .collect();
                if arm_ids.is_empty() {
                    self.graph()
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if arm_ids.len() == 1 {
                    arm_ids[0]
                } else {
                    self.graph().intern_node_with_scope(
                        SemanticNodeData::Intersection(Arc::from(arm_ids.into_boxed_slice())),
                        scope.clone(),
                    )
                }
            }
            TypeExpr::Object(_) | TypeExpr::Parenthesized(_) => self
                .shallow_lower_type_expr_with_context(
                    &prepared.body,
                    env,
                    scope,
                    &prepared.name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    context.with_merge_role(MemberMergeRole::OwnBody),
                ),
            _ => self.shallow_lower_type_expr_with_context(
                &prepared.body,
                env,
                scope,
                &prepared.name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                context,
            ),
        }
    }
}
