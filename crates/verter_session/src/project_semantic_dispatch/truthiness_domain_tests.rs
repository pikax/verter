//! Unit coverage for the demand-scoped truthiness-domain authority
//! (`ClassifyTruthinessDomain`): the checker-measured classification
//! rules the public flow boundary rows cannot reach one by one, plus the
//! admission contract — a fully decided domain warms under its own query
//! identity, an undecided one is `ReturnOnly` and cold-computes again.
//!
//! Every `Yes`/`No` in the rule table is measured against the pinned tsc
//! 7.0.2 (`--strict --declaration --emitDeclarationOnly`) via truthiness
//! guards over the corresponding authored types; `Undecided` rows are the
//! forms the classifier must not resolve (unresolved carriers), where a
//! decided verdict in either direction would be a guess.

use std::sync::Arc;

use super::raise::{dispatch_cold_for, dispatch_warm_for};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    LiteralValue, PrimitiveKind, QueryError, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    TruthinessDomain, TruthinessInhabitance,
};
use crate::VerterHost;

fn domain_of(host: &VerterHost, subject: SemanticNodeId) -> TruthinessDomain {
    ProjectSemanticDispatch::new(host)
        .classify_truthiness_domain_read(subject)
        .value
}

fn prim(host: &VerterHost, kind: PrimitiveKind) -> SemanticNodeId {
    host.project_type_store()
        .semantic_graph()
        .intern_node(SemanticNodeData::Primitive(kind))
}

fn lit(host: &VerterHost, value: LiteralValue) -> SemanticNodeId {
    host.project_type_store()
        .semantic_graph()
        .intern_node(SemanticNodeData::Literal(value))
}

fn template(host: &VerterHost, quasis: &[&str], expressions: &[SemanticNodeId]) -> SemanticNodeId {
    host.project_type_store()
        .semantic_graph()
        .intern_node(SemanticNodeData::TemplateLiteral {
            quasis: quasis.iter().map(|q| Arc::from(*q)).collect(),
            expressions: Arc::from(expressions.to_vec().into_boxed_slice()),
        })
}

#[test]
fn truthiness_rules_match_the_pinned_checker() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();

    let string = prim(&host, PrimitiveKind::String);
    let number = prim(&host, PrimitiveKind::Number);
    let never = prim(&host, PrimitiveKind::Never);
    let lit_a = lit(&host, LiteralValue::String("a".into()));
    let lit_empty = lit(&host, LiteralValue::String(String::new()));

    // Primitives — `symbol` and `object` are truthy-only; `undefined` /
    // `null` / `void` falsy-only; the broad scalars inhabit both; `never`
    // neither (measured: `symbol | 0` and `object | 0` leave the falsy
    // edge, `void | "a"` leaves the truthy edge).
    let cases: Vec<(SemanticNodeId, TruthinessDomain, &str)> = vec![
        (
            prim(&host, PrimitiveKind::Symbol),
            TruthinessDomain::TRUTHY_ONLY,
            "symbol",
        ),
        (
            prim(&host, PrimitiveKind::Object),
            TruthinessDomain::TRUTHY_ONLY,
            "object",
        ),
        (
            prim(&host, PrimitiveKind::Undefined),
            TruthinessDomain::FALSY_ONLY,
            "undefined",
        ),
        (
            prim(&host, PrimitiveKind::Null),
            TruthinessDomain::FALSY_ONLY,
            "null",
        ),
        (
            prim(&host, PrimitiveKind::Void),
            TruthinessDomain::FALSY_ONLY,
            "void",
        ),
        (never, TruthinessDomain::UNINHABITED, "never"),
        (string, TruthinessDomain::BOTH, "string"),
        (
            prim(&host, PrimitiveKind::Unknown),
            TruthinessDomain::BOTH,
            "unknown",
        ),
        // Literals classify by value; `-0` is falsy (`-0.0 == 0.0`) and a
        // bigint is falsy iff its digits are all zero, sign ignored.
        (
            lit(&host, LiteralValue::Boolean(true)),
            TruthinessDomain::TRUTHY_ONLY,
            "true",
        ),
        (
            lit(&host, LiteralValue::Boolean(false)),
            TruthinessDomain::FALSY_ONLY,
            "false",
        ),
        (
            lit(&host, LiteralValue::Number(-0.0)),
            TruthinessDomain::FALSY_ONLY,
            "-0",
        ),
        (
            lit(&host, LiteralValue::BigInt("-00".into())),
            TruthinessDomain::FALSY_ONLY,
            "-0n",
        ),
        (
            lit(&host, LiteralValue::BigInt("10".into())),
            TruthinessDomain::TRUTHY_ONLY,
            "10n",
        ),
        (lit_empty, TruthinessDomain::FALSY_ONLY, "\"\""),
        // Template literals — the falsy inhabitant is exactly `""`:
        // a non-empty quasi rules it out; `${number}` renders only
        // non-empty text; `${string}` admits both; `${""}` renders only
        // `""` (its truthy edge is dead — measured q7); a `never`
        // placeholder empties the template (measured q6); a nested
        // non-empty template rules `""` out through the outer
        // placeholder (measured q12).
        (
            template(&host, &["item-", ""], &[string]),
            TruthinessDomain::TRUTHY_ONLY,
            "`item-${string}`",
        ),
        (
            template(&host, &["", ""], &[string]),
            TruthinessDomain::BOTH,
            "`${string}`",
        ),
        (
            template(&host, &["", ""], &[number]),
            TruthinessDomain::TRUTHY_ONLY,
            "`${number}`",
        ),
        // The checker's template facts treat an `any` placeholder as
        // non-empty (measured: the falsy edge of `` `${any}` `` is
        // `never`, even though `""` is assignable to the template).
        (
            template(&host, &["", ""], &[prim(&host, PrimitiveKind::Any)]),
            TruthinessDomain::TRUTHY_ONLY,
            "`${any}`",
        ),
        (
            template(&host, &["", ""], &[lit_empty]),
            TruthinessDomain::FALSY_ONLY,
            "`${\"\"}`",
        ),
        (
            template(&host, &["a", ""], &[never]),
            TruthinessDomain::UNINHABITED,
            "`a${never}`",
        ),
        (
            template(&host, &[""], &[]),
            TruthinessDomain::FALSY_ONLY,
            "``",
        ),
    ];
    for (subject, expected, label) in cases {
        assert_eq!(domain_of(&host, subject), expected, "{label}");
    }

    // `${"a" | ""}` admits both buckets (measured q11: the falsy edge
    // keeps the arm as `""`).
    let ab = graph.intern_node(SemanticNodeData::Union(
        crate::semantic_query::composite::CompositeList::test_fixture(Arc::from([
            lit_a, lit_empty,
        ])),
    ));
    assert_eq!(
        domain_of(&host, template(&host, &["", ""], &[ab])),
        TruthinessDomain::BOTH,
        "`${{\"a\" | \"\"}}`"
    );
    // Nested non-empty template as a placeholder: `` `${`a${string}`}` ``
    // has no falsy inhabitant (measured q12).
    let nested = template(&host, &["a", ""], &[string]);
    assert_eq!(
        domain_of(&host, template(&host, &["", ""], &[nested])),
        TruthinessDomain::TRUTHY_ONLY,
        "`${{`a${{string}}`}}`"
    );

    // A type parameter classifies through its constraint (measured q2/q3:
    // `T extends "a"` leaves the falsy edge, `T extends string` keeps
    // it); unconstrained is `unknown`'s domain (measured q1).
    let decl = crate::semantic_query::DeclIdentity {
        canonical_id: Arc::from("/wb/t.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: Default::default(),
        decl_name: Arc::from("f"),
    };
    let param = |constraint: Option<SemanticNodeId>| {
        graph.intern_node(SemanticNodeData::TypeParam {
            decl: decl.clone(),
            param_index: 0,
            constraint,
            default: None,
            display_name: Arc::from("T"),
        })
    };
    assert_eq!(
        domain_of(&host, param(Some(lit_a))),
        TruthinessDomain::TRUTHY_ONLY
    );
    assert_eq!(
        domain_of(&host, param(Some(string))),
        TruthinessDomain::BOTH
    );
    assert_eq!(domain_of(&host, param(None)), TruthinessDomain::BOTH);

    // An unresolved carrier is UNDECIDED — reported, never guessed in
    // either direction.
    let opaque = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    assert_eq!(domain_of(&host, opaque), TruthinessDomain::UNDECIDED);
    let keyof = graph.intern_node(SemanticNodeData::KeyOf { base: param(None) });
    assert_eq!(domain_of(&host, keyof), TruthinessDomain::UNDECIDED);
}

/// The admission contract: a fully decided domain is memoized under its
/// own query identity (the second read replays warm, zero cold computes),
/// while an undecided one is `ReturnOnly` — the family memo refuses it
/// and every read cold-computes again. This is also the bounded-work
/// evidence that repeated per-arm consumption from flow does not repeat
/// the classification walk.
#[test]
fn decided_domain_warms_and_undecided_domain_never_does() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let dispatch = ProjectSemanticDispatch::new(&host);

    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let decided_key = SemanticQueryKey::ClassifyTruthinessDomain { subject: string };
    let first = dispatch.classify_truthiness_domain_read(string);
    assert_eq!(first.value, TruthinessDomain::BOTH);
    assert!(!first.cache_suppress, "a decided domain is admissible");
    let cold_after_first = dispatch_cold_for(&decided_key);
    let second = dispatch.classify_truthiness_domain_read(string);
    assert_eq!(second.value, TruthinessDomain::BOTH);
    assert_eq!(
        dispatch_cold_for(&decided_key),
        cold_after_first,
        "the second decided read must NOT cold-compute"
    );
    assert!(
        dispatch_warm_for(&decided_key) >= 1,
        "the second decided read replays warm"
    );

    let opaque = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let undecided_key = SemanticQueryKey::ClassifyTruthinessDomain { subject: opaque };
    let first = dispatch.classify_truthiness_domain_read(opaque);
    assert_eq!(first.value, TruthinessDomain::UNDECIDED);
    assert!(
        first.cache_suppress,
        "an undecided domain suppresses admission"
    );
    assert_eq!(first.value.truthy, TruthinessInhabitance::Undecided);
    let cold_after_first = dispatch_cold_for(&undecided_key);
    let second = dispatch.classify_truthiness_domain_read(opaque);
    assert_eq!(second.value, TruthinessDomain::UNDECIDED);
    assert!(second.cache_suppress);
    assert_eq!(
        dispatch_cold_for(&undecided_key),
        cold_after_first + 1,
        "an undecided domain is ReturnOnly: every read cold-computes again"
    );
}
