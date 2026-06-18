//! Demand-time reduction of `TypeOf` carriers that apply instantiation
//! arguments at their reference site (`typeof f<Arg>` / `typeof C.make<Arg>`).
//!
//! The three reduction entry points that encounter a `TypeOf` carrier as an
//! operand each resolve the carrier the same way: resolve the value root
//! through the single `typeof` query key, project the carrier's dotted path,
//! and THEN apply the carrier's `type_args` to the projected signature through
//! the shared `apply_typeof_instantiation_args` substitution. The three entry
//! points are:
//!
//! - the deferred-shell evaluator (`evaluate_deferred_semantic_node`),
//! - the `PathWalker` (a `ProjectPath` hop landing on the carrier), and
//! - the semantic-reduce reducer (`raise_and_reduce_with_context`).
//!
//! These fixtures construct a `TypeOf` carrier directly through the sanctioned
//! `SemanticNodeData::new_typeof` constructor (the carrier's args are carried
//! IN at construction) and drive it through each entry point, asserting the
//! reduced result is the INSTANTIATED signature, never the still-generic one
//! and never an honest miss. The `value_root` points at a real generic value
//! declaration in a hermetic workspace so the `typeof` key resolves to a
//! genuine generic `Function` node that `apply_typeof_instantiation_args` can
//! instantiate.
//!
//! Discrimination: a reducer arm that dropped the carrier's `type_args` (the
//! pre-reduction shape) would leave the result generic — every `<T>(x: T) => T`
//! assertion below would fail. The path+args fixtures additionally fail if the
//! args are applied BEFORE the path projection (the projected member would
//! never see the args) or if the path is discarded entirely (the un-projected
//! root would surface instead of the member).

use std::sync::Arc;

use verter_type_expr::{PrimitiveName, TypeExpr};

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    PrimitiveKind, ProjectionMode, ProjectionReductionContext, ScopeId, SemanticNodeData,
    SemanticNodeId, ValueRootKey,
};
use crate::types::HostConfig;
use crate::{CompileErrorPolicy, FileLanguage, UpsertRequest, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

/// `(value_root, args)` builder for a `typeof <name>(.<path>)*<Primitive>`
/// carrier rooted in `canonical`. The single instantiation argument is the
/// interned primitive `arg`.
fn typeof_carrier_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    name: &str,
    path: &[&str],
    arg: PrimitiveKind,
) -> SemanticNodeId {
    let graph = dispatch.graph();
    let arg_node = graph.intern_node(SemanticNodeData::Primitive(arg));
    let value_root = ValueRootKey {
        scope: ScopeId {
            canonical_id: Arc::from(canonical),
            local_scope: None,
        },
        name: Arc::from(name),
    };
    let path: Arc<[Arc<str>]> = Arc::from(
        path.iter()
            .map(|segment| Arc::<str>::from(*segment))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    let args: Arc<[SemanticNodeId]> = Arc::from(vec![arg_node].into_boxed_slice());
    graph.intern_node(SemanticNodeData::new_typeof(value_root, path, args))
}

/// `(type_parameter_count, param0_primitive_name)` for a `Function` node, or
/// `None` for a non-`Function` shape (an honest miss / wrong carrier). The
/// param-0 primitive is `None` when param 0 is not a primitive (e.g. it is a
/// surviving free `TypeParam`, which is the pre-reduction generic shape).
fn function_shape(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<(usize, Option<PrimitiveName>)> {
    let graph = dispatch.graph();
    let data = graph.node_data(node)?;
    match data.as_ref() {
        SemanticNodeData::Function {
            params,
            type_parameters,
            ..
        } => {
            let param0_prim = params.first().and_then(|p| {
                graph.node_data(p.ty).and_then(|d| match d.as_ref() {
                    SemanticNodeData::Primitive(kind) => Some(primitive_name_of(*kind)),
                    _ => None,
                })
            });
            Some((type_parameters.len(), param0_prim))
        }
        _ => None,
    }
}

fn primitive_name_of(kind: PrimitiveKind) -> PrimitiveName {
    match kind {
        PrimitiveKind::String => PrimitiveName::String,
        PrimitiveKind::Number => PrimitiveName::Number,
        PrimitiveKind::Boolean => PrimitiveName::Boolean,
        other => panic!("unexpected primitive kind in fixture: {other:?}"),
    }
}

fn is_opaque_miss(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> bool {
    matches!(
        dispatch.graph().node_data(node).as_deref(),
        Some(SemanticNodeData::Opaque(_))
    )
}

const GENERIC_FN_TS: &str = "export const f = <T>(x: T): T => x;\n";
const GENERIC_METHOD_HOLDER_TS: &str =
    "export const C = { make: <T>(x: T): { v: T } => ({ v: x }) };\n";
const ONE_PARAM_FN_TS: &str = "export const g = <T>(x: T): T => x;\n";

// ── A1 / evaluate ─────────────────────────────────────────────────────────
//
// `typeof f<string>` (empty path, one arg) driven through the deferred-shell
// evaluator must reduce to the INSTANTIATED `(x: string) => string`: the
// signature loses its type parameter and its sole parameter is substituted to
// `Primitive(String)`.
//
// Discrimination: the pre-reduction arm dropped the carrier args and returned
// the resolved generic `<T>(x: T) => T` — the `type_parameters == 0` assertion
// and the `param0 == String` assertion both fail against that shape. A miss
// would fail `function_shape`.
#[test]
fn evaluate_typeof_carrier_applies_instantiation_args() {
    let host = host();
    upsert_ts(&host, "/m.ts", GENERIC_FN_TS);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let carrier = typeof_carrier_node(&dispatch, "/m.ts", "f", &[], PrimitiveKind::String);
    let reduced = dispatch.evaluate_deferred_semantic_node_with_context(
        carrier,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );

    assert!(
        !is_opaque_miss(&dispatch, reduced),
        "`typeof f<string>` must reduce to an instantiated signature, not an Opaque miss"
    );
    let (tp, param0) = function_shape(&dispatch, reduced).unwrap_or_else(|| {
        panic!(
            "evaluate of `typeof f<string>` must produce a Function; got {:?}",
            dispatch.graph().node_data(reduced).as_deref()
        )
    });
    assert_eq!(
        tp, 0,
        "the instantiated `typeof f<string>` must be NON-generic (type params stripped); \
         a dropped-args reducer leaves it generic with {tp} type params"
    );
    assert_eq!(
        param0,
        Some(PrimitiveName::String),
        "the instantiated signature's parameter must be substituted to `string`; \
         a dropped-args reducer leaves it the free `TypeParam(T)`"
    );
}

// ── A1 / walk ───────────────────────────────────────────────────────────────
//
// Same `typeof f<string>` carrier driven through the `PathWalker`'s per-segment
// `TypeOf` arm. The walker reaches the carrier mid-walk (the carrier is the
// base of a non-empty path); the arm resolves the value root, projects the
// carrier's own path, and must apply the carrier's `type_args`.
//
// A `TypeOf` carrier reduced to a `Function` is a non-projectable terminal, so
// the remaining walker segment always misses and the walk's RETURN value cannot
// witness the args-application — the post-instantiation node is an internal
// transient. The `probe_walk_typeof_resolved` test hook captures exactly the
// node the per-segment `TypeOf` arm produces AFTER applying args, so this test
// can assert the instantiation happened.
//
// Discrimination: a dropped-args walker arm sets the resolved node to the
// GENERIC `<T>(x: T) => T` (1 type param) — the `0 type params` + `string
// param` assertions fail against that shape; `None` (arm did not fire) also
// fails.
#[test]
fn walk_typeof_carrier_applies_instantiation_args() {
    use crate::semantic_query::PathSegment;
    let host = host();
    upsert_ts(&host, "/m.ts", GENERIC_FN_TS);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let carrier = typeof_carrier_node(&dispatch, "/m.ts", "f", &[], PrimitiveKind::String);
    // Carrier as the base + a non-empty path drives the per-segment loop and
    // hits the `TypeOf` arm at index 0 (an empty path skips the loop entirely
    // and would NOT exercise this arm).
    let resolved = crate::project_semantic_dispatch::walk::probe_walk_typeof_resolved(
        &dispatch,
        carrier,
        Arc::from(vec![PathSegment::Member(Arc::from("__probe__"))].into_boxed_slice()),
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    )
    .expect("the PathWalker's TypeOf arm must fire for a carrier reached mid-walk");

    assert!(
        !is_opaque_miss(&dispatch, resolved),
        "the walk arm must resolve `typeof f<string>` to an instantiated signature, not a miss"
    );
    let (tp, param0) = function_shape(&dispatch, resolved).unwrap_or_else(|| {
        panic!(
            "the walk arm's resolved `typeof f<string>` must be a Function; got {:?}",
            dispatch.graph().node_data(resolved).as_deref()
        )
    });
    assert_eq!(
        tp, 0,
        "the walk arm's resolved `typeof f<string>` must be NON-generic; a dropped-args \
         walker leaves {tp} type params"
    );
    assert_eq!(
        param0,
        Some(PrimitiveName::String),
        "the walk arm's resolved signature parameter must be substituted to `string`"
    );
}

// ── A1 / walk — INTERNAL PATH PROJECTS IN Navigate (not the caller mode) ────
//
// The walk arm's INTERNAL `typeof v.path` projection must run in
// `ProjectionMode::Navigate` — the intermediate-hop rule — matching the
// evaluate (`evaluate.rs:~413`) and raise (`raise.rs:~1513`) arms, NOT the
// caller's outer mode. With an EXPANDED (or Identity) outer demand, reusing the
// caller mode for the internal projection over-expands/materializes the
// internal carrier path too early, causing behaviour + cache-shape divergence
// from evaluate/raise.
//
// Discrimination is DIRECT on the changed line: the
// `probe_walk_typeof_internal_path_mode` hook captures the `ProjectionMode` the
// TypeOf arm dispatches the internal `typeof v.path` `ProjectPath` under. The
// outer demand is EXPANDED; pre-fix the arm reuses that caller mode for the
// internal projection (captures `Expanded`) — the `== Navigate` assertion
// FAILS. Post-fix the internal projection is forced to `Navigate` (captures
// `Navigate`) and the assertion PASSES. A `Skeleton`/`Shallow`/`Identity` outer
// demand would equally discriminate — only `Navigate` already matches.
const GENERIC_FN_HOLDER_TS: &str =
    "export const C = { make: <T>(x: T): { v: T } => ({ v: x }) };\n";

#[test]
fn walk_typeof_internal_path_projects_in_navigate_not_caller_mode() {
    use crate::semantic_query::PathSegment;
    let host = host();
    upsert_ts(&host, "/holder.ts", GENERIC_FN_HOLDER_TS);
    let dispatch = ProjectSemanticDispatch::new(&host);

    // `typeof C.make` — a NON-EMPTY internal path (`["make"]`) so the arm runs
    // the internal-path `ProjectPath` (an empty internal path would skip it).
    let graph = dispatch.graph();
    let value_root = ValueRootKey {
        scope: ScopeId {
            canonical_id: Arc::from("/holder.ts"),
            local_scope: None,
        },
        name: Arc::from("C"),
    };
    let carrier = graph.intern_node(SemanticNodeData::new_typeof(
        value_root,
        Arc::from(vec![Arc::<str>::from("make")].into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
    ));

    // Drive the carrier as the base of an OUTER non-empty path (so the
    // per-segment `TypeOf` arm fires) under an EXPANDED outer demand; capture
    // the mode the arm used for its INTERNAL `typeof C.make` projection.
    let internal_mode =
        crate::project_semantic_dispatch::walk::probe_walk_typeof_internal_path_mode(
            &dispatch,
            carrier,
            Arc::from(vec![PathSegment::Member(Arc::from("__probe__"))].into_boxed_slice()),
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .expect("the PathWalker's TypeOf arm must fire and project a non-empty internal path");

    assert_eq!(
        internal_mode,
        ProjectionMode::Navigate,
        "the walk arm's INTERNAL `typeof C.make` projection must dispatch in Navigate (the \
         intermediate-hop rule, matching evaluate.rs/raise.rs) — NOT the caller's outer \
         Expanded mode; got {internal_mode:?}"
    );
    assert_ne!(
        internal_mode,
        ProjectionMode::Expanded,
        "the internal typeof-path projection must NOT inherit the caller's Expanded mode — that \
         over-expands the intermediate hop and diverges from the evaluate/raise cache-shape"
    );
}

// ── A1 / raise ───────────────────────────────────────────────────────────────
//
// Same `typeof f<string>` carrier reduced through the semantic-reduce path
// (`raise_and_reduce_with_context`). The reduced node must be the instantiated
// signature.
#[test]
fn raise_reduce_typeof_carrier_applies_instantiation_args() {
    let host = host();
    upsert_ts(&host, "/m.ts", GENERIC_FN_TS);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let carrier = typeof_carrier_node(&dispatch, "/m.ts", "f", &[], PrimitiveKind::String);
    let materialized = dispatch.raise_and_reduce_with_context(
        carrier,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    let reduced = materialized
        .node_id
        .expect("raise_and_reduce must carry the reduced node id for the typeof carrier");

    assert!(
        !is_opaque_miss(&dispatch, reduced),
        "raise-reduce of `typeof f<string>` must instantiate, not miss"
    );
    let (tp, param0) = function_shape(&dispatch, reduced).unwrap_or_else(|| {
        panic!(
            "raise-reduce of `typeof f<string>` must produce a Function; got {:?}",
            dispatch.graph().node_data(reduced).as_deref()
        )
    });
    assert_eq!(
        tp, 0,
        "the raise-reduced `typeof f<string>` must be NON-generic; a dropped-args reducer \
         leaves {tp} type params"
    );
    assert_eq!(
        param0,
        Some(PrimitiveName::String),
        "the raise-reduced signature's parameter must be substituted to `string`"
    );
}

// ── A2 / evaluate — PATH + ARGS (order discriminator) ───────────────────────
//
// `typeof C.make<number>` carries a NON-EMPTY path (`["make"]`) AND args
// (`[number]`). Correct order: resolve `C` → project `.make` (the generic
// method) → apply `<number>`. The result is the instantiated method
// `(x: number) => { v: number }` (non-generic).
//
// This is the order discriminator: an apply-before-project bug would apply
// the args to the OBJECT root `C` (not a Function — an honest miss), and a
// dropped-path bug would resolve `C` and ignore `.make` entirely. Either
// failure mode fails the assertions below; only resolve→project→apply passes.
#[test]
fn evaluate_typeof_carrier_with_path_projects_then_applies_args() {
    let host = host();
    upsert_ts(&host, "/holder.ts", GENERIC_METHOD_HOLDER_TS);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let carrier = typeof_carrier_node(
        &dispatch,
        "/holder.ts",
        "C",
        &["make"],
        PrimitiveKind::Number,
    );
    let reduced = dispatch.evaluate_deferred_semantic_node_with_context(
        carrier,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );

    assert!(
        !is_opaque_miss(&dispatch, reduced),
        "`typeof C.make<number>` must resolve C, project `.make`, then instantiate — \
         an apply-before-project bug applies args to the non-Function object C (a miss)"
    );
    let (tp, param0) = function_shape(&dispatch, reduced).unwrap_or_else(|| {
        panic!(
            "`typeof C.make<number>` must project to the `.make` Function then instantiate; \
             got {:?}",
            dispatch.graph().node_data(reduced).as_deref()
        )
    });
    assert_eq!(
        tp, 0,
        "projected-then-instantiated `.make` must be NON-generic; got {tp} type params"
    );
    assert_eq!(
        param0,
        Some(PrimitiveName::Number),
        "the projected `.make`'s parameter must be substituted to `number` AFTER the \
         path projection reached it"
    );
}

// ── A3 / raise — DROPPED-PATH BUG (pre-existing) ────────────────────────────
//
// The semantic-reduce arm previously discarded the carrier `path`
// (`let (value_root, _path) = …`), so `typeof C.make<number>` reduced to
// `typeof C` (the un-projected object root) instead of the projected `.make`
// method. This fixture drives the path+args carrier through the raise path and
// asserts the projected MEMBER surfaces (a non-generic Function with a `number`
// parameter), which the dropped-path shape cannot produce: an un-projected `C`
// is an `Object`, not a `Function`, so `function_shape` returns `None`.
#[test]
fn raise_reduce_typeof_carrier_does_not_drop_path() {
    let host = host();
    upsert_ts(&host, "/holder.ts", GENERIC_METHOD_HOLDER_TS);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let carrier = typeof_carrier_node(
        &dispatch,
        "/holder.ts",
        "C",
        &["make"],
        PrimitiveKind::Number,
    );
    let materialized = dispatch.raise_and_reduce_with_context(
        carrier,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    let reduced = materialized
        .node_id
        .expect("raise_and_reduce must carry a node id for the path+args carrier");

    // The dropped-path reducer reduced to `typeof C` (an Object) and never
    // reached `.make`; `function_shape` returns None for that. The fixed
    // reducer projects `.make` (a Function) then instantiates it.
    let (tp, param0) = function_shape(&dispatch, reduced).unwrap_or_else(|| {
        panic!(
            "raise-reduce must PROJECT `.make` (not drop the path and surface the object \
             root `typeof C`); got {:?}",
            dispatch.graph().node_data(reduced).as_deref()
        )
    });
    assert_eq!(
        tp, 0,
        "the projected `.make` must be NON-generic after the path is honoured; got {tp} \
         type params"
    );
    assert_eq!(
        param0,
        Some(PrimitiveName::Number),
        "the projected `.make`'s parameter must be `number` — proving the path was NOT \
         dropped and the args applied to the member"
    );
}

// ── A4 — arity/shape mismatch composes an honest miss AFTER projection ───────
//
// `typeof g<A, B>` supplies TWO args to a one-type-parameter generic `g`.
// `apply_typeof_instantiation_args` returns an honest `Opaque(Miss)` for the
// over-arity case (args.len() > type_parameters.len()) — and it must do so
// AFTER projecting/resolving the root, not panic and not silently instantiate.
#[test]
fn typeof_carrier_arity_overflow_is_honest_miss_after_projection() {
    let host = host();
    upsert_ts(&host, "/one.ts", ONE_PARAM_FN_TS);
    let dispatch = ProjectSemanticDispatch::new(&host);

    // Two args for a one-type-param `g`.
    let graph = dispatch.graph();
    let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let value_root = ValueRootKey {
        scope: ScopeId {
            canonical_id: Arc::from("/one.ts"),
            local_scope: None,
        },
        name: Arc::from("g"),
    };
    let carrier = graph.intern_node(SemanticNodeData::new_typeof(
        value_root,
        Arc::from(Vec::new().into_boxed_slice()),
        Arc::from(vec![a, b].into_boxed_slice()),
    ));

    let reduced = dispatch.evaluate_deferred_semantic_node_with_context(
        carrier,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    assert!(
        is_opaque_miss(&dispatch, reduced),
        "`typeof g<A, B>` over a one-type-param generic must compose an honest Opaque(Miss) \
         after projection — not a panic and not a wrong instantiation; got {:?}",
        dispatch.graph().node_data(reduced).as_deref()
    );
}

// ── C1 — resolution-equivalence: reducer arm == eager lowering path ─────────
//
// The carrier-aware reducer's `typeof f<string>` result must match the shape
// the already-wired EAGER lowering path produces for the same input (the eager
// path lowers `typeof f<string>` and applies the args at lowering time via the
// same `apply_typeof_instantiation_args`). Both must yield a non-generic
// `(x: string) => string`. Compared by node DATA shape (interning does no
// structural dedup, so node ids differ even for equal shapes).
#[test]
fn typeof_carrier_reduction_equals_eager_lowering_path() {
    let host = host();
    upsert_ts(&host, "/m.ts", GENERIC_FN_TS);
    // Anchor the import route so the eager `typeof import(...)` lowering
    // resolves the specifier deterministically from the consumer scope.
    upsert_ts(
        &host,
        "/consumer.ts",
        "import { f } from './m';\nexport const reExport = f;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);

    // Carrier-aware reducer result.
    let carrier = typeof_carrier_node(&dispatch, "/m.ts", "f", &[], PrimitiveKind::String);
    let via_reducer = dispatch.evaluate_deferred_semantic_node_with_context(
        carrier,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    let reducer_shape = function_shape(&dispatch, via_reducer)
        .expect("carrier reducer must yield a Function for `typeof f<string>`");

    // Eager lowering path: `typeof import("./m").f<string>` lowered in the
    // consumer scope (the eager `ImportType` typeof_query branch at
    // lower.rs applies the args at lowering time).
    let eager_expr = TypeExpr::import_type(
        "./m",
        vec![Arc::from("f")],
        true,
        vec![TypeExpr::Primitive(PrimitiveName::String)],
    );
    let via_eager = dispatch
        .lower_type_expr_in_scope_with_context(
            "/consumer.ts",
            &eager_expr,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .expect("eager lowering of `typeof import(\"./m\").f<string>`");
    let eager_shape = function_shape(&dispatch, via_eager)
        .expect("eager lowering must yield a Function for `typeof import(\"./m\").f<string>`");

    assert_eq!(
        reducer_shape, eager_shape,
        "the carrier-aware reducer's instantiated `typeof f<string>` shape must equal the \
         eager lowering path's shape (both NON-generic `(x: string) => string`); \
         reducer={reducer_shape:?} eager={eager_shape:?}"
    );
    // Both must be the fully-instantiated shape (defensive: equal-but-wrong
    // would otherwise pass the equality).
    assert_eq!(
        reducer_shape,
        (0usize, Some(PrimitiveName::String)),
        "both paths must produce the NON-generic `string`-parameter signature"
    );
}

// ── B3 — typeof-substitution cycle reduced THROUGH the carrier arm ──────────
//
// A `TypeOf` carrier whose value root is a self-referentially-typed value
// (`const y = null as unknown as typeof y`) resolves through `typeof_key_for`
// into a cycle. Reducing such a carrier (WITH non-empty args) through the
// evaluate arm must TERMINATE with a sentinel (Opaque / Miss) rather than
// recurse forever: the typeof root resolution hits the engine's recursion guard
// before `apply_typeof_instantiation_args` is ever reached, and the arm
// composes the miss honestly.
//
// Discrimination: a regression that let the arm's typeof root resolution
// (root + path + args) escape the recursion guard would stack-overflow the
// worker thread; the 32 MiB stack surfaces that as a join error rather than a
// process crash. Termination + an Opaque sentinel is the contract.
#[test]
fn typeof_carrier_substitution_cycle_terminates_through_arm() {
    let host = std::sync::Arc::new(host());
    upsert_ts(
        &host,
        "/cyc.ts",
        "export const y: typeof y = null as unknown as typeof y;\n",
    );
    let host_for_thread = std::sync::Arc::clone(&host);
    let join = std::thread::Builder::new()
        .name("typeof_carrier_substitution_cycle_terminates_through_arm".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let dispatch = ProjectSemanticDispatch::new(&*host_for_thread);
            let _ = host_for_thread
                .shallow_file_state("/cyc.ts")
                .expect("/cyc.ts shallow state");
            // `typeof y<string>` carrier over the self-referential value.
            let carrier =
                typeof_carrier_node(&dispatch, "/cyc.ts", "y", &[], PrimitiveKind::String);
            let reduced = dispatch.evaluate_deferred_semantic_node_with_context(
                carrier,
                ProjectionReductionContext::published(ProjectionMode::Expanded),
            );
            is_opaque_miss(&dispatch, reduced)
        })
        .expect("spawn worker thread for typeof-substitution-cycle reduction");
    let is_miss = join.join().expect(
        "worker thread MUST terminate without panic; a stack-overflow regression in the \
         carrier arm's typeof root resolution (root + path + args) on a self-referential \
         value would surface here as a join error",
    );
    assert!(
        is_miss,
        "a self-referential `typeof y<string>` carrier must terminate with an Opaque sentinel \
         through the reducer arm, not a forged Value"
    );
}

// ── B4 — LEAK TEST: typeof resolution for an instantiate body stays within
//        the build_instantiate push→pop window (active-identity sentinel) ──────
//
// `type SelfT = typeof selfV` with `const selfV: SelfT` is a self-referential
// alias whose BODY is a `typeof` projection that resolves back to the SAME
// alias. Instantiating `SelfT` drives the engine through `build_instantiate`:
// the body `typeof selfV` is lowered/resolved WHILE `(/leak.ts, SelfT)` is on
// the `instantiate_active` stack (build.rs push→pop window). Resolving
// `typeof selfV` re-reads `selfV`'s declared type `SelfT`, which re-instantiates
// `SelfT` — a re-entry the active stack MUST catch, surfacing a
// `RecursiveRef` / `Opaque` / `Recursive` sentinel.
//
// HOW THIS DISCRIMINATES THE POST-POP LEAK specifically: the active-identity
// `RecursiveRef` can ONLY be minted while the identity is still pushed. If a
// regression moved the body's `typeof` resolution to AFTER
// `pop_instantiate_active`, the active stack would be empty at the re-entry, the
// same-identity guard would NOT fire, and the self-reference would recurse
// unboundedly (stack overflow → worker join error on the 32 MiB stack) OR
// resolve to a forged non-sentinel Value. Asserting BOTH (a) the worker
// terminates bounded AND (b) the resolved shape carries a recursion sentinel
// pins the body-time `typeof` resolution INSIDE the window. This is the same
// resolve→project→apply path the three reducer arms now run for `typeof`
// carriers, exercised through the real instantiate-body window.
#[test]
fn typeof_resolution_stays_within_instantiate_window() {
    use crate::semantic_query::{
        InstantiateContext, QueryResult, ResolvedDeclSlotIdentity, SemanticQueryApi,
        SemanticQueryKey, SemanticQueryOutput,
    };
    let host = std::sync::Arc::new(host());
    upsert_ts(
        &host,
        "/leak.ts",
        "export const selfV: SelfT = null as unknown as SelfT;\n\
         export type SelfT = typeof selfV;\n",
    );
    let host_for_thread = std::sync::Arc::clone(&host);
    let join = std::thread::Builder::new()
        .name("typeof_resolution_stays_within_instantiate_window".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let dispatch = ProjectSemanticDispatch::new(&*host_for_thread);
            let _ = host_for_thread
                .shallow_file_state("/leak.ts")
                .expect("/leak.ts shallow state");
            // Instantiate the self-referential `SelfT`; its body `typeof selfV`
            // resolves back to `SelfT` while `SelfT` is on the active stack.
            let self_t = ResolvedDeclSlotIdentity::type_slot_unscoped(
                std::sync::Arc::from("/leak.ts"),
                std::sync::Arc::from("SelfT"),
            );
            let key = SemanticQueryKey::Instantiate {
                base: self_t,
                args: std::sync::Arc::from(
                    Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice(),
                ),
                context: InstantiateContext::new(
                    ProjectionReductionContext::published(ProjectionMode::Expanded),
                    Default::default(),
                ),
            };
            match dispatch.execute_type_node(key) {
                QueryResult::Value(SemanticQueryOutput { value, .. }) => {
                    format!("{:?}", dispatch.graph().node_data(value).as_deref())
                }
                QueryResult::Recursive(_) => "Recursive".to_string(),
                QueryResult::Error(e) => format!("Error({e:?})"),
            }
        })
        .expect("spawn worker thread for the instantiate-window leak fixture");
    let shape = join.join().expect(
        "worker thread MUST terminate without panic; a regression that moved the instantiate \
         body's `typeof` resolution PAST pop_instantiate_active would lose the active identity, \
         fail the same-identity guard, and stack-overflow here",
    );
    // The active-identity guard fires only while the identity is pushed; its
    // SPECIFIC `RecursiveRef` sentinel — carrying the active-identity name
    // `SelfT` — proves the body-time typeof resolution stayed INSIDE the
    // window. Asserting the EXACT `RecursiveRef { name: "SelfT" }` sentinel (not
    // a broad `Opaque`/`Miss` OR) discriminates the post-pop leak: a plain
    // `Miss` from an unrelated failure, or a leak that resolved to a bare
    // `Opaque(Miss)` after the identity was popped, would NOT carry the
    // active-identity `RecursiveRef` and so FAILS this assertion. The worker
    // thread + bounded-termination join above remains the anti-stack-overflow
    // discriminator.
    assert!(
        shape.contains("RecursiveRef { name: \"SelfT\" }"),
        "the self-referential `type SelfT = typeof selfV` instantiation must terminate with the \
         SPECIFIC active-identity `RecursiveRef {{ name: \"SelfT\" }}` sentinel produced WHILE the \
         identity was pushed (a plain `Miss`/`Opaque` from an unrelated failure or a post-pop \
         leak does NOT carry it); got {shape}"
    );
}
