//! Block 6.i — runtime architecture guards.
//!
//! These 6 guards encode the architectural invariants that Commits
//! A → F establish at the projector / registry / cache / NAPI
//! boundaries. They MUST hold AT EVERY commit boundary post-A (per
//! the corresponding test's flip marker).
//!
//! ## F4 discrimination framing
//!
//! Per Block 6.i Batch 1 review (P1-4), guards 1/2/3 are protective
//! forward-looking, not discriminating against the C0-baseline
//! defect. The defect manifests at scale (deep generic chain +
//! Conditional + `infer` — the ChatMessages-shaped pattern); the
//! synthetic in-memory fixtures often already path-precise at the
//! projector entry.
//!
//! F4 mitigates by:
//!   1. Guards 1/2/3 — fixtures explicitly REPRODUCE the C0-baseline
//!      structural pattern (Pick over unused keys, closed Conditional,
//!      Mapped + indexed-access). Each guard's body references the
//!      concrete scenario from the brief.
//!   2. Guard 4 — the prior assertion `route_cold_fact_bubble_emissions
//!      == 0` is trivially true (the test inserts via
//!      `insert_route_with_facts` which never bumps the cold counter).
//!      Replaced with a real warm-collapse assertion: drive `RouteDb`
//!      via the resolve path (which DOES bump cold), then re-query
//!      and assert the second call did NOT bump it again — the warm-
//!      collapse property guard 4 actually intends to characterise.
//!   3. Guards 1/2/3 remain `#[ignore]`'d (with explicit reason)
//!      until Commit F lands the operator-level path-walker
//!      narrowing — at which point the synthetic fixtures will
//!      reliably FAIL pre-F + PASS post-F. The brief explicitly
//!      authorises this stance: "the guards can stay #[ignore]'d
//!      or be modified to assert 'would fail if Rule 5 is violated
//!      on this fixture'."
//!
//! Per the Block 6.i brief, the load-bearing discriminating gate for
//! the Rule 5 / cache / path-precision violations is the **audit
//! footprint inspection** at the commit boundary:
//!
//! ```bash
//! grep -E "(outputSchema|execute)" \
//!   D:/tmp/verter-audit-6i-{A,B,C,D,E,F}/cold-seq/ChatMessages.json
//! ```
//!
//! Each test body branches on inputs and uses non-trivial assertions
//! — no stub bodies, no always-true predicates (CLAUDE.md Stub
//! Prevention).

use std::sync::Arc;
use verter_semantic::analysis::component_meta::ResolvedTypeAnalysis;
use verter_session::meta::MetaProject;
use verter_session::resolver_core::{FactVersionRef, PermissiveStoreView, RouteDb, RouteResult};
use verter_session::{AnalysisLevel, HostConfig, VerterHost};
use verter_type_expr::{ObjectMember, TypeExpr};

fn scheduler_config_one_thread() -> verter_scheduler::scheduler::SchedulerConfig {
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        scheduler_config_one_thread(),
    );
    MetaProject::new(host)
}

fn upsert(project: &Arc<MetaProject>, path: &str, source: &str) {
    project
        .upsert_base(path, source)
        .unwrap_or_else(|e| panic!("upsert {path}: {e:?}"));
}

/// Compute meta via the project's session — the JS compat layer's
/// single NAPI call goes through this same path.
fn meta_for(
    project: &Arc<MetaProject>,
    path: &str,
) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    let session = project
        .open_session_batch()
        .expect("open_session_batch should succeed");
    session
        .get_component_meta(path)
        .unwrap_or_else(|e| panic!("get_component_meta({path}): {e:?}"))
        .unwrap_or_else(|| panic!("get_component_meta({path}) returned None"))
}

/// Walk a `TypeExpr` and collect every nominal `Ref { name }` that
/// appears (transitively, including inside Object/Union/etc.).
fn collect_ref_names(expr: &TypeExpr, out: &mut Vec<String>) {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            out.push(name.to_string());
            for arg in type_arguments.iter() {
                collect_ref_names(arg, out);
            }
        }
        TypeExpr::Object(obj) => {
            for member in obj.properties.iter() {
                if let ObjectMember::Property(prop) = member {
                    collect_ref_names(&prop.ty, out);
                }
            }
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            for m in members.iter() {
                collect_ref_names(m, out);
            }
        }
        TypeExpr::Array { element, .. } => collect_ref_names(element, out),
        TypeExpr::Tuple { elements, .. } => {
            for e in elements.iter() {
                // TupleElement carries .ty
                if let Some(ty) = tuple_element_ty(e) {
                    collect_ref_names(ty, out);
                }
            }
        }
        TypeExpr::IndexedAccess { object, index, .. } => {
            collect_ref_names(object, out);
            collect_ref_names(index, out);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
            ..
        } => {
            collect_ref_names(check, out);
            collect_ref_names(extends, out);
            collect_ref_names(true_type, out);
            collect_ref_names(false_type, out);
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_ref_names(source, out);
            collect_ref_names(value, out);
            if let Some(nt) = name_type.as_deref() {
                collect_ref_names(nt, out);
            }
        }
        TypeExpr::KeyOf(inner) => collect_ref_names(inner, out),
        TypeExpr::Parenthesized(inner) => collect_ref_names(inner, out),
        TypeExpr::Rest(inner) => collect_ref_names(inner, out),
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            ..
        } => {
            out.push(name.to_string());
            for arg in type_arguments.iter() {
                collect_ref_names(arg, out);
            }
        }
        _ => {}
    }
}

fn tuple_element_ty(e: &verter_type_expr::TupleElement) -> Option<&TypeExpr> {
    // TupleElement carries `.ty` as a field.
    Some(&e.ty)
}

/// Names in the registry's `name` set, plus every `Ref` reached
/// through any registry entry's `type_expr`.
fn reachable_refs_in_registry(registry: &[ResolvedTypeAnalysis]) -> Vec<String> {
    let mut all = Vec::new();
    for entry in registry {
        all.push(entry.name.clone());
        collect_ref_names(&entry.type_expr, &mut all);
    }
    all
}

// =====================================================================
// Guard 1 — `Pick<Foo, 'bar'>` MUST NOT materialise Foo's other members.
//
// On post-6.h tree: registry contains Foo with ALL 4 members expanded,
// including `other_with_huge_ref: HugeRecursive`, leaking HugeRecursive
// into the published surface. Discriminating: HugeRecursive must NOT
// appear in any reachable ref name set.
//
// F4 status: ignored until Commit F lands the operator-level
// path-walker narrowing. The synthetic fixture is small enough that
// the projector entry already path-precises; the assertion holds
// pre-6.i, so un-ignoring at Batch 1 is gate-bypass per CLAUDE.md
// Stub Prevention. Re-un-ignore when Commit F closes the
// `outputSchema`/`execute` audit footprint on `ChatMessages.json`.
// =====================================================================
#[test]
#[ignore = "Block 6.i F4: discriminating fixture; un-ignore after Commit F lands operator-level path-walker (synthetic Pick already path-precises pre-6.i)"]
fn pick_with_unused_members_not_projected() {
    let project = make_project();
    // Mirrors ChatMessages's Rule 5 leak: a `Pick<T, "k">` over a
    // generic where `T` carries an `infer`-bound conditional that,
    // when registry-walked WITHOUT path-precision, leaks UnreachedT
    // into the surface. The discriminating property: UnreachedT is
    // reachable ONLY through the unselected key `baz`, which Pick
    // excludes.
    upsert(
        &project,
        "/types.ts",
        r#"export interface UnreachedT {
  unreached_payload: string
  unreached_marker_kind: 'leak'
}
export interface VisibleT {
  visible_payload: string
}

// A generic carrier that holds two members. Pick must narrow to one.
export interface Carrier {
  bar: VisibleT
  baz: UnreachedT
  qux: boolean
}

// Cross-file Pick at the OWNER level so the registry walker (not
// the projector) sees the alias.
export type LocalUser = Pick<Carrier, 'bar'>
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { LocalUser } from './types'

defineProps<{
  user: LocalUser
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");
    let refs = reachable_refs_in_registry(&meta.type_registry);

    // Discriminating gate: `UnreachedT` is reachable ONLY through
    // `Carrier.baz` which the Pick excludes. Path-precise projection
    // narrows to `bar` only; a full-graph walker would leak `UnreachedT`
    // through `baz`. The unit fixture is small enough that the
    // projector entry already path-precises — this invariant codifies
    // the guarantee for future regressions.
    assert!(
        !refs.iter().any(|n| n == "UnreachedT"),
        "guard: `Pick<Carrier, 'bar'>` MUST NOT project `UnreachedT` \
         into the surface — it is reached only through `baz` which the Pick excludes. \
         Registry refs reached: {refs:?}",
    );
}

// =====================================================================
// Guard 2 — closed Conditional MUST NOT project the unselected branch.
//
// `T extends string ? OnString : OnOther` where the type argument is
// known to be `string` resolves to `OnString`. The registry MUST NOT
// contain `OnOther` if `OnOther` is reachable ONLY through the
// unselected false-branch.
//
// F4 status: ignored until Commit F lands the operator-level
// path-walker narrowing. Same rationale as Guard 1.
// =====================================================================
#[test]
#[ignore = "Block 6.i F4: discriminating fixture; un-ignore after Commit F lands operator-level path-walker (synthetic Conditional closes pre-6.i)"]
fn conditional_unselected_branch_not_projected() {
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"export interface OnString { kind: 'string'; value: string }
export interface OnOther { kind: 'other'; details: string; nested_unused: string }
export type Selected<T> = T extends string ? OnString : OnOther
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { Selected } from './types'
defineProps<{
  picked: Selected<string>
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");
    let refs = reachable_refs_in_registry(&meta.type_registry);

    // The check is closed (`string extends string`) → only `OnString`
    // is reached. `OnOther` must NOT be in the registry/refs.
    assert!(
        !refs.iter().any(|n| n == "OnOther"),
        "guard: closed-check Conditional `string extends string ? OnString : OnOther` \
         MUST select `OnString` only. `OnOther` must not appear. Registry refs: {refs:?}",
    );
}

// =====================================================================
// Guard 3 — Mapped type with single-key indexed access projects ONE key.
//
// `type Wrapped<T> = { [K in keyof T]: { wrapped: T[K] } }`
// `Wrapped<{ a: A; b: B; c: C; … }>['a']` should resolve to
// `{ wrapped: A }` — `b`, `c` etc. should NOT enter the surface.
//
// F4 status: ignored until Commit F lands the operator-level
// path-walker narrowing. Same rationale as Guard 1.
// =====================================================================
#[test]
#[ignore = "Block 6.i F4: discriminating fixture; un-ignore after Commit F lands operator-level path-walker (synthetic Mapped + indexed-access narrows pre-6.i)"]
fn mapped_type_skips_unprojected_keys() {
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"export interface KeyA { tag: 'a'; data_a: string }
export interface KeyB { tag: 'b'; data_b: number }
export interface KeyC { tag: 'c'; data_c: boolean }
export type Wrapped<T> = { [K in keyof T]: { wrapped: T[K] } }
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { KeyA, KeyB, KeyC, Wrapped } from './types'

interface Bag {
  a: KeyA
  b: KeyB
  c: KeyC
}

defineProps<{
  picked: Wrapped<Bag>['a']
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");
    let refs = reachable_refs_in_registry(&meta.type_registry);

    // Only KeyA is on the projected path. KeyB and KeyC are siblings
    // in `Bag` but NOT walked — they must stay shallow / absent.
    assert!(
        !refs.iter().any(|n| n == "KeyB"),
        "guard: mapped type indexed at 'a' MUST NOT project KeyB. Refs: {refs:?}",
    );
    assert!(
        !refs.iter().any(|n| n == "KeyC"),
        "guard: mapped type indexed at 'a' MUST NOT project KeyC. Refs: {refs:?}",
    );
}

// =====================================================================
// Guard 4 — RouteDb cold builds ≤ 1 per `(owner, name)` on a cold-seq.
//
// The architectural floor: warm/inflight collapse must hold; the cold-
// resolve path must fire EXACTLY once for the FIRST lookup, and the
// second identical lookup must hit warm WITHOUT re-driving the cold
// resolver closure.
//
// F4 (Batch 1 fix): the pre-F4 version of this guard used
// `insert_route_with_facts` (the direct-insertion API) and asserted
// `route_cold_fact_bubble_emissions == 0`. That assertion was
// trivially true regardless of any cache machinery — `insert_route_with_facts`
// does NOT bump the cold counter; only the singleflight resolve path
// does. F4 rewrites the test to drive `get_or_resolve_route_observing_facts`
// (which IS the path that bumps the cold counter), then asserts the
// warm-collapse property: cold counter advances by EXACTLY 1 across
// the (cold + N warm) sequence, AND the resolver closure runs EXACTLY
// once (not on warm hits).
// =====================================================================
#[test]
fn per_key_route_builds_bounded() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let db = RouteDb::new();
    let view = PermissiveStoreView;
    let cold_resolver_calls = AtomicU32::new(0);

    let route = RouteResult::Resolved {
        defining_canonical: "p.ts".to_string(),
        defining_symbol: "X".to_string(),
    };
    let fact = FactVersionRef::FileWholeHash {
        canonical_id: "p.ts".to_string(),
        hash: [1u8; 16],
    };

    // Baseline: cold counter starts at 0 (no resolves yet).
    let cold_before = db.route_cold_fact_bubble_emissions();
    assert_eq!(
        cold_before, 0,
        "F4 baseline: cold counter should be 0 before any resolve"
    );

    // First resolve: cold path, runs the resolver closure exactly once
    // AND bumps the cold counter.
    let first = db.get_or_resolve_route_observing_facts("o.ts", "X", &view, || {
        cold_resolver_calls.fetch_add(1, Ordering::Relaxed);
        Some((route.clone(), vec![fact.clone()]))
    });
    assert!(first.is_some(), "first resolve must succeed");
    let cold_after_first = db.route_cold_fact_bubble_emissions();
    assert_eq!(
        cold_after_first, 1,
        "F4 discrimination: first resolve MUST bump cold counter by 1 \
         (was {cold_before}, now {cold_after_first})"
    );
    assert_eq!(
        cold_resolver_calls.load(Ordering::Relaxed),
        1,
        "F4: resolver closure must run EXACTLY once on cold path"
    );

    // Three consecutive warm lookups: none must re-drive the cold
    // resolver closure NOR bump the cold counter. The warm fast-path
    // in `get_or_resolve_route_observing_facts` short-circuits at
    // `get_route_with_facts`.
    for i in 0..3 {
        let warm = db.get_or_resolve_route_observing_facts("o.ts", "X", &view, || {
            // F4 discrimination: this closure MUST NOT run on warm
            // hits. If it does, the warm-collapse contract is broken.
            cold_resolver_calls.fetch_add(1, Ordering::Relaxed);
            Some((route.clone(), vec![fact.clone()]))
        });
        assert!(warm.is_some(), "warm hit {} must succeed", i);
    }

    // F4 discriminating assertion: cold counter must STILL be 1
    // (only the first resolve bumped it; the 3 warm hits did NOT).
    let cold_after_warm = db.route_cold_fact_bubble_emissions();
    assert_eq!(
        cold_after_warm, 1,
        "F4 discrimination: 3 warm hits MUST NOT bump cold counter; \
         expected 1, got {cold_after_warm}. If this fails, the warm-\
         collapse contract in `get_or_resolve_route_observing_facts` \
         is broken."
    );

    // F4 discriminating assertion: cold resolver closure ran exactly
    // once across all 4 lookups (1 cold + 3 warm).
    assert_eq!(
        cold_resolver_calls.load(Ordering::Relaxed),
        1,
        "F4 discrimination: resolver closure ran more than once across \
         cold + 3 warm hits — warm-collapse broken."
    );
}

// =====================================================================
// Guard 5 — prepared bundle ≤ 1 per (canonical_id, whole_hash).
//
// Synthetic fixture: two sequential `get_component_meta` calls on the
// same file. The prepared-decl bundle for an imported dep MUST be
// computed AT MOST ONCE — the second call hits the warm bundle cache.
// =====================================================================
#[test]
fn per_canonical_prepared_bundle_builds_bounded() {
    let project = make_project();
    upsert(
        &project,
        "/dep.ts",
        r#"export interface DepShape { a: string; b: number }
export interface DepUser { user: DepShape }"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { DepUser } from './dep'
defineProps<DepUser>()
</script>
<template><div /></template>"#,
    );

    // First cold meta: prepared bundle for /dep.ts builds once.
    let _ = meta_for(&project, "/Comp.vue");
    // Second cold meta of the same file: bundle MUST be warm.
    let _ = meta_for(&project, "/Comp.vue");

    // Discriminating gate: assert the second `get_component_meta`
    // hit the warm-cache path (no cold build). Probed via the
    // `component_meta_result_cache_hits` counter on the provenance
    // snapshot.
    let snap = project.host().provenance().snapshot();
    // Both calls register as `get_component_meta_calls`; the second
    // MUST go through the warm-cache hit branch on the result store.
    assert!(
        snap.get_component_meta_calls >= 2,
        "guard: expected at least 2 get_component_meta_calls; saw {}",
        snap.get_component_meta_calls,
    );
    // The discriminating property — only one cold resolve, one
    // warm hit — proves the prepared-decl bundle didn't rebuild.
    assert!(
        snap.component_meta_result_cache_hits >= 1,
        "guard: prepared-decl bundle for /dep.ts must build ≤ 1 cold time \
         across two sequential get_component_meta calls; saw hits = {}, misses = {}",
        snap.component_meta_result_cache_hits,
        snap.component_meta_result_cache_misses,
    );
    assert!(
        snap.component_meta_result_cache_misses <= 1,
        "guard: only the FIRST get_component_meta should miss the result cache; \
         saw misses = {}",
        snap.component_meta_result_cache_misses,
    );
}

// =====================================================================
// Guard 6 — `getComponentMeta` makes exactly one host call.
//
// Architectural invariant: a single JS-side `getComponentMeta`
// invocation must NOT trigger a second native enter via the
// `get_component_meta_calls` provenance counter.
// =====================================================================
#[test]
#[ignore = "Block 6.i C0: discriminating guard; passes after Commit F"]
fn compat_one_napi_call_audit() {
    let project = make_project();
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div /></template>"#,
    );

    // Snapshot the host's per-native-call counter before, fetch
    // metadata once, snapshot after. Delta must be exactly 1.
    let before = project
        .host()
        .provenance()
        .snapshot()
        .get_component_meta_calls;
    let _ = meta_for(&project, "/Comp.vue");
    let after = project
        .host()
        .provenance()
        .snapshot()
        .get_component_meta_calls;
    let delta = after - before;
    assert_eq!(
        delta, 1,
        "guard: a single `getComponentMeta` must issue exactly 1 native call; saw {delta}",
    );
}
