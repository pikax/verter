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
    if snippet_imports.is_empty() {
        return PropCallableRole::Other;
    }
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
