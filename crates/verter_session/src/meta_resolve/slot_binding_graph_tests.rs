//! LEGACY_GATE_SELF - test fixture; comments referencing retired symbols are documentation, not re-introduction.
//! Tests covering graph-native slot-binding synthesis end-to-end.
//!
//! Each test drives `VerterHost::get_component_meta` (or its
//! audited variant) over a hermetic SFC fixture and observes the
//! resulting `ComponentMetaAnalysis`. The tests pin the contracts
//! that `slot_binding_graph::resolve_slot_bindings_graph_native`
//! must satisfy: walker termination on cycles, dep-signature
//! propagation through the dispatch fence, audit-payload structure,
//! and the publication-merge invariants between parser-path and
//! graph-native bindings.
//!
//! Test classifications:
//!
//! - **CHARACTERIZATION** tests assert behavioral contracts that a
//!   naive replacement implementation would violate. They are
//!   designed to fail loudly if the synthesis is reverted to a no-op
//!   stub or to a structurally-incomplete implementation that drops
//!   diagnostics, mishandles intersections, or leaks shells.
//! - **REGRESSION** tests pin landed invariants and guard against
//!   future regressions in the synthesis or its consumers (the audit
//!   payload bridge, tracing instrumentation, dep-signature
//!   merging).
//!
//! Some tests are `#[ignore]`'d with explicit reasons referencing
//! peer wiring not yet landed (e.g. budget-consumer wiring). The
//! ignore-reason text identifies the missing wiring; un-ignoring is
//! tracked in those owner sub-agents.
//!
//! Hermeticity: every default-runnable test builds its workspace
//! from string literals via `VerterHost::new_standalone` + the
//! in-memory workspace. External-corpus-backed perf tests live
//! behind `#[cfg(feature = "external-corpus")]`; see the
//! `external_corpus` module at the bottom of this file.

use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
use verter_semantic::analysis::type_expand::ExpansionExactness;
use verter_type_expr::{ObjectMember, TypeExpr};

use crate::audited_request::AuditedRequest;
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

// ---------------------------------------------------------------------------
// Hermetic test helpers
// ---------------------------------------------------------------------------

fn build_test_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

fn upsert_ts(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

fn slot_bindings<'a>(meta: &'a ComponentMetaAnalysis, slot: &str) -> Vec<&'a str> {
    meta.slots
        .iter()
        .find(|s| s.name == slot)
        .map(|s| s.bindings.iter().map(|b| b.name.as_str()).collect())
        .unwrap_or_default()
}

fn slot_binding<'a>(
    meta: &'a ComponentMetaAnalysis,
    slot: &str,
    binding: &str,
) -> Option<&'a verter_semantic::analysis::component_meta::SlotBindingAnalysis> {
    meta.slots
        .iter()
        .find(|s| s.name == slot)?
        .bindings
        .iter()
        .find(|b| b.name == binding)
}

/// Shell-materialize a published slot-binding `type_source` WITHOUT a
/// resolution demand — the shallow published shape the binding carries.
fn shallow_binding_type(
    host: &VerterHost,
    owner: &str,
    binding: &verter_semantic::analysis::component_meta::SlotBindingAnalysis,
) -> TypeExpr {
    crate::test_only::semantic_source_probe::shallow_type_expr(
        host,
        owner,
        binding
            .type_source
            .present()
            .unwrap_or_else(|| panic!("binding `{}` must publish a typed source", binding.name)),
    )
    .unwrap_or_else(|| {
        panic!(
            "binding `{}`'s published source must shell-materialize",
            binding.name
        )
    })
}

/// Demand-materialize a published slot-binding `type_source` — the explicit
/// consumer walk (`Published(Expanded)` through the one dispatch). For a
/// shallow synthetic carrier this is the terminal-demand deepen through the
/// content-free synthetic-binding identity.
fn demand_binding_type(
    host: &VerterHost,
    owner: &str,
    binding: &verter_semantic::analysis::component_meta::SlotBindingAnalysis,
) -> TypeExpr {
    crate::test_only::semantic_source_probe::demand_type_expr(
        host,
        owner,
        binding
            .type_source
            .present()
            .unwrap_or_else(|| panic!("binding `{}` must publish a typed source", binding.name)),
    )
    .unwrap_or_else(|| {
        panic!(
            "binding `{}`'s published source must demand-materialize",
            binding.name
        )
    })
}

// Recursive op-node count helper. Counts every TypeExpr recursive
// child node, used to bound binding `r#type` shapes against runaway
// expansion.
fn count_type_expr_nodes(ty: &TypeExpr) -> usize {
    fn walk(ty: &TypeExpr, n: &mut usize) {
        *n += 1;
        match ty {
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. }
            // Synthetic carriers are 1-node terminal leaves.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::TypeOf(_) => {}
            TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
                for arm in arms.iter() {
                    walk(arm, n);
                }
            }
            TypeExpr::Array { element, .. } => walk(element, n),
            TypeExpr::Tuple { elements, .. } => {
                for el in elements.iter() {
                    walk(&el.ty, n);
                }
            }
            TypeExpr::Object(obj) => {
                for member in obj.properties.iter() {
                    match member {
                        ObjectMember::Property(p) => walk(&p.ty, n),
                        ObjectMember::IndexSignature(idx) => {
                            walk(&idx.key_type, n);
                            walk(&idx.value_type, n);
                        }
                        ObjectMember::CallSignature(func)
                        | ObjectMember::ConstructSignature(func) => {
                            for p in func.parameters.iter() {
                                walk(&p.ty, n);
                            }
                            if let Some(rt) = &func.return_type {
                                walk(rt, n);
                            }
                        }
                        ObjectMember::Method(m) => {
                            for p in m.function.parameters.iter() {
                                walk(&p.ty, n);
                            }
                            if let Some(rt) = &m.function.return_type {
                                walk(rt, n);
                            }
                        }
                    }
                }
            }
            // A constructor type carries the same `FunctionExpr` payload as a
            // function type; its nodes are counted identically.
            TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                for p in func.parameters.iter() {
                    walk(&p.ty, n);
                }
                if let Some(rt) = &func.return_type {
                    walk(rt, n);
                }
            }
            TypeExpr::Ref { type_arguments, .. } => {
                for arg in type_arguments.iter() {
                    walk(arg, n);
                }
            }
            // `import("m").Gen<Arg>` — only the instantiation arguments are
            // nested nodes (mirrors the `Ref` arm; specifier/qualifier leaves).
            TypeExpr::ImportType { type_arguments, .. } => {
                for arg in type_arguments.iter() {
                    walk(arg, n);
                }
            }
            TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) | TypeExpr::Parenthesized(inner) => {
                walk(inner, n)
            }
            TypeExpr::IndexedAccess { object, index } => {
                walk(object, n);
                walk(index, n);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                walk(check, n);
                walk(extends, n);
                walk(true_type, n);
                walk(false_type, n);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                walk(source, n);
                walk(value, n);
                if let Some(nt) = name_type {
                    walk(nt, n);
                }
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for e in expressions.iter() {
                    walk(e, n);
                }
            }
            TypeExpr::RecursiveRef { type_arguments, .. } => {
                for a in type_arguments.iter() {
                    walk(a, n);
                }
            }
        }
    }
    let mut n = 0;
    walk(ty, &mut n);
    n
}

// ---------------------------------------------------------------------------
// Test #1 — CHARACTERIZATION
// ---------------------------------------------------------------------------
//
// Bound the operator-node count of every published binding `r#type`.
// The graph-native synthesis walks a bounded shallow surface and
// publishes `IndexedAccess` / `Ref` shells, not full materialiser-
// driven expansions. A naive replacement that drove the carrier
// through an Expanded-mode `Instantiate`
// (`context.projection_reduction.mode = Expanded`) would compound
// heritage hops into huge operator trees and exceed the 256-node
// aggregate budget on the 4-layer fixture.
#[test]
fn slot_bindings_field_type_node_count_is_bounded() {
    let host = build_test_host();
    upsert_ts(
        &host,
        "/src/heritage.ts",
        r#"
        export interface BaseSlot { ctx: { tag: string } }
        export interface Layer1 extends BaseSlot { ctx: { tag: string } }
        export interface Layer2 extends Layer1 { ctx: { tag: string } }
        export interface Layer3 extends Layer2 { ctx: { tag: string } }
        export interface Slots {
          default(props: Layer3): any;
        }
        "#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './heritage'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");

    // We expect a single `default.ctx` binding. Whatever the
    // published shape, its operator-node count must stay below the
    // per-binding budget. A naive synthesis that expanded
    // intersections/heritage greedily would exceed this budget on
    // this fixture.
    let mut total_nodes = 0;
    let mut any_binding = false;
    for slot in &meta.slots {
        for binding in &slot.bindings {
            any_binding = true;
            total_nodes +=
                count_type_expr_nodes(&shallow_binding_type(&host, "/src/Comp.vue", binding));
        }
    }
    assert!(
        any_binding,
        "fixture must produce at least one binding (slot bindings={:?})",
        meta.slots
            .iter()
            .map(|s| (s.name.as_str(), s.bindings.len()))
            .collect::<Vec<_>>(),
    );
    assert!(
        total_nodes <= 256,
        "binding `r#type` aggregate operator-node count must stay <= 256 (graph-native synthesis \
         publishes shallow shells); observed total={total_nodes} across \
         {} bindings",
        meta.slots.iter().map(|s| s.bindings.len()).sum::<usize>(),
    );
}

// ---------------------------------------------------------------------------
// Test #2 — REGRESSION (un-ignored by SA-1.B-impl)
// ---------------------------------------------------------------------------
//
// Verifies the synthesis path stays in `Navigate` mode and never issues
// an Expanded-mode `Instantiate`
// (`context.projection_reduction.mode = Expanded`) from the slot-binding-carrier
// shallow walker. The
// `SLOT_BINDING_EXPANDED_INSTANTIATE_CALLS` counter is bumped by
// `project_semantic_dispatch::build::build_instantiate` whenever an
// `Expanded` body-mode is requested.
//
// Scope note: the host-global counter remains process-wide and may
// bump from peer dispatches that share the same workspace under
// concurrent test execution. The per-request mirror snapshotted on
// the audit payload (via `RequestContext::expanded_instantiate_calls`)
// gives synthesis-attributable visibility independently of those peer
// dispatches; that mirror is what this regression asserts.
#[test]
fn enrich_does_not_eagerly_instantiate_carrier() {
    let result = AuditedRequest::builder()
        .files([
            (
                "/src/slots.ts",
                "export interface Slots { default(props: { row: string; index: number }): any }",
            ),
            (
                "/src/Comp.vue",
                r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
            ),
        ])
        .resolve_component_meta("/src/Comp.vue");
    let (_meta, _resolved, record) = result.expect("audited resolution");
    let observed = match &record.kind_payload {
        verter_audit::RequestKindPayload::ComponentMeta(payload) => {
            payload.expanded_instantiate_calls
        }
        other => panic!("expected ComponentMeta payload, got {other:?}"),
    };
    assert_eq!(
        observed, 0,
        "graph-native slot-binding synthesis must drive the carrier walk in Navigate mode; \
         observed synthesis-attributable Expanded Instantiate dispatches={observed} \
         (per-request snapshot from RequestContext)"
    );
}

// ---------------------------------------------------------------------------
// Test #2b — REGRESSION: synthesis-SCOPED Expanded Instantiate counter
// ---------------------------------------------------------------------------
//
// The request-WIDE `expanded_instantiate_calls` counter (Test #2) also
// counts the canonical macro-surface PRODUCER's legitimate `Expanded`
// expansions of imported macro roots — so a cross-file `defineSlots` whose
// payload imports a heritage chain may bump the request-wide counter from
// producer work, NOT from synthesis. This test pins the SYNTHESIS-SCOPED
// counter (`synthesis_expanded_instantiate_calls`), which isolates "an
// Expanded Instantiate fired INSIDE the slot-binding synthesis" via the
// `SynthesisScopeGuard` depth marker. It must be ZERO: synthesis drives the
// carrier walk in Navigate / Skeleton, never the giant-tree Expanded body
// mode, regardless of producer-phase expansions.
//
// The fixture uses a CROSS-FILE heritage slot payload (`Slots` extends an
// imported `BaseSlots`), exercising the carrier path the carrier-complete
// surface reader resolves in Skeleton — the path that would have driven an
// Expanded dispatch had the reader been wired to `Expanded`.
#[test]
fn synthesis_scoped_expanded_instantiate_is_zero_for_cross_file_heritage_slots() {
    let result = AuditedRequest::builder()
        .files([
            (
                "/src/base.ts",
                "export interface BaseSlots { default(props: { row: string; index: number }): any }",
            ),
            (
                "/src/slots.ts",
                "import type { BaseSlots } from './base'\nexport interface Slots extends BaseSlots { header(props: { title: string }): any }",
            ),
            (
                "/src/Comp.vue",
                r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
            ),
        ])
        .resolve_component_meta("/src/Comp.vue");
    let (_meta, _resolved, record) = result.expect("audited resolution");
    let synthesis_scoped = match &record.kind_payload {
        verter_audit::RequestKindPayload::ComponentMeta(payload) => {
            payload.synthesis_expanded_instantiate_calls
        }
        other => panic!("expected ComponentMeta payload, got {other:?}"),
    };
    assert_eq!(
        synthesis_scoped, 0,
        "slot-binding synthesis must NOT issue an Expanded Instantiate even for \
         cross-file heritage slot payloads; observed synthesis-scoped Expanded \
         Instantiate dispatches={synthesis_scoped} (per-request snapshot from \
         RequestContext::synthesis_expanded_instantiate_calls)"
    );
}

// ---------------------------------------------------------------------------
// Test #3 — CHARACTERIZATION
// ---------------------------------------------------------------------------
//
// Inline + heritage-derived bindings must both surface on the published
// `ComponentMetaAnalysis`. The legacy path miscounts when heritage
// sits on a slot's parameter type and the carrier is imported.
#[test]
fn slot_bindings_resolve_via_graph_native_dispatch() {
    let host = build_test_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        r#"
        export interface BasePayload { id: string; index: number }
        export interface ExtendedPayload extends BasePayload { tag: string }
        export interface Slots {
          default(props: ExtendedPayload): any;
        }
        "#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");
    let names = slot_bindings(&meta, "default");
    assert!(
        names.contains(&"id") && names.contains(&"index") && names.contains(&"tag"),
        "graph-native synthesis must surface inline + heritage bindings; observed={:?}",
        names,
    );
}

// ---------------------------------------------------------------------------
// Test #4 — REGRESSION
// ---------------------------------------------------------------------------
//
// Optionality through intersection: required wins over optional under
// TypeScript's intersection rule. The graph-native synthesis must
// resolve `SlotA & SlotB` so that `default.value` publishes with
// optional=false (the required arm dominates). The synthesis must
// surface a binding row at all (a no-op synthesis would fail the
// `expect("expected default.value binding")` lookup), and the published
// `ExpandedField.optional` must be `false` (a naive synthesis that
// lifts the optional flag from the first arm would publish optional=true
// and fail the final assertion).
#[test]
fn slot_binding_optionality_preserved_through_intersection() {
    let host = build_test_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        r#"
        export interface SlotA { default(props: { value?: string }): any }
        export interface SlotB { default(props: { value: string }): any }
        export type Slots = SlotA & SlotB;
        "#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");
    let binding = slot_binding(&meta, "default", "value").expect("expected default.value binding");
    assert!(
        binding.raw_type.is_none() || binding.raw_type.as_deref() != Some("undefined"),
        "binding raw_type leaked undefined (defensive negative)",
    );
    // Required-wins: under TS intersection, the binding must NOT be
    // optional. A naive synthesis that lifts the optional flag from
    // the first arm would publish optional=true here.
    let in_slot = meta
        .slots
        .iter()
        .find(|s| s.name == "default")
        .expect("default slot");
    let any_optional_field_in_template_meta = in_slot.bindings.iter().any(|b| b.name == "value");
    assert!(
        any_optional_field_in_template_meta,
        "required-wins intersection must publish a non-optional binding; observed bindings={:?}",
        in_slot
            .bindings
            .iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>(),
    );

    // The `SlotBindingAnalysis.raw_type` does not carry optional; we
    // therefore reach into the host's resolved expansion to read the
    // flag via `resolve_component_meta(Expanded)`. A naive synthesis
    // that emits optional=true for the first arm fails this assertion.
    let resolved = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved expanded");
    let key = "default.value".to_string();
    let expanded_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|e| e.slot_bindings.iter().find(|f| f.name == key));
    let optional = expanded_field
        .map(|f| f.optional)
        .expect("default.value must appear in expanded slot_bindings");
    assert!(
        !optional,
        "intersection of optional + required slot binding must publish optional=false (TS \
         required-wins); observed optional={optional}",
    );
}

// ---------------------------------------------------------------------------
// Test #5 — REGRESSION
// ---------------------------------------------------------------------------
//
// An unresolvable carrier (lazy binding) must produce zero binding
// rows; a resolvable inline carrier must produce a real binding row.
// The discriminating contract: the graph-native synthesis publishes
// the row as a shallow `SyntheticSlotBinding` carrier (never a leaked
// synthetic `string` binding, never an eager expansion), and the
// explicit terminal-demand walk deepens `default.row` to the concrete
// `string` through the content-free synthetic-binding identity.
//
// TODO(follow-up): a multi-hop unresolvable carrier (e.g. `import type
// { Slots } from './a'; export type { Slots } from './missing'`) would
// genuinely discriminate the IndexedAccess-shell invariant from a
// naive "drop unresolved" alternative. Today's fixture only exercises
// the first-hop unresolvable case; the inline `row: string` arm is
// what catches a no-op synthesis.
#[test]
fn raised_typeexpr_for_lazy_binding_is_indexed_access_shell() {
    let host = build_test_host();
    // Unresolvable: imports from a missing module.
    upsert_vue(
        &host,
        "/src/Unresolvable.vue",
        r#"<script setup lang="ts">
import type { Slots } from './missing-module'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    // Resolvable inline: full type with literal members.
    upsert_vue(
        &host,
        "/src/Resolvable.vue",
        r#"<script setup lang="ts">
defineSlots<{ default(props: { row: string }): any }>()
</script>
<template><div /></template>
"#,
    );

    let unresolvable_meta = host
        .get_component_meta("/src/Unresolvable.vue")
        .expect("unresolvable meta");
    let resolvable_meta = host
        .get_component_meta("/src/Resolvable.vue")
        .expect("resolvable meta");

    // Unresolvable: at most an empty slot, no concrete bindings.
    let unresolvable_bindings: Vec<&str> = unresolvable_meta
        .slots
        .iter()
        .flat_map(|s| s.bindings.iter().map(|b| b.name.as_str()))
        .collect();
    assert!(
        unresolvable_bindings.is_empty(),
        "unresolvable carrier must publish zero bindings; observed={unresolvable_bindings:?}",
    );

    // Resolvable: the `row` binding PUBLISHES the shallow first-class
    // SyntheticSlotBinding carrier (shallow-by-default — a graph-native
    // no-payload row never eagerly expands at publication). A naive
    // synthesis that doesn't enter the inline arm would publish no
    // binding row at all.
    let resolvable_bindings = slot_bindings(&resolvable_meta, "default");
    assert_eq!(
        resolvable_bindings,
        vec!["row"],
        "resolvable inline must publish exactly the `row` binding",
    );
    let row_binding = slot_binding(&resolvable_meta, "default", "row")
        .expect("default.row must exist on resolvable");
    let row_type = shallow_binding_type(&host, "/src/Resolvable.vue", row_binding);
    let TypeExpr::SyntheticSlotBinding(carrier) = &row_type else {
        panic!(
            "resolvable inline `row` binding must publish the shallow \
             SyntheticSlotBinding carrier; observed type_expr={row_type:?}",
        );
    };
    assert_eq!(carrier.slot_name.as_deref(), Some("default"));
    assert_eq!(carrier.binding_name.as_ref(), "row");
    // Explicit deepening/demand: the terminal-demand walk resolves the
    // carrier through the content-free synthetic-binding identity to the
    // concrete inline annotation (`row: string`).
    let demanded = demand_binding_type(&host, "/src/Resolvable.vue", row_binding);
    assert!(
        matches!(
            demanded,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "explicit demand must materialize default.row to the concrete `string`; \
         observed {demanded:?}",
    );
}

// ---------------------------------------------------------------------------
// Test #6h — REGRESSION (hermetic warm-pass)
// ---------------------------------------------------------------------------
//
// First call <60s; second call <100ms. Cache reuse path must drop into
// the `ComponentMetaResultDb` warm cache. SA-1.B-impl tightens the
// dep-signature wiring so this regression triggers consistently.
#[test]
fn slot_bindings_warm_pass_o1_for_mocked_heritage() {
    let host = build_test_host();
    // 50-member heritage chain — purely synthetic, no external corpus.
    let mut heritage = String::new();
    heritage.push_str("export interface Base { tag0: string }\n");
    for i in 1..50 {
        heritage.push_str(&format!(
            "export interface I{i} extends I{prev} {{ tag{i}: string }}\n",
            prev = i - 1,
            i = i,
        ));
    }
    heritage.push_str("export interface Slots { default(props: I49): any }\n");
    heritage = heritage.replace("extends I0", "extends Base");
    upsert_ts(&host, "/src/heritage.ts", &heritage);
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './heritage'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let cold_start = Instant::now();
    let _ = host.get_component_meta("/src/Comp.vue").expect("cold meta");
    let cold = cold_start.elapsed();
    assert!(
        cold.as_secs() < 60,
        "cold pass for 50-member heritage must finish under 60s; observed {cold:?}",
    );

    let warm_start = Instant::now();
    let _ = host.get_component_meta("/src/Comp.vue").expect("warm meta");
    let warm = warm_start.elapsed();
    assert!(
        warm.as_millis() < 100,
        "warm pass must hit the result cache; observed {warm:?}",
    );
}

// ---------------------------------------------------------------------------
// Test #7h — REGRESSION
// ---------------------------------------------------------------------------
//
// 50-member heritage Intersection cold-resolves under 500ms. The
// graph-native synthesis must keep the per-binding walker bounded; a
// naive O(N^2) traversal would blow past 500ms once N reaches 50.
//
// TODO(follow-up): the synthetic 50-member intersection completes well
// under 500ms even with a moderately inefficient traversal. The
// corpus-scale slowness that would genuinely discriminate against an
// O(N^2) baseline requires deeper macro carriers (see the gated Test
// #7 in the external_corpus module). A hermetic alternative would be
// a per-thread tracer that asserts the walker's visited-set high-water
// mark stays below a threshold proportional to N rather than N^2.
#[test]
fn cold_synthesis_terminates_within_500ms_for_50_member_heritage() {
    let host = build_test_host();
    // Compose a 50-member intersection.
    let mut decls = String::new();
    let mut intersection = String::new();
    for i in 0..50 {
        decls.push_str(&format!(
            "export interface S{i} {{ default(props: {{ tag{i}: string }}): any }}\n",
            i = i,
        ));
        if i > 0 {
            intersection.push_str(" & ");
        }
        intersection.push_str(&format!("S{i}"));
    }
    decls.push_str(&format!("export type Slots = {intersection};\n"));
    upsert_ts(&host, "/src/types.ts", &decls);
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let started = Instant::now();
    let _ = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");
    let elapsed = started.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "cold synthesis on 50-member intersection must finish under 500ms; observed {elapsed:?}",
    );
}

// ---------------------------------------------------------------------------
// Test #8h — REGRESSION
// ---------------------------------------------------------------------------
//
// The audit payload's serialized byte size for a heritage-bearing
// component must stay below 64 KiB on hermetic mocked-corpus
// fixtures. Discriminating contract: the synthesis must bound the
// per-binding diagnostic emission and the per-binding `r#type`
// shell so a 30-slot cyclic-heritage fixture stays under the
// 64 KiB envelope.
//
// The 30-slot fixture is sized so that it (a) genuinely populates
// the diagnostics path (cyclic heritage forces at least one
// `MacroExpansionDiagnostics` entry per slot), (b) exercises the
// per-binding `compute_exactness_for_node` shell-builder more than
// once, and (c) fits within the 64 KiB envelope today. A naive
// synthesis that emitted O(slot * heritage-depth) diagnostics, or
// that inlined the full carrier `TypeExpr` per binding, would
// breach the envelope on this fixture.
//
// TODO(follow-up): the corpus-scale ChatMessage equivalent (200+
// bindings + 5+ heritage layers) requires a 2 MiB envelope, gated
// behind `external-corpus`. The hermetic 30-slot mock here is the
// smallest fixture that exercises both the diagnostics fan-out and
// the per-binding shell synthesis without falling under the
// noise floor of an empty payload.
#[test]
fn synthesis_audit_payload_byte_budget_mocked() {
    // 30-slot cyclic-heritage fixture. Cyclic `CycleA <-> CycleB`
    // forces the diagnostics path; 30 slots × 1 binding each
    // exercises the per-binding fan-out without crossing the 64 KiB
    // envelope.
    let mut heritage = String::new();
    heritage.push_str("export interface CycleA extends CycleB { tagA: string }\n");
    heritage.push_str("export interface CycleB extends CycleA { tagB: string }\n");
    heritage.push_str("export interface Slots {\n");
    for i in 0..30 {
        heritage.push_str(&format!(
            "  slot{i}(props: {{ binding{i}: CycleA }}): any;\n",
            i = i,
        ));
    }
    heritage.push_str("}\n");

    let result = AuditedRequest::builder()
        .files([
            ("/src/heritage.ts", heritage.as_str()),
            (
                "/src/Comp.vue",
                r#"<script setup lang="ts">
import type { Slots } from './heritage'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
            ),
        ])
        .resolve_component_meta("/src/Comp.vue");

    match result {
        Ok((_meta, _resolution, record)) => {
            let bytes =
                serde_json::to_vec(&record).expect("audit record must serialize via serde_json");
            let len = bytes.len();
            assert!(
                len < 64 * 1024,
                "audit record serialized size must stay < 64 KiB on 30-slot cyclic-heritage \
                 corpus; observed {len} bytes",
            );
            // Discriminating negative: assert the payload genuinely
            // populates above the empty-payload baseline. A no-op
            // synthesis would emit zero diagnostics and the serialized
            // record would be near the empty-payload size. The
            // 1 KiB threshold is comfortably above the empty payload
            // and well below the 64 KiB ceiling.
            assert!(
                len > 1024,
                "30-slot cyclic-heritage fixture must populate the audit payload above the \
                 empty-payload baseline; observed {len} bytes (suggests a no-op synthesis)",
            );
        }
        Err(err) => panic!(
            "synthesis_audit_payload_byte_budget_mocked: AuditedRequest failed unexpectedly: \
             {err:?}",
        ),
    }
}

// ---------------------------------------------------------------------------
// Test #10 — REGRESSION
// ---------------------------------------------------------------------------
//
// Note: the prior in-src source-guard
// (`no_phase_archaeology_in_slot_binding_graph`) was removed because
// the broader-scope `slot_binding_graph_no_phase_archaeology` guard in
// `crates/verter_session/tests/cases/architecture_guards.rs` already scans
// `slot_binding_graph.rs` with a superset of the same needles, and
// keeping needle literals in `crates/*/src/**` is incompatible with
// the strict `phase_archaeology_test_files_count_zero` invariant.

// ---------------------------------------------------------------------------
// Test #11 — REGRESSION
// ---------------------------------------------------------------------------
//
// Cyclic heritage on a slot's payload type must terminate without
// panic and surface a diagnostic (not a giant TypeExpr cycle). The
// graph-native synthesis emits `MacroExpansionDiagnostics` via the
// walker's `ShallowDiagnostic::CycleShortCircuited` accumulator. A
// no-op synthesis would publish zero diagnostics and fail this
// assertion.
//
// TODO(follow-up): a tighter discriminator would assert the specific
// `MacroExpansionKind::DefineSlots` diagnostic kind and the cycle
// short-circuit count; today the test relies on the single
// "diagnostics non-empty" property to discriminate.
#[test]
fn slot_bindings_with_cyclic_heritage_terminates() {
    let host = build_test_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        r#"
        export interface A extends B { tagA: string }
        export interface B extends A { tagB: string }
        export interface Slots { default(props: A): any }
        "#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    // Drive in a worker thread with a generous stack. The invariant
    // under test is that cyclic heritage TERMINATES — the cycle is
    // short-circuited and `get_component_meta` returns successfully
    // with a cycle diagnostic. The cycle detector bounds recursion
    // depth, so the resolution completes; this is a completion test,
    // not an unbounded-recursion guard. The 2 MiB cap is deliberately
    // generous so the per-frame cost of the cold-build
    // cooperative-admission path (each hop of a deep type resolution
    // nests one cold build, and the strict warm-read validator threads
    // a resolver-context handle through every cold build) cannot trip
    // a false overflow on this legitimately-completing resolution.
    let host_for_thread = Arc::clone(&host);
    let join = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            host_for_thread
                .get_component_meta("/src/Comp.vue")
                .expect("component meta")
        })
        .expect("spawn");
    let meta = join
        .join()
        .expect("worker must not panic on cyclic heritage");

    // Diagnostics must record the cycle.
    let has_cycle_diag = !meta.macro_expansion_diagnostics.is_empty();
    assert!(
        has_cycle_diag,
        "cyclic heritage must surface MacroExpansionDiagnostics on the component meta; \
         observed diagnostics count={}",
        meta.macro_expansion_diagnostics.len(),
    );
}

// ---------------------------------------------------------------------------
// Test #12 — REGRESSION
// ---------------------------------------------------------------------------
//
// Deep heritage chain on a slot payload must publish binding rows
// whose `r#type` shapes stay bounded. The graph-native synthesis
// publishes a fixed-depth shell for each heritage member (one node
// per `tag{i}` annotation, plus the binding-name shell). A naive
// synthesis that compounded heritage hops into a single per-binding
// `TypeExpr` would exceed the 32-node budget for the I0..I29 chain.
//
// Discriminating: we assert the bindings vector is non-empty (so a
// no-op synthesis fails) AND every published binding's op-node count
// stays under the budget.
#[test]
fn synthesis_stack_depth_bounded() {
    let host = build_test_host();
    let mut decls = String::new();
    decls.push_str("export interface I0 { tag0: string }\n");
    for i in 1..30 {
        decls.push_str(&format!(
            "export interface I{i} extends I{prev} {{ tag{i}: string }}\n",
            prev = i - 1,
            i = i,
        ));
    }
    decls.push_str("export interface Slots { default(props: I29): any }\n");
    upsert_ts(&host, "/src/heritage.ts", &decls);
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './heritage'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");
    // Discriminating against a no-op synthesis: the 30-level heritage
    // chain must publish at least one binding (the `tag29` field from
    // the leaf interface, plus inherited `tag0..tag28`). A no-op
    // synthesis would publish zero bindings and the inner loop below
    // would execute zero times — the empty-bindings case is rejected
    // here so the bounded-depth assertion below must actually run.
    let total_bindings: usize = meta.slots.iter().map(|s| s.bindings.len()).sum();
    assert!(
        total_bindings > 0,
        "30-level heritage must publish at least one binding (no-op synthesis would publish \
         none); observed total bindings={total_bindings}",
    );
    // Per-binding op-node depth must stay shallow even for 30-level
    // heritage. A naive synthesis that compounded heritage hops into
    // a single `TypeExpr` per binding would exceed this budget.
    for slot in &meta.slots {
        for binding in &slot.bindings {
            let n = count_type_expr_nodes(&shallow_binding_type(&host, "/src/Comp.vue", binding));
            assert!(
                n <= 32,
                "30-level heritage binding {}.{} type_expr op-node count must stay <= 32 \
                 (graph-native publishes shallow shells); observed {n}",
                slot.name,
                binding.name,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Test #13 — REGRESSION
// ---------------------------------------------------------------------------
//
// The graph-native lowering's `raise_node_to_type_expr` recursion depth
// stays bounded. SA-1.B-impl wires a thread-local depth probe; on base
// no probe exists. We assert the depth via the published binding
// `r#type` shape as a proxy: the graph-native shell stays shallow even
// for 100-level heritage. This complements the runtime stack bound in
// #12.
#[test]
fn raise_node_to_type_expr_recursion_depth_bounded() {
    let host = build_test_host();
    let mut decls = String::new();
    decls.push_str("export interface I0 { tag0: string }\n");
    for i in 1..100 {
        decls.push_str(&format!(
            "export interface I{i} extends I{prev} {{ tag{i}: string }}\n",
            prev = i - 1,
            i = i,
        ));
    }
    decls.push_str("export interface Slots { default(props: I99): any }\n");
    upsert_ts(&host, "/src/heritage.ts", &decls);
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './heritage'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");

    // Discriminating against a no-op synthesis: the 100-level
    // heritage must publish at least one binding so the bounded-depth
    // loop below has work to validate.
    let total_bindings: usize = meta.slots.iter().map(|s| s.bindings.len()).sum();
    assert!(
        total_bindings > 0,
        "100-level heritage must publish at least one binding (no-op synthesis would publish \
         none); observed total bindings={total_bindings}",
    );
    // Each binding's r#type op-node count is an upper bound on the
    // raise-recursion depth. Asserting <= 32 catches a naive synthesis
    // that retained the full heritage TypeExpr per binding.
    for slot in &meta.slots {
        for binding in &slot.bindings {
            let depth =
                count_type_expr_nodes(&shallow_binding_type(&host, "/src/Comp.vue", binding));
            assert!(
                depth <= 32,
                "raise depth proxy: binding {}.{} type_expr op-node count must stay <= 32; \
                 observed {depth}",
                slot.name,
                binding.name,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Test #14 — REGRESSION
// ---------------------------------------------------------------------------
//
// Editing the carrier file invalidates the slot-binding cache: the
// second `get_component_meta` call must reflect the updated shape,
// not stale published bindings. The host upsert path invalidates the
// workspace, the dep-signature fence rebuilds, and the graph-native
// synthesis re-runs against the new carrier shape. A naive
// implementation that cached slot bindings under the SFC's content
// hash alone (ignoring carrier dep-signature) would publish stale
// `["row"]` on the second call and fail the second assertion.
#[test]
fn slot_bindings_invalidate_on_carrier_edit() {
    let host = build_test_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Slots { default(props: { row: string }): any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let first = host
        .get_component_meta("/src/Comp.vue")
        .expect("first meta");
    let first_names = slot_bindings(&first, "default");
    assert_eq!(
        first_names,
        vec!["row"],
        "first call must publish exactly `row`; observed {first_names:?}",
    );

    // Edit the carrier — replace `row` with `column` and add `index`.
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Slots { default(props: { column: number; index: number }): any }",
    );

    let second = host
        .get_component_meta("/src/Comp.vue")
        .expect("second meta");
    let mut second_names = slot_bindings(&second, "default");
    second_names.sort_unstable();
    assert_eq!(
        second_names,
        vec!["column", "index"],
        "carrier edit must invalidate the slot-binding cache; observed {second_names:?} \
         (expected [column, index])",
    );
}

// ---------------------------------------------------------------------------
// Test #16 — CHARACTERIZATION
// ---------------------------------------------------------------------------
//
// Partial publish: when one heritage arm is cyclic but another is
// concrete, the concrete arm's bindings must publish; the cyclic arm
// must surface a diagnostic.
#[test]
fn slot_bindings_partial_when_one_arm_recursive_others_succeed() {
    let host = build_test_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        r#"
        export interface Cyclic extends Cyclic { c: string }
        export interface Concrete { a: string; b: number }
        export interface Slots {
          default(props: Cyclic & Concrete): any;
        }
        "#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");
    let names = slot_bindings(&meta, "default");
    assert!(
        names.contains(&"a") && names.contains(&"b"),
        "concrete-arm bindings must publish even when a peer arm is cyclic; observed={:?}",
        names,
    );
    assert!(
        !meta.macro_expansion_diagnostics.is_empty(),
        "cyclic-arm must surface a MacroExpansionDiagnostics entry alongside the partial publish",
    );
}

// ---------------------------------------------------------------------------
// Test #17 — REGRESSION
// ---------------------------------------------------------------------------
//
// `dep_signature` on the cached `ComponentMetaResultDb` entry must
// include the carrier's canonical id when the SFC's macro
// argument resolves through an imported type alias. The
// `dep_signature_for_owner_in_test` accessor exposes the merged
// set for inspection.
//
// Carrier-fact propagation is owned by the synthesis path: every
// `defineSlots` / `defineEmits` / peer macro lowering produces a
// graph node that may carry a cross-file `DeclRef` /
// `InstantiationRef` whose `identity.canonical_id` differs from
// the owner. The synthesis walks the freshly-lowered argument
// and pushes `(canonical_id, WholeHash)` carrier facts into the
// per-request dep-signature accumulator (drained at publish into
// the cached entry's `dep_signature`). Without this, an inner
// walker that uses bare `dispatch.execute_type_node(ResolveDecl(..))` would
// discard the carrier's whole-hash via `build_project_path`'s
// project-generation-only fence and the warm cache would not
// invalidate when the carrier is edited.
#[test]
fn slot_bindings_dep_signature_merges_carrier_deps() {
    let host = build_test_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Slots { default(props: { row: string }): any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );
    let _ = host.get_component_meta("/src/Comp.vue");
    let dep_canonical_ids: Vec<Arc<str>> =
        crate::component_meta_result_db::ComponentMetaResultDb::dep_signature_for_owner_in_test(
            &host,
            "/src/Comp.vue",
        );
    assert!(
        dep_canonical_ids
            .iter()
            .any(|c| c.as_ref() == "/src/types.ts"),
        "dep_signature must include the imported carrier; observed {dep_canonical_ids:?}",
    );
}

// ---------------------------------------------------------------------------
// Test #18 — REGRESSION
// ---------------------------------------------------------------------------
//
// Parser-path metadata (`raw_type` from `AnalyzedSlotFieldBinding`)
// must be merged with the graph-native binding type. The published
// `default.x` row keeps the parser-side display `raw_type` AND the
// shallow first-class `SyntheticSlotBinding` carrier as its typed
// source (graph-native no-payload rows publish shallow-by-default);
// the concrete `string` materializes ONLY through the explicit
// terminal-demand walk. A naive synthesis that emitted the
// parser-side `AnalyzedSlotFieldBinding` without walking the inline
// `{ x: string }` annotation would publish no graph-native row (and
// the demand below could never resolve `string`).
//
// TODO(follow-up): once `SlotBindingSource` is exposed on
// `SlotBindingAnalysis`, this test should additionally assert
// `binding.source == SlotBindingSource::GraphNative` for an inline
// `defineSlots` carrier.
#[test]
fn slot_bindings_parser_path_metadata_merged_with_graph_type() {
    let host = build_test_host();
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
defineSlots<{
  default(props: { x: string }): any;
}>();
</script>
<template>
  <slot name="default" :x="'a'" />
</template>
"#,
    );

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");
    let row = slot_binding(&meta, "default", "x").expect("default.x binding must publish");
    // Parser-path display metadata must survive the merge onto the
    // graph-native row.
    assert_eq!(
        row.raw_type.as_deref(),
        Some("string"),
        "parser-path raw_type must be preserved on the merged row; observed {:?}",
        row.raw_type,
    );
    // The published typed source stays the shallow SyntheticSlotBinding
    // carrier (shallow-by-default for graph-native no-payload rows).
    let row_type = shallow_binding_type(&host, "/src/Comp.vue", row);
    let TypeExpr::SyntheticSlotBinding(carrier) = &row_type else {
        panic!(
            "graph-native no-payload row must publish the shallow \
             SyntheticSlotBinding carrier; observed type_expr={row_type:?}",
        );
    };
    assert_eq!(carrier.slot_name.as_deref(), Some("default"));
    assert_eq!(carrier.binding_name.as_ref(), "x");
    // The concrete value materializes ONLY under explicit demand: the
    // terminal-demand walk deepens through the content-free
    // synthetic-binding identity to the inline `x: string` annotation.
    let demanded = demand_binding_type(&host, "/src/Comp.vue", row);
    assert!(
        matches!(
            demanded,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "explicit demand must materialize default.x to the concrete `string`; \
         observed {demanded:?}",
    );
}

// ---------------------------------------------------------------------------
// Test #19 — CHARACTERIZATION
// ---------------------------------------------------------------------------
//
// Compute exactness must unwrap aliases for concrete primitives.
// `defineProps<{ msg: MyStr }>` where `type MyStr = string` must
// classify the `msg` field as `ExactConcrete`. Both the slot-binding
// synthesis path and the `defineProps` fast-path route through the
// shared
// [`crate::meta_resolve::exactness::classify_node`] /
// [`crate::meta_resolve::exactness::classify_type_expr`] predicates so
// `Alias(Primitive)` resolves to `Primitive` and publishes as
// concrete. This test exercises the props side; the slot-binding
// equivalent is exercised by sibling tests.
#[test]
fn compute_exactness_unwraps_alias_for_concrete() {
    let host = build_test_host();
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
type MyStr = string;
defineProps<{ msg: MyStr }>()
</script>
<template><div /></template>
"#,
    );

    let resolved = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved expanded");
    let evaluated = resolved
        .evaluated_types
        .as_ref()
        .expect("expanded component types must be present");

    let msg_field = evaluated
        .props
        .iter()
        .find(|f| f.name == "msg")
        .expect("`msg` prop must publish");
    assert_eq!(
        msg_field.exactness,
        ExpansionExactness::ExactConcrete,
        "alias for primitive must publish ExactConcrete; observed={:?}",
        msg_field.exactness,
    );
}

// ---------------------------------------------------------------------------
// Test #20 — REGRESSION
// ---------------------------------------------------------------------------
//
// 512-arm intersection over the Shallow surface. The walker must terminate
// without consuming host stack per structural arm. This exercises the public
// `get_component_meta` path end-to-end in an isolated 2 MiB subprocess, so an
// abort cannot take down the parent harness.
#[test]
fn walker_stack_safe_for_512_intersection_on_2_mib() {
    const CHILD_MARKER: &str = "VERTER_SLOT_INTERSECTION_STACK_CHILD";
    if std::env::var_os(CHILD_MARKER).is_none() {
        let exe = std::env::current_exe().expect("current unit-test executable");
        let status = Command::new(exe)
            .arg("--exact")
            .arg(
                "meta_resolve::slot_binding_graph_tests::walker_stack_safe_for_512_intersection_on_2_mib",
            )
            .arg("--nocapture")
            .env(CHILD_MARKER, "1")
            .env_remove("RUST_MIN_STACK")
            .status()
            .expect("spawn isolated slot-intersection stack child");
        assert!(
            status.success(),
            "the isolated 2 MiB slot-intersection child must exit cleanly; status={status}"
        );
        return;
    }

    let host = build_test_host();
    let mut decls = String::new();
    let mut intersection = String::new();
    for i in 0..512 {
        decls.push_str(&format!(
            "export interface S{i} {{ default(props: {{ tag{i}: string }}): any }}\n",
            i = i,
        ));
        if i > 0 {
            intersection.push_str(" & ");
        }
        intersection.push_str(&format!("S{i}"));
    }
    decls.push_str(&format!("export type Slots = {intersection};\n"));
    upsert_ts(&host, "/src/types.ts", &decls);
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    // A 512-arm intersection is a legitimate finite resolution. The public
    // component-meta path must iterate it on a normal 2 MiB worker stack;
    // structural depth is not an operational limit. The subprocess isolates
    // an aborting stack regression from the parent test runner.
    let host_for_thread = Arc::clone(&host);
    let join = std::thread::Builder::new()
        .name("slot-intersection-stack-2mib".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            host_for_thread
                .get_component_meta("/src/Comp.vue")
                .map(|_| ())
        })
        .expect("spawn 2 MiB slot-intersection worker");
    let outcome = join
        .join()
        .expect("2 MiB slot-intersection worker must not panic");
    assert!(
        outcome.is_some(),
        "512-arm intersection through the public component-meta entry must resolve on a 2 MiB stack"
    );
}

// ---------------------------------------------------------------------------
// Test #21 — REGRESSION
// ---------------------------------------------------------------------------
//
// Warm-pass diagnostic replay: a parameterized recursive type
// (`Cycle<T> = { self: Cycle<T> }`) drives the walker's
// `InstantiationRef` arm through `QueryResult::Recursive`, which
// emits `ShallowDiagnostic::CyclicInstantiation`. Cold-pass
// diagnostics must be replayed on warm-cache hits via
// `CacheRead::walker_diagnostics`. A naive publication path that
// stripped diagnostics on the warm hit would publish
// `warm_diag_count == 0` and fail the equality below.
#[test]
fn slot_bindings_warm_pass_replays_walker_diagnostics_for_cyclic_heritage() {
    let host = build_test_host();
    // Parameterized recursive type: `Cycle<T>`'s body references
    // `Cycle<T>` itself. The walker's `InstantiationRef` arm
    // dispatches `Instantiate { Cycle, args=[T] }` which short-
    // circuits as `QueryResult::Recursive` and emits
    // `ShallowDiagnostic::CyclicInstantiation { decl: Cycle }`.
    upsert_ts(
        &host,
        "/src/types.ts",
        r#"
        export type Cycle<T> = { self: Cycle<T>; tag: T };
        export interface Slots { default(props: Cycle<string>): any }
        "#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    let cold = host.get_component_meta("/src/Comp.vue").expect("cold meta");
    let cold_diag_count = cold.macro_expansion_diagnostics.len();
    assert!(
        cold_diag_count > 0,
        "cold pass must emit cycle diagnostics; observed count={cold_diag_count}",
    );

    let warm = host.get_component_meta("/src/Comp.vue").expect("warm meta");
    let warm_diag_count = warm.macro_expansion_diagnostics.len();
    assert_eq!(
        warm_diag_count, cold_diag_count,
        "warm pass must replay walker diagnostics (cold={cold_diag_count}, warm={warm_diag_count})",
    );
}

// ---------------------------------------------------------------------------
// Test #22 — REGRESSION
// ---------------------------------------------------------------------------
//
// Budget-exceeded synthesis must not warm the cache. The
// `publish_component_meta_cache_entry` site gates the publish on
// `resolved.synthesis_should_suppress`; the synthesis loop in
// `resolve_slot_bindings_graph_native` consumes
// `HostConfig::recursion_budget_overrides.synthesis_steps` per
// sub-action (lower / payload / slot-surface / param-surface) and
// pushes a `MacroExpansionDiagnostics` envelope carrying
// `ExpansionStopReason::BudgetExceeded` when the cap is exceeded,
// flipping `should_suppress = true` so the cache write is skipped.
#[test]
fn slot_bindings_skip_cache_on_budget_exceeded() {
    let mut config = HostConfig::default();
    config.recursion_budget_overrides.synthesis_steps = Some(1);
    let host = Arc::new(VerterHost::new_standalone(config));
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Slots { default(props: { row: string }): any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );

    // Resolve component meta and inspect both the cache state AND the
    // synthesis state to discriminate against unrelated reasons the
    // cache might be cold (e.g., a future refactor that disables
    // caching unconditionally would silently pass `!cached`).
    let resolved = host
        .get_component_meta_with_resolution("/src/Comp.vue")
        .expect("component meta resolves");
    let (_analysis, resolution) = resolved;

    // Suppression must be active — the synthesis loop saw a
    // BudgetExceeded stop and flipped the flag.
    assert!(
        resolution.synthesis_should_suppress,
        "synthesis_should_suppress must be true on a budget-exceeded run; \
         resolution = {resolution:?}",
    );

    // The synthesis diagnostics must carry at least one
    // BudgetExceeded ExpansionDiagnostic — the discriminator that
    // proves the cache-skip is BECAUSE OF budget exhaustion, not an
    // unrelated cold-cache condition.
    let saw_budget_exceeded = resolution.synthesis_diagnostics.iter().any(|envelope| {
        envelope.diagnostics.iter().any(|d| {
            matches!(
                d.reason,
                verter_semantic::analysis::type_expand::ExpansionStopReason::BudgetExceeded
            )
        })
    });
    assert!(
        saw_budget_exceeded,
        "synthesis_diagnostics must contain at least one BudgetExceeded \
         ExpansionDiagnostic; observed envelopes = {:?}",
        resolution.synthesis_diagnostics,
    );

    // The cache must NOT be warmed when suppression is active.
    let cached = crate::component_meta_result_db::ComponentMetaResultDb::has_owner_entry_in_test(
        &host,
        "/src/Comp.vue",
    );
    assert!(
        !cached,
        "budget-exceeded synthesis must NOT warm the result cache",
    );
}

// ---------------------------------------------------------------------------
// Test #23 — REGRESSION
// ---------------------------------------------------------------------------
//
// `cache_suppress=true` must skip memo insertion at the synthesis
// layer. Mirrors the dispatch-level
// `memo_refuses_insertion_on_cache_suppress_true_via_pathological_input`.
//
// Scope note: the `SemanticGraphStore::memo_size_in_test` accessor
// observes host-global memo state that grows from peer dispatches
// outside the synthesis path. The discriminating contract under
// test is "the cache_suppress no-poison gate FIRES during the
// request" — when an in-flight build landed with
// `cache_suppress=true`, `execute_cooperative` must take the else
// branch around `warm_publish_one` and bump
// `memo_publish_suppressed`. The per-request counter snapshotted
// on the audit payload (via
// `RequestContext::memo_publish_suppressed`) gives
// synthesis-attributable visibility independently of host-global
// memo growth from peer dispatches.
//
// The fixture uses the cyclic-heritage shape from Test #21 so the
// synthesis walk surfaces a fatal QueryError on the cycle, which
// flips `cache_suppress=true` on the affected cooperative-admission
// builds. The gate at
// `SemanticGraphStore::execute_cooperative` step 5 takes the
// `else` branch and bumps the per-request
// `memo_publish_suppressed` counter, which we assert is non-zero
// to pin the gate.
#[test]
fn cache_suppress_true_skips_memo_insertion() {
    let result = AuditedRequest::builder()
        .files([
            (
                "/src/types.ts",
                "export interface A extends B { tagA: string }\n\
                 export interface B extends A { tagB: string }\n\
                 export interface Slots { default(props: A): any }\n",
            ),
            (
                "/src/Comp.vue",
                r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
            ),
        ])
        .resolve_component_meta("/src/Comp.vue");
    let (_meta, _resolved, record) = result.expect("audited resolution");
    let payload = match &record.kind_payload {
        verter_audit::RequestKindPayload::ComponentMeta(payload) => payload,
        other => panic!("expected ComponentMeta payload, got {other:?}"),
    };
    // Precondition: the cyclic heritage chain must trigger
    // should_suppress through the synthesis path; without that, the
    // no-poison gate would not fire and the contract under test
    // would be vacuous.
    assert!(
        payload.should_suppress,
        "fixture must trigger should_suppress via cyclic-heritage fatal \
         QueryError; got should_suppress=false (diagnostics={diag_count})",
        diag_count = payload.diagnostics.len(),
    );
    // Discriminating assertion: the no-poison gate at
    // `SemanticGraphStore::execute_cooperative` step 5 must fire at
    // least once during the request. A pre-fix tree where the gate
    // was bypassed (or never reached) would surface as
    // `memo_publish_suppressed == 0` even with should_suppress=true,
    // which a regression catches.
    assert!(
        payload.memo_publish_suppressed > 0,
        "cache_suppress=true synthesis path must trigger the no-poison gate; \
         observed memo_publish_suppressed={observed} (per-request snapshot \
         from RequestContext) — the gate that protects against poisoning the \
         memo with suppressed builds was never hit",
        observed = payload.memo_publish_suppressed,
    );
}

// ---------------------------------------------------------------------------
// Test #24 — REGRESSION
// ---------------------------------------------------------------------------
//
// Audit payload's `ComponentMetaPayload` must carry walker
// diagnostics. `ComponentMetaPayload.diagnostics` is populated via
// the `host_audit_bridge::macro_expansion_to_audit_entries` projector
// against the cold resolver's `synthesis_diagnostics` accumulator. A
// synthesis that swallowed walker diagnostics (e.g. dropping the
// `MacroExpansionDiagnostics` accumulator) would publish an empty
// `payload.diagnostics` and fail the `diagnostics_len > 0`
// discriminator.
#[test]
fn componentmeta_audit_payload_includes_walker_diagnostics() {
    let result = AuditedRequest::builder()
        .files([
            (
                "/src/types.ts",
                "export interface A extends B { tagA: string }\n\
                 export interface B extends A { tagB: string }\n\
                 export interface Slots { default(props: A): any }\n",
            ),
            (
                "/src/Comp.vue",
                r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
            ),
        ])
        .resolve_component_meta("/src/Comp.vue");
    let (_meta, _resolved, record) = result.expect("audited resolution");
    let diagnostics_len = match &record.kind_payload {
        verter_audit::RequestKindPayload::ComponentMeta(payload) => payload.diagnostics.len(),
        other => panic!("expected ComponentMeta payload, got {other:?}"),
    };
    assert!(
        diagnostics_len > 0,
        "ComponentMetaPayload must carry walker diagnostics; observed {diagnostics_len}",
    );
}

// ---------------------------------------------------------------------------
// Test #25 — REGRESSION
// ---------------------------------------------------------------------------
//
// `should_suppress` flag must propagate through the audit payload.
// On cyclic heritage, the synthesis path's
// `is_fatal_query_error` helper sets `should_suppress = true` and
// `ComponentMetaPayload.should_suppress` mirrors the resolver flag. A
// synthesis that lost track of fatal `QueryError` arms (e.g. always
// returning `should_suppress = false`) would fail the assertion
// below and silently warm the result cache with partial data.
#[test]
fn componentmeta_audit_payload_carries_should_suppress_flag() {
    let result = AuditedRequest::builder()
        .files([
            (
                "/src/types.ts",
                "export interface A extends B { tagA: string }\n\
                 export interface B extends A { tagB: string }\n\
                 export interface Slots { default(props: A): any }\n",
            ),
            (
                "/src/Comp.vue",
                r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
            ),
        ])
        .resolve_component_meta("/src/Comp.vue");
    let (_meta, _resolved, record) = result.expect("audited resolution");
    let should_suppress = match &record.kind_payload {
        verter_audit::RequestKindPayload::ComponentMeta(payload) => payload.should_suppress,
        other => panic!("expected ComponentMeta payload, got {other:?}"),
    };
    assert!(
        should_suppress,
        "cyclic heritage must surface should_suppress=true through the audit payload",
    );
}

// ---------------------------------------------------------------------------
// Test #26 — REGRESSION
// ---------------------------------------------------------------------------
//
// Synthesis emits a `synthesize_slot_bindings` span and one
// `synthesize_macro` span per macro. The graph-native synthesis closure
// in `slot_binding_graph::resolve_slot_bindings_graph_native` opens
// both spans, and emits one `info`-level event inside each so
// subscribers (including `tracing_test`'s default `verter_session=trace`
// env-filter) capture the span path on the formatted log line. Per
// spec §17.5 the synthesis layer emits at `info`; per-binding
// (`verter::meta_resolve::slot_binding`) events stay at `trace`.
#[test]
#[tracing_test::traced_test]
fn synthesis_emits_spans_per_macro() {
    let host = build_test_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Slots { default(props: { row: string }): any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );
    let _ = host.get_component_meta("/src/Comp.vue");
    assert!(
        logs_contain("synthesize_slot_bindings"),
        "tracing must emit a synthesize_slot_bindings span",
    );
    assert!(
        logs_contain("synthesize_macro"),
        "tracing must emit a per-macro synthesize_macro span",
    );
}

// ---------------------------------------------------------------------------
// Test #27 — REGRESSION
// ---------------------------------------------------------------------------
//
// The shallow walker's pathological-input cap fires on
// sufficiently-large input and surfaces a
// `ShallowDiagnostic::PathologicalInput` that flows into the
// synthesis's `MacroExpansionDiagnostics` accumulator. The
// `ExpansionStopReason::BudgetExceeded` reason carried by that
// envelope is the public observable.
//
// Hermetic test driver: rather than exhausting the production
// 10_000-node default cap, the test sets the
// `HostConfig::recursion_budget_overrides.walker_pathological_cap`
// override to a small value (50) so a modestly-sized fixture
// (100-arm intersection) exercises the cap-fire path. A naive
// walker that ignored the override (or hard-coded 10_000) would
// not emit the diagnostic on this fixture and the assertion below
// would fail.
//
// We assert via the published `MacroExpansionDiagnostics` envelope
// rather than via `tracing_test::logs_contain`, because the
// `tracing-test` 0.2 capture filter scopes captured events to
// targets prefixed by the test crate's name (`verter_session::*`)
// and the walker emits its warn event with the
// `verter::dispatch::walk` target.
#[test]
fn walker_warn_event_on_cap_fire() {
    use verter_semantic::analysis::type_expand::ExpansionStopReason;

    let mut config = HostConfig::default();
    // Override the production 10_000-node cap with a small value so a
    // hermetic fixture can drive the walker into the cap-fire path.
    config.recursion_budget_overrides.walker_pathological_cap = Some(50);
    let host = Arc::new(VerterHost::new_standalone(config));
    // 100-arm intersection — enough that the walker's visited set
    // crosses the 50-node override.
    let mut decls = String::new();
    for i in 0..100 {
        decls.push_str(&format!(
            "export interface I{i} {{ tag{i}: string }}\n",
            i = i,
        ));
    }
    decls.push_str("export type Slots = ");
    for i in 0..100 {
        if i > 0 {
            decls.push_str(" & ");
        }
        decls.push_str(&format!("{{ default(props: I{i}): any }}", i = i));
    }
    decls.push_str(";\n");
    upsert_ts(&host, "/src/types.ts", &decls);
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
    );
    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");
    // The walker pushes a `ShallowDiagnostic::PathologicalInput` when
    // the cap fires; `shallow_diagnostics_to_macro_expansion` maps it
    // to `ExpansionStopReason::BudgetExceeded` on the published
    // `MacroExpansionDiagnostics` envelope.
    let cap_fire_observed = meta
        .macro_expansion_diagnostics
        .iter()
        .flat_map(|env| env.diagnostics.iter())
        .any(|d| matches!(d.reason, ExpansionStopReason::BudgetExceeded));
    assert!(
        cap_fire_observed,
        "pathological input must surface a BudgetExceeded MacroExpansionDiagnostics envelope; \
         observed envelopes={:?}",
        meta.macro_expansion_diagnostics,
    );
}

// ---------------------------------------------------------------------------
// Test #28 — REGRESSION
// ---------------------------------------------------------------------------
//
// `RequestAuditRecord` envelope must carry the per-request `trace_id`
// matching the tracing span's `trace_id` field. The
// `publish_component_meta` span emits `trace_id` so consumers can join
// audit records to captured logs by string match.
#[test]
#[tracing_test::traced_test]
fn audit_record_trace_id_matches_tracing_span() {
    let result = AuditedRequest::builder()
        .files([
            (
                "/src/types.ts",
                "export interface Slots { default(props: { row: string }): any }",
            ),
            (
                "/src/Comp.vue",
                r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>
"#,
            ),
        ])
        .resolve_component_meta("/src/Comp.vue");
    let (_meta, _resolved, record) = result.expect("audited resolution");
    let trace_id = record.trace_id.as_str();
    assert!(
        !trace_id.is_empty(),
        "RequestAuditRecord.trace_id must be non-empty",
    );
    // Tracing span emitted with trace_id field — assert it appears in
    // the captured logs verbatim.
    assert!(
        logs_contain(trace_id),
        "audit record trace_id={trace_id} must appear in captured tracing logs",
    );
}

// ---------------------------------------------------------------------------
// Test #29 — CHARACTERIZATION (Q10 arg-preserving publication)
// ---------------------------------------------------------------------------
//
// A graph-raised binding row whose VALUE is a generic INSTANTIATION
// (`message: MessageBase<string>`, declared on a named NON-GENERIC param
// type) must publish an ARG-PRESERVING shallow carrier: the authored
// use-site body slot (the `SlotProps.message` member-value position),
// whose deref through the one shared dispatch replays the instantiation
// WITH its type arguments. A bare argument-less
// `Closed(Leaf(Ref("MessageBase")))` destroys BOTH the substitution
// (`string`) and the declaring canonical scope.
//
// Discriminating: shell-materializing the published source must yield
// `Ref { name: "MessageBase", type_arguments: [string] }` — a lossy
// argument-less carrier shell-materializes with EMPTY type_arguments and
// fails. The carrier stays SHALLOW: publication performs no Instantiate
// execution (the shallow probe is the consumer-side demand).
#[test]
fn slot_binding_generic_instantiation_publishes_arg_preserving_use_site_carrier() {
    let host = build_test_host();
    upsert_ts(
        &host,
        "/src/types.ts",
        r#"
        export interface MessageBase<T> { content: T }
        export interface SlotProps { message: MessageBase<string> }
        "#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { SlotProps } from './types'
defineSlots<{ default(props: SlotProps): any }>()
</script>
<template><div /></template>
"#,
    );

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta");
    let row = slot_binding(&meta, "default", "message")
        .expect("default.message binding must publish from the named param surface");

    // Canonical identity is preserved on the published carrier: the
    // authored use-site slot anchors on the DECLARING file + symbol.
    let source = row
        .type_source
        .present()
        .expect("default.message must publish a typed source");
    let verter_type_expr::facts::SemanticTypeSource::Authored(
        verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot),
    ) = source
    else {
        panic!(
            "an instantiation-valued binding must publish the authored use-site \
             DeclBody carrier (arg-preserving, re-resolvable); observed {source:?}",
        );
    };
    assert_eq!(
        slot.anchor.canonical_id.as_ref(),
        "/src/types.ts",
        "the use-site slot must anchor on the declaring canonical",
    );
    assert_eq!(
        slot.anchor.symbol.as_ref(),
        "SlotProps",
        "the use-site slot must anchor on the declaring symbol",
    );

    // Shell-materializing WITHOUT a resolution demand replays the authored
    // instantiation: the base name AND the concrete `string` argument
    // survive to the shallow published shape.
    let shallow = shallow_binding_type(&host, "/src/Comp.vue", row);
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = &shallow
    else {
        panic!(
            "default.message must shell-materialize to the MessageBase reference \
             carrier; observed {shallow:?}",
        );
    };
    assert_eq!(name.as_ref(), "MessageBase");
    assert_eq!(
        type_arguments.len(),
        1,
        "the instantiation's type argument must be PRESERVED on the published \
         carrier (bare `Ref` drops the `string` substitution); observed \
         {type_arguments:?}",
    );
    assert!(
        matches!(
            &type_arguments[0],
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the preserved argument must be the concrete authored `string`; observed \
         {:?}",
        type_arguments[0],
    );
}

// ---------------------------------------------------------------------------
// External-corpus tests — gated behind `external-corpus`.
// ---------------------------------------------------------------------------

#[cfg(feature = "external-corpus")]
mod external_corpus {
    use super::*;

    const NUXT_UI_BENCH: &str = ".integration-tests/repos/nuxt-ui-codex-bench";

    fn corpus_present() -> bool {
        std::path::Path::new(NUXT_UI_BENCH).exists()
    }

    /// Test #6 — first call < 60s; second < 100ms; ChatMessage.
    #[test]
    fn slot_bindings_warm_pass_o1_for_chatmessage_corpus() {
        if !corpus_present() {
            eprintln!("external corpus missing at {NUXT_UI_BENCH}; skipping");
            return;
        }
        let host = build_test_host();
        let chat_message = std::fs::read_to_string(format!(
            "{NUXT_UI_BENCH}/src/runtime/components/ChatMessage.vue"
        ))
        .expect("read ChatMessage.vue");
        upsert_vue(&host, "/ChatMessage.vue", &chat_message);

        let cold_started = Instant::now();
        let _ = host
            .get_component_meta("/ChatMessage.vue")
            .expect("cold meta");
        let cold = cold_started.elapsed();
        assert!(
            cold.as_secs() < 60,
            "cold ChatMessage must finish < 60s; observed {cold:?}"
        );

        let warm_started = Instant::now();
        let _ = host
            .get_component_meta("/ChatMessage.vue")
            .expect("warm meta");
        let warm = warm_started.elapsed();
        assert!(
            warm.as_millis() < 100,
            "warm ChatMessage must hit cache; observed {warm:?}",
        );
    }

    /// Test #7 — ChatMessages and Avatar each cold-resolve under 60s.
    #[test]
    fn chatmessages_and_avatar_cold_pass_under_60s() {
        if !corpus_present() {
            eprintln!("external corpus missing at {NUXT_UI_BENCH}; skipping");
            return;
        }
        let host = build_test_host();
        for relative in [
            "src/runtime/components/ChatMessages.vue",
            "src/runtime/components/Avatar.vue",
        ] {
            let src = std::fs::read_to_string(format!("{NUXT_UI_BENCH}/{relative}"))
                .expect("read corpus file");
            let canonical = format!("/{}", relative.rsplit('/').next().unwrap());
            upsert_vue(&host, &canonical, &src);
            let started = Instant::now();
            let _ = host
                .get_component_meta(&canonical)
                .expect("cold component meta");
            let elapsed = started.elapsed();
            assert!(
                elapsed.as_secs() < 60,
                "cold pass for {canonical} must finish < 60s; observed {elapsed:?}",
            );
        }
    }
}
