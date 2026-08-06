//! Per-variant coverage of the single [`QueryError`] disposition authority.
//!
//! Every one of the 15 variants appears in [`every_variant`], and the suite
//! asserts each variant's disposition, its published reason, and the derived
//! predicates. The inventory is anti-vacuity-checked: a duplicate or a changed
//! variant count fails `every_variant_is_covered_exactly_once`.

use std::sync::Arc;

use verter_type_expr::{ClosedLiteralDomainUnresolvedReason, ReactiveWrapperUnresolvedReason};

use super::{classify_query_error, query_error_disposition, QueryErrorDisposition};
use crate::resolver_core::{BudgetDomain, BudgetExceededFailure};
use crate::semantic_query::{QueryError, SemanticQueryValueTag};

fn budget_failure() -> BudgetExceededFailure {
    BudgetExceededFailure {
        domain: BudgetDomain::SolverResolveSteps,
        limit: 10,
        actual: 11,
        context: "disposition fixture".to_string(),
    }
}

/// One row per `QueryError` variant, in declaration order.
fn every_variant() -> Vec<QueryError> {
    vec![
        QueryError::Miss,
        QueryError::UnsupportedIntrinsic {
            name: Arc::from("NoSuchIntrinsic"),
        },
        QueryError::BudgetExceeded(budget_failure()),
        QueryError::Cancelled,
        QueryError::UnstableState { attempts: 3 },
        QueryError::AliasCycle {
            chain: Arc::from(vec![Arc::<str>::from("A"), Arc::<str>::from("B")].into_boxed_slice()),
        },
        QueryError::RecursiveRef {
            name: Arc::from("TreeNode"),
        },
        QueryError::Other(Arc::from("boom")),
        QueryError::DeclPlaceholder {
            canonical_id: Arc::from("/a.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            name: Arc::from("Foo"),
            whole_hash: Default::default(),
        },
        QueryError::ValueDomainMismatch {
            expected: SemanticQueryValueTag::TypeNode,
            actual: SemanticQueryValueTag::OverloadSet,
        },
        QueryError::RaiseAliasCycle,
        QueryError::TypeParamCycle,
        QueryError::RaiseMiss,
        QueryError::UnrepresentableSurface,
        QueryError::UnrepresentableSurfaceMember,
    ]
}

#[test]
fn every_variant_is_covered_exactly_once() {
    let rows = every_variant();
    let mut seen: Vec<String> = rows
        .iter()
        .map(|err| format!("{:?}", std::mem::discriminant(err)))
        .collect();
    let before = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen.len(),
        before,
        "the fixture inventory lists a variant twice"
    );
    // The table has exactly 15 rows. A new variant must be added here AND
    // given a disposition; this pins the count so neither is silently skipped.
    assert_eq!(
        rows.len(),
        15,
        "expected the 15-row QueryError disposition table; update this fixture AND \
         classify_query_error together"
    );
}

/// `Miss` and `RaiseMiss` — and ONLY those two — are absence-eligible.
#[test]
fn only_miss_and_raise_miss_are_optional_absence() {
    for err in every_variant() {
        let is_absence = query_error_disposition(&err) == QueryErrorDisposition::OptionalAbsence;
        let expected = matches!(err, QueryError::Miss | QueryError::RaiseMiss);
        assert_eq!(
            is_absence,
            expected,
            "{err:?} must{} be optional-absence",
            if expected { "" } else { " NOT" }
        );
    }
}

/// `RecursiveRef` raises AS recursion: it is a publishable carrier, never a
/// failure, never absence.
#[test]
fn recursive_ref_raises_as_recursion() {
    let err = QueryError::RecursiveRef {
        name: Arc::from("TreeNode"),
    };
    let class = classify_query_error(&err);
    assert_eq!(class.disposition, QueryErrorDisposition::RecursionCarrier);
    assert!(
        !class.disposition.is_unknown_materializing(),
        "a recursive reference materializes as a real published shape, not an Unknown shell"
    );
    assert!(!class.disposition.is_error_type());
}

/// `DeclPlaceholder` is the expandable `Instantiate` carrier — and is never
/// published as "not found" (`MissingDependency`).
#[test]
fn decl_placeholder_is_expandable_and_never_missing_dependency() {
    let err = QueryError::DeclPlaceholder {
        canonical_id: Arc::from("/a.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        name: Arc::from("Foo"),
        whole_hash: Default::default(),
    };
    let class = classify_query_error(&err);
    assert_eq!(class.disposition, QueryErrorDisposition::ExpandableDecl);
    assert!(
        !class.disposition.is_unknown_materializing(),
        "the declaration placeholder raises to the named Ref shell"
    );
    assert_ne!(
        class.domain_reason,
        ClosedLiteralDomainUnresolvedReason::MissingDependency,
        "a resolved-but-unmaterialized declaration is NOT a missing dependency — publishing \
         that is exactly the \"not found\" answer the disposition table forbids"
    );
    assert_eq!(
        class.domain_reason,
        ClosedLiteralDomainUnresolvedReason::Fault
    );
}

/// The three cycle sentinels stay typed control carriers.
#[test]
fn cycle_sentinels_remain_control_carriers() {
    for err in [
        QueryError::AliasCycle {
            chain: Arc::from(vec![Arc::<str>::from("A")].into_boxed_slice()),
        },
        QueryError::RaiseAliasCycle,
        QueryError::TypeParamCycle,
    ] {
        let class = classify_query_error(&err);
        assert_eq!(
            class.disposition,
            QueryErrorDisposition::ControlCarrier,
            "{err:?}"
        );
        assert_eq!(
            class.domain_reason,
            ClosedLiteralDomainUnresolvedReason::Cycle,
            "{err:?}"
        );
        assert!(!class.disposition.is_error_type(), "{err:?}");
    }
}

/// An `AliasCycle`'s participant chain survives classification — the
/// classifier reads it, never rewrites or drops it.
#[test]
fn alias_cycle_preserves_its_participant_chain() {
    let chain: Arc<[Arc<str>]> =
        Arc::from(vec![Arc::<str>::from("A"), Arc::<str>::from("B")].into_boxed_slice());
    let err = QueryError::AliasCycle {
        chain: Arc::clone(&chain),
    };
    assert_eq!(
        classify_query_error(&err).disposition,
        QueryErrorDisposition::ControlCarrier
    );
    let QueryError::AliasCycle { chain: observed } = &err else {
        panic!("variant changed");
    };
    assert_eq!(observed.as_ref(), chain.as_ref());
}

/// The three resource / fence signals are typed PARTIALS, and each publishes
/// its OWN reason — a budget trip, a cancellation and a fence give-up are
/// three distinct diagnostics, not one.
#[test]
fn partials_keep_distinct_reasons() {
    let rows = [
        (
            QueryError::BudgetExceeded(budget_failure()),
            ClosedLiteralDomainUnresolvedReason::BudgetExceeded,
        ),
        (
            QueryError::Cancelled,
            ClosedLiteralDomainUnresolvedReason::Cancelled,
        ),
        (
            QueryError::UnstableState { attempts: 3 },
            ClosedLiteralDomainUnresolvedReason::RevisionMismatch,
        ),
    ];
    let mut reasons = Vec::new();
    for (err, expected_reason) in rows {
        let class = classify_query_error(&err);
        assert_eq!(class.disposition, QueryErrorDisposition::Partial, "{err:?}");
        assert_eq!(class.domain_reason, expected_reason, "{err:?}");
        assert!(
            !class.disposition.is_error_type(),
            "{err:?} is a partial, not the error type"
        );
        reasons.push(class.domain_reason);
    }
    reasons.dedup();
    assert_eq!(
        reasons.len(),
        3,
        "the three partials must stay distinguishable"
    );
}

/// `UnstableState` is a typed PARTIAL, not a genuine fault.
#[test]
fn unstable_state_is_a_partial_not_a_fault() {
    let class = classify_query_error(&QueryError::UnstableState { attempts: 3 });
    assert_eq!(class.disposition, QueryErrorDisposition::Partial);
    assert_ne!(
        class.domain_reason,
        ClosedLiteralDomainUnresolvedReason::Fault,
        "the completion fence gave up because the observed inputs kept moving — a revision \
         mismatch, not a fault in the query"
    );
}

/// The three genuine failures are the §22 error type.
#[test]
fn genuine_failures_are_the_error_type() {
    for err in [
        QueryError::Other(Arc::from("boom")),
        QueryError::UnsupportedIntrinsic {
            name: Arc::from("NoSuchIntrinsic"),
        },
        QueryError::ValueDomainMismatch {
            expected: SemanticQueryValueTag::TypeNode,
            actual: SemanticQueryValueTag::OverloadSet,
        },
    ] {
        let class = classify_query_error(&err);
        assert_eq!(class.disposition, QueryErrorDisposition::Failure, "{err:?}");
        assert!(class.disposition.is_error_type(), "{err:?}");
        assert!(class.disposition.is_unknown_materializing(), "{err:?}");
    }
}

/// The two unsupported-surface sentinels keep their own class and their
/// existing `Unsupported` output reason.
#[test]
fn unsupported_surface_sentinels_keep_their_output_semantics() {
    for err in [
        QueryError::UnrepresentableSurface,
        QueryError::UnrepresentableSurfaceMember,
    ] {
        let class = classify_query_error(&err);
        assert_eq!(
            class.disposition,
            QueryErrorDisposition::UnsupportedSurface,
            "{err:?}"
        );
        assert_eq!(
            class.domain_reason,
            ClosedLiteralDomainUnresolvedReason::Unsupported,
            "{err:?}"
        );
        assert!(class.disposition.is_unknown_materializing(), "{err:?}");
    }
}

/// The §22 error-type predicate is EXACTLY the `Failure` disposition — the
/// derivation `QueryError::is_error_type` now routes through.
#[test]
fn error_type_predicate_is_exactly_the_failure_disposition() {
    for err in every_variant() {
        let expected = matches!(
            err,
            QueryError::Other(_)
                | QueryError::UnsupportedIntrinsic { .. }
                | QueryError::ValueDomainMismatch { .. }
        );
        assert_eq!(
            query_error_disposition(&err).is_error_type(),
            expected,
            "{err:?}"
        );
    }
}

/// The unknown-materializing predicate excludes EXACTLY the two publishable
/// carriers — the rule `node_is_unknown_materializing_failure` now derives.
#[test]
fn unknown_materializing_excludes_exactly_the_two_publishable_carriers() {
    for err in every_variant() {
        let publishable = matches!(
            err,
            QueryError::RecursiveRef { .. } | QueryError::DeclPlaceholder { .. }
        );
        assert_eq!(
            query_error_disposition(&err).is_unknown_materializing(),
            !publishable,
            "{err:?}"
        );
    }
}

/// The wrapper reason mirrors the domain reason for every variant — one
/// mapping, no per-variant re-inspection.
#[test]
fn wrapper_reason_mirrors_domain_reason_for_every_variant() {
    fn mirror(reason: ClosedLiteralDomainUnresolvedReason) -> ReactiveWrapperUnresolvedReason {
        match reason {
            ClosedLiteralDomainUnresolvedReason::AnalysisUnavailable => {
                ReactiveWrapperUnresolvedReason::AnalysisUnavailable
            }
            ClosedLiteralDomainUnresolvedReason::RevisionMismatch => {
                ReactiveWrapperUnresolvedReason::RevisionMismatch
            }
            ClosedLiteralDomainUnresolvedReason::MissingDependency => {
                ReactiveWrapperUnresolvedReason::MissingDependency
            }
            ClosedLiteralDomainUnresolvedReason::Cycle => ReactiveWrapperUnresolvedReason::Cycle,
            ClosedLiteralDomainUnresolvedReason::BudgetExceeded => {
                ReactiveWrapperUnresolvedReason::BudgetExceeded
            }
            ClosedLiteralDomainUnresolvedReason::WorkLimitExceeded => {
                ReactiveWrapperUnresolvedReason::WorkLimitExceeded
            }
            ClosedLiteralDomainUnresolvedReason::Cancelled => {
                ReactiveWrapperUnresolvedReason::Cancelled
            }
            ClosedLiteralDomainUnresolvedReason::Unsupported => {
                ReactiveWrapperUnresolvedReason::Unsupported
            }
            ClosedLiteralDomainUnresolvedReason::Fault => ReactiveWrapperUnresolvedReason::Fault,
        }
    }
    for err in every_variant() {
        let class = classify_query_error(&err);
        assert_eq!(
            class.wrapper_reason(),
            mirror(class.domain_reason),
            "{err:?}"
        );
    }
}
