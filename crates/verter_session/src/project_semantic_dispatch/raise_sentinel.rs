//! Typed sentinel classification over [`QueryError`] — the SOLE classification
//! surface of the raise subsystem.
//!
//! There is NO raw-string sentinel recogniser anywhere in the raise
//! subsystem: resolver degradation reaches the shape-engine fold as a TYPED
//! [`QueryError`] (`SemanticNodeData::Opaque` / the converted control arms),
//! and a genuine
//! [`UnknownValue`](verter_type_expr::UnknownValue) is NEVER a sentinel —
//! even when spelled identically to a legacy sentinel string. The
//! classification below runs on explicit [`QueryError`] variants only.
//!
//! Historical note: the legacy tree encoded degradation as sentinel
//! spellings inside `Unknown { raw }` and classified them back with a raw
//! recogniser (`raw_is_unmaterialized_sentinel`, deleted). The terminal
//! compatibility projection still emits the same spellings (wire/display/hash
//! bytes are unchanged), but they are inert text — the sidecar /
//! [`QueryError`] variant is the only control channel.

use crate::semantic_query::QueryError;

/// The TYPED authority for "does this `QueryError` read as UNMATERIALISED" —
/// the node-domain counterpart of the (deleted) raw recogniser.
///
/// Classification is computed directly on the typed variant (no
/// synthesise-then-reclassify round-trip). [`QueryError::Other`] carries an
/// arbitrary caller-supplied string that is NEVER inspected: an
/// `Other("semanticMiss")` payload is inert text, NOT a sentinel — only the
/// typed [`QueryError::Miss`] variant is the miss. The match is EXHAUSTIVE
/// (no `_` wildcard) so a future `QueryError` variant is forced to declare
/// its materialisation class.
#[must_use]
pub(in crate::project_semantic_dispatch) fn query_error_is_unmaterialized_sentinel(
    err: &QueryError,
) -> bool {
    match err {
        // Recognised sentinels ⇒ UNMATERIALISED.
        QueryError::Miss
        | QueryError::UnsupportedIntrinsic { .. }
        | QueryError::BudgetExceeded(_)
        | QueryError::SignatureOverflow
        | QueryError::StaleSemanticOperand
        | QueryError::IncompleteSemanticOperand { .. }
        | QueryError::Cancelled
        | QueryError::UnstableState { .. }
        | QueryError::AliasCycle { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::OpenSurface
        | QueryError::UnrepresentableSurface
        // A position the flow substrate cannot model never materialised
        // a value.
        | QueryError::UnmodeledPosition
        | QueryError::UnrepresentableSurfaceMember => true,
        // Deliberately NOT unmaterialised: `<raise miss>` (RaiseMiss) and
        // `semanticTypeParamCycle` (TypeParamCycle) are carrier-arg / cycle
        // placeholders the legacy inline check treated as materialised;
        // `recursiveRef(name)` (RecursiveRef) raises to a materialised
        // `RecursiveRef` leaf; `valueDomainMismatch(...)` raises to a
        // non-sentinel spelling; `DeclPlaceholder` raises to the named `Ref`
        // shell; `Other(..)` is inert caller text, never a sentinel. A
        // FOREIGN operand (`ForeignSemanticOperand`) is likewise NOT an
        // unmaterialised hole: the disposition authority classifies it
        // `Failure`/`Fault` — a caller reached the boundary with an operand
        // minted by another store/generation, which is a hard fault a
        // consumer must observe, not a partially-materialised value to fold
        // into a hole (unlike the genuinely incomplete forces
        // `Stale`/`Incomplete` above).
        QueryError::RaiseMiss
        | QueryError::TypeParamCycle
        | QueryError::RecursiveRef { .. }
        | QueryError::ValueDomainMismatch { .. }
        | QueryError::ForeignSemanticOperand
        | QueryError::Other(_)
        | QueryError::DeclPlaceholder { .. } => false,
    }
}

/// The DOMAIN-NEUTRAL object-surface-sentinel predicate: `true` iff `err` IS
/// the typed [`QueryError::UnrepresentableSurface`] carrier — the arm the
/// intersection reducer drops as vacuous. ONLY the typed variant triggers
/// removal: a `QueryError::Other("semanticObjectSurface")` payload NEVER acts
/// as the sentinel, and a genuine `UnknownValue` spelled identically is never
/// dropped either. The match is EXHAUSTIVE (no `_` wildcard) so a future
/// `QueryError` variant is forced to declare whether it is the object-surface
/// sentinel.
#[must_use]
pub(in crate::project_semantic_dispatch) fn query_error_is_object_surface_sentinel(
    err: &QueryError,
) -> bool {
    match err {
        QueryError::UnrepresentableSurface => true,
        QueryError::Miss
        | QueryError::UnsupportedIntrinsic { .. }
        | QueryError::BudgetExceeded(_)
        | QueryError::SignatureOverflow
        | QueryError::ForeignSemanticOperand
        | QueryError::StaleSemanticOperand
        | QueryError::IncompleteSemanticOperand { .. }
        | QueryError::Cancelled
        | QueryError::UnstableState { .. }
        | QueryError::AliasCycle { .. }
        | QueryError::RecursiveRef { .. }
        | QueryError::ValueDomainMismatch { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::TypeParamCycle
        | QueryError::RaiseMiss
        | QueryError::OpenSurface
        | QueryError::Other(_)
        | QueryError::DeclPlaceholder { .. }
        | QueryError::UnmodeledPosition
        | QueryError::UnrepresentableSurfaceMember => false,
    }
}

/// The DOMAIN-NEUTRAL semantic-MISS-sentinel predicate: `true` iff `err` IS
/// the typed [`QueryError::Miss`] carrier — the SINGLE sentinel the
/// published-operator predicate suppresses. NARROWER than
/// [`query_error_is_unmaterialized_sentinel`] (which is `true` for
/// object-surface / surface-member / budget / cycle / … carriers too); an
/// `Other("semanticMiss")` payload is NEVER the miss sentinel. The match is
/// EXHAUSTIVE (no `_` wildcard) so a future `QueryError` variant must declare
/// whether it is the miss sentinel.
#[must_use]
pub(in crate::project_semantic_dispatch) fn query_error_is_semantic_miss_sentinel(
    err: &QueryError,
) -> bool {
    match err {
        QueryError::Miss => true,
        QueryError::UnsupportedIntrinsic { .. }
        | QueryError::BudgetExceeded(_)
        | QueryError::SignatureOverflow
        | QueryError::ForeignSemanticOperand
        | QueryError::StaleSemanticOperand
        | QueryError::IncompleteSemanticOperand { .. }
        | QueryError::Cancelled
        | QueryError::UnstableState { .. }
        | QueryError::AliasCycle { .. }
        | QueryError::RecursiveRef { .. }
        | QueryError::ValueDomainMismatch { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::TypeParamCycle
        | QueryError::RaiseMiss
        | QueryError::OpenSurface
        | QueryError::Other(_)
        | QueryError::DeclPlaceholder { .. }
        | QueryError::UnrepresentableSurface
        // NOT the miss sentinel: the flow marker is a DISTINCT carrier
        // precisely so a consumer keyed on `Miss` cannot mistake it for
        // one.
        | QueryError::UnmodeledPosition
        | QueryError::UnrepresentableSurfaceMember => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        query_error_is_object_surface_sentinel, query_error_is_semantic_miss_sentinel,
        query_error_is_unmaterialized_sentinel, QueryError,
    };
    use crate::resolver_core::component_meta_query_engine::{
        semantic_query_error_raw, SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE,
    };
    use crate::resolver_core::shallow_file_state::{BudgetDomain, BudgetExceededFailure};
    use crate::semantic_query::SemanticQueryValueTag;

    /// Every `QueryError` variant, so the classification pins cannot silently
    /// miss a variant added later (the matches are exhaustive at the call
    /// sites; the fixture list is enumerated here for the round-trip
    /// cross-check). ADD NEW VARIANTS HERE when extending `QueryError` (the
    /// sibling hash-tag fixture in `semantic_query.rs` carries the same nudge).
    fn all_query_error_variants() -> Vec<QueryError> {
        vec![
            QueryError::Miss,
            QueryError::Cancelled,
            QueryError::UnsupportedIntrinsic {
                name: Arc::from("Foo"),
            },
            QueryError::BudgetExceeded(BudgetExceededFailure {
                domain: BudgetDomain::ProjectionOperation,
                limit: 1,
                actual: 2,
                context: "sentinel-classification-fixture".to_string(),
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
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
            QueryError::OpenSurface,
            QueryError::UnrepresentableSurface,
            QueryError::UnrepresentableSurfaceMember,
            QueryError::SignatureOverflow,
            QueryError::ForeignSemanticOperand,
            QueryError::StaleSemanticOperand,
            QueryError::IncompleteSemanticOperand {
                reasons: crate::semantic_query::PartialReasonSet::empty(),
            },
        ]
    }

    /// Adversarial text-bearing payloads whose CARRIED text is itself a
    /// legacy sentinel spelling. These NEVER classify as
    /// sentinels: only the typed variant carries control meaning.
    fn adversarial_text_bearing_variants() -> Vec<QueryError> {
        vec![
            QueryError::Other(Arc::from("semanticMiss")),
            QueryError::Other(Arc::from("semanticObjectSurface")),
            QueryError::Other(Arc::from("budgetExceeded(x)")),
            QueryError::Other(Arc::from("projectedOpenSurface")),
            QueryError::Other(Arc::from("genuinely free text")),
        ]
    }

    /// The typed unmaterialised authority: exactly the typed sentinel
    /// carriers classify unmaterialised; EVERY text-bearing `Other` payload —
    /// even one spelled identically to a legacy sentinel — is MATERIALISED.
    /// The unmaterialised set agrees with the disposition authority's
    /// partial/absence classes; a `Failure`/`Fault` variant (a foreign
    /// operand) is a hard fault, never a foldable hole.
    #[test]
    fn unmaterialized_classification_is_typed_only() {
        let expected_unmaterialized = |err: &QueryError| {
            matches!(
                err,
                QueryError::Miss
                    | QueryError::UnsupportedIntrinsic { .. }
                    | QueryError::BudgetExceeded(_)
                    | QueryError::SignatureOverflow
                    | QueryError::StaleSemanticOperand
                    | QueryError::IncompleteSemanticOperand { .. }
                    | QueryError::Cancelled
                    | QueryError::UnstableState { .. }
                    | QueryError::AliasCycle { .. }
                    | QueryError::RaiseAliasCycle
                    | QueryError::OpenSurface
                    | QueryError::UnrepresentableSurface
                    | QueryError::UnrepresentableSurfaceMember
            )
        };
        for variant in all_query_error_variants()
            .into_iter()
            .chain(adversarial_text_bearing_variants())
        {
            assert_eq!(
                query_error_is_unmaterialized_sentinel(&variant),
                expected_unmaterialized(&variant),
                "typed-only unmaterialised classification for {variant:?}"
            );
        }

        // _discriminates: the authority is not constant.
        assert!(query_error_is_unmaterialized_sentinel(
            &QueryError::UnrepresentableSurface
        ));
        assert!(!query_error_is_unmaterialized_sentinel(
            &QueryError::RaiseMiss
        ));
        assert!(!query_error_is_unmaterialized_sentinel(
            &QueryError::ForeignSemanticOperand
        ));
        assert!(!query_error_is_unmaterialized_sentinel(&QueryError::Other(
            Arc::from("semanticMiss")
        )));
    }

    /// The object-surface-sentinel predicate: ONLY the typed
    /// `UnrepresentableSurface` carrier triggers intersection arm removal —
    /// an `Other("semanticObjectSurface")` payload NEVER does (the flipped
    /// legacy equation).
    #[test]
    fn object_surface_sentinel_is_only_the_typed_carrier() {
        assert_eq!(
            semantic_query_error_raw(&QueryError::UnrepresentableSurface),
            SEMANTIC_OBJECT_SURFACE,
            "the terminal projection keeps the legacy spelling"
        );

        for variant in all_query_error_variants()
            .into_iter()
            .chain(adversarial_text_bearing_variants())
        {
            let expected = matches!(variant, QueryError::UnrepresentableSurface);
            assert_eq!(
                query_error_is_object_surface_sentinel(&variant),
                expected,
                "only the typed UnrepresentableSurface carrier is the object-surface sentinel ({variant:?})"
            );
        }

        // _discriminates + the flipped equation.
        assert!(query_error_is_object_surface_sentinel(
            &QueryError::UnrepresentableSurface
        ));
        assert!(
            !query_error_is_object_surface_sentinel(&QueryError::Other(Arc::from(
                "semanticObjectSurface"
            ))),
            "Other(\"semanticObjectSurface\") NEVER acts as the surface sentinel"
        );
        assert!(!query_error_is_object_surface_sentinel(
            &QueryError::UnrepresentableSurfaceMember
        ));
    }

    /// The semantic-miss-sentinel predicate: ONLY the typed `Miss` carrier —
    /// an `Other("semanticMiss")` payload is never the miss sentinel.
    #[test]
    fn semantic_miss_sentinel_is_only_the_typed_carrier() {
        assert_eq!(
            semantic_query_error_raw(&QueryError::Miss),
            SEMANTIC_MISS,
            "the terminal projection keeps the legacy spelling"
        );

        for variant in all_query_error_variants()
            .into_iter()
            .chain(adversarial_text_bearing_variants())
        {
            let expected = matches!(variant, QueryError::Miss);
            assert_eq!(
                query_error_is_semantic_miss_sentinel(&variant),
                expected,
                "only the typed Miss carrier is the miss sentinel ({variant:?})"
            );
        }

        // _discriminates: unmaterialised is BROADER than miss.
        assert!(query_error_is_semantic_miss_sentinel(&QueryError::Miss));
        assert!(
            !query_error_is_semantic_miss_sentinel(&QueryError::Other(Arc::from("semanticMiss"))),
            "Other(\"semanticMiss\") NEVER acts as the miss sentinel"
        );
        assert!(!query_error_is_semantic_miss_sentinel(
            &QueryError::UnrepresentableSurface
        ));
        assert!(query_error_is_unmaterialized_sentinel(
            &QueryError::UnrepresentableSurface
        ));
    }

    /// The `OpenSurface` placeholder: an unmaterialised degradation (the
    /// legacy `projectedOpenSurface` spelling) that is NEITHER the
    /// object-surface sentinel NOR the miss sentinel.
    #[test]
    fn open_surface_is_unmaterialized_but_neither_narrow_sentinel() {
        assert_eq!(
            semantic_query_error_raw(&QueryError::OpenSurface),
            "projectedOpenSurface",
            "the terminal projection keeps the legacy spelling"
        );
        assert!(query_error_is_unmaterialized_sentinel(
            &QueryError::OpenSurface
        ));
        assert!(!query_error_is_object_surface_sentinel(
            &QueryError::OpenSurface
        ));
        assert!(!query_error_is_semantic_miss_sentinel(
            &QueryError::OpenSurface
        ));
    }
}
