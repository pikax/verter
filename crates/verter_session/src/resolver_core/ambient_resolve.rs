//! session-side ambient global resolver.
//!
//! Bare-name fallback for symbols that aren't found in scope or in the
//! import graph. Looks the symbol up against the workspace's per-project
//! ambient lib symbol_index (A2) and produces a `ResolvedRootIdentity`
//! whose `canonical_id` is the project-scoped ambient virtual id
//! (`ambient:/<tag>/<canonical>`). The dispatch's library-global lookup
//! (`ProjectSemanticDispatch::lib_global_declaration`) is its caller, and
//! every reader of a library global asks that lookup.

use verter_session_query::resolution::ProjectStableKey;
use verter_session_query::type_solver::host::ResolvedRootIdentity;

use crate::resolver_core::ResolverContext;

/// Resolve a bare-name symbol against the consumer project's ambient lib
/// registry. Returns `None` when no registered lib in the project exposes
/// `symbol`.
///
/// On a hit:
/// - records the consumer → ambient virtual_id reverse-dep edge so a
///   subsequent re-registration of the lib invalidates this consumer
///   through the standard dep-fact validators.
/// - returns a `ResolvedRootIdentity` whose `canonical_id` is the ambient
///   virtual id, so the recorded fact reaches the ambient `WholeHash`
///   arm on warm-read validation through the live `StoreView`.
pub(crate) fn resolve_ambient_global<C: crate::resolver_core::ResolverCapabilities>(
    ctx: &dyn ResolverContext<C>,
    consumer_canonical: &str,
    consumer_project_stable_key: ProjectStableKey,
    symbol: &str,
) -> Option<ResolvedRootIdentity> {
    // workspace mutators are NOT exposed on `ResolverContext`.
    // The two narrow ambient capabilities `lookup_ambient_symbol` and
    // `record_ambient_dependency` replace the broad `workspace()` accessor
    // that the previous `&VerterHost` callsite consulted.
    let hit = ctx.lookup_ambient_symbol(consumer_project_stable_key, symbol)?;
    ctx.record_ambient_dependency(consumer_canonical, hit.virtual_id.as_ref());
    Some(ResolvedRootIdentity::new_in_owner(
        hit.virtual_id.as_ref(),
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        symbol,
    ))
}
