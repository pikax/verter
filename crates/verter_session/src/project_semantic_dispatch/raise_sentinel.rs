//! Shared owner of the unmaterialised-`Unknown { raw }` sentinel spelling.
//!
//! The shared `shape_engine::fold_node` materialisation algebra emits a small
//! fixed set of `TypeExpr::Unknown { raw }` sentinel strings when dispatch cannot
//! materialise a node (an unrepresentable surface, an alias cycle, a Vue
//! macro placeholder, a budget-exceeded carrier, …). Two classification
//! surfaces share this owner — one RAW, one TYPED — and there is no
//! raise-to-`TypeExpr`-then-classify round-trip on the typed path:
//! - The `TypeExpr`-domain recogniser
//!   [`dispatch_route_expr_is_materialized`](crate::resolver_core::component_meta_query_engine::dispatch_route_expr_is_materialized)
//!   classifies a raised `Unknown { raw }` STRING via the raw recogniser
//!   [`raw_is_unmaterialized_sentinel`] DIRECTLY.
//! - The node-domain raised-shape projection (owner-local in [`super::raise`],
//!   the `summary::opaque_sentinel` fn in `raise::shape_engine::node_domain`)
//!   classifies on the TYPED [`QueryError`] variant DIRECTLY — via the typed
//!   authorities [`query_error_is_unmaterialized_sentinel`] /
//!   [`query_error_is_object_surface_sentinel`] defined below — never by raising
//!   the error to a `TypeExpr` and re-spelling it.
//!
//! The raw recogniser and the typed authorities are held BYTE-FOR-BYTE in
//! agreement by the no-drift contract test in this file, so the two surfaces
//! classify a sentinel identically and the spelling has exactly one owner and
//! can never drift.
//!
//! The set is the EXACT spelling `dispatch_route_expr_is_materialized`
//! historically inlined: the three [`SEMANTIC_MISS`] / [`SEMANTIC_OBJECT_SURFACE`]
//! / [`SEMANTIC_SURFACE_MEMBER`] consts, the four exact strings
//! (`semanticAliasCycle`, `semanticFunction`, `VueMacroElements`,
//! `projectedOpenSurface`), and the five prefixes (`materialize:`,
//! `unsupportedIntrinsic(`, the [`BUDGET_EXCEEDED_SENTINEL_PREFIX`],
//! `unstableState(`, `aliasCycle(`). Everything else — including the
//! `<raise miss>` carrier-arg placeholder and `semanticTypeParamCycle` —
//! is MATERIALISED (returns `false`), exactly as the legacy inline check
//! treated them.

use crate::resolver_core::component_meta_query_engine::{
    BUDGET_EXCEEDED_SENTINEL_PREFIX, SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE,
    SEMANTIC_SURFACE_MEMBER,
};
use crate::semantic_query::QueryError;

/// Returns `true` when `raw` is one of the sentinel spellings that marks a
/// `TypeExpr::Unknown { raw }` as UNMATERIALISED (a dispatch miss the
/// dispatch-first path falls back to `owner_engine` for).
///
/// This recogniser is the authority for strings that arrive RAW — externally
/// interned `Unknown` nodes (`RawFallback`), the prefix-bearing carriers, and
/// any text without a typed [`QueryError`] source. The `TypeExpr`-domain
/// [`dispatch_route_expr_is_materialized`](crate::resolver_core::component_meta_query_engine::dispatch_route_expr_is_materialized)
/// classifies a raised spelling through this recogniser DIRECTLY.
///
/// The node-domain raised-shape projection's TYPED [`QueryError`] path does NOT
/// round-trip through this raw recogniser: it classifies on the typed variant
/// directly via [`query_error_is_unmaterialized_sentinel`] (the typed authority).
/// (Its raw-input arm — a genuinely-raw `Unknown { raw }` node with no typed
/// source — does reach this recogniser, exactly as the `TypeExpr`-domain caller
/// above; that is a raw classification, not a raise-then-classify round-trip.)
/// The typed authority is held byte-for-byte in agreement with this recogniser
/// by the no-drift contract test pinned below — so the spelling has exactly one
/// home across the raw and typed surfaces.
#[must_use]
pub(crate) fn raw_is_unmaterialized_sentinel(raw: &str) -> bool {
    let is_exact_sentinel = matches!(
        raw,
        SEMANTIC_MISS
            | SEMANTIC_OBJECT_SURFACE
            | SEMANTIC_SURFACE_MEMBER
            | "semanticAliasCycle"
            | "semanticFunction"
            | "VueMacroElements"
            | "projectedOpenSurface"
    );
    let is_prefix_sentinel = raw.starts_with("materialize:")
        || raw.starts_with("unsupportedIntrinsic(")
        || raw.starts_with(BUDGET_EXCEEDED_SENTINEL_PREFIX)
        || raw.starts_with("unstableState(")
        || raw.starts_with("aliasCycle(");
    is_exact_sentinel || is_prefix_sentinel
}

/// The TYPED authority for "does this `QueryError`, when raised to a
/// `TypeExpr::Unknown { raw }`, read as UNMATERIALISED" — the node-domain
/// (typed) counterpart of [`raw_is_unmaterialized_sentinel`].
///
/// Classification is computed directly on the typed variant (no
/// synthesise-then-reclassify round-trip), so the node-domain `opaque_sentinel`
/// path never has to re-spell a sentinel string to decide materialisation. The
/// invariant — this typed answer equals
/// `raw_is_unmaterialized_sentinel(&semantic_query_error_raw(err))` for EVERY
/// variant — is the no-drift contract pinned by
/// `query_error_sentinel_classification_agrees_with_raw_recogniser` below.
///
/// [`QueryError::Other`] carries an arbitrary string, so it DELEGATES to the raw
/// recogniser on its raised spelling rather than asserting a fixed answer: its
/// raw IS its payload text (a caller-supplied string that CAN happen to be a
/// recognised sentinel, e.g. `Other("semanticMiss")`). Delegating makes the
/// no-drift invariant hold BY CONSTRUCTION for that payload, not just the benign
/// ones. [`QueryError::DeclPlaceholder`]'s raised raw is always the
/// `declPlaceholder(<name>)` wrapper, which is NOT in the recogniser set (no such
/// exact spelling or prefix), so it is provably `false` for every name and is
/// classified DIRECTLY (no allocation) — exactly as the sibling
/// [`query_error_is_object_surface_sentinel`] handles it.
#[must_use]
pub(in crate::project_semantic_dispatch) fn query_error_is_unmaterialized_sentinel(
    err: &QueryError,
) -> bool {
    match err {
        // Recognised sentinels (exact or prefix) ⇒ UNMATERIALISED.
        QueryError::Miss
        | QueryError::UnsupportedIntrinsic { .. }
        | QueryError::BudgetExceeded(_)
        | QueryError::UnstableState { .. }
        | QueryError::AliasCycle { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::UnrepresentableSurface
        | QueryError::UnrepresentableSurfaceMember
        | QueryError::VueMacroElementsPlaceholder => true,
        // Deliberately NOT in the recogniser set ⇒ MATERIALISED:
        // `<raise miss>` (RaiseMiss) and `semanticTypeParamCycle`
        // (TypeParamCycle) are carrier-arg / cycle placeholders the legacy
        // inline check treated as materialised; `recursiveRef(name)`
        // (RecursiveRef) raises to a materialised `RecursiveRef` leaf;
        // `valueDomainMismatch(...)` raises to a non-sentinel spelling.
        QueryError::RaiseMiss
        | QueryError::TypeParamCycle
        | QueryError::RecursiveRef { .. }
        | QueryError::ValueDomainMismatch { .. } => false,
        // `Other`'s raw IS its payload `text` (a caller-supplied string that CAN
        // happen to be a recognised sentinel, e.g. `Other("semanticMiss")`), so it
        // DELEGATES to the raw recogniser — the no-drift invariant holds by
        // construction even for an adversarial sentinel-looking payload.
        QueryError::Other(text) => raw_is_unmaterialized_sentinel(text),
        // `DeclPlaceholder`'s raised raw is always the `declPlaceholder(<name>)`
        // wrapper, and the recogniser set has NO `declPlaceholder(` exact spelling
        // or prefix (only `materialize:` / `unsupportedIntrinsic(` /
        // `budgetExceeded(` / `unstableState(` / `aliasCycle(` prefixes plus the
        // seven exact sentinels), so the wrapper is provably unrecognised for
        // EVERY name. Return `false` directly — delegating would only allocate the
        // wrapper string to re-derive this constant answer. Mirrors the direct
        // `DeclPlaceholder { .. } => false` arm in
        // [`query_error_is_object_surface_sentinel`]. The no-drift invariant is
        // pinned by `query_error_sentinel_classification_agrees_with_raw_recogniser`,
        // which includes a `DeclPlaceholder` whose name embeds a sentinel spelling.
        QueryError::DeclPlaceholder { .. } => false,
    }
}

/// The DOMAIN-NEUTRAL object-surface-sentinel predicate: `true` iff `err`'s
/// raised raw spelling IS the [`SEMANTIC_OBJECT_SURFACE`] sentinel — the carrier
/// the intersection reducer drops as a vacuous arm. By construction this equals
/// `semantic_query_error_raw(err) == SEMANTIC_OBJECT_SURFACE` for EVERY variant
/// (the same by-construction agreement [`query_error_is_unmaterialized_sentinel`]
/// holds with the raw recogniser): the typed [`QueryError::UnrepresentableSurface`]
/// carrier round-trips to that spelling, and the text-bearing
/// [`QueryError::Other`] arm DELEGATES to its payload text (a caller-supplied
/// string CAN spell the sentinel, e.g. `Other("semanticObjectSurface")`), so it
/// tags the object-surface sentinel exactly when the raw-string `summary::unknown`
/// tag rule would — never drifting on an adversarial sentinel-text payload.
/// [`QueryError::DeclPlaceholder`]'s raw is always `declPlaceholder(<name>)`,
/// which never equals the object-surface spelling, so it is `false` for every
/// name. This module owns sentinel/materialisation classification over
/// `QueryError` + raw strings and returns ONLY domain-neutral types (`bool`); the
/// node-domain `FactShapeTag` mapping that reads this predicate lives with
/// `FactShapeTag` itself, in `node_domain::summary::opaque_sentinel`. The match is
/// EXHAUSTIVE (no `_` wildcard) so a future `QueryError` variant is forced to
/// declare whether it is the object-surface sentinel.
#[must_use]
pub(in crate::project_semantic_dispatch) fn query_error_is_object_surface_sentinel(
    err: &QueryError,
) -> bool {
    match err {
        QueryError::UnrepresentableSurface => true,
        // `Other`'s raised raw IS its payload text — delegate (no allocation:
        // compare the borrowed text to the const directly) so the predicate is
        // exactly `raised-raw == SEMANTIC_OBJECT_SURFACE` even for an adversarial
        // sentinel-spelling payload.
        QueryError::Other(text) => text.as_ref() == SEMANTIC_OBJECT_SURFACE,
        // `declPlaceholder(<name>)` never equals the object-surface spelling.
        QueryError::DeclPlaceholder { .. } => false,
        QueryError::Miss
        | QueryError::UnsupportedIntrinsic { .. }
        | QueryError::BudgetExceeded(_)
        | QueryError::UnstableState { .. }
        | QueryError::AliasCycle { .. }
        | QueryError::RecursiveRef { .. }
        | QueryError::ValueDomainMismatch { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::TypeParamCycle
        | QueryError::RaiseMiss
        | QueryError::UnrepresentableSurfaceMember
        | QueryError::VueMacroElementsPlaceholder => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        query_error_is_object_surface_sentinel, query_error_is_unmaterialized_sentinel,
        raw_is_unmaterialized_sentinel, QueryError, SEMANTIC_OBJECT_SURFACE,
    };
    use crate::resolver_core::component_meta_query_engine::{
        semantic_query_error_raw, BUDGET_EXCEEDED_SENTINEL_PREFIX,
    };
    use crate::resolver_core::shallow_file_state::{BudgetDomain, BudgetExceededFailure};
    use crate::semantic_query::SemanticQueryValueTag;

    /// Every `QueryError` variant, so the agreement guard cannot silently miss a
    /// variant added later (the match is exhaustive at the call sites, but the
    /// fixture list is enumerated here for the round-trip cross-check).
    fn all_query_error_variants() -> Vec<QueryError> {
        vec![
            QueryError::Miss,
            QueryError::UnsupportedIntrinsic {
                name: Arc::from("Foo"),
            },
            QueryError::BudgetExceeded(BudgetExceededFailure {
                domain: BudgetDomain::ProjectionOperation,
                limit: 1,
                actual: 2,
                context: "sentinel-agreement-fixture".to_string(),
            }),
            QueryError::UnstableState { attempts: 3 },
            QueryError::AliasCycle {
                chain: Arc::from(vec![Arc::from("A"), Arc::from("B")].into_boxed_slice()),
            },
            QueryError::RecursiveRef {
                name: Arc::from("Tree"),
            },
            QueryError::Other(Arc::from("custom failure text")),
            QueryError::DeclPlaceholder {
                canonical_id: Arc::from("/w/p.ts"),
                name: Arc::from("Pending"),
                whole_hash: [0u8; 16],
            },
            QueryError::ValueDomainMismatch {
                expected: SemanticQueryValueTag::TypeNode,
                actual: SemanticQueryValueTag::Relation,
            },
            QueryError::RaiseAliasCycle,
            QueryError::TypeParamCycle,
            QueryError::RaiseMiss,
            QueryError::UnrepresentableSurface,
            QueryError::UnrepresentableSurfaceMember,
            QueryError::VueMacroElementsPlaceholder,
        ]
    }

    /// Adversarial text-bearing payloads whose CARRIED text (the thing
    /// `semantic_query_error_raw` raises them to) is itself a recognised sentinel
    /// spelling. Pre-delegation the typed authority hardcoded `Other(_) => false`
    /// / `DeclPlaceholder { .. } => false`, so it DISAGREED with the raw
    /// recogniser for these (the recogniser scores the raised spelling); after
    /// the text-bearing arms delegate, agreement holds by construction. Both an
    /// exact-sentinel payload (`semanticMiss` / `semanticObjectSurface` /
    /// `VueMacroElements`) and a prefix-sentinel payload (`budgetExceeded(x)`)
    /// are covered, plus a benign free-text payload that genuinely is NOT a
    /// sentinel (so the loop is not vacuously all-true), plus a `DeclPlaceholder`
    /// whose NAME embeds a sentinel string — proving the wrapper prefix
    /// (`declPlaceholder(...)`) keeps it materialised regardless of the name.
    fn adversarial_text_bearing_variants() -> Vec<QueryError> {
        vec![
            QueryError::Other(Arc::from("semanticMiss")),
            QueryError::Other(Arc::from("semanticObjectSurface")),
            QueryError::Other(Arc::from("budgetExceeded(x)")),
            QueryError::Other(Arc::from("VueMacroElements")),
            QueryError::Other(Arc::from("genuinely free text")),
            QueryError::DeclPlaceholder {
                canonical_id: Arc::from("/w/p.ts"),
                name: Arc::from("semanticMiss"),
                whole_hash: [0u8; 16],
            },
        ]
    }

    /// The no-drift contract: the TYPED authority
    /// (`query_error_is_unmaterialized_sentinel`) returns exactly what the RAW
    /// recogniser scores on the variant's round-tripped sentinel string, for
    /// EVERY variant — INCLUDING adversarial text-bearing payloads whose carried
    /// text is itself a sentinel spelling. A future edit that changed one
    /// classifier but not the other — or added a variant the typed match scored
    /// differently from its raw spelling — fails here. DISCRIMINATING: the
    /// adversarial set (e.g. `Other("semanticMiss")`) FAILS against the pre-fix
    /// `Other(_) => false` arm (the typed authority said materialised while the
    /// recogniser scores the raised `"semanticMiss"` as a sentinel), and the
    /// `_discriminates` block proves the typed authority is not a constant.
    #[test]
    fn query_error_sentinel_classification_agrees_with_raw_recogniser() {
        for variant in all_query_error_variants()
            .into_iter()
            .chain(adversarial_text_bearing_variants())
        {
            let typed = query_error_is_unmaterialized_sentinel(&variant);
            let raw = raw_is_unmaterialized_sentinel(&semantic_query_error_raw(&variant));
            assert_eq!(
                typed,
                raw,
                "typed sentinel classification must agree with the raw recogniser for {variant:?} \
                 (raw spelling = {:?})",
                semantic_query_error_raw(&variant)
            );
        }

        // _discriminates: the typed authority is not constant.
        assert!(
            query_error_is_unmaterialized_sentinel(&QueryError::UnrepresentableSurface),
            "UnrepresentableSurface must classify as an unmaterialised sentinel"
        );
        assert!(
            !query_error_is_unmaterialized_sentinel(&QueryError::RaiseMiss),
            "RaiseMiss (`<raise miss>`) is deliberately NOT a recognised sentinel"
        );
        assert!(
            !query_error_is_unmaterialized_sentinel(&QueryError::TypeParamCycle),
            "TypeParamCycle (`semanticTypeParamCycle`) is deliberately NOT recognised"
        );

        // The adversarial cases concretely: an `Other` carrying a sentinel
        // spelling classifies AS that sentinel (delegation), a benign one does
        // not, and a `DeclPlaceholder` stays materialised even when its name
        // looks like a sentinel (the `declPlaceholder(...)` wrapper is never a
        // recognised prefix).
        assert!(
            query_error_is_unmaterialized_sentinel(&QueryError::Other(Arc::from("semanticMiss"))),
            "Other(\"semanticMiss\") must delegate to the recogniser and read as a sentinel"
        );
        assert!(
            query_error_is_unmaterialized_sentinel(&QueryError::Other(Arc::from(
                "budgetExceeded(x)"
            ))),
            "Other(\"budgetExceeded(x)\") matches the budget prefix ⇒ a sentinel"
        );
        assert!(
            !query_error_is_unmaterialized_sentinel(&QueryError::Other(Arc::from(
                "genuinely free text"
            ))),
            "a benign Other payload is NOT a sentinel"
        );
        assert!(
            !query_error_is_unmaterialized_sentinel(&QueryError::DeclPlaceholder {
                canonical_id: Arc::from("/w/p.ts"),
                name: Arc::from("semanticMiss"),
                whole_hash: [0u8; 16],
            }),
            "DeclPlaceholder stays materialised even with a sentinel-looking name"
        );
    }

    /// The object-surface-sentinel no-drift contract: the TYPED predicate
    /// (`query_error_is_object_surface_sentinel`) returns exactly
    /// `semantic_query_error_raw(err) == SEMANTIC_OBJECT_SURFACE` for EVERY
    /// variant — the same by-construction agreement the `materialized` predicate
    /// holds with the raw recogniser, so the typed `tag` decision and the
    /// raw-string `summary::unknown` tag rule classify a sentinel identically.
    /// This is a DOMAIN-NEUTRAL `bool` predicate (no node-domain `FactShapeTag`
    /// here — the tag mapping that reads this lives with `FactShapeTag` in
    /// `node_domain::summary::opaque_sentinel`). DISCRIMINATING via the
    /// adversarial text-bearing payloads: an `Other` carrying the
    /// `semanticObjectSurface` SPELLING raises to that raw and so DELEGATES to a
    /// `true` verdict (matching the raw rule), while the typed
    /// `UnrepresentableSurface` carrier is the producer that natively round-trips
    /// to the spelling, and every other variant is `false`.
    #[test]
    fn query_error_object_surface_sentinel_agrees_with_raw_object_surface_spelling() {
        // The object-surface sentinel round-trips to this exact spelling, the one
        // the raw-string `summary::unknown` tag rule keys on.
        assert_eq!(
            semantic_query_error_raw(&QueryError::UnrepresentableSurface),
            SEMANTIC_OBJECT_SURFACE
        );

        for variant in all_query_error_variants()
            .into_iter()
            .chain(adversarial_text_bearing_variants())
        {
            let typed = query_error_is_object_surface_sentinel(&variant);
            let raw_is_object_surface =
                semantic_query_error_raw(&variant) == SEMANTIC_OBJECT_SURFACE;
            assert_eq!(
                typed,
                raw_is_object_surface,
                "object-surface-sentinel predicate must equal \
                 (semantic_query_error_raw(err) == SEMANTIC_OBJECT_SURFACE) for {variant:?} \
                 (raw spelling = {:?})",
                semantic_query_error_raw(&variant)
            );
        }

        // _discriminates: the predicate is not a constant, and it correctly
        // SEPARATES the producer carrier and an adversarial sentinel-text `Other`
        // from a benign `Other` / a non-sentinel variant.
        assert!(
            query_error_is_object_surface_sentinel(&QueryError::UnrepresentableSurface),
            "the UnrepresentableSurface carrier IS the object-surface sentinel"
        );
        assert!(
            query_error_is_object_surface_sentinel(&QueryError::Other(Arc::from(
                "semanticObjectSurface"
            ))),
            "Other(\"semanticObjectSurface\") delegates to its raised spelling ⇒ object-surface \
             sentinel (this is the case that drifted before the text-bearing delegation)"
        );
        assert!(
            !query_error_is_object_surface_sentinel(&QueryError::Other(Arc::from(
                "genuinely free text"
            ))),
            "a benign Other payload is NOT the object-surface sentinel"
        );
        assert!(
            !query_error_is_object_surface_sentinel(&QueryError::UnrepresentableSurfaceMember),
            "the surface-MEMBER carrier round-trips to SEMANTIC_SURFACE_MEMBER, not the \
             object-surface spelling"
        );
    }

    /// Behavioural pin for the budget-exceeded prefix: the classifier recognises
    /// any string CARRYING the `BUDGET_EXCEEDED_SENTINEL_PREFIX` prefix as an
    /// unmaterialised sentinel, and rejects a near-miss prefix. This runs the
    /// real classifier (the AST-level reference is pinned separately by
    /// `budget_exceeded_sentinel_prefix_is_pinned_and_in_parity`), so a regression
    /// that stopped honouring the prefix — or forked the spelling — fails here.
    /// DISCRIMINATING: a near-miss prefix and the bare verb (no `(`) must be
    /// classified as MATERIALISED.
    #[test]
    fn budget_exceeded_prefix_classifies_as_unmaterialized_sentinel() {
        assert!(
            raw_is_unmaterialized_sentinel(&format!("{BUDGET_EXCEEDED_SENTINEL_PREFIX}42)")),
            "a string carrying the budget-exceeded prefix must classify as an \
             unmaterialised sentinel"
        );
        assert!(
            !raw_is_unmaterialized_sentinel("budgetExceede("),
            "a near-miss prefix (missing the trailing `d`) must NOT classify as a sentinel"
        );
        assert!(
            !raw_is_unmaterialized_sentinel("budgetExceeded"),
            "the bare verb without the `(` boundary must NOT classify as a sentinel"
        );
    }
}
