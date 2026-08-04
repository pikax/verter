//! Svelte prop callable-role classification from exact symbol identity.

use verter_type_expr::facts::SvelteSnippetImportFact;
use verter_type_expr::{PropCallableRole, ResolutionExactness, ResolutionProvenance};

use crate::project_semantic_dispatch::symbol_identity::SymbolIdentityDemandOutcome;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::SemanticNodeId;

/// Compare one prop value's resolved identity with package-validated Svelte
/// `Snippet` import identities.
pub(super) fn classify_svelte_callable_role(
    dispatch: &ProjectSemanticDispatch<'_>,
    prop_value: SemanticNodeId,
    snippet_imports: &[SvelteSnippetImportFact],
) -> PropCallableRole {
    let expected = snippet_imports
        .iter()
        .filter_map(|import| match import {
            SvelteSnippetImportFact::Resolved { symbol, .. } => Some(symbol.clone()),
            SvelteSnippetImportFact::Unresolved { .. } => None,
        })
        .collect::<Vec<_>>();
    let actual = match dispatch.demand_symbol_identity(prop_value, &expected) {
        SymbolIdentityDemandOutcome::Complete(Some(symbol)) => symbol,
        SymbolIdentityDemandOutcome::Complete(None) => return PropCallableRole::Other,
        SymbolIdentityDemandOutcome::Partial(reason) => {
            return PropCallableRole::Unresolved { reason };
        }
    };

    let mut unresolved = None;
    for import in snippet_imports {
        let expected = match import {
            SvelteSnippetImportFact::Resolved { symbol, .. } => Some(symbol.clone()),
            SvelteSnippetImportFact::Unresolved { reason, .. } => {
                unresolved.get_or_insert(*reason);
                None
            }
        };
        if expected
            .as_ref()
            .is_some_and(|expected| expected == &actual)
        {
            return PropCallableRole::SvelteSnippet {
                symbol: actual,
                exactness: ResolutionExactness::ExactSymbolic,
                provenance: ResolutionProvenance::FrameworkSurface,
            };
        }
    }

    unresolved.map_or(PropCallableRole::Other, |reason| {
        PropCallableRole::Unresolved { reason }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use verter_type_expr::PropCallableRoleUnresolvedReason;

    use super::*;
    use crate::semantic_query::{QueryError, SemanticNodeData};

    /// Mutation recipe: restore the empty-`snippet_imports` early return in
    /// `classify_svelte_callable_role`; this test must fail with `Other`.
    #[test]
    fn empty_snippet_inventory_still_demands_carrier_completeness() {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let dispatch = ProjectSemanticDispatch::new(&host);
        let unresolved = dispatch
            .graph()
            .intern_node(SemanticNodeData::Opaque(QueryError::Miss));

        assert_eq!(
            classify_svelte_callable_role(&dispatch, unresolved, &[]),
            PropCallableRole::Unresolved {
                reason: PropCallableRoleUnresolvedReason::MissingDependency,
            }
        );

        let unsupported = dispatch.graph().intern_node(SemanticNodeData::RawFallback {
            raw: Arc::from("unsupported"),
        });
        assert_eq!(
            classify_svelte_callable_role(&dispatch, unsupported, &[]),
            PropCallableRole::Unresolved {
                reason: PropCallableRoleUnresolvedReason::Unsupported,
            },
            "an incomplete carrier cannot prove Other even with no expected identities"
        );

        let complete_non_match = dispatch.graph().intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
        assert_eq!(
            classify_svelte_callable_role(&dispatch, complete_non_match, &[]),
            PropCallableRole::Other,
            "a complete non-match may prove Other"
        );
    }
}
