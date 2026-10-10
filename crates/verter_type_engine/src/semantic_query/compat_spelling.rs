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
pub const SEMANTIC_MISS: &str = "semanticMiss";
/// `QueryError::UnrepresentableSurface` — the arm the intersection fold drops.
pub const SEMANTIC_OBJECT_SURFACE: &str = "semanticObjectSurface";
/// `QueryError::UnrepresentableSurfaceMember`.
pub const SEMANTIC_SURFACE_MEMBER: &str = "semanticSurfaceMember";
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
pub const UNMODELED_POSITION: &str = "unmodeledPosition";
/// `QueryError::Cancelled`.
pub(crate) const CANCELLED: &str = "cancelled";
/// `QueryError::ForeignSemanticOperand`.
pub(crate) const SEMANTIC_FOREIGN_OPERAND: &str = "semanticForeignOperand";
/// `QueryError::StaleSemanticOperand`.
pub(crate) const SEMANTIC_STALE_OPERAND: &str = "semanticStaleOperand";

// ---------------------------------------------------------------------------
// Parameterised prefixes
// ---------------------------------------------------------------------------

/// `QueryError::BudgetExceeded` — `budgetExceeded(<domain:?>)`. The SINGLE
/// source of truth for the budget-exceeded spelling: an INERT
/// compatibility-projection spelling the terminal projection
/// (`semantic_query_error_raw`) emits for that variant; any test pinning the
/// budget spelling references this constant, so it can never silently drift.
pub const BUDGET_EXCEEDED_SENTINEL_PREFIX: &str = "budgetExceeded(";
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
/// `QueryError::IncompleteSemanticOperand` —
/// `semanticIncompleteOperand(<reason>|<reason>)`.
pub(crate) const SEMANTIC_INCOMPLETE_OPERAND_PREFIX: &str = "semanticIncompleteOperand(";
/// `QueryError::CheckerRecovery` — `checkerRecovery(TS<code>)`. The carrier
/// raises as its recovery type, so this spelling appears only where a
/// caller asks for the terminal projection of the error itself.
pub(crate) const CHECKER_RECOVERY_PREFIX: &str = "checkerRecovery(";

/// The recorded partial reasons as stable `|`-joined names in bit order,
/// or `none` for an empty set.
///
/// This is what the observable spellings render instead of the reason
/// set's `Debug` shape: `Debug` on a bitflag newtype leaks the numeric
/// representation into strings component-meta consumers can read, and it
/// changes whenever a bit is added or reordered. The names come from the
/// closed [`PartialReason`](super::PartialReason) taxonomy, so a new
/// reason class gets a name rather than a number.
pub fn spell_partial_reasons(reasons: super::PartialReasonSet) -> String {
    let mut spelled = String::new();
    for reason in reasons.iter() {
        if !spelled.is_empty() {
            spelled.push('|');
        }
        spelled.push_str(reason.name());
    }
    if spelled.is_empty() {
        spelled.push_str("none");
    }
    spelled
}

/// DISPLAY-ONLY predicate: does `raw` spell one of the legacy sentinel
/// strings the terminal compatibility projection can emit (exact family plus
/// the parameterised prefixes)? The family intentionally mirrors the DELETED
/// raw recogniser's set (JSDoc display parity), NOT the full projection
/// family (`semantic_query_error_raw` also emits non-family spellings like
/// `<raise miss>` / `recursiveRef(..)` / `declPlaceholder(..)` / `cancelled`).
/// This is NOT a classifier — no raw spelling is ever read as dispatch
/// control flow (degradation is typed); the only consumer is display
/// disambiguation (the JSDoc sanitize escape).
pub fn spells_legacy_sentinel_family(raw: &str) -> bool {
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

/// The terminal compatibility projection of a typed [`QueryError`] — the
/// inert `Unknown` spelling the compat tree carries. Every spelling/prefix is
/// built from the owned consts in
/// [`crate::semantic_query::compat_spelling`] (the single family home), so
/// producer and detector can never fork a spelling.
pub fn semantic_query_error_raw(err: &crate::semantic_query::QueryError) -> String {
    use crate::semantic_query::compat_spelling as spell;
    use crate::semantic_query::QueryError;
    match err {
        QueryError::Miss => spell::SEMANTIC_MISS.to_string(),
        QueryError::Other(text) => text.as_ref().to_string(),
        QueryError::PermissiveWildcard => "permissiveWildcard".to_string(),
        QueryError::UnsupportedIntrinsic { name } => {
            format!("{}{name})", spell::UNSUPPORTED_INTRINSIC_PREFIX)
        }
        QueryError::BudgetExceeded(failure) => {
            format!(
                "{}{:?})",
                spell::BUDGET_EXCEEDED_SENTINEL_PREFIX,
                failure.domain
            )
        }
        QueryError::Cancelled => spell::CANCELLED.to_string(),
        QueryError::UnstableState { attempts } => {
            format!("{}{attempts})", spell::UNSTABLE_STATE_PREFIX)
        }
        QueryError::AliasCycle { chain } => {
            format!("{}{})", spell::ALIAS_CYCLE_PREFIX, chain.len())
        }
        QueryError::RecursiveRef { name, .. } => format!("{}{name})", spell::RECURSIVE_REF_PREFIX),
        QueryError::DeclPlaceholder { name, .. } => {
            format!("{}{name})", spell::DECL_PLACEHOLDER_PREFIX)
        }
        QueryError::ValueDomainMismatch { expected, actual } => {
            format!(
                "{}expected={expected:?},actual={actual:?})",
                spell::VALUE_DOMAIN_MISMATCH_PREFIX
            )
        }
        QueryError::RaiseAliasCycle => spell::SEMANTIC_ALIAS_CYCLE.to_string(),
        QueryError::TypeParamCycle => spell::SEMANTIC_TYPE_PARAM_CYCLE.to_string(),
        QueryError::RaiseMiss => spell::RAISE_MISS.to_string(),
        QueryError::UnrepresentableSurface => spell::SEMANTIC_OBJECT_SURFACE.to_string(),
        QueryError::UnrepresentableSurfaceMember => spell::SEMANTIC_SURFACE_MEMBER.to_string(),
        QueryError::OpenSurface => spell::OPEN_SURFACE.to_string(),
        QueryError::UnmodeledPosition => spell::UNMODELED_POSITION.to_string(),
        QueryError::ForeignSemanticOperand => spell::SEMANTIC_FOREIGN_OPERAND.to_string(),
        QueryError::StaleSemanticOperand => spell::SEMANTIC_STALE_OPERAND.to_string(),
        QueryError::IncompleteSemanticOperand { reasons } => {
            format!(
                "{}{})",
                spell::SEMANTIC_INCOMPLETE_OPERAND_PREFIX,
                spell::spell_partial_reasons(*reasons)
            )
        }
        QueryError::CheckerRecovery { diagnostic, .. } => {
            format!(
                "{}TS{})",
                spell::CHECKER_RECOVERY_PREFIX,
                diagnostic.code.code()
            )
        }
    }
}
