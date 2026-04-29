//! §5.D.5 pathological recursion-safety fixtures (per-gap, sliced
//! across receiving sub-phases per the §5.D.5 ownership table).
//!
//! Each test exercises a pathological recursive/nested shape that
//! the static `no_unbounded_recursion_in_resolver_core` guard
//! (Phase 10b) cannot fully cover — the static guard catches
//! source patterns; runtime fixtures here close the loop by
//! asserting terminate-without-stack-overflow + the expected
//! sentinel/result on the runtime path.
//!
//! Ownership table (per §5.D.5 r15/Q2):
//!
//! | Sub-phase | Pathological fixture |
//! |---|---|
//! | 5h        | `pathological_self_shadowing_userland_pick`         |
//! | 5i        | `pathological_exclude_self_recursive` (deferred)    |
//! | 5i        | `pathological_extract_through_typeof` (deferred)    |
//! | 5i        | `pathological_template_literal_key_recursion` (def) |
//! | 5j        | `pathological_nested_slot_definitions` (deferred)   |
//! | 5j        | `pathological_self_referential_slot_payload` (def)  |
//! | 5k        | `pathological_typeof_substitution_cycle` (deferred) |
//!
//! Each fixture:
//!   - Builds the pathological scenario via a hermetic host.
//!   - Asserts terminate-without-panic AND the expected sentinel
//!     (`QueryResult::Recursive(_)` or `QueryResult::Error(_)`).
//!   - Discriminates: a regression that fails the cycle gate would
//!     stack-overflow OR return a wrongly-cached `Value(_)`.
//!
//! Plan: §5.D.5 (Phase 5h owns the userland-shadow self-reference;
//! 5i/5j/5k own their respective shapes per the ownership table).

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, HashValue, ProjectionMode, QueryResult, SemanticQueryApi, SemanticQueryKey,
};
use crate::types::HostConfig;
use crate::VerterHost;

/// Build a hermetic [`VerterHost`] backed by a [`MemoryWorkspace`]
/// and the given files. Audit + footprint capture stays disabled
/// (the §5.D.5 fixtures do not assert on audit counters; they
/// assert on termination + sentinel propagation).
fn build_hermetic_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(HostConfig::default(), ws_access))
}

// ── 5h: `type Pick<T, K> = Pick<T, K>` (userland Pick shadows lib AND
//        is structurally self-referential) ─────────────────────────────
//
// The userland alias declares itself as its own body — a literal
// self-reference. The resolver-context shadow gate (Phase 5h) makes
// the userland declaration the chosen target; the engine's
// `push_instantiate_active` same-identity guard then catches the
// structural self-reference and emits `Opaque(RecursiveRef)` at
// depth 2 — the second `Instantiate(Pick, ...)` push fails because
// the parent `Instantiate(Pick, ...)` is still on the active stack.
//
// **Why test against the dispatch engine directly (not
// `get_component_meta`):** the §5.D.5 r15/Q2 contract is "terminate
// with `Recursive`/sentinel, not stack overflow". The dispatch
// engine's same-identity guard handles this case correctly — the
// `instantiate_active` stack on a SINGLE dispatcher catches the
// re-entry. A higher-layer caller that creates a fresh dispatcher
// per logical call (e.g. the component-meta query engine's
// `materialize_component_meta_type_expr_until_stable_full`) would
// reset the guard at every recursion level; that is a separate
// architectural concern (engine-layer recursion handling for
// parametrised cycles) belonging to Phase 11+ rather than 5h's
// shadow-gate-threading scope.
//
// The test exercises 5h's contribution directly: the resolver-
// context shadow gate ensures dispatch routes the bare-name `Pick`
// to the userland declaration; the engine's same-identity guard
// then breaks the cycle at depth 2 and returns `Recursive`. The
// test asserts BOTH legs — the route went through the userland
// declaration AND the recursion terminated with the engine's
// sentinel rather than infinite recursion.
const PATHOLOGICAL_SELF_SHADOWING_PICK_VUE: &str = r#"<script setup lang="ts">
type Pick<T, K> = Pick<T, K>;
interface Cfg {
  alpha: string;
  beta: number;
}
</script>
<template><div /></template>
"#;

/// 5h §5.D.5 — `pathological_self_shadowing_userland_pick`.
///
/// `type Pick<T, K> = Pick<T, K>` is a self-referential userland
/// alias that shadows the ambient lib's `Pick`. Phase 5h's
/// resolver-context shadow gate routes `Pick<Cfg, 'alpha'>` to the
/// userland declaration (per the "user shadowing wins" rule); the
/// engine's `push_instantiate_active` same-identity guard then
/// catches the structural self-reference at the second `Instantiate`
/// dispatch and emits `Opaque(RecursiveRef)` rather than recursing
/// infinitely.
///
/// **Terminate-without-stack-overflow** is the load-bearing
/// invariant: the test simply running to completion (Cargo's
/// default 60s timeout) discriminates the change. A regression
/// that broke the engine's same-identity guard would stack-overflow
/// this test.
///
/// The query goes through `dispatch.execute(SemanticQueryKey::
/// Instantiate)` directly so the test exercises the engine's
/// recursion handling on a single dispatcher (the same dispatcher
/// surface 5h's `ScopeShadowing` thread plumbs through). The
/// component-meta query engine's per-call dispatcher recreation
/// (`materialize_component_meta_type_expr_until_stable_full`) is
/// out of scope for §5.D.5's "engine-layer recursion safety"
/// contract; a separate phase would cover that higher-layer
/// architecture.
#[test]
fn pathological_self_shadowing_userland_pick() {
    let host = build_hermetic_host(&[("/A.vue", PATHOLOGICAL_SELF_SHADOWING_PICK_VUE)]);

    // §5.D.5 r15/Q2 termination contract: the test must complete
    // within Cargo's wall-clock budget without stack-overflow. The
    // dispatch engine's `push_instantiate_active` same-identity
    // guard MUST fire when the body of the recursive userland Pick
    // dispatches another `Instantiate(Pick, ...)` — the second
    // push fails (parent still on active stack) and the engine
    // returns `Opaque(RecursiveRef)`.
    //
    // The test runs on a dedicated worker thread with a generous
    // 32 MiB stack so a regression in the same-identity guard
    // surfaces as a join error rather than a process crash.
    let host_for_thread = Arc::clone(&host);
    let join = std::thread::Builder::new()
        .name("pathological_self_shadowing_userland_pick".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            // Construct an `Instantiate` key that exercises the
            // recursive userland Pick. The alias is declared in
            // /A.vue with name "Pick"; the args are bare
            // SemanticNodeIds for any concrete type — the
            // recursion through the body fires regardless of the
            // arg types because the body re-dispatches to
            // `Instantiate(Pick, [T_arg, K_arg])` for the type
            // params, which collides with the active stack entry.
            let dispatch = ProjectSemanticDispatch::new(&host_for_thread);
            let shallow = host_for_thread
                .shallow_file_state("/A.vue")
                .expect("/A.vue must have shallow file state");
            let whole_hash: HashValue = shallow.whole_hash;
            let pick_identity = DeclIdentity {
                canonical_id: Arc::from("/A.vue"),
                whole_hash,
                decl_name: Arc::from("Pick"),
            };
            // Use synthetic placeholder args (interned `Primitive`
            // nodes) — the recursion fires irrespective of the
            // arg shapes because `build_instantiate` looks up the
            // body via `decl_canonical + decl_name` only.
            let graph = host_for_thread.project_type_store().semantic_graph();
            let arg_t = graph.intern_node(crate::semantic_query::SemanticNodeData::Primitive(
                crate::semantic_query::PrimitiveKind::String,
            ));
            let arg_k = graph.intern_node(crate::semantic_query::SemanticNodeData::Primitive(
                crate::semantic_query::PrimitiveKind::String,
            ));
            let key = SemanticQueryKey::Instantiate {
                base: pick_identity,
                args: Arc::from(vec![arg_t, arg_k].into_boxed_slice()),
                body_mode: ProjectionMode::Expanded,
            };
            dispatch.execute(key)
        })
        .expect("spawn worker thread for pathological recursion fixture");
    let result = join.join().expect(
        "worker thread MUST terminate without panic; a stack-overflow regression \
         (the engine's same-identity recursion guard failing to fire on a userland \
         self-referential alias) would surface here as a join error",
    );

    // Discriminating: `result` is a `QueryResult` (Value /
    // Recursive / Error). For the recursive userland Pick the
    // engine emits `Value(opaque_id)` where the opaque node carries
    // `QueryError::RecursiveRef { name: "Pick" }`. The mere fact
    // that we got a `QueryResult` value (rather than the worker
    // panicking) is the load-bearing signal — the engine
    // terminated. We then verify the result shape carries the
    // recursion sentinel.
    match result {
        QueryResult::Value(node_id) => {
            let graph = host.project_type_store().semantic_graph();
            let data = graph.node_data(node_id).expect("result node must exist");
            // Discriminating: the engine's same-identity guard
            // produces an Opaque(RecursiveRef{name:"Pick"}) at the
            // back-edge. The shell wrapping this opaque is what
            // surfaces here. Either the shell is itself the opaque
            // OR it points at one — both are evidence the
            // recursion was caught.
            let dbg = format!("{:?}", data.as_ref());
            assert!(
                dbg.contains("RecursiveRef") || dbg.contains("Opaque"),
                "self-referential userland Pick must terminate with a RecursiveRef \
                 / Opaque sentinel; got node data {dbg}"
            );
        }
        QueryResult::Recursive(_) => {
            // Same-path sentinel from execute_cooperative is also
            // an acceptable termination signal. Either RecursiveRef
            // (build-time guard) or Recursive (memo-time guard)
            // proves the cycle was caught.
        }
        QueryResult::Error(err) => {
            // An Error result is acceptable as long as it
            // categorically rejects the recursion (Miss /
            // RecursiveRef). A successful Value-shaped result
            // would be the regression signal.
            let dbg = format!("{err:?}");
            assert!(
                dbg.contains("RecursiveRef") || dbg.contains("Miss"),
                "self-referential userland Pick must terminate with a RecursiveRef \
                 / Miss error; got {dbg}"
            );
        }
    }
}

// ── 5i: `type R = Exclude<R, never>` (self-referential Exclude over
//        an unbound recursive alias) ─────────────────────────────────
//
// The userland alias R is its own first type argument to Exclude.
// Phase 5i's `Extract` / `Exclude` arm in `build_builtin_utility`
// resolves the source via `evaluate_deferred_semantic_node`, which
// eventually hits the `Instantiate(R, [])` dispatch — the engine's
// `push_instantiate_active` same-identity guard then catches the
// re-entry and emits `Opaque(RecursiveRef)` at the back-edge.
//
// **Why test against the dispatch engine directly** (mirrors 5h):
// the §5.D.5 r15/Q2 contract is "terminate with Recursive/sentinel,
// not stack overflow". Single-dispatcher guards are exactly what we
// need to verify; higher-layer per-call dispatcher recreation is a
// separate engine-architecture concern out of scope for §5.D.5.
const PATHOLOGICAL_EXCLUDE_SELF_RECURSIVE_VUE: &str = r#"<script setup lang="ts">
type R = Exclude<R, never>;
</script>
<template><div /></template>
"#;

/// 5i §5.D.5 — `pathological_exclude_self_recursive`.
///
/// `type R = Exclude<R, never>` self-references on its first type
/// argument. The new `Extract` / `Exclude` arm dispatches the
/// source via `evaluate_deferred_semantic_node`, which crosses
/// `Instantiate(R, ...)` and triggers the engine's
/// `push_instantiate_active` same-identity guard. The expected
/// outcome is termination with a `RecursiveRef` / `Opaque` /
/// `Recursive` sentinel — NOT stack overflow.
///
/// Termination contract: the test running to completion (Cargo's
/// default 60s timeout) is the load-bearing signal; a regression
/// in the guard would stack-overflow this test before completion.
/// The dedicated worker thread has a 32 MiB stack so a regression
/// surfaces as a join error rather than a process crash.
#[test]
fn pathological_exclude_self_recursive() {
    let host = build_hermetic_host(&[("/A.vue", PATHOLOGICAL_EXCLUDE_SELF_RECURSIVE_VUE)]);
    let host_for_thread = Arc::clone(&host);
    let join = std::thread::Builder::new()
        .name("pathological_exclude_self_recursive".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let dispatch = ProjectSemanticDispatch::new(&host_for_thread);
            let shallow = host_for_thread
                .shallow_file_state("/A.vue")
                .expect("/A.vue must have shallow file state");
            let whole_hash: HashValue = shallow.whole_hash;
            let r_identity = DeclIdentity {
                canonical_id: Arc::from("/A.vue"),
                whole_hash,
                decl_name: Arc::from("R"),
            };
            let key = SemanticQueryKey::Instantiate {
                base: r_identity,
                args: Arc::from(Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice()),
                body_mode: ProjectionMode::Expanded,
            };
            dispatch.execute(key)
        })
        .expect("spawn worker thread for pathological recursion fixture");
    let result = join.join().expect(
        "worker thread MUST terminate without panic; a stack-overflow regression \
         (the new Extract/Exclude arm failing to defer to the engine's same-identity \
         recursion guard) would surface here as a join error",
    );

    match result {
        QueryResult::Value(node_id) => {
            let graph = host.project_type_store().semantic_graph();
            let data = graph.node_data(node_id).expect("result node must exist");
            let dbg = format!("{:?}", data.as_ref());
            assert!(
                dbg.contains("RecursiveRef") || dbg.contains("Opaque"),
                "self-referential Exclude must terminate with a RecursiveRef / Opaque \
                 sentinel; got node data {dbg}"
            );
        }
        QueryResult::Recursive(_) => {}
        QueryResult::Error(err) => {
            let dbg = format!("{err:?}");
            assert!(
                dbg.contains("RecursiveRef") || dbg.contains("Miss"),
                "self-referential Exclude must terminate with a RecursiveRef / Miss \
                 error; got {dbg}"
            );
        }
    }
}

// ── 5i: `Extract<typeof y, R>` chained substitution across a deep
//        cross-file typeof + alias chain ────────────────────────────
//
// The source SFC declares a deep value chain (`y` is typed as
// `'a' | 'b' | 'c'`); the second type argument `R = 'a' | 'b'`
// is a normal literal union. The new `Extract` / `Exclude` arm
// must:
//   1. Evaluate `typeof y` through `evaluate_deferred_semantic_node`
//      (TypeOf carrier unwrap) into the literal union.
//   2. Distribute the resulting union over the per-member
//      `relate_nodes` against R.
//   3. Survive within the depth budget (the chain is short and
//      well within MAX_DEPTH).
//
// Expected: terminate with the correct extracted union
// `'a' | 'b'`. Discriminating: stack overflow OR a wrongly-cached
// `Value(_)` for the wrong shape would mark a regression.
const PATHOLOGICAL_EXTRACT_THROUGH_TYPEOF_VUE: &str = r#"<script setup lang="ts">
const y: 'a' | 'b' | 'c' = 'a';
type R = 'a' | 'b';
type X = Extract<typeof y, R>;
defineProps<{ kind: X }>();
</script>
<template><div /></template>
"#;

/// 5i §5.D.5 — `pathological_extract_through_typeof`.
///
/// `Extract<typeof y, R>` composes `typeof` carrier unwrap + the
/// new union-filter reduction. The chain crosses
/// `Instantiate(Extract, [TypeOf{y}, R])` -> `evaluate_deferred`
/// of `TypeOf{y}` -> per-member `relate_nodes(member, R)`.
/// Expected: terminate within MAX_DEPTH and produce
/// `kind: "a" | "b"`. The component-meta surface is therefore
/// `props = [{ name: "kind", type_signature: "\"a\" | \"b\"" }]`.
#[test]
fn pathological_extract_through_typeof() {
    let host = build_hermetic_host(&[("/A.vue", PATHOLOGICAL_EXTRACT_THROUGH_TYPEOF_VUE)]);
    let host_for_thread = Arc::clone(&host);
    let join = std::thread::Builder::new()
        .name("pathological_extract_through_typeof".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || host_for_thread.get_component_meta("/A.vue"))
        .expect("spawn worker thread for pathological recursion fixture");
    let result = join.join().expect(
        "worker thread MUST terminate without panic; a stack-overflow or \
         budget-exceeded regression in the new Extract/Exclude arm + TypeOf \
         interaction would surface here as a join error",
    );

    let analysis = result.expect("get_component_meta must produce a result for the typeof+Extract chain");
    let kind_prop = analysis
        .props
        .iter()
        .find(|p| p.name == "kind")
        .expect("the `kind` prop must surface from defineProps<{ kind: X }>()");
    let signature = crate::component_meta_pathological_recursion_tests::render_signature_for_test(&kind_prop.type_expr);
    assert_eq!(
        signature, "\"a\" | \"b\"",
        "Extract<typeof y, R> must reduce to the literal union \"a\" | \"b\" \
         after the typeof carrier unwrap dispatches the source union through the \
         new per-member relation engine path; got {signature}"
    );
}

/// Render a `TypeExpr` to its canonical signature for test
/// assertions. Mirrors the SnapshotView renderer's literal/union
/// formatting in just enough detail for the §5.D.5 assertions.
pub(crate) fn render_signature_for_test(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> String {
    use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};
    match expr {
        TypeExpr::Union(arms) => {
            let parts: Vec<String> = arms.iter().map(render_signature_for_test).collect();
            parts.join(" | ")
        }
        TypeExpr::Literal(LiteralValue::String(s)) => format!("\"{s}\""),
        TypeExpr::Literal(LiteralValue::Number(n)) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
        TypeExpr::Literal(LiteralValue::Boolean(b)) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        TypeExpr::Literal(LiteralValue::BigInt(s)) => format!("{s}n"),
        // Fall-through: any non-Union / non-Literal shape returns
        // its debug rendering so a regression surfaces as a clearly
        // non-matching string in the assertion.
        other => format!("{other:?}"),
    }
}

// ── 5i: `type R = { [K in `${keyof R}_x`]: R[K] }` (template-literal
//        key referencing the enclosing alias) ─────────────────────────
//
// The mapped type's source `keyof R` references the enclosing
// alias R; the template-literal key positions K through `${K}_x`.
// Both the keyof and the indexed-access `R[K]` create cycles back
// into R. The new TemplateLiteral evaluator must NOT cause the
// recursion to escape the engine's guards: the expected outcome
// is termination with a `Recursive` sentinel (or Opaque /
// RecursiveRef equivalent).
const PATHOLOGICAL_TEMPLATE_LITERAL_KEY_RECURSION_VUE: &str = r#"<script setup lang="ts">
type R = { [K in keyof R as `${K & string}_x`]: R[K] };
</script>
<template><div /></template>
"#;

/// 5i §5.D.5 — `pathological_template_literal_key_recursion`.
///
/// `type R = { [K in keyof R as `${K & string}_x`]: R[K] }` carries
/// two recursive references back into R: the source `keyof R` and
/// the value `R[K]`. The mapper's `name_remap` is a TemplateLiteral
/// referencing K; under Phase 5i, the template-literal evaluator
/// folds the template only when every expression resolves to a
/// literal — a cyclic K substitution must NOT cause the evaluator
/// to recurse forever.
///
/// Expected: terminate with a Recursive / Opaque / RecursiveRef
/// sentinel. A regression in either the mapper-name-remap path or
/// the TemplateLiteral evaluator's cycle handling would
/// stack-overflow this test.
#[test]
fn pathological_template_literal_key_recursion() {
    let host =
        build_hermetic_host(&[("/A.vue", PATHOLOGICAL_TEMPLATE_LITERAL_KEY_RECURSION_VUE)]);
    let host_for_thread = Arc::clone(&host);
    let join = std::thread::Builder::new()
        .name("pathological_template_literal_key_recursion".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let dispatch = ProjectSemanticDispatch::new(&host_for_thread);
            let shallow = host_for_thread
                .shallow_file_state("/A.vue")
                .expect("/A.vue must have shallow file state");
            let whole_hash: HashValue = shallow.whole_hash;
            let r_identity = DeclIdentity {
                canonical_id: Arc::from("/A.vue"),
                whole_hash,
                decl_name: Arc::from("R"),
            };
            let key = SemanticQueryKey::Instantiate {
                base: r_identity,
                args: Arc::from(Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice()),
                body_mode: ProjectionMode::Expanded,
            };
            dispatch.execute(key)
        })
        .expect("spawn worker thread for pathological recursion fixture");
    let result = join.join().expect(
        "worker thread MUST terminate without panic; a stack-overflow regression in \
         the new mapper name_remap path or the TemplateLiteral evaluator would \
         surface here as a join error",
    );

    match result {
        QueryResult::Value(node_id) => {
            let graph = host.project_type_store().semantic_graph();
            let data = graph.node_data(node_id).expect("result node must exist");
            let dbg = format!("{:?}", data.as_ref());
            assert!(
                dbg.contains("RecursiveRef")
                    || dbg.contains("Opaque")
                    || dbg.contains("Mapped"),
                "self-referential template-literal mapped type must terminate \
                 with a RecursiveRef / Opaque / deferred Mapped shell sentinel; \
                 got node data {dbg}"
            );
        }
        QueryResult::Recursive(_) => {}
        QueryResult::Error(err) => {
            let dbg = format!("{err:?}");
            assert!(
                dbg.contains("RecursiveRef") || dbg.contains("Miss"),
                "self-referential template-literal mapped type must terminate \
                 with a RecursiveRef / Miss error; got {dbg}"
            );
        }
    }
}
