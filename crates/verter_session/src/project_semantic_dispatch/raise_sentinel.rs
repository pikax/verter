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
use verter_type_expr::TypeExpr;

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
        // `Stale`/`Incomplete` above). A checker recovery raises to its
        // recovery type — a materialised value.
        QueryError::RaiseMiss
        | QueryError::CheckerRecovery { .. }
        | QueryError::TypeParamCycle
        | QueryError::RecursiveRef { .. }
        | QueryError::ValueDomainMismatch { .. }
        | QueryError::ForeignSemanticOperand
        | QueryError::Other(_)
        | QueryError::PermissiveWildcard
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
        | QueryError::PermissiveWildcard
        | QueryError::DeclPlaceholder { .. }
        | QueryError::UnmodeledPosition
        | QueryError::CheckerRecovery { .. }
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
        | QueryError::PermissiveWildcard
        | QueryError::DeclPlaceholder { .. }
        | QueryError::UnrepresentableSurface
        // NOT the miss sentinel: the flow marker is a DISTINCT carrier
        // precisely so a consumer keyed on `Miss` cannot mistake it for
        // one.
        | QueryError::UnmodeledPosition
        | QueryError::CheckerRecovery { .. }
        | QueryError::UnrepresentableSurfaceMember => false,
    }
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "TypeExpr parity oracle for the node-domain materialized fact; \
                  production gates read the shape-engine node facts"
    )
)]
fn dispatch_route_expr_is_materialized(expr: &TypeExpr) -> bool {
    match expr {
        // A genuine `UnknownValue` is ALWAYS materialised — there is no
        // raw sentinel classification anywhere. Degradation reaches the
        // node domain as a typed `QueryError` (classified there), and the
        // compat tree's projection leaves are inert text to this oracle.
        TypeExpr::Unknown(_) => true,
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => dispatch_route_expr_is_materialized(element),
        // A DEFERRED compiler operation: the application itself has not
        // reduced, so the route is not materialized regardless of how
        // materialized its operands are.
        TypeExpr::IntrinsicApplication { .. } => false,
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| dispatch_route_expr_is_materialized(&element.ty)),
        TypeExpr::Object(object) => object.properties.iter().all(|member| match member {
            verter_type_expr::ObjectMember::Property(property) => {
                dispatch_route_expr_is_materialized(&property.ty)
            }
            verter_type_expr::ObjectMember::Spread(spread) => {
                dispatch_route_expr_is_materialized(&spread.ty)
            }
            verter_type_expr::ObjectMember::Method(method) => {
                method
                    .function
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && method
                        .function
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_type_expr::ObjectMember::CallSignature(signature)
            | verter_type_expr::ObjectMember::ConstructSignature(signature) => {
                signature
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && signature
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_type_expr::ObjectMember::IndexSignature(signature) => {
                dispatch_route_expr_is_materialized(&signature.key_type)
                    && dispatch_route_expr_is_materialized(&signature.value_type)
            }
        }),
        // A constructor type's signature is checked identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            function
                .return_type
                .as_deref()
                .is_none_or(dispatch_route_expr_is_materialized)
                && function
                    .parameters
                    .iter()
                    .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
        }
        TypeExpr::IndexedAccess { object, index } => {
            dispatch_route_expr_is_materialized(object)
                && dispatch_route_expr_is_materialized(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            dispatch_route_expr_is_materialized(check)
                && dispatch_route_expr_is_materialized(extends)
                && dispatch_route_expr_is_materialized(true_type)
                && dispatch_route_expr_is_materialized(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            dispatch_route_expr_is_materialized(source)
                && dispatch_route_expr_is_materialized(value)
                && name_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Infer { .. }
        // Synthetic carriers are fully materialised at the projector
        // surface — they ARE the published leaf, not a deferred token.
        | TypeExpr::SyntheticSlotBinding(_)
        // An import-type is a published shallow carrier (like a bare `Ref`),
        // not an unmaterialised dispatch sentinel — count it as materialised.
        | TypeExpr::ImportType { .. }
        | TypeExpr::RecursiveRef { .. } => true,
    }
}

/// Detects sentinel tokens emitted by the `shape_engine::fold_node`
/// materialisation algebra when dispatch cannot materialise a node — the
/// whole-tree `TypeExpr`-domain miss walk.
///
/// DISPLAY/PARITY oracle only, NOT a production semantic gate: production
/// reads the node-domain whole-tree miss fact
/// (`node_contains_semantic_miss_with_dispatch`, the typed
/// `!RaisedShapeFacts.materialized` projection) off the shape-engine fold.
/// This `TypeExpr` predicate survives as the oracle the raised-shape parity
/// suite compares that node fact against.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "TypeExpr parity oracle for the node-domain whole-tree miss fact; \
                  production gates read node_contains_semantic_miss_with_dispatch"
    )
)]
pub(crate) fn type_expr_contains_semantic_miss(expr: &TypeExpr) -> bool {
    !dispatch_route_expr_is_materialized(expr)
}

/// Root-level (carrier-position) unmaterialised-sentinel recogniser.
///
/// Returns `true` when the expression IS a raise sentinel at its root
/// (unwrapping only `Parenthesized`) — the shape produced when a
/// published carrier is re-lowered by NAME in a scope where the name
/// does not resolve, so the demanded reduction itself failed. Distinct
/// from [`type_expr_contains_semantic_miss`], which also fires on
/// genuine NESTED partial values: an unresolvable member-value
/// reference (`element?: HTMLElement` without the DOM lib) inside an
/// otherwise-materialised surface is a contract-conformant partial
/// result (Macro Type Traversal — the field that transitively depends
/// on the unresolved name publishes partially; sibling members resolve
/// normally), not a failed reduction.
///
/// Production reads the node-domain root-sentinel fact
/// (`node_root_is_unmaterialized_sentinel_with_dispatch`); this `TypeExpr`
/// predicate survives ONLY as the `#[cfg(test)]` INERT-DISAGREEMENT WITNESS the
/// split-parity suite pins as always-false (the compat tree carries no
/// classification — the typed sidecar is the only channel).
#[cfg(test)]
pub(crate) fn type_expr_root_is_unmaterialized_sentinel(expr: &TypeExpr) -> bool {
    let mut current = expr;
    while let TypeExpr::Parenthesized(inner) = current {
        current = inner;
    }
    match current {
        TypeExpr::Unknown(_) => !dispatch_route_expr_is_materialized(current),
        _ => false,
    }
}

/// Returns `true` when `expr` still carries open deferred shell shapes
/// (`KeyOf`, `IndexedAccess`, `Mapped`, `TypeOf`, `Conditional`) that
/// indicate dispatch could not structurally expand the surface further.
//
// The node-domain `expanded_surface` fact (computed bottom-up in `shape_engine`)
// now drives every production gate; this `TypeExpr` predicate survives ONLY as
// the `#[cfg(test)]` parity ORACLE the raised-shape suite compares the bottom-up
// fact against, so the non-test build sees it as dead.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "TypeExpr parity oracle for the bottom-up expanded_surface fact; \
                  production gates read the node-domain fact via shape_engine"
    )
)]
pub(crate) fn type_expr_is_expanded_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::KeyOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. } => false,
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(type_expr_is_expanded_surface)
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        query_error_is_object_surface_sentinel, query_error_is_semantic_miss_sentinel,
        query_error_is_unmaterialized_sentinel, QueryError,
    };
    use crate::semantic_query::compat_spelling::{
        semantic_query_error_raw, SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE,
    };
    use crate::semantic_query::SemanticQueryValueTag;
    use verter_session_query::inputs::budget::{BudgetDomain, BudgetExceededFailure};

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
                args: std::sync::Arc::from([]),
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
            QueryError::PermissiveWildcard,
            QueryError::CheckerRecovery {
                diagnostic: crate::semantic_query::CheckerDiagnostic {
                    code:
                        crate::semantic_query::CheckerDiagnosticCode::RecursiveFulfillmentCallback,
                    operation: crate::semantic_query::CheckerDiagnosticOperation::AwaitOperand,
                },
                basis: crate::semantic_query::RecoveryBasis::Certified,
                origin: None,
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
