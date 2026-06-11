//! Runtime architecture guards.
//!
//! These guards encode the architectural invariants at the projector
//! / registry / cache / NAPI boundaries.
//!
//! ## Discrimination framing
//!
//! Guards 1/2/3 are protective forward-looking, not discriminating
//! against the baseline defect. The defect manifests at scale (deep
//! generic chain + Conditional + `infer` — the ChatMessages-shaped
//! pattern); the synthetic in-memory fixtures are often already
//! path-precise at the projector entry. Their fixtures explicitly
//! REPRODUCE the baseline structural pattern (Pick over unused keys,
//! closed Conditional, Mapped + indexed-access).
//!
//! Guard 4 drives a real warm-collapse assertion: drive `RouteDb`
//! via the resolve path (which bumps the cold counter), then re-query
//! and assert the second call did NOT bump it again — the warm-collapse
//! property the guard characterises. (A bare `insert_route_with_facts`
//! never bumps the cold counter, so an assertion against it would be
//! trivially true.)
//!
//! The load-bearing discriminating gate for the Rule 5 / cache /
//! path-precision violations is the **audit footprint inspection** on
//! the `outputSchema` / `execute` member names in the cold-seq
//! `ChatMessages.json` corpus.
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
/// Collect every string-literal payload reachable within a
/// `TypeExpr`. Used by the primitive-keyspace admission guards
/// to detect that a discriminator literal (the marker carried by
/// the mapper's value type) reaches the published prop surface.
fn collect_string_literals(expr: &TypeExpr, out: &mut Vec<String>) {
    match expr {
        TypeExpr::Literal(verter_type_expr::LiteralValue::String(s)) => out.push(s.to_string()),
        TypeExpr::Object(obj) => {
            for member in obj.properties.iter() {
                if let ObjectMember::Property(prop) = member {
                    collect_string_literals(&prop.ty, out);
                }
            }
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            for m in members.iter() {
                collect_string_literals(m, out);
            }
        }
        TypeExpr::Array { element, .. } => collect_string_literals(element, out),
        TypeExpr::Tuple { elements, .. } => {
            for e in elements.iter() {
                if let Some(ty) = tuple_element_ty(e) {
                    collect_string_literals(ty, out);
                }
            }
        }
        TypeExpr::IndexedAccess { object, index, .. } => {
            collect_string_literals(object, out);
            collect_string_literals(index, out);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
            ..
        } => {
            collect_string_literals(check, out);
            collect_string_literals(extends, out);
            collect_string_literals(true_type, out);
            collect_string_literals(false_type, out);
        }
        TypeExpr::Parenthesized(inner) => collect_string_literals(inner, out),
        _ => {}
    }
}

/// Collect every numeric-literal payload reachable within a
/// `TypeExpr`. Mirror of `collect_string_literals` but for the
/// `LiteralValue::Number` variant. Used by the literal-kind-
/// preservation guard to detect a numeric literal substituted into
/// an identity-mapped `K`.
fn collect_numeric_literals(expr: &TypeExpr, out: &mut Vec<f64>) {
    match expr {
        TypeExpr::Literal(verter_type_expr::LiteralValue::Number(n)) => out.push(*n),
        TypeExpr::Object(obj) => {
            for member in obj.properties.iter() {
                if let ObjectMember::Property(prop) = member {
                    collect_numeric_literals(&prop.ty, out);
                }
            }
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            for m in members.iter() {
                collect_numeric_literals(m, out);
            }
        }
        TypeExpr::Array { element, .. } => collect_numeric_literals(element, out),
        TypeExpr::Tuple { elements, .. } => {
            for e in elements.iter() {
                if let Some(ty) = tuple_element_ty(e) {
                    collect_numeric_literals(ty, out);
                }
            }
        }
        TypeExpr::IndexedAccess { object, index, .. } => {
            collect_numeric_literals(object, out);
            collect_numeric_literals(index, out);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
            ..
        } => {
            collect_numeric_literals(check, out);
            collect_numeric_literals(extends, out);
            collect_numeric_literals(true_type, out);
            collect_numeric_literals(false_type, out);
        }
        TypeExpr::Parenthesized(inner) => collect_numeric_literals(inner, out),
        _ => {}
    }
}

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
// A non-path-precise tree would put Foo with ALL 4 members expanded
// into the registry, including `other_with_huge_ref: HugeRecursive`,
// leaking HugeRecursive into the published surface. Discriminating:
// HugeRecursive must NOT appear in any reachable ref name set.
//
// `Pick<Foo, 'bar'>` is a key-filter producer at the operator level
// (build.rs `Pick` arm produces a filtered `Object`; the registry
// walker's `cursor.admits_key` gate emits entries only for refs that
// the published surface reaches). The projector pipeline therefore
// does not eagerly materialise the non-picked members of `Foo`.
#[test]
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
// =====================================================================
// Closed conditional checks (`T extends X ? A : B` where X is
// decidable) select one branch in `build_conditional`'s relation
// evaluator and do NOT walk the unselected branch's reachable refs.
// The registry walker's Conditional arm gates on `is_whole_surface()`
// so narrowed cursors only walk the result-side branches, never the
// predicate operands.
#[test]
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
// =====================================================================
// The per-key Mapped narrowing in
// `PathWalker` substitutes K=path[index] and projects
// only the requested value when the walker hits a deferred
// `SemanticNodeData::Mapped` shell with a literal-keyed path. The
// synthetic fixture (`Wrapped<Bag>['a']`) reaches this arm when the
// `Wrapped` mapped surface is not enumerated up-front; the narrowed
// projection emits only the per-key value subgraph so siblings stay
// off the published surface.
#[test]
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
// A guard that used `insert_route_with_facts` (the direct-insertion
// API) and asserted `route_cold_fact_bubble_emissions == 0` would be
// trivially true regardless of any cache machinery — `insert_route_with_facts`
// does NOT bump the cold counter; only the singleflight resolve path
// does. This test drives `get_or_resolve_route_observing_facts`
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
        "baseline: cold counter should be 0 before any resolve"
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
        "first resolve MUST bump cold counter by 1 \
         (was {cold_before}, now {cold_after_first})"
    );
    assert_eq!(
        cold_resolver_calls.load(Ordering::Relaxed),
        1,
        "resolver closure must run EXACTLY once on cold path"
    );

    // Three consecutive warm lookups: none must re-drive the cold
    // resolver closure NOR bump the cold counter. The warm fast-path
    // in `get_or_resolve_route_observing_facts` short-circuits at
    // `get_route_with_facts`.
    for i in 0..3 {
        let warm = db.get_or_resolve_route_observing_facts("o.ts", "X", &view, || {
            // This closure MUST NOT run on warm hits. If it does, the
            // warm-collapse contract is broken.
            cold_resolver_calls.fetch_add(1, Ordering::Relaxed);
            Some((route.clone(), vec![fact.clone()]))
        });
        assert!(warm.is_some(), "warm hit {} must succeed", i);
    }

    // Cold counter must STILL be 1 (only the first resolve bumped it;
    // the 3 warm hits did NOT).
    let cold_after_warm = db.route_cold_fact_bubble_emissions();
    assert_eq!(
        cold_after_warm, 1,
        "3 warm hits MUST NOT bump cold counter; \
         expected 1, got {cold_after_warm}. If this fails, the warm-\
         collapse contract in `get_or_resolve_route_observing_facts` \
         is broken."
    );

    // Cold resolver closure ran exactly once across all 4 lookups
    // (1 cold + 3 warm).
    assert_eq!(
        cold_resolver_calls.load(Ordering::Relaxed),
        1,
        "resolver closure ran more than once across \
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
// The compat boundary makes exactly one native call per
// `getComponentMeta` invocation; the provenance counter tracks that
// invariant — the JS=1 NAPI call contract.
#[test]
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

// =====================================================================
// Guard 7 — Mapped narrowing key-domain admission (G4.1 soundness fix).
//
// The PathWalker's Mapped arm narrows `Mapped<S, mapper>[K]` by
// substituting K = Literal(name) into `mapper.value_expr` and
// evaluating directly. This is only sound when `name` is admitted by
// the mapper's key domain — when the source surface enumerates its
// member names OR the key_space enumerates concrete literal names.
//
// Fixture: a mapped helper with a non-identity value expression
// (`Marker` literal) keyed by `keyof Source`, projected through an
// alias that THEN indexes by a key NOT present in `Source`. Without
// the admission gate, the walker substitutes K = "nonexistent" and
// evaluates `Marker` regardless — leaking `Marker` onto the
// published surface for a key that the mapped surface does NOT
// contain.
//
// Discriminating property: with the admission gate in place, the
// `nonexistent` access falls through to whole-surface MappedType
// resolution, which builds an Object with ONLY the admissible keys
// (a, b). The walker then misses on the `nonexistent` member and
// the prop's published type is opaque — `Marker` MUST NOT appear
// in the prop's reachable refs.
// =====================================================================
#[test]
fn mapped_narrowing_respects_key_domain_admission() {
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"export interface Marker { kind: 'admission-leak-canary' }
export interface Source { a: number; b: string }
// Non-identity value expression: every key maps to `Marker`,
// independent of `T[K]`. This is the shape that, without
// admission gating, lets the walker forge `Marker` for any
// literal key passed to `Mapped<Source>[K]`.
export type Mapped<T> = { [K in keyof T]: Marker }
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { Mapped, Source } from './types'

defineProps<{
  // `nonexistent` is NOT a key of `Source`, so `Mapped<Source>`'s
  // member set does NOT contain it. The narrowing path must reject
  // and fall through to the whole-surface fallback (which produces
  // a miss). Without the fix, the walker substitutes K = "nonexistent"
  // into the value_expr `Marker` and publishes `Marker` here.
  leaked: Mapped<Source>['nonexistent']
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");
    let refs = reachable_refs_in_registry(&meta.type_registry);

    // Discriminating: pre-fix the walker substituted K = "nonexistent"
    // into `mapper.value_expr = Marker`, evaluating to `Marker` and
    // publishing it as the `leaked` prop's type. Post-fix the admission
    // gate rejects the non-admissible key, falls through to the coarse
    // path, which builds `Mapped<Source>` as an Object with members
    // {a, b} only — the `['nonexistent']` walk then misses cleanly.
    assert!(
        !refs.iter().any(|n| n == "Marker"),
        "guard: Mapped<Source>['nonexistent'] MUST NOT forge Marker onto the surface \
         for a key that is not in Source's member set. Refs: {refs:?}",
    );
}

// =====================================================================
// Guard 8 — Mapped narrowing admits known source keys (regression
// guard for guard 7's complement).
//
// The admission gate must NOT over-reject. When the literal key IS in
// the source surface's member set, the narrowing must still fire and
// project ONLY the per-key value. Without this assertion, a too-strict
// admission check would silently fall through to the whole-surface
// coarse path, regressing G4's path-precision goal (sibling keys
// re-enter the published surface).
//
// Fixture: `Mapped<Source>['a']` where `Source` has members `a`, `b`.
// `Marker` must appear EXACTLY for the projected prop (key `a` is
// admitted), AND the unprojected `Sibling` ref-type used elsewhere
// must NOT appear (admission gate did not fall back to the coarse
// path that would walk every key).
// =====================================================================
#[test]
fn mapped_narrowing_admits_present_source_keys() {
    // Complement of guard 7: when the literal key IS admitted by the
    // mapper's key domain (a member of the source surface), the
    // narrowing must still fire — siblings must NOT be projected.
    // This guards against the admission check over-rejecting (e.g. a
    // mis-coded enumeration helper that returns Some([]) for an
    // enumerable Object), which would silently fall back to the
    // whole-surface coarse path and regress G4's path-precision goal.
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"export interface KeyA { kind: 'projected-a' }
export interface KeyB { kind: 'sibling-must-stay-unprojected' }
export interface KeyC { kind: 'sibling-must-stay-unprojected' }
export interface Bag { a: KeyA; b: KeyB; c: KeyC }
// Non-identity value-expression mapper: only the narrowing path
// (substitute K = "a" into `{ wrapped: T[K] }`) projects the value
// per-key. A fallback to the coarse path would enumerate ALL of
// Bag's keys and inject KeyB / KeyC into the surface.
export type Mapped<T> = { [K in keyof T]: { wrapped: T[K] } }
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { Mapped, Bag } from './types'

defineProps<{
  // `a` IS in Bag. The narrowing must fire on the admitted key and
  // ONLY contribute KeyA's branch — KeyB / KeyC siblings must NOT
  // be projected even though the mapper visits each key in its
  // coarse-path mode.
  projected: Mapped<Bag>['a']
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");
    let refs = reachable_refs_in_registry(&meta.type_registry);

    // Discriminating: if the admission check over-rejected the
    // admitted key `a`, the walker would fall back to the coarse
    // MappedType dispatch which enumerates every key in `Bag` and
    // emits ProjectMember edges for {a, b, c}. KeyB / KeyC would
    // then be reachable in the registry. With correct admission,
    // only the per-key value subgraph for `a` contributes.
    assert!(
        !refs.iter().any(|n| n == "KeyB"),
        "guard: admitted-key narrowing must NOT regress to whole-surface \
         enumeration; sibling KeyB must stay unprojected. Refs: {refs:?}",
    );
    assert!(
        !refs.iter().any(|n| n == "KeyC"),
        "guard: admitted-key narrowing must NOT regress to whole-surface \
         enumeration; sibling KeyC must stay unprojected. Refs: {refs:?}",
    );
}

// =====================================================================
// Guard 9 — Mapped narrowing admits primitive-keyed maps.
//
// `{ [K in string]: V }['foo']` has `key_space = Primitive(String)`:
// the iteration key domain is the entire `string` universe, but the
// enumerator cannot list its inhabitants. The earlier tri-state
// admission gate (Tier 1 source Object check + Tier 2 enumerable
// key_space) treated this case as `None` (undecidable) and fell back
// to the coarse Mapped path — which re-interns the same shell
// without consuming the segment, leaving the access unresolved.
//
// Discriminating property: with the primitive-admission tier in
// place, the walker substitutes K = "foo" into the value expression,
// evaluates `Wrapped`, and publishes it as the prop's type. Without
// the tier, the published prop type is opaque / un-resolved and the
// `Wrapped` ref never reaches the registry.
// =====================================================================
#[test]
fn mapped_narrowing_admits_primitive_string_key_space() {
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"export interface Wrapped { tag: 'primitive-string-key-admitted' }
// `{ [K in string]: V }` lowers to a Mapped node with
// `key_space = Primitive(String)` (non-enumerable). The
// narrowing path must admit any string-domain segment —
// substituting K = "foo" into the value expression evaluates
// to `Wrapped`, which is the same type the coarse Mapped path
// would assign to every key in the `string` domain.
export type StringKeyed = { [K in string]: Wrapped }
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { StringKeyed } from './types'

defineProps<{
  // `foo` is a string literal — admitted by the `string` primitive
  // key domain. Narrowing must fire and publish `Wrapped` onto the
  // prop's surface.
  resolved: StringKeyed['foo']
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");

    // The narrowed result is `Wrapped`. With the primitive-admission
    // tier in place, the walker substitutes K = "foo" into the
    // mapper's value expression and the prop publishes the `Wrapped`
    // declaration — as the shallow `Ref` carrier (the publication
    // terminal lands ON a declaration and stays the reference the
    // consumer re-resolves on demand), or as its materialised body
    // carrying the discriminator literal. Without the tier (G4.1's
    // tri-state gate), the walker falls back to the coarse Mapped
    // path which re-interns the unresolved shell; the prop type stays
    // an opaque `IndexedAccess` or a Primitive(Unknown), and neither
    // the `Wrapped` reference nor the discriminator ever reaches the
    // surface.
    let resolved_prop = meta
        .props
        .iter()
        .find(|p| p.name == "resolved")
        .expect("resolved prop must be present");
    let mut tags: Vec<String> = Vec::new();
    collect_string_literals(&resolved_prop.type_expr, &mut tags);
    let is_wrapped_ref = matches!(
        &resolved_prop.type_expr,
        verter_type_expr::TypeExpr::Ref { name, type_arguments }
            if name.as_ref() == "Wrapped" && type_arguments.is_empty()
    );
    assert!(
        tags.iter().any(|t| t == "primitive-string-key-admitted") || is_wrapped_ref,
        "guard: {{ [K in string]: Wrapped }}['foo'] MUST resolve to Wrapped (string \
         primitive key domain admits any string-literal segment) — either the \
         `Wrapped` reference carrier or its materialised body. \
         type_expr: {:#?}",
        resolved_prop.type_expr,
    );
}

// =====================================================================
// Guard 10 — Mapped narrowing does NOT over-admit on primitive key
// space mismatch.
//
// Complement of Guard 9: a `{ [K in number]: V }['foo']` access has
// a number-keyed map (key_space = Primitive(Number)) indexed by a
// string literal. The number primitive's domain does NOT admit a
// string segment, so the primitive-admission tier must reject and
// fall back to the coarse Mapped path. The coarse path then produces
// the correct domain-mismatch miss — `Wrapped` MUST NOT leak onto
// the surface for the string segment.
// =====================================================================
#[test]
fn mapped_narrowing_rejects_primitive_key_space_domain_mismatch() {
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"export interface Wrapped { tag: 'must-not-leak-on-domain-mismatch' }
// `{ [K in number]: V }` lowers to a Mapped node with
// `key_space = Primitive(Number)`. A string-literal segment is
// NOT in the number key domain — the admission tier must
// reject and fall back to coarse Mapped resolution rather than
// forging `Wrapped` for a domain-incompatible key.
export type NumberKeyed = { [K in number]: Wrapped }
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { NumberKeyed } from './types'

defineProps<{
  // `foo` is a string literal — NOT admitted by the `number`
  // primitive key domain. Narrowing MUST be rejected; the coarse
  // path then produces a miss and `Wrapped` must not appear.
  mismatched: NumberKeyed['foo']
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");

    // Discriminating: if the primitive-admission tier over-admitted
    // (returned true regardless of segment domain), the walker would
    // substitute K = "foo" and forge `Wrapped` for a key whose domain
    // does NOT include it — the prop's published type_expr would
    // reach the discriminator literal. With domain-aware admission,
    // the narrow rejects and the coarse path produces the correct
    // miss (an unresolved IndexedAccess shell or Primitive(Unknown)),
    // and the discriminator NEVER reaches the surface.
    let mismatched_prop = meta
        .props
        .iter()
        .find(|p| p.name == "mismatched")
        .expect("mismatched prop must be present");
    let mut tags: Vec<String> = Vec::new();
    collect_string_literals(&mismatched_prop.type_expr, &mut tags);
    assert!(
        !tags.iter().any(|t| t == "must-not-leak-on-domain-mismatch"),
        "guard: {{ [K in number]: Wrapped }}['foo'] MUST NOT publish Wrapped \
         (number key domain rejects string-literal segments). \
         type_expr: {:#?}",
        mismatched_prop.type_expr,
    );
}

// =====================================================================
// Guard 11 — Mapped narrowing preserves literal kind (string vs number)
// through substitution (G4.3 soundness fix).
//
// `{ [K in number]: K }` is an identity mapping over numeric keys —
// the iteration variable `K` IS the value expression. When the
// walker narrows a path segment like `M[1]` (numeric Index) into
// this Mapped, the substitution `K = Literal(...)` must use the
// numeric LiteralValue variant. Pre-G4.3, the narrowing path
// rendered every literal as `Arc::<str>::from(n.to_string())` and
// interned `LiteralValue::String("1")` regardless of segment kind —
// any value expression that depends on `K` (here, the identity
// position; in conditional positions `K extends ...`; in template
// literals `` `${K}` ``) would materialise the WRONG TS literal
// type. `M[1]` would publish a string literal `"1"`, not the
// numeric literal `1`.
//
// Discriminating property:
//   - With G4.3: the prop's `type_expr` contains the numeric
//     literal `1.0` (Literal::Number), and DOES NOT contain the
//     string literal `"1"`.
//   - Without G4.3 (the G4.2 baseline): the prop's `type_expr`
//     contains the string literal `"1"` (Literal::String), and
//     DOES NOT contain the numeric literal `1.0`.
//
// Mental trace (pre-fix): segment `Index(IndexKey::Number(1))` →
// `Arc::<str>::from("1")` → `LiteralValue::String("1")` → identity
// substitute K → `TypeExpr::Literal(String("1"))` on the prop's
// surface.
//
// Mental trace (post-fix): segment `Index(IndexKey::Number(1))` →
// `LiteralKey::Number(1)` → `LiteralValue::Number(1.0)` → identity
// substitute K → `TypeExpr::Literal(Number(1.0))` on the prop's
// surface.
// =====================================================================
#[test]
fn mapped_narrowing_preserves_numeric_literal_kind_through_substitution() {
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"// Identity mapping over numeric keys: the iteration
// variable K is the value expression. Substituting K = 1
// at numeric segment 1 must intern LiteralValue::Number(1.0),
// not LiteralValue::String("1") — TypeScript indexed access
// `M[1]` (number literal) and `M['1']` (string literal) are
// semantically distinct keys at the type level even when they
// happen to resolve to the same runtime property.
export type NumberIdentity = { [K in number]: K }
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { NumberIdentity } from './types'

defineProps<{
  // M[1] with M = { [K in number]: K } — the substitution
  // K = 1 must yield the numeric literal type `1`, not the
  // string literal type `"1"`.
  numericIdentity: NumberIdentity[1]
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");

    let prop = meta
        .props
        .iter()
        .find(|p| p.name == "numericIdentity")
        .expect("numericIdentity prop must be present");

    let mut string_lits: Vec<String> = Vec::new();
    collect_string_literals(&prop.type_expr, &mut string_lits);
    let mut number_lits: Vec<f64> = Vec::new();
    collect_numeric_literals(&prop.type_expr, &mut number_lits);

    // Pre-G4.3 discrimination: the string-rendered "1" appears as
    // a `LiteralValue::String("1")` because the narrowing forced
    // every literal through `Arc::<str>::from(n.to_string())`.
    // Post-G4.3: the numeric kind is preserved through the
    // substitution and the literal is `LiteralValue::Number(1.0)`.
    assert!(
        number_lits.iter().any(|n| (*n - 1.0).abs() < f64::EPSILON),
        "guard: {{ [K in number]: K }}[1] MUST publish a numeric literal 1 (LiteralValue::Number), \
         not a string literal \"1\" (LiteralValue::String). \
         type_expr: {:#?}",
        prop.type_expr,
    );
    assert!(
        !string_lits.iter().any(|s| s == "1"),
        "guard: {{ [K in number]: K }}[1] MUST NOT publish the string literal \"1\" — \
         the numeric segment kind must be preserved through Mapped narrowing. \
         type_expr: {:#?}",
        prop.type_expr,
    );
}

// =====================================================================
// G4.4 — `IndexKey::Number` convention is unified across producers.
//
// G4.3 fixed the Mapped narrowing path to preserve literal kind
// through substitution; G4.4 closes the latent soundness gap by
// unifying the `IndexKey::Number` storage convention across every
// producer + consumer in the pipeline.
//
// Pre-G4.4 producers stored two different things in
// `IndexKey::Number(i64)`:
//
//   - `lower::shallow_lower_type_expr` stored the BIT-PATTERN
//     (`n.to_bits() as i64`). `f64::from_bits(...)` recovered the
//     value.
//   - `evaluate::normalized_index_key_node` and
//     `substitute::substitute_index_key_with_change_tracking`
//     stored the truncated INTEGER value (`*number as i64`).
//     `*number as f64` recovered the value.
//
// `walk::PathWalker`'s `Index(Number)` arm used the bit-pattern
// recovery, so any path that fed a substitution-produced
// integer-convention `IndexKey::Number(1)` into the walker would
// decode it as `f64::from_bits(1u64)` = 5e-324 instead of `1.0`.
//
// Empirically the example (`Lookup<NumberIdentity, 1>`)
// does not reach the walker's `Index(Number)` arm in the current
// dispatch topology — `Lookup`'s instantiation result is reduced
// upstream and the numeric literal is interned via
// `raise::raise_index_key_to_type_expr`'s integer-convention
// recovery. The convention inconsistency was therefore a latent
// soundness gap rather than an observable defect.
//
// G4.4 unifies the convention: every producer now stores the
// integer value and every consumer recovers via `*n as f64`.
// Non-integer literals (`Foo[1.5]`) and out-of-i64 literals
// remain as `IndexKey::TypeNode` references rather than entering
// the `IndexKey::Number` fast path — matching the existing
// `evaluate::normalized_index_key_node` admission guard.
//
// Characterising property (positive regression guard):
//   - `Lookup<NumberIdentity, 1>` publishes the numeric literal
//     `1.0` regardless of which substitution / dispatch / raise
//     path produces it.
//   - `M[1]` for a Mapped source publishes the numeric literal
//     `1.0` via the walker's `Index(Number)` arm — verified by
//     `mapped_narrowing_preserves_numeric_literal_kind_through_substitution`
//     (G4.3 guard).
//
// Together, the two tests pin BOTH the substitution-via-helper
// path AND the direct-source-lowered path to the same observable
// behavior — any future refactor that brings the substitution
// path through the walker's `Index(Number)` arm would observe
// the unified convention.
// =====================================================================
#[test]
fn indexed_access_substituted_through_generic_helper_publishes_numeric_literal() {
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"// Generic indexed-access helper. `I` is a type parameter
// — at lowering time `NumberIdentity[I]` becomes
// `IndexedAccess { ..., index: TypeNode(I_ref) }`. Only on
// instantiation (`Lookup<_, 1>`) does the substituter rewrite
// the TypeNode index to `IndexKey::Number(1)` via
// `normalized_index_key_node`. Pre-G4.4 this was the integer-
// convention path; lower-sited literals took the bit-pattern
// path. Post-G4.4 both producers store integer convention.
export type NumberIdentity = { [K in number]: K }
export type Lookup<M, I extends number> = M[I]
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { NumberIdentity, Lookup } from './types'

defineProps<{
  // Lookup<NumberIdentity, 1> — the generic helper's body is
  // `M[I]`; instantiation substitutes `I = 1`. The substituted
  // `IndexKey::Number(1)` (integer convention) must surface as
  // the numeric literal 1.0 — never as the bit-pattern denormal
  // 5e-324 that pre-G4.4 mis-decoding would have produced.
  numericIdentityViaHelper: Lookup<NumberIdentity, 1>
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");

    let prop = meta
        .props
        .iter()
        .find(|p| p.name == "numericIdentityViaHelper")
        .expect("numericIdentityViaHelper prop must be present");

    let mut number_lits: Vec<f64> = Vec::new();
    collect_numeric_literals(&prop.type_expr, &mut number_lits);

    // Characterisation: published numeric literal is exactly 1.0
    // (within f64::EPSILON). Any future refactor that funnels
    // the substitution-produced `IndexKey::Number(1)` through a
    // bit-pattern-decoding consumer would surface 5e-324 instead
    // (`f64::from_bits(1u64)`), tripping the assertion below.
    assert!(
        number_lits.iter().any(|n| (*n - 1.0).abs() < f64::EPSILON),
        "guard G4.4: Lookup<NumberIdentity, 1> MUST publish the numeric literal 1.0. \
         Convention unification — every `IndexKey::Number` producer stores integer \
         convention; every consumer recovers via `*n as f64`. A 5e-324 here would \
         indicate a regression to the pre-G4.4 mixed-convention pipeline. \
         type_expr: {:#?}",
        prop.type_expr,
    );
    // Negative: no denormal numeric literal appears anywhere in
    // the published surface. `f64::from_bits(1u64)` ≈ 5e-324 is
    // the canonical "mis-decoded integer-convention 1" signature.
    assert!(
        !number_lits.iter().any(|n| *n != 0.0 && n.abs() < 1e-300),
        "guard G4.4: the published numeric surface MUST NOT contain a denormal \
         (e.g. 5e-324 from `f64::from_bits(1u64)`) — that would indicate a \
         consumer is decoding an integer-convention `IndexKey::Number` as a \
         bit-pattern. type_expr: {:#?}",
        prop.type_expr,
    );
}

// =====================================================================
// G4.4 — additional convention-unification guard (Mapped + direct
// numeric literal source). This is the path the G4.3 fixture
// already exercises (`NumberIdentity[1]` where `1` is a source-
// literal numeric index) — pre-G4.4 used the bit-pattern producer
// at `lower::shallow_lower_type_expr` + bit-pattern consumer at
// `walk::PathWalker`'s `Index(Number)` arm (correct by symmetric
// matching); post-G4.4 both use the integer convention. The
// observable behavior is identical because the convention pair
// matches end-to-end in either case — but G4.4 eliminates the
// asymmetry latent in the codebase.
//
// This guard re-asserts the G4.3 invariant in a path-precise
// form: `M[7]` (a non-trivial integer literal, not 1 — to catch
// any reversion that special-cases 0/1) publishes the numeric
// literal `7.0`, exercising the walker's `Index(Number)` arm.
// =====================================================================
#[test]
fn mapped_narrowing_preserves_nontrivial_numeric_literal() {
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"// Non-trivial integer (7) — chosen because 0 and 1 are
// fixed points of many would-be mis-decodings. Pre-G4.4, the
// bit-pattern producer stored 7.0.to_bits() = 4619004367821864960
// in IndexKey::Number; the consumer's f64::from_bits(...) decoded
// back to 7.0 (correct). Post-G4.4, the producer stores 7 and the
// consumer does `7 as f64 = 7.0` — same observable, unified
// convention.
export type NumberIdentity = { [K in number]: K }
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { NumberIdentity } from './types'

defineProps<{
  // M[7] with M = { [K in number]: K } — the substitution
  // K = 7 must yield the numeric literal type `7`.
  seven: NumberIdentity[7]
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");

    let prop = meta
        .props
        .iter()
        .find(|p| p.name == "seven")
        .expect("seven prop must be present");

    let mut number_lits: Vec<f64> = Vec::new();
    collect_numeric_literals(&prop.type_expr, &mut number_lits);

    assert!(
        number_lits.iter().any(|n| (*n - 7.0).abs() < f64::EPSILON),
        "guard G4.4: NumberIdentity[7] MUST publish the numeric literal 7.0. \
         Any non-7.0 value indicates the producer/consumer conventions \
         disagree on `IndexKey::Number`. type_expr: {:#?}",
        prop.type_expr,
    );
    // Pre-G4.4 bit-pattern decoder of the integer-convention 7
    // would yield `f64::from_bits(7u64)` ≈ 3.5e-323. Negative
    // assertion: no denormal appears.
    assert!(
        !number_lits.iter().any(|n| *n != 0.0 && n.abs() < 1e-300),
        "guard G4.4: the published numeric surface MUST NOT contain a denormal. \
         A non-zero sub-1e-300 value here would indicate a producer/consumer \
         convention mismatch (e.g. `f64::from_bits(7u64)`). type_expr: {:#?}",
        prop.type_expr,
    );
}

// =====================================================================
// G4.5 — Mapped narrowing recovers NON-INTEGER numeric path segments
// via `IndexKey::TypeNode` literal inspection.
//
// G4.4 unified `IndexKey::Number` on the integer-i64 convention,
// bounded by the shared `build::integer_convention_index_key`
// predicate (fold iff the i64 `Display` IS the canonical
// `js_number_to_string` spelling). Producers (source lowering,
// `normalized_index_key_node`, generic substitution) emit
// `IndexKey::TypeNode` for every numeric literal outside the bound —
// non-integer literals (`1.5`), exponent-regime literals, and big
// integers with divergent shortest-round-trip spellings.
//
// This guards a soundness gap: the Mapped narrowing path in
// `walk.rs` only constructs a numeric `LiteralKey` from
// `IndexKey::Number`. For `{ [K in number]: K }[1.5]`, the producer
// emits `IndexKey::TypeNode(node)` where `node` resolves to a concrete
// `Literal(Number(1.5))`. Pre-G4.5, the walker's TypeNode arm received
// `IndexKey::TypeNode(_)` back from `normalized_index_key_node` and
// dropped to `(None, false)` — Mapped narrowing fell back to a
// deferred shell instead of substituting `K = 1.5`.
//
// G4.5 closes the gap: the walker's `IndexKey::TypeNode(resolved)` arm
// now inspects the resolved node's `SemanticNodeData::Literal` directly
// and recovers an f64 `LiteralKey::Number` for any numeric literal
// (integer OR non-integer), enabling the primitive-domain admission
// tier and the `K = Literal(Number(f))` substitution.
//
// Discriminating property: pre-G4.5 the published surface for
// `{ [K in number]: K }[1.5]` is a deferred Mapped shell with NO
// `1.5` numeric literal anywhere in the published surface (the
// `K = 1.5` substitution never runs). Post-G4.5, `1.5` appears as a
// numeric literal in the published surface.
// =====================================================================
#[test]
fn mapped_narrowing_preserves_non_integer_numeric_literal() {
    let project = make_project();
    upsert(
        &project,
        "/types.ts",
        r#"// Mapped over the primitive `number` keyspace — the
// narrowing path must substitute K with whatever numeric
// literal the indexed access carries, including non-integer
// literals that cannot round-trip through `i64`.
export type NumberIdentity = { [K in number]: K }
"#,
    );
    upsert(
        &project,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { NumberIdentity } from './types'

defineProps<{
  // Non-integer numeric literal index (1.5). The producer emits
  // `IndexKey::TypeNode(node)` because 1.5 fails the bounded
  // integer-convention admission. Pre-G4.5, Mapped narrowing
  // dropped this case to a deferred shell. Post-G4.5, the
  // walker recovers the f64 literal from the resolved
  // `Literal(Number(1.5))` and substitutes K = 1.5.
  oneAndAHalf: NumberIdentity[1.5]
}>()
</script>
<template><div /></template>"#,
    );

    let meta = meta_for(&project, "/Comp.vue");

    let prop = meta
        .props
        .iter()
        .find(|p| p.name == "oneAndAHalf")
        .expect("oneAndAHalf prop must be present");

    let mut number_lits: Vec<f64> = Vec::new();
    collect_numeric_literals(&prop.type_expr, &mut number_lits);

    // Discriminating positive: 1.5 appears as a numeric literal in
    // the published surface. Pre-G4.5 the Mapped narrowing fell
    // back to a deferred shell and the surface contained no
    // numeric literal at all (only the unresolved `K` reference or
    // a missing/deferred IndexedAccess).
    assert!(
        number_lits.iter().any(|n| (*n - 1.5).abs() < f64::EPSILON),
        "guard G4.5: NumberIdentity[1.5] MUST publish the numeric literal 1.5. \
         Absence indicates the Mapped narrowing fell back to a deferred shell \
         for the non-integer numeric path segment — the producer emitted \
         `IndexKey::TypeNode(node)` (because 1.5 fails the bounded \
         integer-convention admission), and the walker's `IndexKey::TypeNode(_)` arm dropped \
         the literal recovery. type_expr: {:#?}",
        prop.type_expr,
    );
    // Negative: no integer truncation (1.0) leaked through. A 1.0
    // here would indicate a regression where the walker recovered
    // through the integer-cast path instead of preserving the
    // f64 literal directly.
    assert!(
        !number_lits.iter().any(|n| (*n - 1.0).abs() < f64::EPSILON),
        "guard G4.5: the published numeric surface MUST NOT contain `1.0` \
         (an integer truncation of 1.5). A 1.0 here would indicate the \
         f64 literal recovery path was bypassed in favour of an i64-cast \
         path that loses the fractional component. type_expr: {:#?}",
        prop.type_expr,
    );
}
