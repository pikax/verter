//! `Function.prototype.call` ambient proof.
//!
//! `.call` normalization is allowed ONLY when ordinary member resolution
//! PROVES the resolved member is the ambient-lib `Function` /
//! `CallableFunction.call` occurrence — never a spelling match on the
//! member name. The proof requires, all at once:
//!
//! 1. the resolved candidate's declaration canonical IS a registered
//!    ambient lib of the active project (registry membership through
//!    [`crate::resolver_core::ResolverContext::lookup_ambient_symbol`],
//!    never a path-shape guess);
//! 2. the containing declaration IS the indexed ambient `Function` /
//!    `CallableFunction` occurrence (the candidate's occurrence identity
//!    names that interface and its canonical is the registered lib's
//!    virtual id);
//! 3. the applicable signature occurrence matches the indexed ambient
//!    signature identity (the same occurrence check — an instantiated
//!    candidate preserves it);
//! 4. NO non-lib augmentation participates in the proved member group:
//!    every `call` member on the DECLARING interface's resolved surface is
//!    declared in the same ambient canonical.
//!
//! Any failure — user augmentation, unbound extraction, ambiguous
//! provenance, or a missing ambient registration — yields `None`, and the
//! caller resolves the site as an ordinary method call.

use std::sync::Arc;

use verter_semantic::resolver_core::ProjectStableKey;

use crate::semantic_query::{
    QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput, SignatureRef,
};

use super::ProjectSemanticDispatch;

/// The proved ambient `Function.prototype.call` identity. The caller
/// rebases to the extracted callable: the receiver expression's type is
/// the callable, argument zero is the new receiver, and the remaining
/// arguments resolve normally.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // consumed by the applicability executor (call resolution)
pub(crate) struct PrototypeCallProof {
    /// The registered ambient lib's virtual canonical that declares the
    /// `Function` / `CallableFunction` interface.
    pub declaring_canonical: Arc<str>,
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Prove that the resolved `call` candidates are the ambient-lib
    /// `Function` / `CallableFunction.call` occurrence. All four contract
    /// clauses must hold for EVERY candidate; any failure is `None`
    /// (ordinary method-call resolution).
    #[allow(dead_code)] // consumed by the applicability executor (call resolution)
    pub(crate) fn prove_prototype_call(
        &self,
        consumer_project: ProjectStableKey,
        candidates: &[SignatureRef],
    ) -> Option<PrototypeCallProof> {
        if candidates.is_empty() {
            return None;
        }
        // Clauses 1–3: every candidate's occurrence names the ambient
        // `Function` / `CallableFunction` interface, and that interface's
        // canonical IS the registered ambient lib of this project.
        let mut declaring: Option<(Arc<str>, crate::semantic_query::ResolvedDeclSlotIdentity)> =
            None;
        for candidate in candidates {
            // Only an AUTHORED occurrence can name the ambient interface;
            // a rootless callable never proves the prototype hop.
            let crate::semantic_query::SignatureOccurrenceIdentity { function, .. } =
                candidate.occurrence.authored()?;
            let slot = &function.declaration_slot;
            let symbol = slot.merged_symbol_name.as_ref();
            if symbol != "Function" && symbol != "CallableFunction" {
                return None;
            }
            let hit = self.ctx.lookup_ambient_symbol(consumer_project, symbol)?;
            if hit.virtual_id.as_ref() != slot.defining_canonical.as_ref() {
                return None;
            }
            let identity = (hit.virtual_id, slot.clone());
            match &declaring {
                None => declaring = Some(identity),
                Some((previous_canonical, previous_slot))
                    if *previous_canonical == identity.0 && *previous_slot == identity.1 => {}
                // Mixed provenance across candidates is not the ambient
                // occurrence.
                Some(_) => return None,
            }
        }
        let (declaring_canonical, declaring_slot) = declaring?;

        // Clause 4: materialize the DECLARING interface's own surface —
        // every `call` member is declared in the same ambient canonical
        // (no user augmentation participates in the proved member group).
        let interface = match self.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                declaring_slot,
                Arc::from(Vec::new().into_boxed_slice()),
                self.instantiate_context_for(
                    declaring_canonical.as_ref(),
                    crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Expanded,
                    ),
                ),
            ),
        )) {
            QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
            QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
        };
        if !self.call_members_all_declared_in(interface, declaring_canonical.as_ref()) {
            return None;
        }
        Some(PrototypeCallProof {
            declaring_canonical,
        })
    }

    /// Whether every `call` member on `node`'s resolved surface (alias
    /// chains unwrapped) is declared in `canonical`. False for a merged /
    /// ambiguous receiver, a foreign `call` contributor (user
    /// augmentation), or a surface with no `call` member at all.
    pub(crate) fn call_members_all_declared_in(
        &self,
        node: SemanticNodeId,
        canonical: &str,
    ) -> bool {
        let graph = self.graph();
        let mut node = node;
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        loop {
            if !visited.insert(node) {
                return false;
            }
            let Some(data) = graph.node_data(node) else {
                return false;
            };
            match &*data {
                SemanticNodeData::Alias(target) => {
                    node = *target;
                }
                SemanticNodeData::Object(surface) => {
                    let mut saw_call_member = false;
                    for member in surface.positive_members() {
                        let crate::semantic_query::AuthoredPropertyKey::String(name) = &member.key
                        else {
                            continue;
                        };
                        if name.as_ref() != "call" {
                            continue;
                        }
                        saw_call_member = true;
                        if member.declaration_origin.as_deref() != Some(canonical) {
                            return false;
                        }
                    }
                    return saw_call_member;
                }
                // A merged / augmented receiver is ambiguous provenance by
                // construction.
                _ => return false,
            }
        }
    }
}

#[cfg(test)]
#[path = "prototype_call_tests.rs"]
mod prototype_call_tests;
