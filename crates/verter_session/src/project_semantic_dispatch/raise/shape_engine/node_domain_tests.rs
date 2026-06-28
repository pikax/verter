use super::summary;
use super::FactShapeTag;
use crate::resolver_core::component_meta_query_engine::semantic_query_error_raw;
use crate::semantic_query::QueryError;

/// The TYPED node-domain summary constructor (`summary::opaque_sentinel`)
/// must yield the SAME FULL summary — `materialized` fact, `expanded_surface`,
/// AND `FactShapeTag` — the LEGACY raw-string node-domain path
/// (`summary::unknown` over the variant's `semantic_query_error_raw`) produced,
/// for the FULL `_ => opaque_sentinel`-reachable variant set the `fold_node`
/// `Opaque(err)` arm routes (every variant EXCEPT `RecursiveRef` and
/// `DeclPlaceholder`, which hit the earlier `recursive_ref` / `reference`
/// sub-arms and are covered by the agreement test in `raise_sentinel.rs`),
/// INCLUDING the `Other`-sentinel-text carrier. This is the node-domain
/// anti-drift guard discharging the `Opaque`-arm behaviour-preservation
/// obligation: a typed classification that disagreed with the raw-string
/// classification would fail here.
///
/// DISCRIMINATING on BOTH the `materialized` text-bearing delegation
/// (`Other("semanticMiss")` — pre-delegation the typed `materialized` fact
/// diverged from the legacy raw fact) AND the `tag` text-bearing delegation
/// (`Other("semanticObjectSurface")` — the payload that tags
/// `ObjectSurfaceSentinel` via the raw rule; reverting the tag-predicate
/// delegation back to `Other(_) => false` makes `typed.tag` report `Other`
/// while the legacy raw rule reports `ObjectSurfaceSentinel`, so the `tag`
/// assertion below FAILS for it).
#[test]
fn opaque_sentinel_summary_matches_legacy_unknown_summary() {
    // The full `_ => opaque_sentinel`-reachable set the `Opaque(err)` arm
    // routes (RecursiveRef + DeclPlaceholder hit earlier sub-arms ⇒ excluded).
    // Includes the recognised prefix-sentinel `UnsupportedIntrinsic` (its raw
    // `unsupportedIntrinsic(<name>)` is unmaterialised via the
    // `unsupportedIntrinsic(` prefix, tag `Other`) and both adversarial
    // text-bearing carriers the delegation covers: `Other("semanticMiss")`
    // (the `materialized` drift case) and `Other("semanticObjectSurface")`
    // (the `tag` drift case).
    let reachable = [
        QueryError::Miss,
        QueryError::UnsupportedIntrinsic {
            name: std::sync::Arc::from("FixtureIntrinsic"),
        },
        QueryError::BudgetExceeded(
            crate::resolver_core::shallow_file_state::BudgetExceededFailure {
                domain: crate::resolver_core::shallow_file_state::BudgetDomain::ProjectionOperation,
                limit: 1,
                actual: 2,
                context: "opaque-sentinel summary fixture".to_string(),
            },
        ),
        QueryError::UnstableState { attempts: 3 },
        QueryError::AliasCycle {
            chain: std::sync::Arc::from(
                vec![std::sync::Arc::from("A"), std::sync::Arc::from("B")].into_boxed_slice(),
            ),
        },
        QueryError::ValueDomainMismatch {
            expected: crate::semantic_query::SemanticQueryValueTag::TypeNode,
            actual: crate::semantic_query::SemanticQueryValueTag::Relation,
        },
        QueryError::RaiseAliasCycle,
        QueryError::TypeParamCycle,
        QueryError::RaiseMiss,
        QueryError::UnrepresentableSurface,
        QueryError::UnrepresentableSurfaceMember,
        QueryError::VueMacroElementsPlaceholder,
        QueryError::Other(std::sync::Arc::from("semanticMiss")),
        QueryError::Other(std::sync::Arc::from("semanticObjectSurface")),
        QueryError::Other(std::sync::Arc::from("budgetExceeded(x)")),
        QueryError::Other(std::sync::Arc::from("genuinely free text")),
    ];
    for variant in reachable {
        let typed = summary::opaque_sentinel(&variant);
        let legacy = summary::unknown(&semantic_query_error_raw(&variant));
        assert_eq!(
            typed.facts.materialized, legacy.facts.materialized,
            "materialized fact drift for {variant:?}"
        );
        assert_eq!(typed.tag, legacy.tag, "tag drift for {variant:?}");
        // `opaque_sentinel` mirrors `unknown`'s always-expanded surface —
        // asserted as PARITY (both are always `true`, so a hardcoded
        // `assert!(typed...)` would pass too, but comparing to `legacy`
        // matches the FULL-summary parity claim and catches a future edit
        // that diverged either side's `expanded_surface` formula).
        assert_eq!(
            typed.facts.expanded_surface, legacy.facts.expanded_surface,
            "expanded_surface drift for {variant:?}"
        );
    }

    // Concretely pin the `tag` discriminator: `Other("semanticObjectSurface")`
    // raises to the SEMANTIC_OBJECT_SURFACE spelling, so BOTH the typed summary
    // and the legacy raw rule must tag it `ObjectSurfaceSentinel` (the carrier
    // the intersection reducer drops). A reverted tag-predicate delegation
    // would tag it `Other` and fail the loop's `tag` assertion above.
    let object_surface_text = summary::opaque_sentinel(&QueryError::Other(std::sync::Arc::from(
        "semanticObjectSurface",
    )));
    assert_eq!(
        object_surface_text.tag,
        FactShapeTag::ObjectSurfaceSentinel,
        "Other(\"semanticObjectSurface\") must tag ObjectSurfaceSentinel via the text-bearing \
             delegation (this is the tag-drift case the fixture previously omitted)"
    );
}

/// Pin the concrete `(materialized, tag)` outcomes so a regression that
/// flipped a single variant's classification is caught directly (not only
/// via the parity loop). Derived first-hand from the raw recogniser:
/// `semanticObjectSurface` / `semanticAliasCycle` / `semanticSurfaceMember`
/// / `VueMacroElements` are recognised sentinels ⇒ NOT materialized;
/// `<raise miss>` and `semanticTypeParamCycle` are deliberately NOT in the
/// recogniser ⇒ materialized. Only the object-surface sentinel tags
/// `ObjectSurfaceSentinel`.
#[test]
fn opaque_sentinel_summary_pins_exact_materialized_and_tag() {
    let surface = summary::opaque_sentinel(&QueryError::UnrepresentableSurface);
    assert!(!surface.facts.materialized);
    assert_eq!(surface.tag, FactShapeTag::ObjectSurfaceSentinel);

    let surface_member = summary::opaque_sentinel(&QueryError::UnrepresentableSurfaceMember);
    assert!(!surface_member.facts.materialized);
    assert_eq!(surface_member.tag, FactShapeTag::Other);

    let alias_cycle = summary::opaque_sentinel(&QueryError::RaiseAliasCycle);
    assert!(!alias_cycle.facts.materialized);
    assert_eq!(alias_cycle.tag, FactShapeTag::Other);

    let vue = summary::opaque_sentinel(&QueryError::VueMacroElementsPlaceholder);
    assert!(!vue.facts.materialized);
    assert_eq!(vue.tag, FactShapeTag::Other);

    // `<raise miss>` is NOT a recognised sentinel ⇒ materialized = true.
    let raise_miss = summary::opaque_sentinel(&QueryError::RaiseMiss);
    assert!(raise_miss.facts.materialized);
    assert_eq!(raise_miss.tag, FactShapeTag::Other);

    // `semanticTypeParamCycle` is NOT a recognised sentinel ⇒ materialized.
    let tp_cycle = summary::opaque_sentinel(&QueryError::TypeParamCycle);
    assert!(tp_cycle.facts.materialized);
    assert_eq!(tp_cycle.tag, FactShapeTag::Other);
}

/// PARITY PIN: the root-only projection ([`super::project_root_summary`]) yields
/// the IDENTICAL `root_kind` the full fold ([`fold_node`](super::super::fold_node)
/// under [`RaisedFactsAlg`](super::RaisedFactsAlg)) does, for every shape the
/// raised-root mirror parity test covers PLUS the collapsed-intersection cases —
/// proving the short-circuit never diverges from the authority it replaces.
///
/// DISCRIMINATING: the shapes span the non-`Other` `RaisedRootKind` classes
/// (Object / IndexedAccess / TypeOf / KeyOf) plus the empty-arm drop +
/// single-survivor collapse + single-call-signature cases, so a mis-wired arm
/// (classifying a single-call-signature object as `Object`, skipping the
/// empty-object arm drop, collapsing a single-arm `Union`, …) makes the root-only
/// verdict diverge from the full fold and FAILS the per-shape `assert_eq!`; the
/// trailing concrete-class anchors fail if the projection collapsed to a constant.
#[test]
fn root_only_projection_root_kind_matches_full_fold() {
    use std::sync::Arc as StdArc;

    use rustc_hash::FxHashSet;

    use super::super::RaisedRootKind;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        IndexKey, PrimitiveKind, QueryError, ScopeId, SemanticNodeData, SurfaceView, ValueRootKey,
    };
    use crate::VerterHost;

    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let dispatch = ProjectSemanticDispatch::new(&host);

    let empty_surface = || SurfaceView {
        members: StdArc::from(Vec::new().into_boxed_slice()),
        call_signatures: StdArc::from(Vec::new().into_boxed_slice()),
        construct_signatures: StdArc::from(Vec::new().into_boxed_slice()),
        index_signatures: StdArc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    };

    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let empty_obj = graph.intern_node(SemanticNodeData::Object(empty_surface()));
    // A non-empty object via an OPEN index surface — member-presence settled by
    // `has_index_signature` with no `SurfaceMember` construction required.
    let open_obj = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        has_index_signature: true,
        ..empty_surface()
    }));
    let dummy = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: dummy,
        index: IndexKey::String(StdArc::from("x")),
    });
    let typeof_node = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: StdArc::from("/p.ts"),
                local_scope: None,
            },
            name: StdArc::from("v"),
        },
        StdArc::from(Vec::new().into_boxed_slice()),
        StdArc::from(Vec::new().into_boxed_slice()),
    ));
    let keyof = graph.intern_node(SemanticNodeData::KeyOf { base: open_obj });
    // A single-call-signature object raises to the call signature's value (a
    // Function root), NOT an Object — the case the raised-root mirror parity test
    // does not cover and the root-only `Object` arm must reproduce.
    let func = graph.intern_node(SemanticNodeData::Function {
        params: StdArc::from(Vec::new().into_boxed_slice()),
        return_type: string,
        type_parameters: StdArc::from(Vec::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    let call_sig_obj = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        call_signatures: StdArc::from(vec![func].into_boxed_slice()),
        ..empty_surface()
    }));

    // Collapsed-intersection cases (directly interned so lowering does NOT
    // pre-collapse them): the fold DROPS the `{}` arm and classifies the survivor.
    let int_obj = graph.intern_node(SemanticNodeData::Intersection(StdArc::from(
        vec![empty_obj, open_obj].into_boxed_slice(),
    )));
    let int_indexed = graph.intern_node(SemanticNodeData::Intersection(StdArc::from(
        vec![empty_obj, indexed].into_boxed_slice(),
    )));
    let int_typeof = graph.intern_node(SemanticNodeData::Intersection(StdArc::from(
        vec![empty_obj, typeof_node].into_boxed_slice(),
    )));
    // A single-arm Union is NOT collapsed → a Union (`Other`) root.
    let union_int_obj = graph.intern_node(SemanticNodeData::Union(StdArc::from(
        vec![int_obj].into_boxed_slice(),
    )));
    // An Intersection collapsing to a single-call-signature object → the Function
    // (`Other`) root, NOT `Object`.
    let int_callsig = graph.intern_node(SemanticNodeData::Intersection(StdArc::from(
        vec![empty_obj, call_sig_obj].into_boxed_slice(),
    )));

    let cases = [
        ("string", string),
        ("empty_obj", empty_obj),
        ("open_obj", open_obj),
        ("indexed", indexed),
        ("typeof", typeof_node),
        ("keyof", keyof),
        ("func", func),
        ("call_sig_obj", call_sig_obj),
        ("int_obj", int_obj),
        ("int_indexed", int_indexed),
        ("int_typeof", int_typeof),
        ("union_int_obj", union_int_obj),
        ("int_callsig", int_callsig),
    ];

    for (label, node) in cases {
        let full = {
            let mut alg = super::RaisedFactsAlg;
            let mut active = FxHashSet::default();
            super::super::fold_node(&mut alg, &dispatch, node, &mut active).map(|s| s.root_kind)
        };
        let root_only = {
            let mut active = FxHashSet::default();
            super::project_root_summary(&dispatch, node, &mut active).map(|s| s.root_kind)
        };
        assert_eq!(
            root_only, full,
            "[{label}] root-only projection root_kind must equal the full-fold root_kind"
        );
        assert!(
            full.is_some(),
            "[{label}] a well-formed node must fold to Some (anti-vacuity)"
        );
    }

    // Concrete-class anchors: the projection produces the DISTINCT non-`Other`
    // classes (not a constant), and a single-call-signature object — alone or as
    // the surviving intersection arm — is the Function (`Other`) root, NOT
    // `Object`.
    let root_kind = |node| {
        let mut active = FxHashSet::default();
        super::project_root_summary(&dispatch, node, &mut active).map(|s| s.root_kind)
    };
    assert_eq!(root_kind(open_obj), Some(RaisedRootKind::Object));
    assert_eq!(root_kind(indexed), Some(RaisedRootKind::IndexedAccess));
    assert_eq!(root_kind(typeof_node), Some(RaisedRootKind::TypeOf));
    assert_eq!(root_kind(keyof), Some(RaisedRootKind::KeyOf));
    assert_eq!(root_kind(int_obj), Some(RaisedRootKind::Object));
    assert_eq!(root_kind(int_indexed), Some(RaisedRootKind::IndexedAccess));
    assert_eq!(root_kind(int_typeof), Some(RaisedRootKind::TypeOf));
    assert_eq!(root_kind(union_int_obj), Some(RaisedRootKind::Other));
    assert_eq!(root_kind(call_sig_obj), Some(RaisedRootKind::Other));
    assert_eq!(root_kind(int_callsig), Some(RaisedRootKind::Other));
}
