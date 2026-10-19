//! The key-domain openness walk and the closedness proofs it delegates to
//! — declaration alias hops, recipe escapes, nested mapped types,
//! instantiation arguments — run on one explicit work stack. A chain
//! thousands of hops deep decides on a small thread exactly as its shallow
//! control does: no re-entry ceiling turns the rest of the chain into an
//! undecidable answer, a genuine cycle still refuses, and a work-budget
//! refusal stays a typed `Unavailable` that never poisons a later proof.

use std::fmt::Write as _;
use std::sync::Arc;

use super::carrier_head_resolution_tests::{host, upsert_ts};
use super::raise::{
    builtin_lowering_argument_is_open, enable_dispatch_trace_for_test,
    keys_built_cold_more_than_once, mapped_type_key_domain_is_open_or_unknown,
    prepared_decl_body_is_closed, utility_enumeration_domain_is_open_or_unknown, ClosednessVerdict,
    DISPATCH_TRACE, ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET,
};
use super::ProjectSemanticDispatch;
use crate::VerterHost;
use verter_type_engine::semantic_query::{
    DeclIdentity, HashValue, IndexSignature, MapperKey, MapperKind, OptionalityMod, PathSegment,
    PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryError, QueryResult,
    ReadonlyMod, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput, SurfaceMember,
};

type Dispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;

/// Declaration hops in a chain: past the 256 native re-entries the walk
/// once allowed, each hop a declaration the host lowers.
const DEPTH: usize = 300;
/// Nesting of interned nodes: far past any native-stack bound.
const NEST_DEPTH: usize = 1_000;
/// The thread every deep probe runs on.
const SMALL_STACK: usize = 1 << 20;
const FILE: &str = "/chain.ts";

fn on_a_small_stack<R: Send + 'static>(probe: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(probe)
        .expect("spawn the small-stack thread")
        .join()
        .expect("the probe answers without exhausting its stack")
}

fn decl_closedness(host: &VerterHost, name: &str, budget: &mut u32) -> ClosednessVerdict {
    let dispatch = ProjectSemanticDispatch::new(host);
    let _builder = dispatch.enter_construction_scope();
    prepared_decl_body_is_closed(
        &dispatch,
        FILE,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        name,
        budget,
    )
}

fn full_budget() -> u32 {
    ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET
}

/// `type A0 = A1; … type A{depth-1} = A{depth}; type A{depth} = <bottom>;`
fn alias_chain(depth: usize, bottom: &str) -> String {
    let mut source = String::new();
    // bounded-loop: authored finite fixture depth supplied by the test.
    for i in 0..depth {
        let _ = writeln!(source, "export type A{i} = A{};", i + 1);
    }
    let _ = writeln!(source, "export type A{depth} = {bottom};");
    source
}

/// A chain whose hops alternate a bare alias, a `Pick` over the next link and
/// a mapped type over the next link's keys — a transparent hop, a recipe
/// escape into a builtin instantiation and a recipe escape into a nested
/// mapped type.
fn mixed_chain(depth: usize, bottom: &str) -> String {
    let mut source = String::new();
    // bounded-loop: authored finite fixture depth supplied by the test.
    for i in 0..depth {
        let next = i + 1;
        let _ = match i % 3 {
            0 => writeln!(source, "export type A{i} = A{next};"),
            1 => writeln!(source, "export type A{i} = Pick<A{next}, 'a'>;"),
            _ => writeln!(
                source,
                "export type A{i} = {{ [K in keyof A{next}]: string }};"
            ),
        };
    }
    let _ = writeln!(source, "export type A{depth} = {bottom};");
    source
}

fn chain_host(source: String) -> Arc<VerterHost> {
    let host = Arc::new(host());
    upsert_ts(&host, FILE, &source);
    host
}

fn decl_ref(dispatch: &Dispatch<'_>, name: &str) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: verter_type_expr::OwnedStr::from(FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: HashValue::default(),
            decl_name: verter_type_expr::OwnedStr::from(name),
        },
    })
}

fn primitive(dispatch: &Dispatch<'_>, kind: PrimitiveKind) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(kind))
}

fn string_literal(dispatch: &Dispatch<'_>, text: &str) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String(text.into()),
    ))
}

fn outer_type_param(dispatch: &Dispatch<'_>, name: &str) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic(name),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: verter_type_expr::OwnedStr::from(name),
    })
}

fn keyof(dispatch: &Dispatch<'_>, base: SemanticNodeId) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::KeyOf { base })
}

/// `{ [K in key_space]: value }` with a fresh binder per call site name.
fn mapper(
    dispatch: &Dispatch<'_>,
    binder: &str,
    key_space: SemanticNodeId,
    value: SemanticNodeId,
) -> MapperKey {
    MapperKey {
        over_type_variable: false,
        parameter_node: outer_type_param(dispatch, binder),
        key_space,
        value_expr: value,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: None,
        kind: MapperKind::Computed,
    }
}

fn mapped(dispatch: &Dispatch<'_>, source: SemanticNodeId, mapper: MapperKey) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::Mapped { source, mapper })
}

fn object_a(dispatch: &Dispatch<'_>, value: SemanticNodeId) -> SemanticNodeId {
    let member = SurfaceMember {
        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
        visibility: verter_type_expr::MemberVisibility::Public,
        key: verter_type_engine::semantic_query::AuthoredPropertyKey::string("a"),
        value,
        optional: false,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        declared_in_macro_type_arg: verter_type_engine::semantic_query::MacroOwnBodyStamp::NEUTRAL,
        merge_role: verter_type_engine::semantic_query::MergeRoleStamp::NEUTRAL,
        spans: Default::default(),
        declaration_origin: None,
    };
    dispatch.graph().intern_node(SemanticNodeData::Object(
        verter_type_engine::test_surface_view! {
            members: Arc::from(vec![member].into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        },
    ))
}

fn builtin(dispatch: &Dispatch<'_>, utility: &str, args: &[SemanticNodeId]) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::InstantiationRef {
            base: builtin_identity(utility),
            args: Arc::<[SemanticNodeId]>::from(args.to_vec().into_boxed_slice()).into(),
        })
}

fn builtin_identity(utility: &str) -> DeclIdentity {
    DeclIdentity {
        canonical_id: verter_type_expr::OwnedStr::from("__builtin__"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: HashValue::default(),
        decl_name: verter_type_expr::OwnedStr::from(utility),
    }
}

/// `{ [K in keyof M]: string }` nested `depth` times around `bottom`.
fn nested_mapped(dispatch: &Dispatch<'_>, depth: usize, bottom: SemanticNodeId) -> SemanticNodeId {
    let string_ty = primitive(dispatch, PrimitiveKind::String);
    let mut node = bottom;
    // bounded-loop: test fixture — fold a fixed nesting depth.
    for level in 0..depth {
        let key_space = keyof(dispatch, node);
        let binder = format!("K{level}");
        let mapper = mapper(dispatch, &binder, key_space, string_ty);
        node = mapped(dispatch, node, mapper);
    }
    node
}

/// `Pick<Pick<…Pick<bottom, 'a'>…, 'a'>, 'a'>` nested `depth` times.
fn nested_pick(dispatch: &Dispatch<'_>, depth: usize, bottom: SemanticNodeId) -> SemanticNodeId {
    let key = string_literal(dispatch, "a");
    let mut node = bottom;
    // bounded-loop: test fixture — fold a fixed nesting depth.
    for _ in 0..depth {
        node = builtin(dispatch, "Pick", &[node, key]);
    }
    node
}

/// The member names the empty-path Shallow projection of `node` publishes;
/// empty for a carrier-stop.
fn shallow_member_names(dispatch: &Dispatch<'_>, node: SemanticNodeId) -> Vec<String> {
    let empty_path: verter_type_expr::OwnedSlice<PathSegment> =
        verter_type_expr::OwnedSlice::default();
    let terminal = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: node,
        path: empty_path,
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        QueryResult::Recursive(id) => id,
        other => panic!("a Shallow projection did not yield a node: {other:?}"),
    };
    match dispatch.graph().node_data(terminal).as_deref() {
        Some(SemanticNodeData::Object(view)) => {
            let mut names: Vec<String> = view
                .positive_members()
                .iter()
                .map(|m| m.string_name().expect("string-key fixture").to_string())
                .collect();
            names.sort();
            names
        }
        _ => Vec::new(),
    }
}

/// `{ [K in keyof A0]: string }` over the chain's head, projected Shallow.
fn mapped_over_head_names(host: &VerterHost) -> Vec<String> {
    let dispatch = ProjectSemanticDispatch::new(host);
    let _builder = dispatch.enter_construction_scope();
    let head = decl_ref(&dispatch, "A0");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);
    let key_space = keyof(&dispatch, head);
    let mapper = mapper(&dispatch, "K", key_space, string_ty);
    let node = mapped(&dispatch, head, mapper);
    shallow_member_names(&dispatch, node)
}

#[test]
fn a_long_alias_chain_decides_on_a_small_stack() {
    on_a_small_stack(|| {
        let closed = chain_host(alias_chain(DEPTH, "{ a: string }"));
        let mut budget = full_budget();
        assert_eq!(
            decl_closedness(&closed, &format!("A{}", DEPTH - 4), &mut budget),
            ClosednessVerdict::ProvenClosed,
            "control: the chain's last few hops prove the closed bottom"
        );
        let mut budget = full_budget();
        assert_eq!(
            decl_closedness(&closed, "A0", &mut budget),
            ClosednessVerdict::ProvenClosed,
            "a {DEPTH}-hop alias chain onto a closed object is as closed as its shallow control"
        );
        assert_eq!(
            mapped_over_head_names(&closed),
            vec!["a".to_string()],
            "a mapped type over the deep chain enumerates the bottom's key, not a carrier-stop"
        );

        let open = chain_host(alias_chain(DEPTH, "() => string"));
        let mut budget = full_budget();
        assert_eq!(
            decl_closedness(&open, "A0", &mut budget),
            ClosednessVerdict::ProvenOpen,
            "the same chain onto a function body proves OPEN — the deep answer is the bottom's"
        );

        let cyclic = chain_host(alias_chain(DEPTH, "A0"));
        let mut budget = full_budget();
        assert_eq!(
            decl_closedness(&cyclic, "A0", &mut budget),
            ClosednessVerdict::Unavailable,
            "a chain that closes back onto its head is a refused cycle, never a proof"
        );
        assert!(
            budget > 0,
            "the cycle refuses through the in-flight guard, not by running out of budget"
        );
    });
}

/// What deciding the head `A0` of one chain cost.
struct ChainProbe {
    verdict: ClosednessVerdict,
    /// The budget the evaluation charged.
    charged: u32,
    /// The dispatcher queries entered, the ones nested evaluations issue
    /// included.
    dispatches: usize,
    /// The queries built cold more than once — each a stored answer that
    /// should have served the repeat.
    rebuilt: usize,
}

/// Decide `A0` of `source` on a fresh small-stack thread, whose dispatch
/// counters start empty.
fn probe_chain_head(source: String) -> ChainProbe {
    on_a_small_stack(move || {
        let host = chain_host(source);
        let _trace = enable_dispatch_trace_for_test();
        let mut budget = full_budget();
        let verdict = decl_closedness(&host, "A0", &mut budget);
        ChainProbe {
            verdict,
            charged: full_budget() - budget,
            dispatches: DISPATCH_TRACE.with(|trace| trace.borrow().len()),
            rebuilt: keys_built_cold_more_than_once(),
        }
    })
}

#[test]
fn mixed_declaration_hops_decide_on_a_small_stack() {
    on_a_small_stack(|| {
        let shallow = chain_host(mixed_chain(6, "{ a: string; b: number }"));
        let mut budget = full_budget();
        assert_eq!(
            decl_closedness(&shallow, "A0", &mut budget),
            ClosednessVerdict::ProvenClosed,
            "control: a short alias / Pick / mapped chain onto a closed object proves closed"
        );

        let deep = chain_host(mixed_chain(DEPTH, "{ a: string; b: number }"));
        let mut budget = full_budget();
        assert_eq!(
            decl_closedness(&deep, "A0", &mut budget),
            ClosednessVerdict::ProvenClosed,
            "a {DEPTH}-hop alias / Pick / mapped chain decides as its shallow control does"
        );
    });

    // An open or cyclic bottom leaves every `Pick` hop unprovable by the
    // registry rule, so each one also reads the reduced instantiation —
    // a query that asks the enumeration gate about the rest of the chain
    // again. The deep chain must still decide as its control does, on the
    // small stack, with work linear in its hops: no query rebuilt, no
    // suffix re-derived by each nested evaluation.
    for (bottom, label) in [("() => string", "a function body"), ("A0", "its own head")] {
        let control = probe_chain_head(mixed_chain(6, bottom));
        assert_ne!(
            control.verdict,
            ClosednessVerdict::ProvenClosed,
            "control: the short chain onto {label} is not proven closed"
        );
        let deep = probe_chain_head(mixed_chain(DEPTH, bottom));
        assert_eq!(
            deep.verdict, control.verdict,
            "a {DEPTH}-hop mixed chain onto {label} decides as its shallow control does"
        );
        assert_eq!(
            deep.rebuilt, 0,
            "the {DEPTH}-hop chain onto {label} built {} queries cold more than once",
            deep.rebuilt
        );
        let linear = |control: usize, deep: usize| deep * 6 <= control * DEPTH * 2;
        assert!(
            linear(control.charged as usize, deep.charged as usize),
            "the evaluation's work grows linearly with the hops onto {label}: {} at 6, {} at \
             {DEPTH}",
            control.charged,
            deep.charged
        );
        assert!(
            linear(control.dispatches, deep.dispatches),
            "the queries the hops onto {label} issue grow linearly: {} at 6, {} at {DEPTH}",
            control.dispatches,
            deep.dispatches
        );
    }
}

#[test]
fn nested_mapped_key_domains_decide_on_a_small_stack() {
    on_a_small_stack(|| {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let _builder = dispatch.enter_construction_scope();
        let string_ty = primitive(&dispatch, PrimitiveKind::String);
        let closed_bottom = object_a(&dispatch, string_ty);
        let open_bottom = outer_type_param(&dispatch, "T");
        let open = |source: SemanticNodeId| {
            let key_space = keyof(&dispatch, source);
            let mapper = mapper(&dispatch, "Outer", key_space, string_ty);
            mapped_type_key_domain_is_open_or_unknown(&dispatch, source, &mapper)
        };

        assert!(
            !open(nested_mapped(&dispatch, 4, closed_bottom)),
            "control: four nested mapped types over a closed object have a closed key domain"
        );
        assert!(
            open(nested_mapped(&dispatch, 4, open_bottom)),
            "control: four nested mapped types over an outer generic have an open key domain"
        );
        assert!(
            !open(nested_mapped(&dispatch, NEST_DEPTH, closed_bottom)),
            "{NEST_DEPTH} nested mapped types over a closed object are as closed as the control"
        );
        assert!(
            open(nested_mapped(&dispatch, NEST_DEPTH, open_bottom)),
            "{NEST_DEPTH} nested mapped types over an outer generic stay open"
        );
    });
}

#[test]
fn nested_instantiation_arguments_decide_on_a_small_stack() {
    on_a_small_stack(|| {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let _builder = dispatch.enter_construction_scope();
        let string_ty = primitive(&dispatch, PrimitiveKind::String);
        let key = string_literal(&dispatch, "a");
        let pick = builtin_identity("Pick");
        let closed_bottom = object_a(&dispatch, string_ty);
        let open_bottom = outer_type_param(&dispatch, "T");

        assert!(
            !utility_enumeration_domain_is_open_or_unknown(
                &dispatch,
                &pick,
                &[nested_pick(&dispatch, 4, closed_bottom), key],
            ),
            "control: Pick over four nested Picks of a closed object enumerates"
        );
        assert!(
            !utility_enumeration_domain_is_open_or_unknown(
                &dispatch,
                &pick,
                &[nested_pick(&dispatch, NEST_DEPTH, closed_bottom), key],
            ),
            "Pick over {NEST_DEPTH} nested Picks of a closed object enumerates like the control"
        );
        assert!(
            builtin_lowering_argument_is_open(&dispatch, nested_pick(&dispatch, 4, open_bottom)),
            "control: four nested Picks of an outer generic still reach it"
        );
        assert!(
            builtin_lowering_argument_is_open(
                &dispatch,
                nested_pick(&dispatch, NEST_DEPTH, open_bottom)
            ),
            "{NEST_DEPTH} nested Picks of an outer generic still reach it"
        );
        assert!(
            !builtin_lowering_argument_is_open(
                &dispatch,
                nested_pick(&dispatch, NEST_DEPTH, closed_bottom)
            ),
            "{NEST_DEPTH} nested Picks of a closed object reach no outer generic"
        );
    });
}

#[test]
fn a_budget_refusal_mid_chain_is_unavailable_and_leaves_no_proof_open() {
    on_a_small_stack(|| {
        let closed = chain_host(alias_chain(DEPTH, "{ a: string }"));
        // One unit per hop: the chain needs two (the declaration and its
        // recipe), so the budget runs out halfway down.
        let mut budget = DEPTH as u32;
        let refused = decl_closedness(&closed, "A0", &mut budget);
        assert_eq!(
            refused,
            ClosednessVerdict::Unavailable,
            "a budget that runs out mid-chain is a refusal — neither a proof of openness nor \
             of closedness"
        );
        assert_eq!(budget, 0, "the refusal is the budget's");

        // The refused run left no declaration on the dispatch-wide in-flight
        // guard (a leaked entry would refuse the retry as a cycle), and —
        // run where a build records its proofs for reuse — kept none of the
        // proofs the refusal decided, so the retry derives its own.
        let dispatch = ProjectSemanticDispatch::new(closed.as_ref());
        let _builder = dispatch.enter_construction_scope();
        let (refused, _) = in_a_traced_build(&closed, &dispatch, || {
            let mut first = DEPTH as u32;
            prepared_decl_body_is_closed(
                &dispatch,
                FILE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "A0",
                &mut first,
            )
        });
        assert_eq!(refused, ClosednessVerdict::Unavailable);
        let (retried, _) = in_a_traced_build(&closed, &dispatch, || {
            let mut retry = full_budget();
            prepared_decl_body_is_closed(
                &dispatch,
                FILE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "A0",
                &mut retry,
            )
        });
        assert_eq!(
            retried,
            ClosednessVerdict::ProvenClosed,
            "the same dispatcher re-derives the proof with a full budget after a refusal"
        );
    });
}

/// Run `probe` where a cold build runs it — under a fact tracer, with a
/// build-local frame open, the scope a completed proof is recorded in —
/// and report whether the finalised read set roots the chain's file.
fn in_a_traced_build<R>(
    host: &VerterHost,
    dispatch: &Dispatch<'_>,
    probe: impl FnOnce() -> R,
) -> (R, bool) {
    use verter_session_query::facts::{
        fact_cache::FactVersionRef, fact_read_set::FactReadSetFinalise,
    };
    let (value, read_set) = host.with_fact_tracer(
        verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
        || {
            let frame = super::BuildLocalTaintGuard::push(&dispatch.build_local_taint);
            let value = probe();
            let _ = frame.finish();
            value
        },
    );
    let facts = match read_set.finalise() {
        FactReadSetFinalise::Ok(facts) => facts,
        other => panic!("the chain's tracer must finalise: {other:?}"),
    };
    let rooted = facts.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == FILE
        )
    });
    (value, rooted)
}

#[test]
fn a_reused_proof_charges_and_roots_what_running_it_did() {
    on_a_small_stack(|| {
        let host = chain_host(mixed_chain(12, "() => string"));
        let dispatch = ProjectSemanticDispatch::new(host.as_ref());
        let _builder = dispatch.enter_construction_scope();
        let prove = || {
            let _trace = enable_dispatch_trace_for_test();
            let mut budget = full_budget();
            let verdict = prepared_decl_body_is_closed(
                &dispatch,
                FILE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "A0",
                &mut budget,
            );
            let dispatches = DISPATCH_TRACE.with(|trace| trace.borrow().len());
            (verdict, full_budget() - budget, dispatches)
        };
        let ((verdict, charged, dispatches), rooted) = in_a_traced_build(&host, &dispatch, prove);
        assert_eq!(verdict, ClosednessVerdict::ProvenOpen);
        assert!(
            dispatches > 0,
            "control: the first proof issues its reductions"
        );
        assert!(rooted, "control: the first proof roots the chain's file");

        let ((reused, recharged, redispatched), rerooted) =
            in_a_traced_build(&host, &dispatch, prove);
        assert_eq!(
            redispatched, 0,
            "the transaction answers the repeated proof without issuing a query"
        );
        assert_eq!(reused, verdict, "a reused proof answers as running it did");
        assert_eq!(
            recharged, charged,
            "a reused proof charges the evaluation what running it charged"
        );
        assert!(
            rerooted,
            "a reused proof roots the chain's file on the tracer live now, as running it would"
        );
    });
}

#[test]
fn a_cancelled_projection_over_a_deep_chain_publishes_no_lost_keys() {
    on_a_small_stack(|| {
        let closed = chain_host(alias_chain(DEPTH, "{ a: string }"));
        let cancelled = verter_type_engine::request_context::RequestContext::new(
            71_001,
            Arc::from(FILE),
            false,
            None,
        );
        cancelled.cancel();
        {
            let _guard =
                verter_type_engine::request_context::RequestContextGuard::install(cancelled);
            let dispatch = ProjectSemanticDispatch::new(closed.as_ref());
            let _builder = dispatch.enter_construction_scope();
            let head = decl_ref(&dispatch, "A0");
            let string_ty = primitive(&dispatch, PrimitiveKind::String);
            let key_space = keyof(&dispatch, head);
            let mapper = mapper(&dispatch, "K", key_space, string_ty);
            let node = mapped(&dispatch, head, mapper);
            let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
                base: node,
                path: verter_type_expr::OwnedSlice::default(),
                context: ProjectionReductionContext::published(ProjectionMode::Shallow),
            });
            assert!(
                matches!(read.value(), QueryResult::Error(QueryError::Cancelled)),
                "a cancelled projection answers the typed cancellation, not an empty surface: \
                 {:?}",
                read.value()
            );
            assert!(
                read.cache_suppress(),
                "a cancelled projection is ReturnOnly"
            );
        }
        let retry = verter_type_engine::request_context::RequestContext::new(
            71_002,
            Arc::from(FILE),
            false,
            None,
        );
        let _guard = verter_type_engine::request_context::RequestContextGuard::install(retry);
        assert_eq!(
            mapped_over_head_names(&closed),
            vec!["a".to_string()],
            "the request after a cancellation enumerates every key of the deep chain"
        );
    });
}
