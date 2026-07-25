//! The ONE owner of the legacy compatibility-spelling family.
//!
//! Every exact spelling / prefix the terminal compatibility projection
//! (`semantic_query_error_raw`) can emit lives here as a named const, and the
//! shared legacy-family predicate (a DISPLAY-ONLY disambiguation aid, never a
//! control-flow classifier) is defined here exactly once. The spellings
//! themselves are inert text — resolver degradation travels as typed
//! [`QueryError`](crate::semantic_query::QueryError) data; these strings exist
//! so the wire/display/hash bytes stay identical to the legacy encoding.

// ---------------------------------------------------------------------------
// Exact spellings
// ---------------------------------------------------------------------------

/// `QueryError::Miss`.
pub(crate) const SEMANTIC_MISS: &str = "semanticMiss";
/// `QueryError::UnrepresentableSurface` — the arm the intersection fold drops.
pub(crate) const SEMANTIC_OBJECT_SURFACE: &str = "semanticObjectSurface";
/// `QueryError::UnrepresentableSurfaceMember`.
pub(crate) const SEMANTIC_SURFACE_MEMBER: &str = "semanticSurfaceMember";
/// `QueryError::RaiseAliasCycle`.
pub(crate) const SEMANTIC_ALIAS_CYCLE: &str = "semanticAliasCycle";
/// `QueryError::TypeParamCycle`.
pub(crate) const SEMANTIC_TYPE_PARAM_CYCLE: &str = "semanticTypeParamCycle";
/// Legacy family member with no current producer (kept for display-family
/// parity).
pub(crate) const SEMANTIC_FUNCTION: &str = "semanticFunction";
/// `QueryError::RaiseMiss` (a materialised-class carrier-arg placeholder).
pub(crate) const RAISE_MISS: &str = "<raise miss>";
/// `QueryError::OpenSurface`.
pub(crate) const OPEN_SURFACE: &str = "projectedOpenSurface";
/// `QueryError::Cancelled`.
pub(crate) const CANCELLED: &str = "cancelled";

// ---------------------------------------------------------------------------
// Parameterised prefixes
// ---------------------------------------------------------------------------

/// `QueryError::BudgetExceeded` — `budgetExceeded(<domain:?>)`. The SINGLE
/// source of truth for the budget-exceeded spelling: an INERT
/// compatibility-projection spelling the terminal projection
/// (`semantic_query_error_raw`) emits for that variant; any test pinning the
/// budget spelling references this constant, so it can never silently drift.
pub(crate) const BUDGET_EXCEEDED_SENTINEL_PREFIX: &str = "budgetExceeded(";
/// `QueryError::UnsupportedIntrinsic` — `unsupportedIntrinsic(<name>)`.
pub(crate) const UNSUPPORTED_INTRINSIC_PREFIX: &str = "unsupportedIntrinsic(";
/// `QueryError::UnstableState` — `unstableState(<attempts>)`.
pub(crate) const UNSTABLE_STATE_PREFIX: &str = "unstableState(";
/// `QueryError::AliasCycle` — `aliasCycle(<len>)`.
pub(crate) const ALIAS_CYCLE_PREFIX: &str = "aliasCycle(";
/// `QueryError::RecursiveRef` — `recursiveRef(<name>)`.
pub(crate) const RECURSIVE_REF_PREFIX: &str = "recursiveRef(";
/// `QueryError::DeclPlaceholder` — `declPlaceholder(<name>)`.
pub(crate) const DECL_PLACEHOLDER_PREFIX: &str = "declPlaceholder(";
/// `QueryError::ValueDomainMismatch` — `valueDomainMismatch(expected=..,actual=..)`.
pub(crate) const VALUE_DOMAIN_MISMATCH_PREFIX: &str = "valueDomainMismatch(";
/// Legacy `materialize:<…>` family prefix (display family only; no current
/// producer).
pub(crate) const MATERIALIZE_PREFIX: &str = "materialize:";

/// DISPLAY-ONLY predicate: does `raw` spell one of the legacy sentinel
/// strings the terminal compatibility projection can emit (exact family plus
/// the parameterised prefixes)? The family intentionally mirrors the DELETED
/// raw recogniser's set (JSDoc display parity), NOT the full projection
/// family (`semantic_query_error_raw` also emits non-family spellings like
/// `<raise miss>` / `recursiveRef(..)` / `declPlaceholder(..)` / `cancelled`).
/// This is NOT a classifier — no raw spelling is ever read as dispatch
/// control flow (degradation is typed); the only consumer is display
/// disambiguation (the JSDoc sanitize escape).
pub(crate) fn spells_legacy_sentinel_family(raw: &str) -> bool {
    let is_exact = matches!(
        raw,
        SEMANTIC_MISS
            | SEMANTIC_OBJECT_SURFACE
            | SEMANTIC_SURFACE_MEMBER
            | SEMANTIC_ALIAS_CYCLE
            | SEMANTIC_FUNCTION
            | OPEN_SURFACE
    );
    let is_prefixed = raw.starts_with(MATERIALIZE_PREFIX)
        || raw.starts_with(UNSUPPORTED_INTRINSIC_PREFIX)
        || raw.starts_with(BUDGET_EXCEEDED_SENTINEL_PREFIX)
        || raw.starts_with(UNSTABLE_STATE_PREFIX)
        || raw.starts_with(ALIAS_CYCLE_PREFIX);
    is_exact || is_prefixed
}
