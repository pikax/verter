//! Unit tests for `prepared_decl.rs`, extracted from the inline
//! `#[cfg(test)] mod tests` to keep the production file under the size ceiling.

use std::sync::Arc;

use verter_semantic::analysis::type_eval::ValueDeclKind;

use super::*;

/// Shared test pool: prepare fns intern identities through it.
fn test_interner() -> Arc<crate::identity_interner::IdentityInterner> {
    Arc::new(crate::identity_interner::IdentityInterner::with_default_budget())
}

fn ordinary_import_canonicalization(entries: &[(&str, &str, &str)]) -> ImportCanonicalization {
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    ImportCanonicalization {
        final_resolution: entries
            .iter()
            .map(|(local_name, canonical_id, symbol_name)| {
                (
                    verter_type_expr::DeclKey::new(owner, *local_name),
                    ResolvedRootIdentity::new_in_owner(*canonical_id, owner, *symbol_name),
                )
            })
            .collect(),
    }
}

#[test]
fn prepares_local_exported_type_decl_from_shallow_file_state() {
    let source = "export interface Props { label: string }";
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared =
        prepare_exported_type_decl("/src/types.ts", &state, "Props", None, &test_interner())
            .expect("Props preparation should succeed")
            .expect("Props should be present");

    assert_eq!(
        prepared.root_identity.canonical_id.as_ref(),
        "/src/types.ts"
    );
    assert_eq!(prepared.root_identity.symbol_name.as_ref(), "Props");
    assert_eq!(prepared.exported_name.as_deref(), Some("Props"));

    // Member index should be auto-populated for interface with properties
    assert!(
        prepared.member_index.contains_key("label"),
        "member index should contain 'label' property"
    );
}

#[test]
fn prepared_type_decl_copies_vue_ignored_heritage_fact_from_lowered_decl() {
    use verter_type_expr::facts::VueIgnoredHeritageFact;

    let source = r#"
import type { Imported } from './base'
export interface Props extends /* @vue-ignore */ Imported<string> { own: number }
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let dep_edges = FxHashMap::from_iter([("./base".to_string(), "/src/base.ts".to_string())]);
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let import_canonicalization = ImportCanonicalization {
        final_resolution: FxHashMap::from_iter([(
            verter_type_expr::DeclKey::new(owner, "Imported"),
            ResolvedRootIdentity::new_in_owner("/src/base.ts", owner, "Imported"),
        )]),
    };
    let prepared = prepare_local_type_decl(
        "/src/types.ts",
        &state,
        "Props",
        Some(&dep_edges),
        &import_canonicalization,
        &test_interner(),
    )
    .expect("Props preparation should succeed")
    .expect("Props should be present");

    assert_eq!(
        prepared.vue_ignored_heritage.as_ref(),
        [VueIgnoredHeritageFact {
            contributor_ordinal: 0,
            intersection_arm_ordinal: 0,
        }]
    );
}

#[test]
fn prepares_local_value_decl_from_shallow_file_state() {
    let source = r#"
export interface Props { label: string }
export const defaults: Props = { label: 'ok' }
"#;
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared =
        prepare_exported_value_decl("/src/types.ts", &state, "defaults", None, &test_interner())
            .expect("defaults should be present");

    assert_eq!(
        prepared.root_identity.canonical_id.as_ref(),
        "/src/types.ts"
    );
    assert_eq!(prepared.root_identity.symbol_name.as_ref(), "defaults");
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
    let import_canonicalization = ordinary_import_canonicalization(&[
        ("ComponentConfig", "/src/types.ts", "ComponentConfig"),
        ("theme", "/src/theme.ts", "theme"),
    ]);

    let prepared = prepare_local_type_decl(
        "/src/button-types.ts",
        &state,
        "Button",
        Some(&dep_edges),
        &import_canonicalization,
        &test_interner(),
    )
    .expect("Button preparation should succeed")
    .expect("Button should be present");

    assert_eq!(
        prepared
            .name_resolution
            .get("theme")
            .map(|id| (id.canonical_id.as_ref(), id.symbol_name.as_ref())),
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
    let import_canonicalization =
        ordinary_import_canonicalization(&[("Separator", "/src/runtime.ts", "Separator")]);

    assert!(
        state.has_type_symbol("Separator"),
        "the authored type alias must remain present in the type namespace"
    );
    assert!(
        state.is_import_local("Separator"),
        "the same-named runtime import is intentionally present as the collision control"
    );

    let slots = prepare_local_type_decl(
        "/src/Separator.vue",
        &state,
        "SeparatorSlots",
        Some(&dep_edges),
        &import_canonicalization,
        &test_interner(),
    )
    .expect("SeparatorSlots preparation should succeed")
    .expect("SeparatorSlots should be present");

    assert_eq!(
        slots
            .name_resolution
            .get("Separator")
            .map(|id| (id.canonical_id.as_ref(), id.symbol_name.as_ref())),
        Some(("/src/Separator.vue", "Separator")),
        "a type declaration body resolves through the same-file type namespace, not the setup runtime import"
    );
    assert!(
        prepare_local_type_decl(
            "/src/Separator.vue",
            &state,
            "Separator",
            Some(&dep_edges),
            &import_canonicalization,
            &test_interner(),
        )
        .expect("Separator preparation should succeed")
        .is_some(),
        "the same-file type declaration must remain addressable even when the value namespace imports the same name"
    );
}

#[test]
fn prepared_type_decl_uses_explicit_canonical_relative_targets() {
    let source = r#"
import type { ComponentConfig } from './tv.ts'
import type { AppConfig } from './schema.ts'
import theme from './theme.ts'

export type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let import_canonicalization = ordinary_import_canonicalization(&[
        ("ComponentConfig", "/src/tv.ts", "ComponentConfig"),
        ("AppConfig", "/src/schema.ts", "AppConfig"),
        ("theme", "/src/theme.ts", "default"),
    ]);

    let prepared = prepare_local_type_decl(
        "/src/Button.vue",
        &state,
        "Button",
        None,
        &import_canonicalization,
        &test_interner(),
    )
    .expect("Button preparation should succeed")
    .expect("Button should be present");

    assert_eq!(
        prepared
            .name_resolution
            .get("ComponentConfig")
            .map(|id| (id.canonical_id.as_ref(), id.symbol_name.as_ref())),
        Some(("/src/tv.ts", "ComponentConfig"))
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("AppConfig")
            .map(|id| (id.canonical_id.as_ref(), id.symbol_name.as_ref())),
        Some(("/src/schema.ts", "AppConfig"))
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("theme")
            .map(|id| (id.canonical_id.as_ref(), id.symbol_name.as_ref())),
        Some(("/src/theme.ts", "default"))
    );
}

#[test]
fn prepared_value_decl_uses_explicit_canonical_relative_targets() {
    let source = r#"
import type { Theme } from './theme.ts'

export const defaults: Theme = {} as Theme
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let import_canonicalization =
        ordinary_import_canonicalization(&[("Theme", "/src/theme.ts", "Theme")]);

    let prepared = prepare_local_value_decl(
        "/src/Button.vue",
        &state,
        "defaults",
        None,
        &import_canonicalization,
        &test_interner(),
    )
    .expect("defaults should be present");

    assert_eq!(
        prepared
            .name_resolution
            .get("Theme")
            .map(|id| (id.canonical_id.as_ref(), id.symbol_name.as_ref())),
        Some(("/src/theme.ts", "Theme"))
    );
}

#[test]
fn does_not_prepare_reexport_without_frontier_routing() {
    let source = r#"export { Props } from "./inner""#;
    let state = ShallowFileState::service_backed_for_test(source);

    assert!(
        prepare_exported_type_decl("/src/barrel.ts", &state, "Props", None, &test_interner())
            .expect("barrel preparation should succeed")
            .is_none()
    );
}

#[test]
fn prepared_type_decl_populates_deps_from_shallow_symbol() {
    let source = r#"
import { Inner } from "./inner"
type Local = { x: number }
export interface Props { child: Inner; data: Local }
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let import_canonicalization =
        ordinary_import_canonicalization(&[("Inner", "/src/inner.ts", "Inner")]);

    let prepared = prepare_local_type_decl(
        "/src/types.ts",
        &state,
        "Props",
        None,
        &import_canonicalization,
        &test_interner(),
    )
    .expect("Props preparation should succeed")
    .expect("Props should be present");

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
        &test_interner(),
    );
    let value_cache = build_prepared_value_decl_cache(
        "/src/types.ts",
        state,
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
        &test_interner(),
    );

    assert!(type_cache.contains_key("Props"));
    assert!(value_cache.contains_key("defaults"));
}

#[test]
fn prepared_type_decl_build_counter_is_thread_local() {
    reset_prepared_type_decl_build_count_for_tests();

    let source = "export interface Props { label: string }";
    let state = ShallowFileState::service_backed_for_test(source);

    let prepared =
        prepare_exported_type_decl("/src/types.ts", &state, "Props", None, &test_interner())
            .expect("Props preparation should succeed")
            .expect("Props should be present");
    assert_eq!(prepared.root_identity.symbol_name.as_ref(), "Props");
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
        &ImportCanonicalization::default(),
        &test_interner(),
    )
    .expect("NS.Foo preparation should succeed")
    .expect("NS.Foo should be present");

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
        &test_interner(),
    )
    .expect("JSX.El preparation should succeed")
    .expect("JSX.El should be present");

    // Positive control: the global TYPE sibling still binds (proves the
    // Global TYPE scan is retained, not gutted along with the value scan).
    assert_eq!(
        prepared
            .name_resolution
            .get("Common")
            .map(|i| (i.canonical_id.as_ref(), i.symbol_name.as_ref())),
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
// Name-resolution precedence characterization.
//
// The prepared-decl `name_resolution` table has a fixed key-wins order
// (last insert wins per key): same-file TYPE/VALUE symbols, then the
// per-declaration namespace-sibling bindings, then import bindings —
// where the TYPE-space import pass skips a local name that is a
// same-file type symbol, and the VALUE-space import pass never skips.
// These tests pin the observable per-key winners so the shared
// per-file base table + per-decl overlay split cannot permute them.
// ---------------------------------------------------------------------

/// A namespaced declaration's DIRECT sibling binding shadows a same-named
/// file-level type symbol: inside `namespace NS`, a bare `Item` reference
/// resolves to `NS.Item`, not the outer file-level `Item` (the TS
/// namespace-scope rule; sibling bindings insert AFTER the file-symbol
/// loops).
#[test]
fn namespace_sibling_shadows_file_level_type_symbol_in_type_space() {
    let source = r#"
export type Item = { file: true };
export namespace NS {
  export type Item = { ns: true };
  export interface Holder { x: Item }
}
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    // Harness invariants the precedence depends on: the namespace member is
    // indexed under its QUALIFIED name and the file-level symbol under its
    // bare name.
    assert!(state.has_type_symbol("Item"));
    assert!(state.has_type_symbol("NS.Item"));

    let prepared = prepare_local_type_decl(
        "/ws/fixture.ts",
        &state,
        "NS.Holder",
        None,
        &ImportCanonicalization::default(),
        &test_interner(),
    )
    .expect("NS.Holder preparation should succeed")
    .expect("NS.Holder should be present");

    assert_eq!(
        prepared
            .name_resolution
            .get("Item")
            .map(|i| (i.canonical_id.as_ref(), i.symbol_name.as_ref())),
        Some(("/ws/fixture.ts", "NS.Item")),
        "a direct namespace sibling must shadow the same-named file-level type symbol"
    );
}

/// An import binding wins over a namespace-sibling binding for the same
/// bare name when that name is NOT a same-file type symbol: the TYPE-space
/// import pass inserts after the sibling pass and only skips names the
/// file's own TYPE namespace declares.
#[test]
fn import_wins_over_namespace_sibling_for_non_type_symbol_name() {
    let source = r#"
import { Item } from './items';
export namespace NS {
  export type Item = { ns: true };
  export interface Holder { x: Item }
}
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let dep_edges = FxHashMap::from_iter([("./items".to_string(), "/src/items.ts".to_string())]);
    let import_canonicalization =
        ordinary_import_canonicalization(&[("Item", "/src/items.ts", "Item")]);
    // The bare name is an import local and NOT a file-scope type symbol
    // (the namespace member is indexed under "NS.Item" only).
    assert!(state.is_import_local("Item"));
    assert!(!state.has_type_symbol("Item"));

    let prepared = prepare_local_type_decl(
        "/ws/fixture.ts",
        &state,
        "NS.Holder",
        Some(&dep_edges),
        &import_canonicalization,
        &test_interner(),
    )
    .expect("NS.Holder preparation should succeed")
    .expect("NS.Holder should be present");

    assert_eq!(
        prepared
            .name_resolution
            .get("Item")
            .map(|i| (i.canonical_id.as_ref(), i.symbol_name.as_ref())),
        Some(("/src/items.ts", "Item")),
        "an import binding wins over a namespace-sibling binding for a \
             name the file's type namespace does not declare"
    );
}

/// TYPE-space vs VALUE-space import shadowing diverge on the SAME file: a
/// same-file type symbol shadows the same-named import in a prepared TYPE
/// decl's table, while a prepared VALUE decl's table lets the import win
/// (its import pass has no type-symbol skip).
#[test]
fn value_space_import_wins_where_type_space_prefers_same_file_type_symbol() {
    let source = r#"
type Separator = { ui: { root: string } }
import { Separator } from './runtime'
export const defaults = { s: 1 }
export interface SeparatorSlots { root: Separator }
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let dep_edges =
        FxHashMap::from_iter([("./runtime".to_string(), "/src/runtime.ts".to_string())]);
    let import_canonicalization =
        ordinary_import_canonicalization(&[("Separator", "/src/runtime.ts", "Separator")]);
    assert!(state.has_type_symbol("Separator"));
    assert!(state.is_import_local("Separator"));

    let type_prepared = prepare_local_type_decl(
        "/src/Separator.vue",
        &state,
        "SeparatorSlots",
        Some(&dep_edges),
        &import_canonicalization,
        &test_interner(),
    )
    .expect("SeparatorSlots preparation should succeed")
    .expect("SeparatorSlots should be present");
    assert_eq!(
        type_prepared
            .name_resolution
            .get("Separator")
            .map(|i| (i.canonical_id.as_ref(), i.symbol_name.as_ref())),
        Some(("/src/Separator.vue", "Separator")),
        "TYPE space: the same-file type symbol shadows the import"
    );

    let value_prepared = prepare_local_value_decl(
        "/src/Separator.vue",
        &state,
        "defaults",
        Some(&dep_edges),
        &import_canonicalization,
        &test_interner(),
    )
    .expect("defaults should be present");
    assert_eq!(
        value_prepared
            .name_resolution
            .get("Separator")
            .map(|i| (i.canonical_id.as_ref(), i.symbol_name.as_ref())),
        Some(("/src/runtime.ts", "Separator")),
        "VALUE space: the import wins — the value-space import pass has no \
             type-symbol skip"
    );
}

/// File symbols and imports agree between a namespaced and a plain decl of
/// the same file for every key the sibling pass does not touch: the
/// namespace overlay is SPARSE (sibling members only), never a divergent
/// rebuild of the file-level entries.
#[test]
fn namespaced_decl_table_matches_plain_decl_table_outside_sibling_keys() {
    let source = r#"
import { helper } from './helper';
export type Plain = { p: number };
export namespace NS {
  export type Member = { m: true };
  export interface Holder { x: Member }
}
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let dep_edges = FxHashMap::from_iter([("./helper".to_string(), "/src/helper.ts".to_string())]);
    let import_canonicalization =
        ordinary_import_canonicalization(&[("helper", "/src/helper.ts", "helper")]);

    let plain = prepare_local_type_decl(
        "/ws/fixture.ts",
        &state,
        "Plain",
        Some(&dep_edges),
        &import_canonicalization,
        &test_interner(),
    )
    .expect("Plain preparation should succeed")
    .expect("Plain should be present");
    let namespaced = prepare_local_type_decl(
        "/ws/fixture.ts",
        &state,
        "NS.Holder",
        Some(&dep_edges),
        &import_canonicalization,
        &test_interner(),
    )
    .expect("NS.Holder preparation should succeed")
    .expect("NS.Holder should be present");

    for key in ["helper", "Plain", "NS.Member", "NS.Holder"] {
        assert_eq!(
            plain.name_resolution.get(key),
            namespaced.name_resolution.get(key),
            "file-level entry {key:?} must be identical across decls of one file"
        );
    }
    // The sibling binding is the ONLY namespaced-decl addition.
    assert!(namespaced.name_resolution.contains_key("Member"));
    assert!(!plain.name_resolution.contains_key("Member"));
}

/// The per-file `name_resolution` base table is built ONCE per prepared-decl
/// cache and SHARED (same `Arc`) by every non-namespaced decl the cache
/// builds — type and value spaces each own one base. A namespaced decl
/// carries its own private table (sibling bindings are declaration-scoped).
/// Pre-split, every prepared decl rebuilt its table by walking every file
/// symbol + import — distinct allocations per decl.
#[test]
fn prepared_decls_share_one_name_resolution_base_per_cache() {
    let source = r#"
import { helper } from './helper';
export type A = { a: number };
export interface B { b: string }
export const c = { c: true };
export const d = 4;
export namespace NS {
  export type Member = { m: true };
  export interface Holder { x: Member }
}
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let import_canonicalization = Arc::new(ordinary_import_canonicalization(&[(
        "helper",
        "/src/helper.ts",
        "helper",
    )]));
    let type_cache = build_prepared_type_decl_cache(
        "/ws/fixture.ts",
        Arc::clone(&state),
        Arc::new(FxHashMap::default()),
        Arc::clone(&import_canonicalization),
        &test_interner(),
    );
    let value_cache = build_prepared_value_decl_cache(
        "/ws/fixture.ts",
        Arc::clone(&state),
        Arc::new(FxHashMap::default()),
        import_canonicalization,
        &test_interner(),
    );

    let a = type_cache
        .get("A")
        .expect("A preparation should succeed")
        .expect("A should be present");
    let b = type_cache
        .get("B")
        .expect("B preparation should succeed")
        .expect("B should be present");
    assert!(
        Arc::ptr_eq(&a.name_resolution, &b.name_resolution),
        "non-namespaced type decls of one file must SHARE one base table, \
             not rebuild it per declaration"
    );

    let c = value_cache.get("c").expect("c should be present");
    let d = value_cache.get("d").expect("d should be present");
    assert!(
        Arc::ptr_eq(&c.name_resolution, &d.name_resolution),
        "value decls of one file must SHARE one base table"
    );

    // The two spaces stay distinct tables (their import shadow rules differ).
    assert!(
        !Arc::ptr_eq(&a.name_resolution, &c.name_resolution),
        "type-space and value-space bases are separate tables"
    );

    // A namespaced decl binds declaration-scoped sibling names, so it owns a
    // PRIVATE table — never a mutated view of the shared base.
    let holder = type_cache
        .get("NS.Holder")
        .expect("NS.Holder preparation should succeed")
        .expect("NS.Holder should be present");
    assert!(
        !Arc::ptr_eq(&a.name_resolution, &holder.name_resolution),
        "a namespaced decl owns a private table"
    );
    assert!(
        holder.name_resolution.contains_key("Member"),
        "the private table carries the sibling binding"
    );
    assert!(
        !a.name_resolution.contains_key("Member"),
        "the shared base must NOT leak a declaration-scoped sibling binding"
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
        &test_interner(),
    );

    // One successful demand pins the retained-snapshot lease and commits Var0.
    assert!(
        cache
            .get("Var0")
            .expect("Var0 preparation should succeed")
            .is_some(),
        "Var0 prepares under a live lease"
    );
    assert!(cache.slot_committed_for_test("Var0"));

    // Break the retained snapshot out-of-band: every subsequent body demand
    // now lease-misses (the unreachable-in-practice invariant-violation).
    state.decl_bodies().release_retained_snapshot_for_test();

    assert!(
        cache
            .get("Var1")
            .expect("Var1 lease-miss demand should not fail")
            .is_none(),
        "a broken-lease prepared-type demand fails CLOSED to None (ReturnOnly)"
    );
    assert!(
        !cache.slot_committed_for_test("Var1"),
        "the broken-lease prepared-type demand must NOT commit the write-once \
             slot to None — the false-warm absence the LowerLocator rail already \
             refuses"
    );
}

/// Typed structural preparation failures remain distinct from genuine symbol
/// absence and leave the write-once slot vacant. If the failure were collapsed
/// through `ok().flatten()`, the cache could publish a permanent `None` for a
/// declaration that becomes preparable once its external owner is available.
#[test]
fn prepared_type_failure_is_not_published_as_absence() {
    let source = "import type { External } from './missing';\n\
                  export interface Props { value: External }\n";
    let state = ShallowFileState::service_backed_for_test(source);
    let cache = build_prepared_type_decl_cache(
        "/ws/fixture.ts",
        Arc::clone(&state),
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
        &test_interner(),
    );

    assert!(matches!(
        cache.get("Props"),
        Err(PreparationFailure::MissingExternalOwner { local_name })
            if local_name == "External"
    ));
    assert!(
        !cache.slot_committed_for_test("Props"),
        "a typed preparation failure must leave the declaration slot vacant"
    );
    let builds_after_failure = cache.cold_build_count_for_test();

    assert!(matches!(cache.get("Absent"), Ok(None)));
    assert_eq!(
        cache.cold_build_count_for_test(),
        builds_after_failure,
        "genuine absence is answered without building a declaration slot"
    );

    assert!(matches!(
        cache.get("Props"),
        Err(PreparationFailure::MissingExternalOwner { local_name })
            if local_name == "External"
    ));
    assert!(
        cache.cold_build_count_for_test() > builds_after_failure,
        "a failed slot must retry instead of serving a cached absence"
    );
}

/// The shared TYPE-space name-resolution base is a lookup acceleration, not
/// the declaration dependency frontier. An unresolved import that the
/// declaration never references must therefore be omitted from that base
/// without preventing the exact declaration from preparing or warming.
#[test]
fn unrelated_unresolved_import_does_not_block_strict_type_preparation() {
    let source = "import { computed } from 'vue';\n\
                  export interface SideMenuProps { visible?: boolean }\n\
                  export namespace Menu {\n\
                    export interface NamespacedProps { open?: boolean }\n\
                  }\n";
    let state = ShallowFileState::service_backed_for_test(source);
    let cache = build_prepared_type_decl_cache(
        "/ws/Comp.vue",
        Arc::clone(&state),
        Arc::new(FxHashMap::default()),
        Arc::new(ImportCanonicalization::default()),
        &test_interner(),
    );

    let prepared = cache
        .get("SideMenuProps")
        .expect("an unrelated unresolved import must not fail strict preparation")
        .expect("SideMenuProps is an authored declaration");
    assert!(prepared.member_index.contains_key("visible"));
    assert!(
        cache.slot_committed_for_test("SideMenuProps"),
        "a complete exact declaration must be admitted to its write-once slot"
    );

    let namespaced = cache
        .get("Menu.NamespacedProps")
        .expect("the private namespace table must omit the same unrelated unresolved import")
        .expect("Menu.NamespacedProps is an authored declaration");
    assert!(namespaced.member_index.contains_key("open"));
    assert!(cache.slot_committed_for_test("Menu.NamespacedProps"));
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
        &test_interner(),
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
            prepare_augmentation_type_decl_outcome_in(
                &Arc::from("/ws/fixture.ts"),
                &state,
                &scope,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "A",
                None,
                &ImportCanonicalization::default(),
                &test_interner(),
            ),
            PreparedDeclOutcome::Ready(Some(_))
        ),
        "augmentation symbol A prepares under a live lease"
    );

    state.decl_bodies().release_retained_snapshot_for_test();

    // A DIFFERENT, not-yet-lowered augmentation symbol now lease-misses: the
    // outcome MUST be the distinct LeaseMiss, never a collapsed Ready(None).
    assert!(
        matches!(
            prepare_augmentation_type_decl_outcome_in(
                &Arc::from("/ws/fixture.ts"),
                &state,
                &scope,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "B",
                None,
                &ImportCanonicalization::default(),
                &test_interner(),
            ),
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
        &test_interner(),
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
                .expect("T7 preparation should succeed")
                .expect("T7 should be present")
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
        &test_interner(),
    );

    // Pin the lease with one successful build, then break the snapshot.
    assert!(
        cache
            .get("Var0")
            .expect("Var0 preparation should succeed")
            .is_some(),
        "Var0 prepares under a live lease"
    );
    state.decl_bodies().release_retained_snapshot_for_test();

    assert!(
        cache
            .get("Var1")
            .expect("Var1 lease-miss demand should not fail")
            .is_none(),
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
        cache
            .get("Var1")
            .expect("Var1 retry should not fail")
            .is_none(),
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
        &test_interner(),
    );
    assert!(
        fresh_cache
            .get("Var1")
            .expect("fresh Var1 preparation should succeed")
            .is_some(),
        "under a live lease the symbol recovers — the lease-miss was never a \
             genuine absence"
    );
}

// ---------------------------------------------------------------------------
// Identity interning at the prepared-decl minting boundary
// ---------------------------------------------------------------------------

#[test]
fn prepared_decl_identities_share_the_pool_canonical_allocation() {
    let source = r#"
export interface Props { label: string }
export type Variant = 'solid' | 'outline'
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let interner = Arc::new(crate::identity_interner::IdentityInterner::with_default_budget());
    let bundle = build_prepared_decl_bundle(
        "/src/types.ts",
        Arc::clone(&state),
        FxHashMap::default(),
        FxHashMap::default(),
        ImportCanonicalization::default(),
        &interner,
    );

    let props = bundle
        .prepared_type_decls
        .get("Props")
        .expect("Props preparation should succeed")
        .expect("Props should be present");
    let variant = bundle
        .prepared_type_decls
        .get("Variant")
        .expect("Variant preparation should succeed")
        .expect("Variant should be present");

    // Every identity minted for this file shares ONE canonical-id
    // allocation — the pool's — instead of a fresh String per identity.
    let pooled_canonical = interner.intern("/src/types.ts");
    assert!(Arc::ptr_eq(
        &props.root_identity.canonical_id,
        &pooled_canonical
    ));
    assert!(Arc::ptr_eq(
        &variant.root_identity.canonical_id,
        &pooled_canonical
    ));
    // Cross-decl name_resolution entries share too: Props' view of
    // `Variant` resolves to the same canonical allocation.
    let (key, resolved) = props
        .name_resolution
        .get_key_value("Variant")
        .expect("local sibling resolves");
    assert!(Arc::ptr_eq(&resolved.canonical_id, &pooled_canonical));
    // The map key and the identity's symbol name are ONE allocation for a
    // local symbol (key == resolved symbol).
    assert!(Arc::ptr_eq(key, &resolved.symbol_name));
}

#[test]
fn prepared_value_and_type_caches_share_one_pooled_canonical() {
    let source = r#"
export interface Props { label: string }
export const defaults = { label: 'ok' }
"#;
    let state = ShallowFileState::service_backed_for_test(source);
    let interner = Arc::new(crate::identity_interner::IdentityInterner::with_default_budget());
    let bundle = build_prepared_decl_bundle(
        "/src/types.ts",
        Arc::clone(&state),
        FxHashMap::default(),
        FxHashMap::default(),
        ImportCanonicalization::default(),
        &interner,
    );
    let ty = bundle
        .prepared_type_decls
        .get("Props")
        .expect("Props preparation should succeed")
        .expect("Props should be present");
    let value = bundle
        .prepared_value_decls
        .get("defaults")
        .expect("defaults should be present");
    assert!(
        Arc::ptr_eq(
            &ty.root_identity.canonical_id,
            &value.root_identity.canonical_id
        ),
        "type and value identities of one file share the pooled canonical"
    );
}

#[test]
fn prepared_bundle_partitions_declaration_scope_by_exact_owner() {
    let source = r#"
import type { ModuleOnly } from './module-dep'
interface Shared { module: ModuleOnly }
import type { InstanceOnly } from './instance-dep'
interface Shared { instance: InstanceOnly }
"#;
    let module = verter_type_expr::TopLevelOwnerId::module(0);
    let instance = verter_type_expr::TopLevelOwnerId::instance(0);
    let state = ShallowFileState::service_backed_for_test_with_statement_owners(
        "/src/Fixture.vue",
        source,
        &[module, module, instance, instance],
    );
    let mut dep_edges = FxHashMap::default();
    dep_edges.insert("./module-dep".to_string(), "/src/module-dep.ts".to_string());
    dep_edges.insert(
        "./instance-dep".to_string(),
        "/src/instance-dep.ts".to_string(),
    );
    let mut setup_bindings = FxHashMap::default();
    setup_bindings.insert(
        "SetupGeneric".to_string(),
        TypeParamBinding {
            name: Arc::from("SetupGeneric"),
            ordinal: 0,
            constraint: None,
            default: None,
        },
    );

    let bundle = build_prepared_decl_bundle(
        "/src/Fixture.vue",
        state,
        dep_edges,
        setup_bindings,
        ImportCanonicalization::default(),
        &test_interner(),
    );
    let module_scope = bundle.owner_scope(module).expect("module scope");
    let instance_scope = bundle.owner_scope(instance).expect("instance scope");

    assert!(module_scope.scope_type_names.contains("Shared"));
    assert!(instance_scope.scope_type_names.contains("Shared"));
    assert!(module_scope.import_bindings.contains_key("ModuleOnly"));
    assert!(!module_scope.import_bindings.contains_key("InstanceOnly"));
    assert!(instance_scope.import_bindings.contains_key("InstanceOnly"));
    assert!(!instance_scope.import_bindings.contains_key("ModuleOnly"));
    assert!(module_scope.script_setup_type_bindings.is_empty());
    assert!(instance_scope
        .script_setup_type_bindings
        .contains_key("SetupGeneric"));
    assert!(bundle
        .owner_scope(verter_type_expr::TopLevelOwnerId::instance(1))
        .is_none());
}
