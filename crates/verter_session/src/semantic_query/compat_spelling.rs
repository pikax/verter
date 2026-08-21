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
/// `QueryError::UnmodeledPosition`.
pub(crate) const UNMODELED_POSITION: &str = "unmodeledPosition";
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

// ---------------------------------------------------------------------------
// Embedded-occurrence leaked-sentinel screen
// ---------------------------------------------------------------------------

/// Every raw spelling in the UNMATERIALIZED-sentinel class: the exact
/// [`QueryError`](crate::semantic_query::QueryError) variants
/// `query_error_is_unmaterialized_sentinel`
/// (`crate::project_semantic_dispatch::raise_sentinel`) classifies as a
/// genuine degradation rather than a deliberately-materialised placeholder.
/// This list mirrors that predicate's `true` arm exactly — keep the two in
/// sync on any `QueryError` variant change. Deliberately EXCLUDES the
/// materialised-placeholder family (`RaiseMiss`, `TypeParamCycle`,
/// `RecursiveRef`, `ValueDomainMismatch`, `DeclPlaceholder`, `Other`) — those
/// are legitimate, by-design content the fold intentionally produces inside
/// an otherwise-complete materialized tree, never a leak.
const UNMATERIALIZED_SENTINEL_EXACT: &[&str] = &[
    SEMANTIC_MISS,
    CANCELLED,
    SEMANTIC_ALIAS_CYCLE,
    OPEN_SURFACE,
    SEMANTIC_OBJECT_SURFACE,
    UNMODELED_POSITION,
    SEMANTIC_SURFACE_MEMBER,
];

/// Parameterised-prefix counterpart of [`UNMATERIALIZED_SENTINEL_EXACT`] —
/// same class, same sync obligation.
const UNMATERIALIZED_SENTINEL_PREFIX: &[&str] = &[
    UNSUPPORTED_INTRINSIC_PREFIX,
    BUDGET_EXCEEDED_SENTINEL_PREFIX,
    UNSTABLE_STATE_PREFIX,
    ALIAS_CYCLE_PREFIX,
];

/// Whether `text` — a rendered TSC/declaration display string — carries a
/// STANDALONE-TOKEN occurrence of a reserved compat-projection sentinel from
/// the unmaterialized-sentinel family: a genuine resolver degradation (a
/// nested `QueryError::Miss`, `UnmodeledPosition`, `BudgetExceeded`, …) that
/// a caller failed to bubble up as an `Err` and instead baked into the
/// returned text as literal sentinel content.
///
/// NOT a general classifier: no raw spelling is ever read as resolver
/// control flow. This is a narrow safety screen for a producer boundary that
/// must never publish a leaked internal sentinel as if it were a real type —
/// the caller degrades to its own honest failure outcome instead.
///
/// Matches only a standalone identifier-like token, never a substring of a
/// longer identifier, so a real user type that happens to contain a
/// fragment of a sentinel spelling as part of a longer name is never
/// misclassified — see each call site for the accepted residual risk of a
/// real type or member named EXACTLY one of these reserved spellings.
pub(crate) fn text_embeds_unmaterialized_sentinel(text: &str) -> bool {
    let is_ident_byte = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$';
    let bytes = text.as_bytes();
    let boundary_before = |start: usize| {
        start
            .checked_sub(1)
            .and_then(|index| bytes.get(index))
            .is_none_or(|&byte| !is_ident_byte(byte))
    };
    UNMATERIALIZED_SENTINEL_EXACT.iter().any(|sentinel| {
        text.match_indices(sentinel).any(|(start, matched)| {
            let end = start + matched.len();
            boundary_before(start) && bytes.get(end).is_none_or(|&byte| !is_ident_byte(byte))
        })
    }) || UNMATERIALIZED_SENTINEL_PREFIX.iter().any(|prefix| {
        text.match_indices(prefix)
            .any(|(start, _)| boundary_before(start))
    })
}
