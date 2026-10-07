use super::*;

/// PUBLIC BOUNDARY, RENDERED BYTES — an UNVERIFIED flow return publishes its
/// complete member set with validation OFF, rather than deleting the module.
///
/// `FLOW_RETURN_UNVERIFIED` states exactly one thing: every member is present
/// and one member's TYPE may be wrong. The member set is therefore
/// publishable and the member TYPES are not, so the honest emit is the full
/// name set with `type: null` — the same encoding the lane already uses for a
/// member it could not resolve. Refusing instead deleted every byte of the
/// module for parameter reassignment and a conditional `var`, both of which
/// are ordinary TypeScript that the previous implementation compiled
/// correctly.
///
/// The fixtures ride assignments inside a `while` test
/// (`while ((v = 2)) { break; }`): a plain `=`, a compound write at statement
/// position, a write inside a logical operand and one inside an `if` test are
/// applied by the evaluator and verified, so the unverified class is
/// exercised through the loop-test position nobody applies
/// (the destructured-parameter row included — its plain element binding is
/// modelled, so it too needs the test form to stay unverified).
///
/// The degradation is a property of the FRAME (it is seeded from the lowered
/// slice's effect list before any member is evaluated), so it applies to
/// every member. Attributing it per member by intersecting each member
/// value's slot reads with the write's targets is FAIL-OPEN and must not be
/// done — see `degradation_reason_class` for the counter-example and for the
/// sound complement.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`):
/// row 1 is `{ label: string }`, rows 2 and 3 are `{ label: string; n: number
/// }` — every row is an ordinary object type, so deleting the module is not a
/// defensible answer for any of them.
///
/// Discrimination: refusing again fails the `Props` destructure; emitting a
/// constructor derived from the unverified value fails the `type: null`
/// assertion.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_unverified_flow_return_publishes_its_member_set_with_validation_off() {
    /// `(canonical, script, member names)`
    const ROWS: &[(&str, &str, &[&str])] = &[
        (
            "/src/W1Param.vue",
            "function makeProps(seed: string) { while ((seed = \"y\")) { break; } return { label: seed } }",
            &["label"],
        ),
        (
            "/src/W2CondVar.vue",
            "function makeProps(k: boolean) { var v = 1; if (k) { while ((v = 2)) { break; } } return { label: \"x\", n: v } }",
            &["label", "n"],
        ),
        (
            "/src/W3Destructure.vue",
            "function makeProps({ seed }: {seed: string}) { while ((seed = \"y\")) { break; } return { label: seed, n: 1 } }",
            &["label", "n"],
        ),
    ];

    for (canonical, script, members) in ROWS {
        let RenderedRuntime::Props(props) = render_runtime_props(canonical, script) else {
            panic!(
                "{canonical}: an unverified value has a COMPLETE member set — deleting the \
                 whole module is not the honest answer"
            );
        };
        for member in *members {
            assert!(
                props.contains(&format!("{member}: {{ type: null")),
                "{canonical}: `{member}` must be published with validation off — its name is \
                 known and its type is not:\n{props}"
            );
        }
        assert!(
            !props.contains("type: String") && !props.contains("type: Number"),
            "{canonical}: no member may carry a constructor derived from a value the \
             substrate could not verify:\n{props}"
        );
    }
}

/// BUDGET-PARITY: the SESSION outer install at
/// `component_meta_entry_resolution.rs` (the
/// `get_component_meta_with_resolution_via_view` body) is load-bearing for the
/// PRE-CHOKE macro-DTO extraction — work the fallthrough choke backstop does
/// NOT cover.
///
/// `extract_component_meta_from_resolved` calls `component_meta_resolved_macros`
/// → `vue_macro_dtos_with_ctx` BEFORE `compute_fallthrough_outcome_from_resolved_state`
/// installs the choke's install-if-none backstop. That cold macro-DTO path
/// charges projection ops ONLY while a request budget is active, and a
/// budget-tripped DTO is returned partial and REFUSED `vue_surface_store`
/// admission (`vue_exec` admits ONLY a Complete bundle, so a warm DTO hit is
/// unconditionally Complete — admitting a partial would launder a warm-Complete
/// replay). The session outer install keeps ONE request budget alive across the
/// resolve AND this pre-choke extraction. Without it the inner resolve's
/// install-if-none budget drops before the extract, the pre-choke macro-DTO
/// recompute runs UNBOUNDED, completes, and IS admitted — warming the store.
///
/// Discriminating: a budget-tripping `defineProps<Partial<S01> & … & Partial<S32>>()`
/// with NO fallthrough spread (the choke charges ~0 ops, so it cannot bound this
/// work). WITH the outer install the pre-choke macro DTO is partial and NOT
/// admitted (`vue_surface_store` stays EMPTY). Reverting ONLY the outer install
/// at `component_meta_entry_resolution.rs` — leaving the fallthrough choke
/// intact — drops the budget before the extract: the pre-choke recompute runs
/// unbounded, the macro DTO completes, and `vue_surface_store` GROWS. RED.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn session_pre_choke_macro_dto_budget_partial_not_admitted_to_vue_surface_store() {
    // Tight budget: the 32-arm intersection trips the projection-op fuse
    // mid-materialisation during the pre-choke macro-DTO extraction.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 6,
        ..HostConfig::default()
    });
    upsert_macro_dto_budget_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    // No ambient context — the session via-view entry's install-if-none must arm
    // the budget across the FULL body (resolve AND the pre-choke extract). An
    // ambient context here would no-op that install and defeat the discriminator.
    assert!(
        verter_type_engine::request_context::current_request_context().is_none(),
        "test precondition: no ambient request context"
    );
    assert_eq!(
        host.vue_surface_store().len(),
        0,
        "the macro-DTO store starts empty"
    );

    // Cold drive through the SESSION view path (site 281). The pre-choke
    // macro-DTO extraction runs under the outer install's budget. The partial is
    // still RETURNED to the caller (the projection-op fuse produces a partial
    // surface, not a hard `Err` — the symbolic-expansion budget that surfaces
    // `Err` is a separate, untripped fuse), just not warmed.
    let session = project.open_session_batch().unwrap();
    let returned = session
        .get_component_meta_with_resolution(canonical)
        .expect("session with-resolution request must succeed");
    assert!(
        returned.is_some(),
        "a partial result is still RETURNED to the caller"
    );

    // THE DISCRIMINATOR: the budget-tripped pre-choke macro DTO is returned
    // partial and REFUSED `vue_surface_store` admission, so the store stays
    // EMPTY. The admission gate (`vue_exec`) admits EVERY Complete bundle and
    // refuses ONLY a partial, so an empty store after the macro-DTO resolution
    // ran proves the DTO was a budget-tripped partial. Reverting the outer
    // install at `component_meta_entry_resolution.rs` (leaving the fallthrough
    // choke intact) drops the budget before the extract — the pre-choke
    // macro-DTO recompute runs UNBOUNDED, the DTO completes, and the store GROWS.
    assert_eq!(
        host.vue_surface_store().len(),
        0,
        "the session outer install bounds the PRE-CHOKE macro-DTO extraction: the \
         budget-tripped `defineProps` DTO is returned partial and REFUSED \
         `vue_surface_store` admission, so the store stays empty. Reverting the outer \
         install at `component_meta_entry_resolution.rs` drops the budget before the \
         extract — the pre-choke macro-DTO recompute runs UNBOUNDED, completes, and is \
         admitted, growing the store (the fallthrough choke does not cover this \
         pre-choke work)"
    );
}

// ---------------------------------------------------------------------------
// Singleflight lane session-scoping characterization test
// ---------------------------------------------------------------------------

#[test]
fn singleflight_lanes_are_session_scoped() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ base: string }>()\n</script>\n<template><div/></template>",
        )
        .unwrap();

    let session_a = project.open_session_batch().unwrap();
    session_a
        .upsert(
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ fromA: number }>()\n</script>\n<template><div/></template>"
                .to_string(),
        )
        .unwrap();

    let session_b = project.open_session_batch().unwrap();
    session_b
        .upsert(
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ fromB: boolean }>()\n</script>\n<template><div/></template>"
                .to_string(),
        )
        .unwrap();

    let meta_a = session_a
        .get_component_meta("/src/Comp.vue")
        .expect("session_a query should succeed")
        .expect("session_a should produce component-meta");

    let meta_b = session_b
        .get_component_meta("/src/Comp.vue")
        .expect("session_b query should succeed")
        .expect("session_b should produce component-meta");

    let prop_names_a: Vec<&str> = meta_a.props.iter().map(|p| p.name.as_str()).collect();
    let prop_names_b: Vec<&str> = meta_b.props.iter().map(|p| p.name.as_str()).collect();

    assert!(
        prop_names_a.contains(&"fromA"),
        "session_a must see its own overlay prop 'fromA', got: {prop_names_a:?}"
    );
    assert!(
        !prop_names_a.contains(&"fromB"),
        "session_a must NOT see session_b's overlay prop 'fromB', got: {prop_names_a:?}"
    );

    assert!(
        prop_names_b.contains(&"fromB"),
        "session_b must see its own overlay prop 'fromB', got: {prop_names_b:?}"
    );
    assert!(
        !prop_names_b.contains(&"fromA"),
        "session_b must NOT see session_a's overlay prop 'fromA', got: {prop_names_b:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Architectural rule: published types stay shallow when not used.
//
// These negative tests assert the projector path's shallow contract:
//
// - Plain alias references (`type Foo = ...`) stay as bare `Ref` —
//   the consumer re-resolves through the registry on demand.
// - `Pick<Foo, "bar">` materialises ONLY the `bar` member; other Foo
//   properties stay shallow (path-precise, per the rule "Pick is just
//   a shortcut, same as a userland implementation").
// - `Omit<Foo, "bar">` keeps `bar` shallow (it is excluded from the
//   surface) and materialises the others.
// - Top-level utility wrappers around imported aliases stay symbolic
//   (the wrapper itself is a `Ref`; the Union or Intersection in
//   which it appears keeps the wrapper unexpanded).
// ─────────────────────────────────────────────────────────────────────────

/// Architectural rule: bare imported alias names stay shallow.
///
/// `defineProps<{ user: ImportedUser }>` MUST publish `user`'s type
/// as the bare `Ref { name: "ImportedUser" }`. Consumers re-resolve
/// `ImportedUser` through the registry on demand. The projector
/// path does not eagerly inline the imported declaration's body.
///
/// Pairs with [`published_same_file_alias_stays_shallow`] — the
/// shallow-by-default rule is unconditional, so the same-file case
/// behaves identically.
#[test]
fn published_bare_alias_ref_stays_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number,
  name: string
  password: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: ImportedUser
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    let user_ty = evaluated_prop_shallow_type(&project, "/Comp.vue", &evaluated, "user");
    match &user_ty {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "ImportedUser",
                "bare alias `ImportedUser` must publish as a `Ref` carrier"
            );
            assert!(
                type_arguments.is_empty(),
                "bare alias must publish without type arguments"
            );
        }
        TypeExpr::Object(_) => panic!(
            "FAIL (architectural rule): bare alias was eagerly expanded \
             to its Object body. Imported alias names MUST stay shallow \
             at the published surface — consumers re-resolve through \
             the registry on demand. Got {user_ty:?}"
        ),
        other => panic!("FAIL: bare alias `ImportedUser` must publish as `Ref`, got {other:?}"),
    }
}

/// Architectural rule: same-file alias names ALSO stay shallow.
///
/// `defineProps<{ user: Foo }>` where `Foo` is a same-file
/// `type Foo = string` MUST publish `user`'s type as the bare
/// `TypeExpr::Ref { name: "Foo" }`. The shallow-by-default rule is
/// unconditional — there is no same-file vs cross-file split. The
/// projector publishes the alias name as a carrier and consumers
/// re-resolve `Foo` through the registry on demand.
///
/// Pairs with [`published_bare_alias_ref_stays_shallow`] (the
/// cross-file case): together they document that bare alias names
/// stay shallow regardless of where the declaration lives.
///
/// Discriminating: a regression that re-introduces eager bare-`Ref`
/// reduction in the projector (e.g. a `expr_needs_projection_rescue`
/// gate that inspects the declaration body and inlines aliases whose
/// body is a primitive / utility wrapper / non-object surface) lands
/// as `TypeExpr::Primitive(String)` here and fails this test.
#[test]
fn published_same_file_alias_stays_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
type Foo = string

defineProps<{
  user: Foo
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    let user_ty = evaluated_prop_shallow_type(&project, "/Comp.vue", &evaluated, "user");
    match &user_ty {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Foo",
                "same-file alias `Foo` must publish as a `Ref` carrier"
            );
            assert!(
                type_arguments.is_empty(),
                "same-file alias must publish without type arguments"
            );
        }
        TypeExpr::Primitive(PrimitiveName::String) => panic!(
            "FAIL (architectural rule): same-file alias `type Foo = string` \
             was eagerly inlined to `Primitive(String)` at the published \
             surface. The shallow-by-default rule is unconditional — bare \
             alias references publish as `Ref {{ name: \"Foo\" }}` regardless \
             of whether the declaration lives in the same file or across a \
             file boundary. The projector pipeline must not eagerly inline \
             alias bodies. See CLAUDE.md \"Component-Meta Shallow-By-Default \
             Rule\". Got {user_ty:?}"
        ),
        other => panic!(
            "FAIL: same-file alias `type Foo = string` must publish as \
             `Ref {{ name: \"Foo\" }}`; got {other:?}"
        ),
    }
}

/// Architectural rule: nested indexed access only materialises the
/// terminal path's key, not other Foo members.
///
/// `Foo['a']['b']` materialises only the `b` value of `Foo.a`. The
/// other `Foo` keys stay shallow — they're not on the path.
#[test]
fn nested_indexed_access_publishes_only_terminal_path() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Inner { x: string, y: number }
export interface Foo { a: Inner, other: { z: boolean } }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Foo } from './types'

defineProps<{
  hop: Foo['a']['x']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let hop_ty = evaluated_prop_type(&project, "/Comp.vue", &evaluated, "hop");

    // The terminal path collapses to `string` (Inner.x's declared type).
    // The fixture deliberately uses different primitives at different
    // depths (`Inner.x: string`, `Inner.y: number`, `Foo.other.z:
    // boolean`) so the assertion discriminates the SPECIFIC terminal
    // primitive — a regression that mis-routes to `y` would land on
    // `number`, a regression that walks into `other.z` would land on
    // `boolean`. Both would fail this assertion; a `Primitive(_)`
    // wildcard would not.
    match hop_ty {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        other => panic!(
            "FAIL (architectural rule): `Foo['a']['x']` must collapse to \
             the terminal `string` primitive (path-precise materialisation \
             loads only `a` and `x`); got {other:?}"
        ),
    }
}

/// §1a DISCRIMINATION: the converted `output_sink` publication/reduction fns make
/// their shape decisions in NODE DOMAIN — they consult the node-fact APIs and do
/// NOT materialise a `TypeExpr` and then run a `TypeExpr` predicate on it.
///
/// Discrimination: each FORBIDDEN call is exactly the materialize-then-decide the
/// conversion removed, so re-introducing any of them FAILS this test (it was
/// present on the pre-change tree); each REQUIRED call is the node-domain fact the
/// conversion routes through.
#[test]
fn output_sink_conversions_decide_in_node_domain_not_on_materialized_type_expr() {
    const OUTPUT_SINK_SRC: &str = include_str!("../../meta_resolve/projectors/output_sink.rs");

    // ── `project_model` — reducibility decided on the payload NODE, not on the
    //    raised `TypeExpr` (the `type_expr_contains_reducible_operator(&raised)`
    //    decide is gone; `classify_node_reduction_gates(node)` replaces it).
    let project_model = output_sink_calls_in(OUTPUT_SINK_SRC, "project_model");
    assert!(
        !project_model.contains("type_expr_contains_reducible_operator"),
        "project_model must NOT run `type_expr_contains_reducible_operator` on the raised \
         TypeExpr (materialize-then-decide); calls seen: {project_model:?}"
    );
    assert!(
        project_model.contains("classify_node_reduction_gates"),
        "project_model must decide reducibility via `classify_node_reduction_gates` (node \
         domain); calls seen: {project_model:?}"
    );

    // ── `member_shape_peek_or_compute` — package-backed / cycle / reducibility
    //    gates run on the member-value NODE; the carrier seals through the
    //    node→carrier terminal, never via `shell_raise_to_type_expr` + a
    //    `TypeExpr` gate predicate.
    let member_shape = output_sink_calls_in(OUTPUT_SINK_SRC, "member_shape_peek_or_compute");
    for forbidden in [
        "shell_raise_to_type_expr",
        "seal_type_expr",
        "type_expr_contains_reducible_operator",
        "peek_member_shape_known",
    ] {
        assert!(
            !member_shape.contains(forbidden),
            "member_shape_peek_or_compute must NOT call `{forbidden}` (materialize-then-decide on \
             the raised TypeExpr); calls seen: {member_shape:?}"
        );
    }
    for required in [
        "node_package_backed_object_like_root_with_fence",
        "classify_node_reduction_gates",
        "node_root_reaches_transitive_cycle_with_fence",
        "raise_node_to_sealed_carrier",
    ] {
        assert!(
            member_shape.contains(required),
            "member_shape_peek_or_compute must call `{required}` (node-domain gate / terminal \
             seal); calls seen: {member_shape:?}"
        );
    }

    // ── `reduce_field_value_node` — the NODE-start per-field reducer: returns a
    //    CARRIER; every gate (package-backed / cycle / reducibility / no-poison
    //    sentinel) reads node facts off the OBSERVED input node — no TypeExpr is
    //    materialised for a decision, no TypeExpr-start reducer runs, and nothing
    //    admits into a shared cache slot from this publication path.
    let reduce_field = output_sink_calls_in(OUTPUT_SINK_SRC, "reduce_field_value_node");
    for forbidden in [
        // the retired materialize-then-decide sentinel helper
        "materialized_root_is_unmaterialized_sentinel",
        // the orchestrator never unwraps to a bare TypeExpr (it returns a carrier)
        "unwrap_materialized",
        // the retired TypeExpr-start reducer terminals
        "materialize_component_meta_type_expr_until_stable_full",
        "materialize_field_value_carrier",
        "seal_input_as_carrier",
        "peek_member_shape_known",
        // no shared-slot admission from the publication reduce path
        "admit_type_expr_shape_if_possible",
        "admit_computed",
    ] {
        assert!(
            !reduce_field.contains(forbidden),
            "reduce_field_value_node must NOT call `{forbidden}`; calls seen: {reduce_field:?}"
        );
    }
    for required in [
        "node_root_is_unmaterialized_sentinel_with_dispatch",
        "node_contains_semantic_miss_with_dispatch",
        "node_package_backed_object_like_root_with_fence",
        "node_root_reaches_transitive_cycle_with_fence",
        "classify_node_reduction_gates",
        "reduce_member_value_graph_native_with_context",
        "raise_node_to_sealed_carrier",
    ] {
        assert!(
            reduce_field.contains(required),
            "reduce_field_value_node must call `{required}` (node-domain gates / \
             graph-native reduce / terminal seal); calls seen: {reduce_field:?}"
        );
    }

    // ── the published-field finalize — the props shape selection is a NODE-domain
    //    comparison over the reduced carriers; the materialised-TypeExpr scorer is
    //    gone. The finalize pass lives in the `published_finalize` CHILD module
    //    of the sink (same capability mint scope); the per-position half is
    //    `finalize_published_prop_source` (shared by the flat rows and the
    //    `define_props` lane).
    const PUBLISHED_FINALIZE_SRC: &str =
        include_str!("../../meta_resolve/projectors/output_sink/published_finalize.rs");
    let finalize_position =
        output_sink_calls_in(PUBLISHED_FINALIZE_SRC, "finalize_published_prop_source");
    assert!(
        !finalize_position.contains("compare_type_expr_improvement"),
        "finalize_published_prop_source must NOT score materialised TypeExprs via \
         `compare_type_expr_improvement`; calls seen: {finalize_position:?}"
    );
    for required in [
        "compare_node_improvement",
        "node_root_is_explicit_selector_operator",
    ] {
        assert!(
            finalize_position.contains(required),
            "finalize_published_prop_source must select the published shape in node domain via \
             `{required}`; calls seen: {finalize_position:?}"
        );
    }
}

/// BLK1 (inner-cache fenced-serve poison) — the ENCODED-payload cache
/// (`store_meta_payload`) must REFUSE admission when the output-materialization
/// tracer (`output_read_set`) observed a FENCED (ReturnOnly,
/// `store_published == false`) serve. The output stays `Complete` (a fenced
/// serve is non-cacheable, NOT partial), so the token fence + completeness rail
/// both PASS; the ONLY rail that refuses the poisoned payload is
/// `output_read_set.non_cacheable_read_observed()` — which pre-fix the
/// `resolve_one_payload_item` gate never consulted, admitting a payload
/// computed from a served-without-publication basis whose facts validate
/// against the live view.
///
/// DISCRIMINATING: a LOCAL `BareRef` prop type (`type P = …; defineProps<P>()`)
/// resolves through the DIRECT carrier serve during output materialization;
/// `force_carrier_direct_serve_fence` fences that serve and fans a non-cacheable
/// read onto `output_read_set` WITHOUT a generation bump (so the token fence
/// stays admissible). Post-fix the payload is REFUSED (`cached_meta_payload`
/// absent) and the caller still receives it; the unforced recovery admits it.
/// RED-pre (gate not consulting the bit) the fenced payload LANDS in
/// `cached_meta_payload` and a later request stale-serves it.
#[test]
fn output_materialization_fenced_serve_refuses_encoded_payload_and_recovers() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type P = { p: string };
defineProps<P>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let session = project.open_session().expect("session");

    // Arm: every DIRECT carrier serve in this request is treated as FENCED
    // (ReturnOnly), fanning a non-cacheable read onto the active tracer. The
    // output-materialization reduce (resolving the local `P` carrier, traced
    // under `output_read_set`) observes it, so the encoded payload was computed
    // from a served-without-publication basis while its facts validate against
    // the live view. No generation bump ⇒ the token fence stays admissible,
    // isolating the non-cacheability rail.
    host.test_force
        .engine
        .force_carrier_direct_serve_fence_for_tests
        .store(true, Relaxed);
    let payload = session
        .get_component_meta_payload("/App.vue", |output| {
            format!("{:?}", output.into_parts().2.into_lanes().props).into_bytes()
        })
        .expect("a fenced output serve still serves the caller (ReturnOnly)")
        .expect("component resolves");
    host.test_force
        .engine
        .force_carrier_direct_serve_fence_for_tests
        .store(false, Relaxed);
    assert!(
        !payload.is_empty(),
        "the refused-admission payload is still returned to THIS caller",
    );

    // THE PIN: a fenced (non-cacheable) output materialization must NOT admit
    // the encoded payload — else a later request stale-serves it warm.
    assert!(
        host.derived_raw_cache()
            .get("/App.vue")
            .map(|e| e.value().cached_meta_payload.is_none())
            .unwrap_or(true),
        "POISON: a fenced output materialization admitted the encoded payload — \
         `output_read_set.non_cacheable_read_observed()` must refuse the \
         `store_meta_payload` admission (the token fence + completeness rail both \
         pass for a fenced-but-Complete serve)",
    );

    // Recovery: an unforced request admits the payload — the refusal was the
    // admission gate acting, not a broken payload path.
    let payload2 = session
        .get_component_meta_payload("/App.vue", |output| {
            format!("{:?}", output.into_parts().2.into_lanes().props).into_bytes()
        })
        .expect("recovery: the unforced request succeeds")
        .expect("component resolves");
    assert_eq!(
        payload, payload2,
        "the unforced recovery payload equals the fenced-serve value (same content)"
    );
    assert!(
        host.derived_raw_cache()
            .get("/App.vue")
            .map(|e| e.value().cached_meta_payload.is_some())
            .unwrap_or(false),
        "the unforced recovery admits the encoded payload — the fenced refusal was \
         the admission gate, not a broken path",
    );
}

/// AUDIT-lane equivalence: the audited session wrapper, the audited host
/// entry (the LSP route), and the NAPI-shaped payload lane all serve the
/// SAME materialized envelope for the same component.
#[test]
fn output_audit_lsp_and_payload_lanes_serve_identical_envelopes() {
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ p: string }>()
defineEmits<{ e: [n: number] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();

    // (a) The audited host entry — the LSP custom method's route AND the
    // sole producer behind the NAPI/WASM audit-bundle session wrapper
    // (`ComponentMetaSession::get_component_meta_with_audit` delegates
    // here); the request id retrieves the matching audit record exactly as
    // the wrapper does.
    let (lsp_output, rid) = {
        let (output, request_id) = host
            .get_component_meta_output_with_resolution("/App.vue")
            .expect("audited output ok");
        (output.expect("resolves"), request_id)
    };
    let (la, lr, lt) = lsp_output.into_parts();
    assert!(
        lr.is_some(),
        "the audited entry carries the resolution sidecar"
    );
    let lsp_debug = format!("{:?}{:?}", la, lt.into_lanes());
    let record = host
        .host_audit_runtime()
        .take_record(rid)
        .expect("the audited output entry produces the request audit record");
    assert_eq!(
        record.kind,
        crate::component_meta_audit::RequestKind::ComponentMeta
    );

    // (b) A REPEAT audited call (warm path) serves the identical envelope +
    // a from_cache record — the warm arm materializes under the SAME read
    // that validated the cache entry.
    let (warm_output, warm_rid) = {
        let (output, request_id) = host
            .get_component_meta_output_with_resolution("/App.vue")
            .expect("warm audited output ok");
        (output.expect("resolves"), request_id)
    };
    let (wa, wr, wt) = warm_output.into_parts();
    assert!(wr.is_some());
    assert_eq!(
        format!("{:?}{:?}", wa, wt.into_lanes()),
        lsp_debug,
        "the warm audited envelope equals the cold one"
    );
    let warm_record = host
        .host_audit_runtime()
        .take_record(warm_rid)
        .expect("the warm audited hit synthesizes a from_cache record");
    assert!(warm_record.from_cache, "warm hit records from_cache");

    // (c) The NAPI-shaped payload lane consumes the same envelope.
    let meta_session = project.open_session().expect("meta session");
    let payload = meta_session
        .get_component_meta_payload("/App.vue", |output| {
            let (pa, pr, pt) = output.into_parts();
            assert!(
                pr.is_some(),
                "the payload lane seeds the resolution sidecar"
            );
            format!("{:?}{:?}", pa, pt.into_lanes()).into_bytes()
        })
        .expect("payload ok")
        .expect("resolves");
    assert_eq!(
        String::from_utf8(payload).unwrap(),
        lsp_debug,
        "the payload lane's envelope equals the audited surfaces'"
    );
}

/// REQUEST-LOCAL DEDUPE: a source repeated across lanes materializes ONCE
/// per (effective scope, source identity) — the memo's materialize count
/// equals the number of DISTINCT sources, not the number of lane slots.
#[test]
fn output_materialization_dedupes_repeated_sources_across_lanes() {
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();

    let shared = closed_ref_source("SharedAlias");
    let distinct = closed_ref_source("DistinctAlias");
    let mut analysis = blank_output_analysis();
    for name in ["a", "b"] {
        analysis.props.push(
            verter_session_query::analysis::component_meta::PropAnalysis {
                name: name.to_string(),
                callable_role: verter_type_expr::PropCallableRole::default(),
                publication: crate::test_only::type_publication_fixture(
                    verter_type_expr::facts::SourcePosition::Present(shared.clone()),
                    verter_type_expr::ResolutionExactness::ExactConcrete,
                    None,
                    None,
                ),
                type_expansion: None,
                required: true,
                has_default: false,
                default_value: None,
                description: None,
                tags: Vec::new(),
                declared_in_macro_type_arg: false,
            },
        );
    }
    analysis.events.push(
        verter_session_query::analysis::component_meta::EventAnalysis {
            name: "e".to_string(),
            payload: verter_type_expr::facts::SourcePosition::Present(shared.clone()),
            publication: crate::test_only::type_publication_fixture(
                verter_type_expr::facts::SourcePosition::Present(shared.clone()),
                verter_type_expr::ResolutionExactness::ExactConcrete,
                None,
                None,
            ),
            return_publication: None,
            return_publication_scope: None,
            payload_expansion: None,
            raw_signature: None,
            description: None,
            tags: Vec::new(),
        },
    );
    analysis.accepted_props.push(
        verter_session_query::analysis::component_meta::AcceptedPropAnalysis {
            name: "ap".to_string(),
            callable_role: verter_type_expr::PropCallableRole::default(),
            publication: crate::test_only::type_publication_fixture(
                verter_type_expr::facts::SourcePosition::Present(distinct.clone()),
                verter_type_expr::ResolutionExactness::ExactConcrete,
                None,
                None,
            ),
            type_source_scope: None,
            required: false,
            provenance: verter_session_query::analysis::component_meta::MemberProvenance::Declared,
            availability:
                verter_session_query::analysis::component_meta::MemberAvailability::Always,
            kind: verter_session_query::analysis::component_meta::AcceptedPropKind::DeclaredProp,
        },
    );

    let fixture_dispatch_20 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_20,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("closed sources materialize");
    let calls =
        crate::meta_resolve::projectors::LAST_OUTPUT_MATERIALIZE_CALLS.with(std::cell::Cell::get);
    assert_eq!(
        calls, 2,
        "4 populated lane slots over 2 DISTINCT sources must materialize exactly twice \
         (once per (effective scope, source identity))"
    );
    let lanes = output.into_parts().2.into_lanes();
    assert_eq!(
        lanes.props[0], lanes.props[1],
        "deduped slots share the value"
    );
    assert_eq!(
        published_type(&lanes.props[0]),
        &materialized_event_types(&lanes)[0]
    );
}

/// An index-signature VALUE position richer than the closed leaf/tuple
/// vocabulary (`{ [key: string]: { nested: number } }`) is KNOWN structure:
/// it publishes the faithful PRESENT projected INDEX-POSITION replay route
/// (the macro's stamped type-argument base + the signature's surface
/// ordinal + the value role) and the result stays COMPLETE. A consumer
/// demanding the published source through the one shared dispatch reaches
/// the nested leaf. The pre-fix behavior was the typed
/// `Failed(UnrepresentableRequiredMemberValue)` interim (and before that, a
/// fabricated `Closed(Leaf(unknown))` present source inside a COMPLETE
/// result — a fail-open reported as success). The genuinely-OPEN key domain
/// (`[key: string]`) stays a valid PRESENT closed leaf — openness is
/// semantic, never a failure.
#[test]
fn richer_index_signature_value_position_publishes_projected_replay() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ [key: string]: { nested: number } }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("the analysis itself still assembles");
    let evaluated = state
        .evaluated_types
        .as_ref()
        .expect("evaluated types present");
    let shape = &evaluated.define_props[0].result.value;
    assert_eq!(shape.index_signatures.len(), 1, "one index signature");
    let signature = &shape.index_signatures[0];
    assert_eq!(
        signature.key_type,
        verter_type_expr::facts::SourcePosition::Present(
            verter_type_expr::facts::SemanticTypeSource::Closed(
                verter_type_expr::facts::ClosedTypeFact::Leaf(
                    verter_type_expr::facts::LeafTypeFact::Primitive(PrimitiveName::String),
                ),
            ),
        ),
        "the genuinely-open string key domain stays a PRESENT closed leaf"
    );
    // The richer VALUE position publishes the projected INDEX-POSITION
    // replay route — never a fabricated unknown leaf and never a failure.
    match signature.value_type.present() {
        Some(verter_type_expr::facts::SemanticTypeSource::Projected(
            verter_type_expr::facts::ProjectedTypeFact::IndexPosition {
                signature_ordinal,
                position,
                ..
            },
        )) => {
            assert_eq!(*signature_ordinal, 0, "first surface index signature");
            assert_eq!(
                *position,
                verter_type_expr::facts::IndexSignaturePosition::Value,
                "the value role addresses the value type"
            );
        }
        other => panic!(
            "the richer index value position publishes the IndexPosition \
             replay source, got {other:?}"
        ),
    }
    assert!(
        !state.completeness.is_partial(),
        "a representable index value position completes; got {:?}",
        state.completeness
    );
    assert!(
        !state.synthesis_should_suppress,
        "a representable index value position must not suppress warm result admission"
    );

    // Consumer demand-walk: demanding the published replay source through
    // the one shared dispatch materializes the nested object and reaches
    // its nested leaf — never an unknown.
    let demanded = demand_published_type(
        project.host(),
        "/App.vue",
        signature.value_type.present(),
        "index value position",
    );
    let TypeExpr::Object(shape) = &demanded else {
        panic!("the demanded index value materializes the nested object, got {demanded:?}");
    };
    let nested = shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "nested" =>
            {
                Some(property.ty.clone())
            }
            _ => None,
        })
        .expect("the demanded object carries the nested member");
    assert_eq!(
        nested,
        TypeExpr::Primitive(PrimitiveName::Number),
        "the demand-walk reaches the nested leaf"
    );
}

/// A PRESENT authored source whose deref'd body contains an interior
/// unknown-materializing `Opaque` (a method value whose deref interns the
/// shared miss placeholder at a nested position) FAILS output
/// materialization with the typed conservative interior fail-close. The
/// pre-fix behavior — the shell fold rendering the interior failure as a
/// COMPLETED `unknown` inside an otherwise-successful output — was a
/// fail-open reported as success. The graph carries no per-position
/// absent-vs-failed provenance, so a failure inside a dereferenced body
/// cannot be treated as proven schema absence and must reject the source.
#[test]
fn present_source_with_interior_unknown_materializing_opaque_fails_output() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ config: { handler(msg: string) } }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let failure = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect_err(
            "a Present source whose materialized shape carries an interior \
             unknown-materializing failure must FAIL output materialization — \
             never shell-fold to a completed unknown",
        );
    let crate::meta_resolve::ComponentMetaFailure::Output(err) = failure else {
        panic!("an uncancelled request never aborts: {failure:?}");
    };
    assert_eq!(
        err.lane,
        crate::meta_resolve::ComponentMetaOutputLane::Prop,
        "the typed failure names the failed lane"
    );
    assert!(
        matches!(
            err.failure,
            crate::meta_resolve::ComponentMetaOutputFailure::UnknownMaterializingSourceInterior { .. }
        ),
        "the failure class is the conservative interior fail-close; got {:?}",
        err.failure
    );
    assert!(
        err.position.is_present(),
        "the failed slot was a PRESENT source (the interior, not the position, failed); got {:?}",
        err.position
    );
}

#[test]
fn stable_reference_carriers_materialize_without_source_failure() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type Threshold = number | { start: number; end: number }
export interface Props {
  threshold?: Threshold
  boundary?: Element | null | Array<Element | null>
  format?: NumberFormatOptions
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("stable reference carriers are representable output sources")
        .expect("component resolves")
        .into_parts();
    let lanes = types.into_lanes();
    let prop_type = |name: &str| {
        let index = analysis
            .props
            .iter()
            .position(|prop| prop.name == name)
            .unwrap_or_else(|| panic!("the {name} prop publishes"));
        lanes.props[index]
            .materialized_type()
            .expect("published type")
    };

    assert!(
        matches!(prop_type("threshold"), TypeExpr::Ref { name, .. } if name.as_ref() == "Threshold"),
        "exact resolved authority keeps the shallow alias carrier; got {:?}",
        prop_type("threshold")
    );
    let TypeExpr::Union(boundary) = prop_type("boundary") else {
        panic!(
            "the structural reference union remains a union carrier; got {:?}",
            prop_type("boundary")
        );
    };
    assert!(
        boundary
            .iter()
            .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "Element")),
        "the unresolved DOM reference remains an explicit carrier; got {boundary:?}"
    );
    assert!(
        boundary
            .iter()
            .any(|arm| matches!(arm, TypeExpr::Array { .. })),
        "the array shell remains structural; got {boundary:?}"
    );
    assert!(
        matches!(prop_type("format"), TypeExpr::Ref { name, .. } if name.as_ref() == "NumberFormatOptions"),
        "the unresolved Intl-style name remains an explicit carrier; got {:?}",
        prop_type("format")
    );

    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("component resolves");
    assert!(
        !state.completeness.is_partial(),
        "stable unresolved carriers are complete structural results; got {:?}",
        state.completeness
    );
    assert!(
        !state.synthesis_should_suppress,
        "stable carriers do not suppress an otherwise complete result"
    );
}

/// PUBLIC BOUNDARY, RENDERED BYTES — an object return whose ENTRY FORM
/// the substrate once could not lower structurally reaches the runtime
/// lane intact, over a CALL-sourced spread.
///
/// SECONDARY evidence. The thing under test is the substrate's answer for
/// these shapes, asserted against tsgo in
/// `typeinfo_tests::value_inference::
/// object_return_entry_forms_lower_structurally_over_a_call_spread`. This
/// row set exists to prove that answer reaches a consumer that DERIVES
/// bytes from it, rather than being correct only at the graph.
///
/// The regression it pins: three entry forms — a computed key, a numeric
/// key, and a type carrier — bailed the WHOLE literal to the shared
/// shallow-pass leaf answer, and that answer embeds a call-sourced
/// spread's unreduced `ReturnType<callee>` carrier, which the leaf's
/// fabricated-value gate refuses. One unmodellable ENTRY therefore failed
/// the whole RETURN closed, and every module built on it lost every byte.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`):
/// the computed-key row is `{ label: string; z: number }`, the `as const`
/// row `{ readonly label: string; readonly n: 1 }`, the `as const`-only row
/// `{ readonly label: string }`, and the `satisfies` row `{ label: string;
/// n: number }`.
///
/// Discrimination: restoring the whole-literal bail refuses every row and
/// fails the `Props` destructure; dropping the spread's contribution fails
/// the `label` assertion.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_entry_form_over_a_call_spread_reaches_the_runtime_lane_intact() {
    const PRELUDE: &str = "function base() { return { label: \"x\" } }\nconst k = \"z\"";

    /// `(canonical, makeProps body, needles)`.
    const ROWS: &[(&str, &str, &[&str])] = &[
        (
            "/src/F1ComputedKey.vue",
            "function makeProps() { return { ...base(), [k]: 1 } }",
            &["label: { type: String", "z: { type: Number"],
        ),
        (
            "/src/F3AsConst.vue",
            "function makeProps() { return { ...base(), n: 1 } as const }",
            &["label: { type: String", "n: { type: Number"],
        ),
        (
            "/src/F4AsConstOnly.vue",
            "function makeProps() { return { ...base() } as const }",
            &["label: { type: String"],
        ),
        (
            "/src/F5Satisfies.vue",
            "function makeProps() { return { ...base(), n: 1 } satisfies object }",
            &["label: { type: String", "n: { type: Number"],
        ),
    ];

    for (canonical, body, expected) in ROWS {
        let RenderedRuntime::Props(emitted) =
            render_runtime_props(canonical, &format!("{PRELUDE}\n{body}"))
        else {
            panic!(
                "{canonical}: the literal lowers structurally, so the call-sourced spread rides \
                 the evaluator's call sink and reduces — there is a complete member set to \
                 publish"
            );
        };
        for needle in *expected {
            assert!(
                emitted.contains(needle),
                "{canonical}: expected `{needle}` in the emitted props object:\n{emitted}"
            );
        }
        assert!(
            !emitted.contains("type: null"),
            "{canonical}: every member here has a real constructor:\n{emitted}"
        );
    }

    // A NUMERIC-named member reaches the substrate exactly (the semantic
    // test pins `{ label: string; 1: number }`) but does NOT reach this
    // option object: the runtime projection names members through their
    // STRING name and skips a numeric key. The row is here so the gap is
    // an asserted fact rather than an unnoticed one — a Vue projection
    // question, not a substrate one.
    let RenderedRuntime::Props(numeric) = render_runtime_props(
        "/src/F2NumericKey.vue",
        &format!("{PRELUDE}\nfunction makeProps() {{ return {{ ...base(), 1: 2 }} }}"),
    ) else {
        panic!("/src/F2NumericKey.vue: the spread's member is modelled and must publish");
    };
    assert!(
        numeric.contains("label: { type: String"),
        "/src/F2NumericKey.vue: the spread-contributed member survives:\n{numeric}"
    );
    assert!(
        !numeric.contains("1:"),
        "/src/F2NumericKey.vue: the numeric-named member does not reach the runtime props \
         option today — if it starts to, this assertion is the place that says so:\n{numeric}"
    );
}

/// The ENCODED-PAYLOAD lane (`resolve_one_payload_item`, shared by the scalar
/// and batch payload surfaces) is the third envelope-build call site. Its
/// merged signal is already a live local — it decides the payload-cache write
/// forty lines below — so the envelope must carry that value and not the
/// resolve term the two other entries used.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn payload_lane_envelope_carries_extract_phase_partiality() {
    let encode_completeness = |output: crate::meta_resolve::ComponentMetaOutput| {
        let (.., completeness) = output.into_parts_with_contract();
        if completeness.is_partial() {
            b"partial".to_vec()
        } else {
            b"complete".to_vec()
        }
    };

    let project = extract_only_partial_project(EXTRACT_ONLY_PARTIAL_BUDGET);
    let session = project.open_session_batch().expect("batch session");
    let host = project.host();
    let view = crate::session_view::HostViewRef::new(host);
    let fixed = host.capture_batch_fixed_view(&view);
    let payload = session
        .resolve_one_payload_item("/src/WideParent.vue", &view, &fixed, encode_completeness)
        .expect("the payload item resolves")
        .expect("the component resolves");
    assert_eq!(
        payload, b"partial",
        "the payload lane's envelope must report the MERGED completeness — the same signal it \
         uses forty lines later to refuse the payload-cache write"
    );

    let control = extract_only_partial_project(0);
    let control_session = control.open_session_batch().expect("batch session");
    let control_host = control.host();
    let control_view = crate::session_view::HostViewRef::new(control_host);
    let control_fixed = control_host.capture_batch_fixed_view(&control_view);
    let control_payload = control_session
        .resolve_one_payload_item(
            "/src/WideParent.vue",
            &control_view,
            &control_fixed,
            encode_completeness,
        )
        .expect("the control payload item resolves")
        .expect("the control component resolves");
    assert_eq!(
        control_payload, b"complete",
        "the generous-budget control must still publish Complete on the payload lane"
    );
}
