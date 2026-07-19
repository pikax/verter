//! Tri-state + no-materialize fixtures for the fact-native KEY-DOMAIN
//! closedness evaluator (`raise::prepared_decl_body_is_closed` /
//! `raise::prepared_instantiation_key_domain_is_closed` /
//! `raise::userland_instantiation_body_is_closed_object`).
//!
//! The evaluator reads the producer-minted `KeyDomainClosednessFact`
//! (compact recipes + the closed-object SHAPE verdict) and answers
//! TRI-STATE: `ProvenClosed` / `ProvenOpen` / `Unavailable`. These fixtures
//! discriminate the three arms AND the no-poison rails:
//!
//! - a proven-CLOSED source reports `ProvenClosed`;
//! - a proven-OPEN source (a free generic in a key-reachable position)
//!   reports `ProvenOpen`;
//! - an UNAVAILABLE source (missing decl, budget exhaustion) reports
//!   `Unavailable` — NEVER `ProvenOpen` — and a later evaluation with a
//!   fresh budget re-derives the true verdict (no verdict was cached by
//!   the exhausted run);
//! - the instantiation route decides closedness PATH-PRECISELY WITHOUT
//!   materializing members: an open argument confined to member VALUE
//!   positions keeps the domain closed even when another member VALUE is
//!   an UNRESOLVABLE name — a value-descending (materializing) evaluator
//!   could not report `ProvenClosed` there (the L1 carrier-stop's
//!   no-member-expansion witness).

use std::sync::Arc;

use super::carrier_head_resolution_tests::{host, upsert_ts};
use super::raise::{
    prepared_decl_body_is_closed, prepared_instantiation_key_domain_is_closed,
    userland_instantiation_body_is_closed_object, ClosednessVerdict, KeyDomainBinding,
};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{DeclIdentity, HashValue};

fn decl(canonical: &str, name: &str) -> DeclIdentity {
    DeclIdentity {
        canonical_id: Arc::from(canonical),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: HashValue::default(),
        decl_name: Arc::from(name),
    }
}

#[test]
fn proven_closed_source_reports_closed() {
    let host = host();
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Closed { a: string; b: number }\n\
         export type Chain = Closed;",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut budget = 256u32;
    assert_eq!(
        prepared_decl_body_is_closed(
            &dispatch,
            "/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Closed",
            &mut budget,
        ),
        ClosednessVerdict::ProvenClosed,
        "a plain object interface is a proven-closed key domain"
    );
    // The transparent alias hop (FollowRefByName -> name_resolution) reaches
    // the same proof.
    let mut budget = 256u32;
    assert_eq!(
        prepared_decl_body_is_closed(
            &dispatch,
            "/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Chain",
            &mut budget,
        ),
        ClosednessVerdict::ProvenClosed,
        "a bare alias chain onto a closed object proves closed through the ref hop"
    );
}

#[test]
fn proven_open_source_reports_open_not_unavailable() {
    let host = host();
    upsert_ts(
        &host,
        "/types.ts",
        // A function type is not an enumerable key surface — the recipe's
        // decided-open leaf.
        "export type Bare = () => string;",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut budget = 256u32;
    assert_eq!(
        prepared_decl_body_is_closed(
            &dispatch,
            "/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Bare",
            &mut budget,
        ),
        ClosednessVerdict::ProvenOpen,
        "a function-typed body is a PROOF of openness at the key domain, not a refusal"
    );
}

#[test]
fn unavailable_source_is_never_proven_open() {
    let host = host();
    upsert_ts(&host, "/types.ts", "export interface Present { a: string }");
    let dispatch = ProjectSemanticDispatch::new(&host);

    // A missing declaration is a REFUSAL.
    let mut budget = 256u32;
    let missing = prepared_decl_body_is_closed(
        &dispatch,
        "/types.ts",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "Absent",
        &mut budget,
    );
    assert_eq!(missing, ClosednessVerdict::Unavailable);
    assert_ne!(
        missing,
        ClosednessVerdict::ProvenOpen,
        "a missing decl must NOT collapse into a proof of openness"
    );

    // An unresolvable bare-ref hop is a REFUSAL too.
    upsert_ts(
        &host,
        "/dangling.ts",
        "export type Dangle = NotDeclaredAnywhere;",
    );
    let mut budget = 256u32;
    assert_eq!(
        prepared_decl_body_is_closed(
            &dispatch,
            "/dangling.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Dangle",
            &mut budget,
        ),
        ClosednessVerdict::Unavailable,
        "an unresolvable bare name is undecidable — never ProvenOpen"
    );
}

#[test]
fn budget_exhaustion_reports_unavailable_and_recomputes_fresh() {
    let host = host();
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Closed { a: string }\n\
         export type Chain = Closed;",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);

    // Budget zero: the evaluator REFUSES before any step.
    let mut exhausted = 0u32;
    assert_eq!(
        prepared_decl_body_is_closed(
            &dispatch,
            "/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Chain",
            &mut exhausted,
        ),
        ClosednessVerdict::Unavailable,
        "budget exhaustion is a refusal, never a proof (fail-closed, no-poison)"
    );

    // The SAME dispatch with a fresh budget re-derives the true verdict —
    // the exhausted run cached nothing (no warm poisoning of the verdict).
    let mut fresh = 256u32;
    assert_eq!(
        prepared_decl_body_is_closed(
            &dispatch,
            "/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Chain",
            &mut fresh,
        ),
        ClosednessVerdict::ProvenClosed,
        "a fresh evaluation after an exhausted one recomputes the genuine verdict"
    );
}

#[test]
fn instantiation_key_domain_decides_path_precisely_without_materializing_members() {
    let host = host();
    upsert_ts(
        &host,
        "/types.ts",
        // `T` is confined to a member VALUE position; `broken` is an
        // UNRESOLVABLE member value. A value-descending (materializing)
        // evaluator would trip over `NotDeclaredAnywhere`; the key-domain
        // question never consults member values, so the domain stays a
        // PROVEN-closed fixed key set {label, items, broken}.
        "export interface Fixed<T> { label: string; items: T; broken: NotDeclaredAnywhere }",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut budget = 256u32;
    assert_eq!(
        prepared_instantiation_key_domain_is_closed(
            &dispatch,
            &decl("/types.ts", "Fixed"),
            &[KeyDomainBinding::Open],
            &mut budget,
        ),
        ClosednessVerdict::ProvenClosed,
        "an open arg confined to member VALUE positions keeps the key domain closed, and the \
         verdict is reached WITHOUT materializing members (the unresolvable member value is \
         never consulted)"
    );

    // Negative control: the same open argument in a KEY-reachable position
    // (the body IS the parameter) proves OPEN.
    upsert_ts(&host, "/open.ts", "export type KeyReaches<T> = T;");
    let mut budget = 256u32;
    assert_eq!(
        prepared_instantiation_key_domain_is_closed(
            &dispatch,
            &decl("/open.ts", "KeyReaches"),
            &[KeyDomainBinding::Open],
            &mut budget,
        ),
        ClosednessVerdict::ProvenOpen,
        "an open arg in a key-reachable position opens the instantiation"
    );
}

#[test]
fn instantiation_arity_mismatch_is_proven_open_and_missing_decl_unavailable() {
    let host = host();
    upsert_ts(&host, "/types.ts", "export interface One<T> { items: T }");
    let dispatch = ProjectSemanticDispatch::new(&host);

    // Over-application: two args onto a one-param decl — decided open.
    let mut budget = 256u32;
    assert_eq!(
        prepared_instantiation_key_domain_is_closed(
            &dispatch,
            &decl("/types.ts", "One"),
            &[
                KeyDomainBinding::ClosedAbstract,
                KeyDomainBinding::ClosedAbstract
            ],
            &mut budget,
        ),
        ClosednessVerdict::ProvenOpen,
        "an arity-unsatisfiable instantiation is decided from the declaration's own facts"
    );

    // A missing base decl is a refusal, never a proof.
    let mut budget = 256u32;
    assert_eq!(
        prepared_instantiation_key_domain_is_closed(
            &dispatch,
            &decl("/types.ts", "Nowhere"),
            &[KeyDomainBinding::ClosedAbstract],
            &mut budget,
        ),
        ClosednessVerdict::Unavailable,
        "a missing instantiation target refuses"
    );
}

#[test]
fn closed_object_shape_fact_gates_the_publication_carve_out() {
    let host = host();
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Nominal { a: string }\n\
         export type UnionShape = { a: string } | { b: string };\n\
         export type OperatorShape<T> = keyof T;",
    );
    let _dispatch = ProjectSemanticDispatch::new(&host);

    assert!(
        userland_instantiation_body_is_closed_object(&host, &decl("/types.ts", "Nominal")),
        "a closed object interface IS the nominal carve-out shape"
    );
    assert!(
        !userland_instantiation_body_is_closed_object(&host, &decl("/types.ts", "UnionShape")),
        "a union is NOT a closed-object shape (it must keep resolving)"
    );
    assert!(
        !userland_instantiation_body_is_closed_object(&host, &decl("/types.ts", "OperatorShape")),
        "an operator body is NOT a closed-object shape (helpers must reduce)"
    );
    assert!(
        !userland_instantiation_body_is_closed_object(&host, &decl("/types.ts", "Absent")),
        "a missing decl is not provably closed (safe default)"
    );
}

#[test]
fn true_self_recursion_refuses_instead_of_diverging() {
    let host = host();
    upsert_ts(
        &host,
        "/types.ts",
        // A KEY-reachable self-reference: the alias chain loops on itself.
        "export type Loop = Loop2;\nexport type Loop2 = Loop;",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut budget = 256u32;
    assert_eq!(
        prepared_decl_body_is_closed(
            &dispatch,
            "/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Loop",
            &mut budget,
        ),
        ClosednessVerdict::Unavailable,
        "a genuine alias cycle refuses through the dispatch-wide in-flight guard"
    );
    assert!(
        budget > 0,
        "the cycle refusal is guard-driven, not budget-exhaustion-driven"
    );
}
