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
/// are recognised sentinels ⇒ NOT materialized;
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
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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

/// ARM-COVERAGE PARITY: the root-only projection ([`super::project_root_summary`])
/// yields the IDENTICAL `root_kind` the full fold ([`fold_node`](super::super::fold_node)
/// under [`RaisedFactsAlg`](super::RaisedFactsAlg)) does, enumerating EVERY
/// `SemanticNodeData` arm with a non-trivial root mapping the earlier
/// collapsed-intersection test does not — Reference, Conditional, both Mapped
/// sub-cases, ConstructorType, Array, Alias, MergedDecl, RawFallback, Opaque (both
/// the `_` and the `RecursiveRef` sub-arm), ImportType, Tuple, TemplateLiteral,
/// Literal, Infer, TypeParam, SyntheticBinding, and the three `Ref` carriers
/// (DeclRef / InstantiationRef / BareRef) — on top of the object case.
///
/// SC-FIRST: arm-PRESENCE is COMPILER-sealed — `project_root_summary`'s match is
/// wildcard-free, so the compiler already forces every `SemanticNodeData` arm to be
/// handled; these cases verify per-arm root PARITY against the full fold, not mere
/// presence.
///
/// DISCRIMINATING: each parity `assert_eq!` compares the root-only verdict to the
/// full-fold authority, so a mis-wired arm (a wrong `root_kind`, a Mapped sub-case
/// that flips `value_is_semantic_miss`, a `Ref`-carrier arm reverted to a plain
/// leaf, a constant-collapse) FAILS; the trailing concrete anchors fail if the
/// projection lost a distinct class.
#[test]
fn root_only_projection_matches_full_fold_across_all_arms() {
    use std::sync::Arc as StdArc;

    use rustc_hash::FxHashSet;

    use super::super::RaisedRootKind;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        DeclIdentity, HashValue, MapperKey, MapperKind, NodeScopeId, OptionalityMod, PrimitiveKind,
        QueryError, ReadonlyMod, SemanticNodeData, SurfaceView, SyntheticBindingId, TupleElement,
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
    let dummy = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let open_obj = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        has_index_signature: true,
        ..empty_surface()
    }));
    let func = graph.intern_node(SemanticNodeData::Function {
        params: StdArc::from(Vec::new().into_boxed_slice()),
        return_type: string,
        type_parameters: StdArc::from(Vec::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });

    let reference = graph.intern_node(SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
        canonical_id: StdArc::from("/p.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        name: StdArc::from("Foo"),
        whole_hash: HashValue::default(),
    }));
    let import = graph.intern_node(SemanticNodeData::new_import_type(
        StdArc::from("./m"),
        StdArc::from(Vec::new().into_boxed_slice()),
        StdArc::from(Vec::new().into_boxed_slice()),
        false,
    ));
    let raw_fallback = graph.intern_node(SemanticNodeData::RawFallback {
        raw: StdArc::from("SomeRawText"),
    });
    let opaque_other = graph.intern_node(SemanticNodeData::Opaque(QueryError::RaiseMiss));
    let array = graph.intern_node(SemanticNodeData::Array {
        element: string,
        readonly: false,
    });
    let conditional = graph.intern_node(SemanticNodeData::Conditional {
        check: dummy,
        extends: dummy,
        true_branch_ref: dummy,
        false_branch_ref: dummy,
        distributive: false,
    });
    let ctor = graph.intern_node(SemanticNodeData::ConstructorType { signature: func });
    let alias = graph.intern_node(SemanticNodeData::Alias(open_obj));
    let merged = graph.intern_node(SemanticNodeData::MergedDecl {
        contributors: StdArc::from(vec![open_obj].into_boxed_slice()),
    });

    // Leaf / carrier arms the earlier collapsed-intersection test does not
    // enumerate. Each raises identically under both the full fold and the
    // root-only projection.
    let literal = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("lit".to_string()),
    ));
    let infer = graph.intern_node(SemanticNodeData::Infer {
        name: StdArc::from("I"),
    });
    let template = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: StdArc::from(vec![StdArc::from("q")].into_boxed_slice()),
        expressions: StdArc::from(Vec::new().into_boxed_slice()),
    });
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity {
            canonical_id: StdArc::from("/p.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: HashValue::default(),
            decl_name: StdArc::from("Owner"),
        },
        param_index: 0,
        constraint: None,
        default: None,
        display_name: StdArc::from("T"),
    });
    let synthetic = graph.intern_node(SemanticNodeData::SyntheticBinding {
        id: SyntheticBindingId {
            scope_canonical_id: StdArc::from("/p.ts"),
            surface_kind: verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: None,
            binding_name: StdArc::from("b"),
        },
        value_node: 0,
    });
    // The three `Ref` carriers — each raises to a `TypeExpr::Ref`
    // (`RaisedRootKind::Reference`).
    let decl_ref = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: StdArc::from("/p.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: HashValue::default(),
            decl_name: StdArc::from("Foo"),
        },
    });
    let instantiation_ref = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: DeclIdentity {
            canonical_id: StdArc::from("/p.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: HashValue::default(),
            decl_name: StdArc::from("Gen"),
        },
        args: StdArc::from(vec![string].into_boxed_slice()),
    });
    let bare_ref = graph.intern_node(SemanticNodeData::new_bare_ref(
        StdArc::from("Bare"),
        NodeScopeId::Global,
        StdArc::from(Vec::new().into_boxed_slice()),
    ));
    let tuple = graph.intern_node(SemanticNodeData::Tuple {
        elements: StdArc::from(
            vec![TupleElement {
                label: None,
                value: string,
                optional: false,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    // The `Opaque(RecursiveRef)` sub-arm (distinct from the `_` Opaque conduit) —
    // raises to a materialized/expanded leaf (root `Other`).
    let recursive_ref = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
        name: StdArc::from("Rec"),
    }));

    let mapped_with = |value_expr| {
        graph.intern_node(SemanticNodeData::Mapped {
            source: dummy,
            mapper: MapperKey {
                parameter_node: dummy,
                key_space: dummy,
                value_expr,
                optionality: OptionalityMod::Keep,
                readonly: ReadonlyMod::Keep,
                name_remap: None,
                kind: MapperKind::Identity,
            },
        })
    };
    // The mapped VALUE raises to `semanticMiss` (Opaque(Miss)) vs a concrete
    // non-miss leaf — the two Mapped sub-cases that flip `value_is_semantic_miss`.
    let mapped_miss = mapped_with(graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss)));
    let mapped_nonmiss = mapped_with(string);

    let assert_parity = |label: &str, node| {
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
            "[{label}] a well-formed node must fold to Some"
        );
    };

    for (label, node) in [
        ("reference", reference),
        ("import", import),
        ("raw_fallback", raw_fallback),
        ("opaque_other", opaque_other),
        ("array", array),
        ("conditional", conditional),
        ("ctor", ctor),
        ("alias", alias),
        ("merged", merged),
        ("mapped_miss", mapped_miss),
        ("mapped_nonmiss", mapped_nonmiss),
        ("open_obj", open_obj),
        ("literal", literal),
        ("infer", infer),
        ("template", template),
        ("type_param", type_param),
        ("synthetic", synthetic),
        ("decl_ref", decl_ref),
        ("instantiation_ref", instantiation_ref),
        ("bare_ref", bare_ref),
        ("tuple", tuple),
        ("recursive_ref", recursive_ref),
    ] {
        assert_parity(label, node);
    }

    // Concrete-class anchors: each new arm produces its DISTINCT root class (not a
    // constant), and the two Mapped sub-cases differ in `value_is_semantic_miss`.
    let root_kind = |node| {
        let mut active = FxHashSet::default();
        super::project_root_summary(&dispatch, node, &mut active).map(|s| s.root_kind)
    };
    assert_eq!(root_kind(reference), Some(RaisedRootKind::Reference));
    assert_eq!(root_kind(conditional), Some(RaisedRootKind::Conditional));
    assert_eq!(
        root_kind(mapped_miss),
        Some(RaisedRootKind::Mapped {
            value_is_semantic_miss: true
        })
    );
    assert_eq!(
        root_kind(mapped_nonmiss),
        Some(RaisedRootKind::Mapped {
            value_is_semantic_miss: false
        })
    );
    assert_eq!(root_kind(alias), Some(RaisedRootKind::Object));
    assert_eq!(root_kind(merged), Some(RaisedRootKind::Object));
    assert_eq!(root_kind(array), Some(RaisedRootKind::Other));
    assert_eq!(root_kind(import), Some(RaisedRootKind::Other));
    // The three `Ref` carriers classify `Reference` — a mis-wire reverting the
    // reference arm to a plain leaf flips these to `Other` (and FAILS the
    // `assert_parity` above, which compares against the full fold).
    assert_eq!(root_kind(decl_ref), Some(RaisedRootKind::Reference));
    assert_eq!(
        root_kind(instantiation_ref),
        Some(RaisedRootKind::Reference)
    );
    assert_eq!(root_kind(bare_ref), Some(RaisedRootKind::Reference));
    // Leaf / non-root-mirror arms classify `Other` (still asserted against the
    // full fold via `assert_parity` above; these pin the concrete class so a
    // production mis-wire that flips any of them is caught).
    assert_eq!(root_kind(literal), Some(RaisedRootKind::Other));
    assert_eq!(root_kind(infer), Some(RaisedRootKind::Other));
    assert_eq!(root_kind(template), Some(RaisedRootKind::Other));
    assert_eq!(root_kind(type_param), Some(RaisedRootKind::Other));
    assert_eq!(root_kind(synthetic), Some(RaisedRootKind::Other));
    assert_eq!(root_kind(tuple), Some(RaisedRootKind::Other));
    assert_eq!(root_kind(recursive_ref), Some(RaisedRootKind::Other));
}

/// MALFORMED-CHILD `None` PARITY: a node with a DANGLING required child (a child
/// id with no arena entry — production-unreachable today) folds to `None` under
/// BOTH the full fold and the root-only projection, because the root-only
/// projection propagates the SAME required-edge `?` aborts `fold_node` does.
/// Object MEMBER values are deliberately NOT a required edge: the full fold wraps a
/// missing member as a sentinel, so a dangling member value stays `Some` on both
/// sides — pinning that root-only must NOT deep-walk member values.
///
/// DISCRIMINATING: a mutation that drops the required-edge `?` from a root-only arm
/// makes that arm return `Some` while `fold_node` still returns `None`, failing the
/// `root_only.is_none()` assertion; a mutation that ADDED a member deep-walk would
/// flip the object-member case to `None` and fail its `Some` assertion. The corpus
/// covers EVERY required edge ONE-AT-A-TIME so dropping any single `?` is caught:
/// `KeyOf.base`, `Array.element`, `IndexedAccess.object` + a `TypeNode` index, EACH
/// of the four `Conditional` operands (`check` / `extends` / `true_branch_ref` /
/// `false_branch_ref`), `Mapped`'s source / value / OPTIONAL `name_remap`, and the
/// `ConstructorType` signature.
#[test]
fn root_only_projection_returns_none_on_malformed_required_child_like_full_fold() {
    use std::sync::Arc as StdArc;

    use rustc_hash::FxHashSet;

    use super::super::RaisedRootKind;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        IndexKey, MapperKey, MapperKind, OptionalityMod, PrimitiveKind, ReadonlyMod,
        SemanticNodeData, SemanticNodeId, SurfaceMember, SurfaceView,
    };
    use crate::VerterHost;
    use verter_type_expr::MemberVisibility;

    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let dispatch = ProjectSemanticDispatch::new(&host);

    // A node id with no arena entry — a dangling required child.
    let dangling = SemanticNodeId(u64::MAX);
    let present = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    let mapper_with = |key_space, value_expr| MapperKey {
        parameter_node: present,
        key_space,
        value_expr,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: None,
        kind: MapperKind::Identity,
    };

    let malformed = [
        (
            "keyof.base",
            graph.intern_node(SemanticNodeData::KeyOf { base: dangling }),
        ),
        (
            "array.element",
            graph.intern_node(SemanticNodeData::Array {
                element: dangling,
                readonly: false,
            }),
        ),
        (
            "indexed.object",
            graph.intern_node(SemanticNodeData::IndexedAccess {
                object: dangling,
                index: IndexKey::String(StdArc::from("x")),
            }),
        ),
        (
            "indexed.index_typenode",
            graph.intern_node(SemanticNodeData::IndexedAccess {
                object: present,
                index: IndexKey::TypeNode(dangling),
            }),
        ),
        (
            "conditional.check",
            graph.intern_node(SemanticNodeData::Conditional {
                check: dangling,
                extends: present,
                true_branch_ref: present,
                false_branch_ref: present,
                distributive: false,
            }),
        ),
        (
            "conditional.extends",
            graph.intern_node(SemanticNodeData::Conditional {
                check: present,
                extends: dangling,
                true_branch_ref: present,
                false_branch_ref: present,
                distributive: false,
            }),
        ),
        (
            "conditional.true_branch_ref",
            graph.intern_node(SemanticNodeData::Conditional {
                check: present,
                extends: present,
                true_branch_ref: dangling,
                false_branch_ref: present,
                distributive: false,
            }),
        ),
        (
            "conditional.false_branch_ref",
            graph.intern_node(SemanticNodeData::Conditional {
                check: present,
                extends: present,
                true_branch_ref: present,
                false_branch_ref: dangling,
                distributive: false,
            }),
        ),
        (
            "mapped.value",
            graph.intern_node(SemanticNodeData::Mapped {
                source: present,
                mapper: mapper_with(present, dangling),
            }),
        ),
        (
            "mapped.source",
            graph.intern_node(SemanticNodeData::Mapped {
                source: present,
                mapper: mapper_with(dangling, present),
            }),
        ),
        (
            // The OPTIONAL name-remap edge: present source + value, dangling
            // `name_remap`. `fold_node`'s Mapped arm `?`-propagates the folded
            // name-remap, and `project_root_summary` mirrors it with
            // `if let Some(remap) = mapper.name_remap { project_root_summary(..)? }`,
            // so a dangling remap aborts BOTH to None.
            "mapped.name_remap",
            graph.intern_node(SemanticNodeData::Mapped {
                source: present,
                mapper: MapperKey {
                    parameter_node: present,
                    key_space: present,
                    value_expr: present,
                    optionality: OptionalityMod::Keep,
                    readonly: ReadonlyMod::Keep,
                    name_remap: Some(dangling),
                    kind: MapperKind::Identity,
                },
            }),
        ),
        (
            "ctor.signature",
            graph.intern_node(SemanticNodeData::ConstructorType {
                signature: dangling,
            }),
        ),
    ];

    for (label, node) in malformed {
        let full = {
            let mut alg = super::RaisedFactsAlg;
            let mut active = FxHashSet::default();
            super::super::fold_node(&mut alg, &dispatch, node, &mut active)
        };
        let root_only = {
            let mut active = FxHashSet::default();
            super::project_root_summary(&dispatch, node, &mut active)
        };
        assert!(
            full.is_none(),
            "[{label}] full fold returns None for a dangling required child"
        );
        assert!(
            root_only.is_none(),
            "[{label}] root-only projection MUST also return None for a dangling required child \
             (a mutation removing the required-edge `?` makes this Some and FAILS)"
        );
    }

    // Object-member short-circuit POSITIVE pin: an Object whose MEMBER VALUE is
    // dangling is NOT a required-edge `None` — the full fold wraps the missing value
    // as a sentinel (member present), so BOTH sides return Some with an Object root.
    // A root-only member deep-walk would FALSELY propagate None here.
    let dangling_member_obj = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: StdArc::from(
            vec![SurfaceMember {
                visibility: MemberVisibility::Public,
                name: StdArc::from("a"),
                value: dangling,
                optional: false,
                readonly: false,
                is_method: false,
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                spans: Default::default(),
                declaration_origin: None,
            }]
            .into_boxed_slice(),
        ),
        call_signatures: StdArc::from(Vec::new().into_boxed_slice()),
        construct_signatures: StdArc::from(Vec::new().into_boxed_slice()),
        index_signatures: StdArc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));
    let full_obj = {
        let mut alg = super::RaisedFactsAlg;
        let mut active = FxHashSet::default();
        super::super::fold_node(&mut alg, &dispatch, dangling_member_obj, &mut active)
            .map(|s| s.root_kind)
    };
    let root_only_obj = {
        let mut active = FxHashSet::default();
        super::project_root_summary(&dispatch, dangling_member_obj, &mut active)
            .map(|s| s.root_kind)
    };
    assert_eq!(
        full_obj,
        Some(RaisedRootKind::Object),
        "an Object with a dangling member value still folds (member -> sentinel), root Object"
    );
    assert_eq!(
        root_only_obj, full_obj,
        "root-only must NOT deep-walk member values: a dangling member value stays Some(Object), \
         matching the full fold (a member deep-walk would FALSELY return None)"
    );
}

/// DELEGATION GUARD: `project_node_root_kind` (the source the root-kind classifiers
/// read) delegates to the ROOT-ONLY projection (`node_domain::project_root_summary`),
/// NOT the full `fold_node` — so the short-circuit perf win is real, not silently
/// reverted to a whole-tree walk.
///
/// DISCRIMINATING: reverting the body to `fold_node(...).root_kind` makes it
/// reference `fold_node` and drop `project_root_summary`, failing BOTH asserts.
#[test]
fn project_node_root_kind_delegates_to_root_only_projection_not_full_fold() {
    const SRC: &str = include_str!("mod.rs");
    let start = SRC
        .find("fn project_node_root_kind")
        .expect("project_node_root_kind is defined in mod.rs");
    let after = &SRC[start..];
    let end = after.find("\n}").map(|e| e + 2).unwrap_or(after.len());
    let body = &after[..end];
    assert!(
        body.contains("project_root_summary"),
        "project_node_root_kind must delegate to the root-only projection \
         node_domain::project_root_summary; body:\n{body}"
    );
    assert!(
        !body.contains("fold_node"),
        "project_node_root_kind must NOT use the full fold_node (the root-only projection is the \
         short-circuit authority); body:\n{body}"
    );
}
