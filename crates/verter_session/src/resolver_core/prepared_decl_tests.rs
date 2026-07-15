//! Unit tests for `prepared_decl.rs`, extracted from the inline
//! `#[cfg(test)] mod tests` to keep the production file under the size ceiling.

use std::sync::Arc;

use verter_semantic::analysis::type_eval::ValueDeclKind;

use super::*;

#[test]
fn prepares_local_exported_type_decl_from_shallow_file_state() {
    let source = "export interface Props { label: string }";
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props", None)
        .expect("Props should prepare");

    assert_eq!(prepared.root_identity.canonical_id, "/src/types.ts");
    assert_eq!(prepared.root_identity.symbol_name, "Props");
    assert_eq!(prepared.exported_name.as_deref(), Some("Props"));

    // Member index should be auto-populated for interface with properties
    assert!(
        prepared.member_index.contains_key("label"),
        "member index should contain 'label' property"
    );
}

#[test]
fn prepares_local_value_decl_from_shallow_file_state() {
    let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared = prepare_exported_value_decl("/src/types.ts", &state, "defaults", None)
        .expect("defaults should prepare");

    assert_eq!(prepared.root_identity.canonical_id, "/src/types.ts");
    assert_eq!(prepared.root_identity.symbol_name, "defaults");
    assert_eq!(prepared.exported_name.as_deref(), Some("defaults"));
    assert_eq!(prepared.kind, ValueDeclKind::Const);
    assert!(!matches!(
        prepared.type_annotation.classification,
        verter_type_expr::facts::ValueAnnotationClass::Absent
    ));
}

#[test]
fn prepared_type_decl_name_resolution_includes_typeof_imports() {
    let source = r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let dep_edges = FxHashMap::from_iter([
        ("./types".to_string(), "/src/types.ts".to_string()),
        ("./theme".to_string(), "/src/theme.ts".to_string()),
    ]);

    let prepared =
        prepare_exported_type_decl("/src/button-types.ts", &state, "Button", Some(&dep_edges))
            .expect("Button should prepare");

    assert_eq!(
        prepared
            .name_resolution
            .get("theme")
            .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
        Some(("/src/theme.ts", "theme"))
    );
}

#[test]
fn prepared_type_decl_prefers_same_file_type_over_same_named_value_import() {
    let source = r#"
type Separator = { ui: { root: string } }
export interface SeparatorSlots {
    default?(props: { ui: Separator['ui'] }): unknown
}
import { Separator } from './runtime'
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let dep_edges =
        FxHashMap::from_iter([("./runtime".to_string(), "/src/runtime.ts".to_string())]);

    assert!(
        state.has_type_symbol("Separator"),
        "the authored type alias must remain present in the type namespace"
    );
    assert!(
        state.is_import_local("Separator"),
        "the same-named runtime import is intentionally present as the collision control"
    );

    let slots = prepare_exported_type_decl(
        "/src/Separator.vue",
        &state,
        "SeparatorSlots",
        Some(&dep_edges),
    )
    .expect("SeparatorSlots should prepare");

    assert_eq!(
        slots
            .name_resolution
            .get("Separator")
            .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
        Some(("/src/Separator.vue", "Separator")),
        "a type declaration body resolves through the same-file type namespace, not the setup runtime import"
    );
    assert!(
        prepare_local_type_decl(
            "/src/Separator.vue",
            &state,
            "Separator",
            Some(&dep_edges),
            &ImportCanonicalization::default(),
        )
        .is_some(),
        "the same-file type declaration must remain addressable even when the value namespace imports the same name"
    );
}

#[test]
fn prepared_type_decl_falls_back_to_canonical_relative_targets_without_dep_edges() {
    let source = r#"
import type { ComponentConfig } from './tv.ts'
import type { AppConfig } from './schema.ts'
import theme from './theme.ts'

export type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
"#;
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared = prepare_exported_type_decl("/src/Button.vue", &state, "Button", None)
        .expect("Button should prepare");

    assert_eq!(
        prepared
            .name_resolution
            .get("ComponentConfig")
            .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
        Some(("/src/tv.ts", "ComponentConfig"))
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("AppConfig")
            .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
        Some(("/src/schema.ts", "AppConfig"))
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("theme")
            .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
        Some(("/src/theme.ts", "default"))
    );
}

#[test]
fn prepared_value_decl_falls_back_to_canonical_relative_targets_without_dep_edges() {
    let source = r#"
import type { Theme } from './theme.ts'

export const defaults: Theme = {} as Theme
"#;
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared = prepare_exported_value_decl("/src/Button.vue", &state, "defaults", None)
        .expect("defaults should prepare");

    assert_eq!(
        prepared
            .name_resolution
            .get("Theme")
            .map(|id| (id.canonical_id.as_str(), id.symbol_name.as_str())),
        Some(("/src/theme.ts", "Theme"))
    );
}

#[test]
fn does_not_prepare_reexport_without_frontier_routing() {
    let source = r#"export { Props } from "./inner""#;
    let state = ShallowFileState::service_backed_for_test(source);

    assert!(prepare_exported_type_decl("/src/barrel.ts", &state, "Props", None).is_none());
}

#[test]
fn prepared_type_decl_populates_deps_from_shallow_symbol() {
    let source = r#"
import { Inner } from "./inner"
type Local = { x: number }
export interface Props { child: Inner; data: Local }
"#;
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props", None)
        .expect("Props should prepare");

    // Should have a member index for 'child' and 'data'
    assert!(
        prepared.member_index.contains_key("child"),
        "member index should contain 'child'"
    );
    assert!(
        prepared.member_index.contains_key("data"),
        "member index should contain 'data'"
    );
}

#[test]
fn builds_local_prepared_decl_caches_from_shallow_file_state() {
    let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
    let state = ShallowFileState::service_backed_for_test(source);

    let type_cache = build_prepared_type_decl_cache(
        "/src/types.ts",
        Arc::clone(&state),
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
    );
    let value_cache = build_prepared_value_decl_cache(
        "/src/types.ts",
        state,
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
    );

    assert!(type_cache.contains_key("Props"));
    assert!(value_cache.contains_key("defaults"));
}

#[test]
fn prepared_type_decl_build_counter_is_thread_local() {
    reset_prepared_type_decl_build_count_for_tests();

    let source = "export interface Props { label: string }";
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared = prepare_exported_type_decl("/src/types.ts", &state, "Props", None)
        .expect("Props should prepare");
    assert_eq!(prepared.root_identity.symbol_name, "Props");
    assert_eq!(prepared_type_decl_build_count_for_tests(), 1);

    let other_thread_count = std::thread::spawn(prepared_type_decl_build_count_for_tests)
        .join()
        .expect("thread-local counter probe should join cleanly");
    assert_eq!(
        other_thread_count, 0,
        "prepared decl build counters should not leak across test threads",
    );
}

// Scope-matched namespace-sibling binding: a namespaced decl binds bare
// sibling names ONLY from the inventory visible for its declaration ORIGIN,
// and only where that sibling has a buildable prepared decl. The two tests
// below pin the two regressions the shared (origin-blind) helper introduced.

#[test]
fn module_augmentation_namespace_decl_does_not_bind_global_sibling() {
    // A `declare module "ext" { namespace NS { ... } }` decl resolves its
    // OWN module siblings — NEVER a global-augmentation `namespace NS`
    // sibling of the same namespace name. The origin-blind helper scanned
    // `Global` augmentation keys unconditionally and leaked the sibling
    // `GlobalOnly` (declared in `declare global { namespace NS }`) into the
    // Module-scope decl's `name_resolution`, crossing scopes. Module
    // siblings are not consumable today (no Module-scope prepared-decl
    // slot), so the Module arm binds NOTHING.
    use verter_semantic::analysis::type_eval::AugmentationScopeKind;
    let source = r#"
export {};
declare global { namespace NS { type GlobalOnly = { g: string } } }
declare module "ext" { namespace NS { interface Foo { x: GlobalOnly } } }
"#;
    let state = ShallowFileState::service_backed_for_test(source);

    // Harness invariant the leak depends on: the module namespace member is
    // retained under `(Module("ext"), "NS.Foo")` and a global sibling under
    // `(Global, "NS.GlobalOnly")`, distinct scopes sharing the `NS.` prefix.
    let prepared = prepare_augmentation_type_decl(
        "/src/aug.ts",
        &state,
        &AugmentationScopeKind::Module("ext".into()),
        "NS.Foo",
        None,
    )
    .expect("NS.Foo should prepare");

    assert!(
        !prepared.name_resolution.contains_key("GlobalOnly"),
        "a module-augmentation namespace decl must NOT bind a \
             global-augmentation sibling of the same namespace name"
    );
}

#[test]
fn global_augmentation_namespace_type_decl_binds_type_sibling_not_value_sibling() {
    // A global-augmentation namespace TYPE decl binds its global TYPE
    // siblings (consumable through `prepare_local_type_decl`'s global
    // fallback) but NOT its global VALUE siblings: no prepared-value slot or
    // value fallback exists for a `(Global, "NS.member")` value key, so a
    // binding would dangle. The origin-blind helper chained the value keys
    // and bound the dangling `VERSION`; the Global arm is TYPE-only.
    let source = r#"
export {};
declare global { namespace JSX {
  type Common = { id?: string };
  export const VERSION: string;
  interface El { x: Common }
} }
"#;
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared = prepare_local_type_decl(
        "/src/aug.ts",
        &state,
        "JSX.El",
        None,
        &ImportCanonicalization::default(),
    )
    .expect("JSX.El should prepare");

    // Positive control: the global TYPE sibling still binds (proves the
    // Global TYPE scan is retained, not gutted along with the value scan).
    assert_eq!(
        prepared
            .name_resolution
            .get("Common")
            .map(|i| (i.canonical_id.as_str(), i.symbol_name.as_str())),
        Some(("/src/aug.ts", "JSX.Common")),
    );
    // Restrict: the non-consumable global VALUE sibling must NOT be bound.
    assert!(
        !prepared.name_resolution.contains_key("VERSION"),
        "a non-consumable global-augmentation VALUE sibling must NOT be \
             bound into name_resolution"
    );
}

// ---------------------------------------------------------------------
// Broken-lease no-warm rail at the CACHE-ADMITTING prepared-decl boundary.
//
// The locator-deref rail (`deref_locator_body` → `LocatorBodyDerefError::
// LeaseMiss` → `cache_suppress`) already refuses to warm a transient
// ReturnOnly. These tests pin the SAME no-warm contract on the OTHER
// body-consumer: a broken decl-body lease pin during a prepared-decl build
// must NOT commit the write-once slot (nor the `type_deps` classification
// cache) with a body-less result for a REAL symbol — a false-warm absence
// that (write-once) a later live-lease demand could never recover from.
// ---------------------------------------------------------------------

/// A broken-lease prepared-TYPE demand fails closed to `None` (ReturnOnly)
/// and — the discriminating no-warm assertion — leaves the write-once slot
/// for the REAL symbol VACANT. Pre-change (`get_or_init`) the slot committed
/// `None`; post-change the slot stays uncommitted so a retry recovers.
#[test]
fn broken_lease_prepared_type_decl_get_does_not_warm_admit_none_slot() {
    let source = "export type Var0 = { v: 0 };\nexport type Var1 = { v: 1 };\n";
    let state = ShallowFileState::service_backed_for_test(source);
    let cache = build_prepared_type_decl_cache(
        "/ws/fixture.ts",
        Arc::clone(&state),
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
    );

    // One successful demand pins the retained-snapshot lease and commits Var0.
    assert!(
        cache.get("Var0").is_some(),
        "Var0 prepares under a live lease"
    );
    assert!(cache.slot_committed_for_test("Var0"));

    // Break the retained snapshot out-of-band: every subsequent body demand
    // now lease-misses (the unreachable-in-practice invariant-violation).
    state.decl_bodies().release_retained_snapshot_for_test();

    assert!(
        cache.get("Var1").is_none(),
        "a broken-lease prepared-type demand fails CLOSED to None (ReturnOnly)"
    );
    assert!(
        !cache.slot_committed_for_test("Var1"),
        "the broken-lease prepared-type demand must NOT commit the write-once \
             slot to None — the false-warm absence the LowerLocator rail already \
             refuses"
    );
}

/// VALUE-space counterpart of the type-slot no-warm test.
#[test]
fn broken_lease_prepared_value_decl_get_does_not_warm_admit_none_slot() {
    let source = "export const alpha = 1;\nexport const beta = 2;\n";
    let state = ShallowFileState::service_backed_for_test(source);
    let cache = build_prepared_value_decl_cache(
        "/ws/fixture.ts",
        Arc::clone(&state),
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
    );

    assert!(
        cache.get("alpha").is_some(),
        "alpha prepares under a live lease"
    );
    assert!(cache.slot_committed_for_test("alpha"));

    state.decl_bodies().release_retained_snapshot_for_test();

    assert!(
        cache.get("beta").is_none(),
        "a broken-lease prepared-value demand fails CLOSED to None (ReturnOnly)"
    );
    assert!(
        !cache.slot_committed_for_test("beta"),
        "the broken-lease prepared-value demand must NOT commit the write-once \
             slot to None"
    );
}

/// A broken-lease `type_deps` classification fails closed to `None`
/// (ReturnOnly) and must NOT cache the transient `None` as genuine absence:
/// a cached wrong-empty would under-classify a REAL symbol's dependency
/// edges for the artifact's life (under-invalidation). Pre-change the `None`
/// was cached; post-change no entry is committed.
#[test]
fn broken_lease_type_deps_is_not_cached_as_absence() {
    let source = "export type Var0 = { v: 0 };\nexport type Var1 = { v: 1 };\n";
    let state = ShallowFileState::service_backed_for_test(source);

    assert!(
        state.type_deps("Var0").is_some(),
        "Var0 classifies under a live lease"
    );

    state.decl_bodies().release_retained_snapshot_for_test();

    assert!(
        state.type_deps("Var1").is_none(),
        "a broken-lease type_deps fails CLOSED to None (ReturnOnly)"
    );
    assert!(
        !state.type_deps_cache_has_none_entry("Var1"),
        "the broken-lease type_deps must NOT cache a None as genuine absence"
    );
}

/// The augmentation-scope prepared build surfaces the DISTINCT `LeaseMiss`
/// outcome (never a collapsed `Ready(None)`) so the cross-file augmentation
/// stitch folds a broken-lease augmenter into the `source_env_unobservable`
/// no-warm rail instead of silently dropping the contributor. Discriminates
/// a correct implementation from one that collapses the lease-miss.
#[test]
fn broken_lease_augmentation_prepared_build_surfaces_lease_miss() {
    use verter_semantic::analysis::type_eval::AugmentationScopeKind;

    let source = "declare module \"ext\" { interface A { x: string } }\n\
                      declare module \"ext\" { interface B { y: number } }\n";
    let state = ShallowFileState::service_backed_for_test(source);
    let scope = AugmentationScopeKind::Module("ext".to_string());

    // Pin the augmenter memo's lease with one successful augmentation demand.
    assert!(
        matches!(
            prepare_augmentation_type_decl_outcome("/ws/fixture.ts", &state, &scope, "A", None),
            PreparedDeclOutcome::Ready(Some(_))
        ),
        "augmentation symbol A prepares under a live lease"
    );

    state.decl_bodies().release_retained_snapshot_for_test();

    // A DIFFERENT, not-yet-lowered augmentation symbol now lease-misses: the
    // outcome MUST be the distinct LeaseMiss, never a collapsed Ready(None).
    assert!(
        matches!(
            prepare_augmentation_type_decl_outcome("/ws/fixture.ts", &state, &scope, "B", None),
            PreparedDeclOutcome::LeaseMiss
        ),
        "a broken-lease augmentation prepare must surface the DISTINCT \
             LeaseMiss, not a cacheable Ready(None)"
    );
}

// ---------------------------------------------------------------------
// Cold single-flight around the warm prepared-decl slot.
//
// The write-once warm `OnceLock` cannot commit a LeaseMiss (a permanent
// false-warm absence), so the slot pairs it with a resettable in-flight
// gate: concurrent cold callers serialise on the gate and reuse the
// winner's committed result (ONE build), while a LeaseMiss leaves the slot
// vacant so a later demand under a live lease recomputes.
// ---------------------------------------------------------------------

/// Concurrent cold callers for one symbol run exactly ONE prepared-decl
/// build (single-flight) and all observe the SAME committed Arc. Pre-fix
/// (check/build/`slot.set`, no gate) every racing cold caller builds its own
/// decl — the atomic get-build count is > 1. Post-fix the in-flight gate
/// serialises the cold build so exactly one runs.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn concurrent_cold_prepared_type_get_is_single_flight() {
    // Heavy fixture so each prepared-decl build does real name_resolution
    // work and concurrent cold callers genuinely overlap.
    let mut source = String::new();
    for i in 0..400 {
        source.push_str(&format!("export type T{i} = {{ v{i}: number }};\n"));
    }
    let state = ShallowFileState::service_backed_for_test(&source);
    let cache = build_prepared_type_decl_cache(
        "/ws/fixture.ts",
        Arc::clone(&state),
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
    );

    const THREADS: usize = 16;
    let barrier = Arc::new(std::sync::Barrier::new(THREADS));
    let mut handles = Vec::new();
    for _ in 0..THREADS {
        let cache = cache.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            cache.get("T7")
        }));
    }
    let arcs: Vec<Arc<PreparedTypeDecl>> = handles
        .into_iter()
        .map(|h| {
            h.join()
                .expect("no caller thread may panic")
                .expect("T7 builds")
        })
        .collect();

    assert_eq!(
        cache.cold_build_count_for_test(),
        1,
        "concurrent cold callers must share ONE prepared-decl build (single-flight); \
             a count > 1 means the in-flight gate was dropped and every racer rebuilt"
    );
    let first = &arcs[0];
    for a in &arcs {
        assert!(
            Arc::ptr_eq(first, a),
            "every cold caller must observe the SAME committed prepared-decl Arc"
        );
    }
}

/// A broken decl-body lease pin leaves the slot VACANT (never a write-once
/// `None`), so it stays re-buildable and a later demand under a live lease
/// recovers. Discriminates the resettable gate from a write-once `OnceLock`
/// (which would serve the committed `None` on retry and never recompute).
#[test]
fn broken_lease_prepared_type_slot_stays_vacant_and_is_rebuildable() {
    let source = "export type Var0 = { v: 0 };\nexport type Var1 = { v: 1 };\n";
    let state = ShallowFileState::service_backed_for_test(source);
    let cache = build_prepared_type_decl_cache(
        "/ws/fixture.ts",
        Arc::clone(&state),
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
    );

    // Pin the lease with one successful build, then break the snapshot.
    assert!(
        cache.get("Var0").is_some(),
        "Var0 prepares under a live lease"
    );
    state.decl_bodies().release_retained_snapshot_for_test();

    assert!(
        cache.get("Var1").is_none(),
        "a broken-lease prepared-type demand fails CLOSED to None (ReturnOnly)"
    );
    assert!(
        !cache.slot_committed_for_test("Var1"),
        "the lease-miss must leave the slot VACANT, not a write-once None"
    );
    let after_first = cache.cold_build_count_for_test();
    // A vacant slot re-runs the build on the next demand; a write-once None
    // would short-circuit and never recompute.
    assert!(
        cache.get("Var1").is_none(),
        "a second broken-lease demand still fails closed to None"
    );
    assert!(
        cache.cold_build_count_for_test() > after_first,
        "a vacant (lease-missed) slot must re-run the build on retry — a \
             write-once None would serve the committed absence without rebuilding"
    );

    // Recovery under a live lease: a fresh cache for the SAME source (a
    // later content generation) DOES build Var1 — the vacant slot policy is
    // what makes that recovery possible.
    let fresh_state = ShallowFileState::service_backed_for_test(source);
    let fresh_cache = build_prepared_type_decl_cache(
        "/ws/fixture.ts",
        Arc::clone(&fresh_state),
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
    );
    assert!(
        fresh_cache.get("Var1").is_some(),
        "under a live lease the symbol recovers — the lease-miss was never a \
             genuine absence"
    );
}
