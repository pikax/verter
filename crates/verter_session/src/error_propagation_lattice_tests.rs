//! Guards for the **error-tolerance-returnonly-non-admission /
//! error-any-never-propagation-lattice** (CRITICAL) rule — the §18
//! fact-rooted admission decision, the §18.3 taint join, and the §22
//! type-lattice absorption fast-reject.
//!
//! Each guard is discriminating (fails against a tree without the behavior,
//! passes with it) and exercises real branching, not always-true assertions.

use std::sync::Arc;

use crate::fact_signature_helpers::ReadSetSignature;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::{DerivedFactKind, FactVersionRef, ResolveImportsFactRef};
use crate::semantic_query::admit::{admit_decision, Admission};
use crate::semantic_query::{
    BrokenInputClass, IndexKey, PathSegment, PrimitiveKind, QueryError, QueryResult,
    RelationResult, ResultTaint, SemanticNodeData, SemanticNodeId,
};
use crate::{CompileErrorPolicy, HostConfig, VerterHost};
use verter_semantic::facts::registry::{
    FactKey, FactLane, InternedName, InternedSpecifier, SymbolSpace,
};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn sig(facts: Vec<FactVersionRef>) -> ReadSetSignature {
    ReadSetSignature::new(Arc::from(facts.into_boxed_slice()))
}

fn import_route_fact() -> FactVersionRef {
    FactVersionRef::DerivedFactHash {
        canonical_id: "/missing.ts".to_string(),
        kind: DerivedFactKind::ImportRoute,
        hash: [7u8; 16],
    }
}

fn negative_resolved_import_fact() -> FactVersionRef {
    FactVersionRef::ResolveImports(ResolveImportsFactRef {
        canonical_id: "/importer.ts".to_string(),
        key: FactKey::ResolvedImportClause {
            specifier: InternedSpecifier::from("./missing"),
            binding: InternedName::from("X"),
            space: SymbolSpace::Type,
            resolved_canonical: Arc::from(
                crate::resolved_import_facts_producer::UNRESOLVED_SENTINEL,
            ),
            resolved_source_name: InternedName::from("X"),
        },
        lane: FactLane::Semantic,
        expected_hash: [3u8; 16],
    })
}

/// Resolve the absorbed result node id from a reducer's `QueryBuildOutput`.
fn absorbed_node(out: crate::project_semantic_dispatch::walk::QueryBuildOutput) -> SemanticNodeId {
    match out.result {
        QueryResult::Value(id) => id,
        other => panic!("expected an absorbed Value, got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Guard 1 — §18.2 fact-rooted admission.
// ─────────────────────────────────────────────────────────────────────────

/// `admit_decision` gates `Warm` on the rooting FACT in the signature, NOT on
/// the taint enum class: a torn / broken-input result is `ReturnOnly`, while a
/// missing-dependency error WHOSE FACT was recorded is `Warm`/cacheable.
#[test]
fn error_tolerance_broken_input_is_returnonly_fact_rooted_error_is_cacheable() {
    // Broken / torn input is never warm, regardless of how rich the signature
    // is.
    let rich = sig(vec![import_route_fact(), negative_resolved_import_fact()]);
    assert_eq!(
        admit_decision(ResultTaint::Broken(BrokenInputClass::TornRead), &rich),
        Admission::ReturnOnly,
        "a torn read must be ReturnOnly even with a rich fact rail"
    );
    assert_eq!(
        admit_decision(ResultTaint::Broken(BrokenInputClass::SyntaxError), &rich),
        Admission::ReturnOnly,
        "a syntax-error broken input must be ReturnOnly"
    );
    assert_eq!(
        admit_decision(
            ResultTaint::Partial(BrokenInputClass::IncompleteDeclaration),
            &rich
        ),
        Admission::ReturnOnly,
        "an incomplete-declaration mid-edit shape has no stable fact -> ReturnOnly"
    );

    // A missing-dependency error WITH its invalidation rail recorded is
    // Warm/cacheable — the discriminator is the FACT, not the enum class.
    assert_eq!(
        admit_decision(
            ResultTaint::Partial(BrokenInputClass::MissingDependency),
            &sig(vec![import_route_fact()]),
        ),
        Admission::Warm,
        "Partial(MissingDependency) WITH the ImportRoute rail is fact-rooted-cacheable"
    );
    // The SAME taint class WITHOUT the rail is ReturnOnly — proving admission
    // does NOT key on the enum class.
    assert_eq!(
        admit_decision(
            ResultTaint::Partial(BrokenInputClass::MissingDependency),
            &sig(vec![FactVersionRef::FileWholeHash {
                canonical_id: "/x.ts".to_string(),
                hash: [1u8; 16],
            }]),
        ),
        Admission::ReturnOnly,
        "Partial(MissingDependency) WITHOUT the ImportRoute rail must NOT warm-admit \
         (a positive FileWholeHash is not the missing-dep rail)"
    );

    // An unresolved reference is Warm only with the negative-resolution fact.
    assert_eq!(
        admit_decision(
            ResultTaint::Partial(BrokenInputClass::UnresolvedReference),
            &sig(vec![negative_resolved_import_fact()]),
        ),
        Admission::Warm,
    );
    assert_eq!(
        admit_decision(
            ResultTaint::Partial(BrokenInputClass::UnresolvedReference),
            &sig(vec![]),
        ),
        Admission::ReturnOnly,
    );

    // Clean over a sound carrier publishes warm.
    assert_eq!(
        admit_decision(ResultTaint::Clean, &sig(vec![])),
        Admission::Warm
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Guard 2 — §22 type-lattice absorption fast-reject.
// ─────────────────────────────────────────────────────────────────────────

/// The §22 absorption table, exercised through the separable `absorb_*`
/// reducer hooks: `X | never = X`, `keyof never`, `any[K] = any`,
/// `unknown[K]` = error, distributive-`any` union, mapped-over-`never`/`unknown`.
#[test]
fn error_any_never_propagation_lattice() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let any = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let never = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
    let error = graph.intern_node(SemanticNodeData::Opaque(QueryError::Other(Arc::from(
        "boom",
    ))));

    let kind = |id: SemanticNodeId| graph.node_data(id).map(|d| (*d).clone());

    // union: X | never = X (the `never` arm is dropped, singleton folds).
    let u = absorbed_node(
        dispatch
            .absorb_union(&[string, never])
            .expect("X|never absorbs"),
    );
    assert_eq!(
        kind(u),
        Some(SemanticNodeData::Primitive(PrimitiveKind::String)),
        "X | never = X"
    );
    // union: X | any = any.
    let u = absorbed_node(
        dispatch
            .absorb_union(&[string, any])
            .expect("X|any absorbs"),
    );
    assert_eq!(
        kind(u),
        Some(SemanticNodeData::Primitive(PrimitiveKind::Any)),
        "X | any = any"
    );
    // union: X | unknown = unknown.
    let u = absorbed_node(
        dispatch
            .absorb_union(&[string, unknown])
            .expect("X|unknown absorbs"),
    );
    assert_eq!(
        kind(u),
        Some(SemanticNodeData::Primitive(PrimitiveKind::Unknown)),
        "X | unknown = unknown"
    );
    // union: NO absorption for a plain union of ordinary types.
    assert!(
        dispatch.absorb_union(&[string, number]).is_none(),
        "string | number must NOT fast-reject"
    );

    // intersection: X & never = never; X & unknown = X; X & any = any.
    let i = absorbed_node(
        dispatch
            .absorb_intersection(&[string, never])
            .expect("X&never absorbs"),
    );
    assert_eq!(
        kind(i),
        Some(SemanticNodeData::Primitive(PrimitiveKind::Never)),
        "X & never = never"
    );
    let i = absorbed_node(
        dispatch
            .absorb_intersection(&[string, unknown])
            .expect("X&unknown absorbs"),
    );
    assert_eq!(
        kind(i),
        Some(SemanticNodeData::Primitive(PrimitiveKind::String)),
        "X & unknown = X"
    );
    let i = absorbed_node(
        dispatch
            .absorb_intersection(&[string, any])
            .expect("X&any absorbs"),
    );
    assert_eq!(
        kind(i),
        Some(SemanticNodeData::Primitive(PrimitiveKind::Any)),
        "X & any = any"
    );

    // keyof never / keyof any = string | number | symbol (TS quirk).
    let k = absorbed_node(dispatch.absorb_key_of(never).expect("keyof never absorbs"));
    match kind(k) {
        Some(SemanticNodeData::Union(members)) => {
            let mut kinds: Vec<PrimitiveKind> = members
                .iter()
                .filter_map(|m| match graph.node_data(*m).map(|d| (*d).clone()) {
                    Some(SemanticNodeData::Primitive(p)) => Some(p),
                    _ => None,
                })
                .collect();
            kinds.sort_by_key(|p| format!("{p:?}"));
            assert!(
                kinds.contains(&PrimitiveKind::String)
                    && kinds.contains(&PrimitiveKind::Number)
                    && kinds.contains(&PrimitiveKind::Symbol),
                "keyof never = string | number | symbol, got {kinds:?}"
            );
        }
        other => panic!("keyof never must be a union of string|number|symbol, got {other:?}"),
    }
    // keyof unknown = never.
    let k = absorbed_node(
        dispatch
            .absorb_key_of(unknown)
            .expect("keyof unknown absorbs"),
    );
    assert_eq!(
        kind(k),
        Some(SemanticNodeData::Primitive(PrimitiveKind::Never)),
        "keyof unknown = never"
    );

    // indexed access: any[K] = any; never[K] = never; unknown[K] = error.
    let a = absorbed_node(dispatch.absorb_indexed_access(any).expect("any[K] absorbs"));
    assert_eq!(
        kind(a),
        Some(SemanticNodeData::Primitive(PrimitiveKind::Any)),
        "any[K] = any"
    );
    let a = absorbed_node(
        dispatch
            .absorb_indexed_access(never)
            .expect("never[K] absorbs"),
    );
    assert_eq!(
        kind(a),
        Some(SemanticNodeData::Primitive(PrimitiveKind::Never)),
        "never[K] = never"
    );
    let a = absorbed_node(
        dispatch
            .absorb_indexed_access(unknown)
            .expect("unknown[K] absorbs"),
    );
    assert!(
        matches!(kind(a), Some(SemanticNodeData::Opaque(_))),
        "unknown[K] = UNCONDITIONAL error (Opaque), got {:?}",
        kind(a)
    );

    // The single-segment indexed-access shape is recognised; a member path is
    // NOT (member projection is a distinct surface).
    assert!(ProjectSemanticDispatch::project_path_is_indexed_access(&[
        PathSegment::Index(IndexKey::String(Arc::from("k")))
    ]));
    assert!(!ProjectSemanticDispatch::project_path_is_indexed_access(&[
        PathSegment::Member(Arc::from("foo"))
    ]));

    // mapped over never = {} (empty object); a DIRECT mapping over unknown is
    // illegal => error.
    let m = absorbed_node(
        dispatch
            .absorb_mapped(never)
            .expect("mapped over never absorbs"),
    );
    match kind(m) {
        Some(SemanticNodeData::Object(view)) => {
            assert!(view.members.is_empty(), "mapped over never = {{}}");
        }
        other => panic!("mapped over never must be an empty Object, got {other:?}"),
    }
    let m = absorbed_node(
        dispatch
            .absorb_mapped(unknown)
            .expect("mapped over unknown absorbs"),
    );
    assert!(
        matches!(kind(m), Some(SemanticNodeData::Opaque(_))),
        "direct mapped over unknown = error"
    );

    // conditional: error check => the error carrier dominates both branches.
    let c = absorbed_node(
        dispatch
            .absorb_conditional(error)
            .expect("error-check absorbs"),
    );
    assert_eq!(
        c, error,
        "error extends T => the same error carrier (carrier-dominating)"
    );
    // A non-special check does NOT fast-reject (the branch logic decides).
    assert!(
        dispatch.absorb_conditional(string).is_none(),
        "an ordinary conditional check must not fast-reject"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Guard 3 — error taints + is ReturnOnly-prone; any/never/unknown are Clean.
// ─────────────────────────────────────────────────────────────────────────

/// The §18.3 taint join is monotone over `Clean ⊑ Partial ⊑ Broken`; the error
/// type taints and is `ReturnOnly`-prone, while `any`/`never`/`unknown` are
/// `Clean` and warm-cacheable. Relation-wise an error carrier relates
/// bidirectionally like `any` (so a broken sub-result does not cascade
/// spurious assignability failures).
#[test]
fn error_type_taints_and_is_returnonly_prone_any_is_cacheable() {
    // ── taint join lattice ──────────────────────────────────────────────
    use BrokenInputClass::*;
    use ResultTaint::*;
    assert_eq!(Clean.join(Clean), Clean);
    assert_eq!(
        Clean.join(Partial(MissingDependency)),
        Partial(MissingDependency),
        "Clean ⊔ Partial = Partial"
    );
    assert_eq!(
        Partial(MissingDependency).join(Broken(TornRead)),
        Broken(TornRead),
        "Partial ⊔ Broken = Broken (Broken dominates)"
    );
    assert_eq!(
        Broken(TornRead).join(Clean),
        Broken(TornRead),
        "Broken ⊔ Clean = Broken (join is monotone-up, order-independent)"
    );
    // Within a level, the more-severe class wins (MissingDependency <
    // UnresolvedReference < IncompleteDeclaration < SyntaxError < TornRead).
    assert_eq!(
        Partial(MissingDependency).join(Partial(IncompleteDeclaration)),
        Partial(IncompleteDeclaration),
        "same-level join keeps the more-severe class"
    );
    assert_eq!(
        Broken(SyntaxError).join(Broken(TornRead)),
        Broken(TornRead),
        "TornRead is the most-severe class"
    );

    // ── error is ReturnOnly-prone; any/never/unknown are Clean cacheable ──
    let rich = sig(vec![import_route_fact(), negative_resolved_import_fact()]);
    // An `error` produced by a torn/broken input is ReturnOnly even with a
    // rich rail.
    assert_eq!(
        admit_decision(Broken(TornRead), &rich),
        Admission::ReturnOnly,
        "an error on a torn input is ReturnOnly-prone"
    );
    // `any` / `never` / `unknown` are Clean — a clean result over a sound
    // carrier warm-admits.
    assert_eq!(
        admit_decision(Clean, &sig(vec![])),
        Admission::Warm,
        "any/never/unknown are Clean and warm-cacheable"
    );

    // ── relation: error relates BIDIRECTIONALLY like any ─────────────────
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let any = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let error = graph.intern_node(SemanticNodeData::Opaque(QueryError::Other(Arc::from(
        "boom",
    ))));

    let is_assignable = |a: SemanticNodeId, b: SemanticNodeId| {
        matches!(
            dispatch.relate_nodes(a, b).0,
            RelationResult::Assignable { .. }
        )
    };
    // error relates both directions (like any) — no spurious NotAssignable.
    assert!(
        is_assignable(error, string),
        "error <: string (bidirectional, like any)"
    );
    assert!(
        is_assignable(string, error),
        "string <: error (bidirectional, like any)"
    );
    // any likewise relates both directions.
    assert!(is_assignable(any, string));
    assert!(is_assignable(string, any));
    // Discriminating negative: the error flip is specifically the error
    // carrier — a plain incompatible primitive pair is still NotAssignable.
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    assert!(
        matches!(
            dispatch.relate_nodes(string, number).0,
            RelationResult::NotAssignable
        ),
        "string <: number must stay NotAssignable — the bidirectional flip is error-specific"
    );
}
