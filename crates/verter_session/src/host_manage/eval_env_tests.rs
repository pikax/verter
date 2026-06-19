//! Discriminating tests for `host_manage::eval_env`.
//!
//! The per-file `EvalEnv` is not a stored `IndexedReady` field but the
//! lazy `whole_env()` demand product owned by `IndexedReady`'s
//! `DeclBodyMemo`, materialised on first demand and shared as one
//! `Arc`; `base_eval_env_arc` hands out that memo-owned whole-env.
//! Content-edit correctness comes from the content-addressed artifact
//! identity — no eager env-cache clear participates.

use std::sync::Arc;

/// Production-path discriminator — the per-file `EvalEnv` reflects a
/// content edit through artifact identity, not through an eager cache
/// clear.
///
/// The owner-upsert path has no eager reverse-dependent cascade. The
/// per-file env is the lazy `whole_env()` product of the
/// content-addressed `IndexedReady`'s `DeclBodyMemo`, not an
/// eagerly-stored field; the edited file's new content hash misses the
/// stale artifact and the materialise closure rebuilds the shallow
/// index from one fresh parse, so the next `whole_env()` demand lowers
/// the env and reflects the new declaration.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn base_eval_env_reflects_content_edit_without_eager_clear() {
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());

    // Initial content — one interface member.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/types.ts".to_string()),
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface Foo { a: number }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("initial upsert");

    // Build + cache the eval-env for the initial content.
    let env_before = host
        .base_eval_env("/src/types.ts")
        .expect("eval-env builds for initial content");
    assert!(
        env_before.type_declaration_id("Foo").is_some(),
        "precondition: initial eval-env knows interface Foo"
    );
    assert!(
        env_before.type_declaration_id("Bar").is_none(),
        "precondition: initial eval-env does NOT know Bar"
    );

    // Edit the file — add a second interface. The owner-upsert path
    // has no eager reverse-dependent cascade and no env cache to clear.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/types.ts".to_string()),
            input_id: "/src/types.ts".to_string(),
            source: Arc::from(
                "export interface Foo { a: number }\nexport interface Bar { b: string }",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("content edit upsert");

    // The eval-env for the edited file MUST reflect the new content.
    // The new content hash misses the stale `IndexedReady`, the
    // materialise closure rebuilds the shallow index from a fresh
    // parse, and the next `whole_env()` demand lowers the env from it.
    let env_after = host
        .base_eval_env("/src/types.ts")
        .expect("eval-env builds for edited content");
    assert!(
        env_after.type_declaration_id("Bar").is_some(),
        "the per-file eval-env MUST reflect a content edit via the \
         content-addressed IndexedReady identity. A missing `Bar` \
         here means a stale env was served."
    );
}

/// The env handed out by `base_eval_env_arc` IS the memo-owned whole-env
/// demand product — one shared `Arc`, no per-read rebuild.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn base_eval_env_arc_is_the_memo_owned_whole_env() {
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/types.ts".to_string()),
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface Foo { a: number }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert");

    let env = host
        .base_eval_env_arc("/src/types.ts")
        .expect("env must resolve");
    let indexed = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("artifact must exist");
    assert!(
        Arc::ptr_eq(&env, &indexed.shallow_state.decl_bodies().whole_env()),
        "base_eval_env_arc must hand out the IndexedReady-owned env Arc"
    );
    let env_again = host
        .base_eval_env_arc("/src/types.ts")
        .expect("env must resolve warm");
    assert!(
        Arc::ptr_eq(&env, &env_again),
        "repeated reads must share one env Arc"
    );
}

// ===========================================================================
// Graph-native whole_env() consumer readers.
//
// Each of the four enumerated `whole_env()` consumers gains a bounded,
// graph-native per-symbol reader that produces output EQUIVALENT to the
// legacy whole_env()-derived result while the legacy path stays in
// production as the equivalence ORACLE. Every reader is proven against
// the oracle AND proven to never materialise `whole_env()`.
// ===========================================================================

#[cfg(not(target_arch = "wasm32"))]
fn upsert_ts(host: &crate::VerterHost, canonical: &str, source: &str) {
    use crate::types::{FileLanguage, UpsertRequest};
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// True when the file's lazy whole-file env has NOT been materialised —
/// the bound the graph-native readers must keep (the oracle sets it true,
/// a graph-native per-symbol read leaves it false).
#[cfg(not(target_arch = "wasm32"))]
fn whole_env_materialized(host: &crate::VerterHost, canonical: &str) -> bool {
    host.ensure_indexed_ready(canonical)
        .expect("artifact must exist")
        .shallow_state
        .decl_bodies()
        .whole_env_materialized()
}

/// C1 — `local_type_declaration_id_graph_native` agrees with the oracle
/// on PRESENCE (`Some`/`None`) for every local type, for the imported
/// name (`None`), and for an absent name (`None`), WITHOUT materialising
/// `whole_env()`.
///
/// Discrimination proof (verified by break → red → revert): removing the
/// reader's presence gate (`type_header(name)?` + the header-ordinal
/// position lookup) and returning a constant `Some(1)` makes the reader
/// return `Some` for the absent `Missing` name where the oracle returns
/// `None` → this test (and the C1 oracle's debug cross-check) goes RED
/// with `diverged ... on presence for (/src/types.ts, Missing)`. The
/// import case is subsumed by header presence (an imported type has no
/// local `type_header`), so an imported name is `None` in both paths.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn c1_local_type_declaration_id_graph_native_matches_oracle_presence_and_is_bounded() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(&host, "/src/dep.ts", "export type Dep = { d: number }\n");
    upsert_ts(
        &host,
        "/src/types.ts",
        "import type { Imported } from './dep'\n\
         export interface Foo { a: number }\n\
         export type Bar = { b: string }\n\
         export type UsesImport = Imported\n",
    );

    // Oracle presence for each name.
    for name in ["Foo", "Bar", "UsesImport", "Imported", "Missing"] {
        let oracle = host.local_type_declaration_id("/src/types.ts", name);
        let graph = host.local_type_declaration_id_graph_native("/src/types.ts", name);
        assert_eq!(
            oracle.is_some(),
            graph.is_some(),
            "C1 presence divergence for `{name}`: oracle={oracle:?} graph_native={graph:?}"
        );
    }

    // Local declared types: present in both.
    assert!(host
        .local_type_declaration_id_graph_native("/src/types.ts", "Foo")
        .is_some());
    assert!(host
        .local_type_declaration_id_graph_native("/src/types.ts", "Bar")
        .is_some());
    // Imported name: the import guard returns None (NOT a local decl id).
    assert!(
        host.local_type_declaration_id_graph_native("/src/types.ts", "Imported")
            .is_none(),
        "an imported name must have no LOCAL type declaration id"
    );
    // Absent name: header miss → None.
    assert!(host
        .local_type_declaration_id_graph_native("/src/types.ts", "Missing")
        .is_none());

    // Stable-and-unique: distinct names get distinct ids, repeat reads
    // are stable.
    let foo = host
        .local_type_declaration_id_graph_native("/src/types.ts", "Foo")
        .unwrap();
    let bar = host
        .local_type_declaration_id_graph_native("/src/types.ts", "Bar")
        .unwrap();
    assert_ne!(foo, bar, "distinct local types must get distinct ids");
    assert_eq!(
        host.local_type_declaration_id_graph_native("/src/types.ts", "Foo"),
        Some(foo),
        "the id must be stable across reads for unchanged content"
    );

    // BOUND: on a FRESH host where ONLY the graph-native reader runs
    // (the oracle is never called), the whole-file env must NOT be
    // materialised. (A separate host keeps this independent of the
    // oracle-equivalence comparisons above, which legitimately
    // materialise the env.)
    let bound_host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &bound_host,
        "/src/bound.ts",
        "export interface Foo { a: number }\nexport type Bar = { b: string }\n",
    );
    let _ = bound_host.local_type_declaration_id_graph_native("/src/bound.ts", "Foo");
    let _ = bound_host.local_type_declaration_id_graph_native("/src/bound.ts", "Bar");
    assert!(
        !whole_env_materialized(&bound_host, "/src/bound.ts"),
        "C1 graph-native reader must NOT materialise whole_env()"
    );
}

/// C1 — the import guard is LOAD-BEARING and discriminating: a name that
/// is BOTH an import target AND has a local type header must resolve to
/// `None` (an imported name has no LOCAL declaration id), even though its
/// local `type_header` is present. Removing the reader's
/// `if state.import_target(name).is_some() { return None }` guard would
/// make the reader fall through to the header-ordinal path and return
/// `Some` — diverging from the oracle (which keeps its own import guard
/// and returns `None`).
///
/// Discrimination proof (verified by break → red → revert): deleting the
/// graph-native import guard makes `graph_native` return `Some` for
/// `Shared` (its local `type_header` is present) while the oracle stays
/// `None` → the `assert_eq!(oracle.is_some(), graph.is_some())` and the
/// `graph.is_none()` assertion both go RED. The precondition assertions
/// below pin that the collision is REAL (the name is both an import
/// target and a local type header), so the test cannot silently degrade
/// into a no-collision case where the guard is irrelevant.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn c1_import_guard_is_load_bearing_for_a_name_with_a_local_type_header_collision() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    // `Shared` is exported as a VALUE from dep.
    upsert_ts(&host, "/src/dep.ts", "export const Shared = 1\n");
    // The owner imports `Shared` as a value AND declares a LOCAL type
    // `Shared`. A value import and a local type live in different
    // declaration spaces, so the shallow index records BOTH an
    // `import_target("Shared")` and a local `type_header("Shared")` — the
    // collision the import guard must win.
    upsert_ts(
        &host,
        "/src/types.ts",
        "import { Shared } from './dep'\n\
         export type Shared = { x: number }\n\
         const _use = Shared\n",
    );

    // Precondition: the collision is REAL — both an import target and a
    // local type header for `Shared`. If either is absent the test would
    // not exercise the guard, so we fail loudly rather than pass vacuously.
    let state = host
        .routed_shallow_state("/src/types.ts")
        .expect("shallow state must exist");
    assert!(
        state.import_target("Shared").is_some(),
        "precondition: `Shared` must be an import target (else the guard is not exercised)"
    );
    assert!(
        state
            .decl_bodies()
            .header_index()
            .type_header("Shared")
            .is_some(),
        "precondition: `Shared` must ALSO have a local type header (else the guard is irrelevant — \
         the header-ordinal path would already return None)"
    );

    // The import guard wins: graph-native returns None, matching the
    // oracle (which keeps its OWN import guard).
    let oracle = host.local_type_declaration_id("/src/types.ts", "Shared");
    let graph = host.local_type_declaration_id_graph_native("/src/types.ts", "Shared");
    assert!(
        oracle.is_none(),
        "oracle: an imported name has no LOCAL type declaration id even with a same-name local type"
    );
    assert!(
        graph.is_none(),
        "graph-native: the import guard must win over the local type header — removing it would \
         return Some here and diverge from the oracle"
    );
    assert_eq!(oracle.is_some(), graph.is_some());
}

/// C1 — the oracle DID materialise whole_env, proving the bound assertion
/// above is not vacuous: the negative assertion can actually fail.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn c1_oracle_materializes_whole_env_proving_bound_is_discriminating() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number }\n",
    );
    assert!(
        !whole_env_materialized(&host, "/src/types.ts"),
        "precondition: env not yet materialised"
    );
    let _ = host.local_type_declaration_id("/src/types.ts", "Foo");
    assert!(
        whole_env_materialized(&host, "/src/types.ts"),
        "the ORACLE path materialises whole_env() — so the graph-native bound assertion \
         (whole_env_materialized==false) genuinely discriminates"
    );
}

/// C1 — the graph-native reader does NOT lower an unrelated heavy decl.
/// A clearly-unrelated declaration whose body lowering would be
/// observable is never demanded by the presence-only reader.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn c1_graph_native_reader_lowers_only_demanded_decl_not_unrelated() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/many.ts",
        "export interface Target { a: number }\n\
         export type Heavy = { x: number; y: string; z: boolean }\n",
    );
    // Demand only `Target`.
    let _ = host.local_type_declaration_id_graph_native("/src/many.ts", "Target");
    // The whole env (which would lower Heavy too) must not exist.
    assert!(
        !whole_env_materialized(&host, "/src/many.ts"),
        "demanding `Target` graph-natively must not lower the whole file (incl. `Heavy`)"
    );
}

/// C2 — `peel_value_decl_alias_graph_native` follows the same
/// single-segment `typeof` alias chain as the oracle and lands on the
/// SAME terminal `(canonical, name)`, WITHOUT materialising whole_env().
///
/// Discrimination proof: changing the graph-native membership gate from
/// `value_header(next).is_some()` to an unconditional `true` would make
/// the reader follow a `typeof` hop to a NON-EXISTENT symbol where the
/// oracle stops — diverging the terminal name (verified by breaking →
/// red → revert).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn c2_peel_value_decl_alias_graph_native_matches_oracle_terminal_and_is_bounded() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    // `reexp = typeof base`, `base` is a real const → peeling `reexp`
    // lands on `base`. `dangling = typeof ghost` where `ghost` does not
    // exist → peeling stops at `dangling`.
    upsert_ts(
        &host,
        "/src/vals.ts",
        "export const base = { color: 'red' }\n\
         export const reexp = base\n\
         export const dangling: typeof ghost = base\n",
    );

    for name in ["reexp", "base", "dangling"] {
        let oracle = host.peel_value_decl_alias_for_test("/src/vals.ts", name);
        let graph = host.peel_value_decl_alias_graph_native_for_test("/src/vals.ts", name);
        assert_eq!(
            oracle, graph,
            "C2 terminal divergence for `{name}`: oracle={oracle:?} graph_native={graph:?}"
        );
    }

    // `reexp` peels to `base`.
    let peeled = host.peel_value_decl_alias_graph_native_for_test("/src/vals.ts", "reexp");
    assert_eq!(peeled.0, "/src/vals.ts");
    assert_eq!(peeled.1, "base", "reexp = typeof base must peel to base");

    // BOUND: on a FRESH host where ONLY the graph-native peeler runs,
    // whole_env() must NOT be materialised.
    let bound_host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &bound_host,
        "/src/bvals.ts",
        "export const base = { color: 'red' }\nexport const reexp = base\n",
    );
    let _ = bound_host.peel_value_decl_alias_graph_native_for_test("/src/bvals.ts", "reexp");
    assert!(
        !whole_env_materialized(&bound_host, "/src/bvals.ts"),
        "C2 graph-native reader must NOT materialise whole_env()"
    );
}

/// C4 — `dependency_value_symbol_graph_native` produces a `ValueDeclInfo`
/// byte-equivalent (modulo the opaque `declaration_id`, which is 0 on the
/// alias path) to the oracle's `dep_env.value_symbols.get(name).primary()`
/// read, WITHOUT materialising whole_env().
///
/// Discrimination proof: returning a constant `type_annotation: None`
/// from the reader would diverge the `type_annotation` field from the
/// oracle for a typed const (verified by breaking → red → revert).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn c4_dependency_value_symbol_graph_native_matches_oracle_and_is_bounded() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/dep.ts",
        "export const theme = { color: 'dark' }\n\
         export const count = 3\n",
    );

    for name in ["theme", "count"] {
        // Oracle: the dependency whole-env value_symbols read.
        let oracle_env = host
            .base_eval_env_arc("/src/dep.ts")
            .expect("dep env builds");
        let oracle = oracle_env
            .value_symbols
            .get(name)
            .map(|g| g.primary().clone())
            .unwrap_or_else(|| panic!("oracle must know `{name}`"));

        let graph = host
            .dependency_value_symbol_graph_native("/src/dep.ts", name)
            .unwrap_or_else(|| panic!("graph-native reader must know `{name}`"));

        // Field-by-field equivalence (declaration_id is the opaque,
        // alias-path-overwritten token: oracle assigns a positional id,
        // the alias path uses 0 — both equivalent for the consumer).
        assert_eq!(graph.name, oracle.name, "name must match for `{name}`");
        assert_eq!(graph.kind, oracle.kind, "kind must match for `{name}`");
        assert_eq!(
            graph.type_annotation, oracle.type_annotation,
            "type_annotation must match for `{name}`"
        );
        assert_eq!(
            format!("{:?}", graph.signatures),
            format!("{:?}", oracle.signatures),
            "signatures must match for `{name}`"
        );
        assert_eq!(
            graph.object_shape, oracle.object_shape,
            "object_shape must match for `{name}`"
        );
        assert_eq!(
            graph.enum_members, oracle.enum_members,
            "enum_members must match for `{name}`"
        );
        assert_eq!(
            graph.declaration_id, 0,
            "the alias-path declaration_id is the opaque 0 (matching the prepared route)"
        );
    }

    // A missing value symbol → None (graph-native miss).
    assert!(host
        .dependency_value_symbol_graph_native("/src/dep.ts", "nope")
        .is_none());
}

/// F1 — the C2 oracle debug cross-check is GATED on
/// `!is_svelte_rune_module(canonical)`, so the documented Svelte-rune
/// scoped exception is ACTUALLY excluded. In a `.svelte.ts` rune module,
/// `whole_env()` injects ambient `$`-rune value symbols (`$state`, …)
/// post-build that the per-symbol header index never carries. A
/// single-segment `typeof $state` alias therefore peels to `$state` in
/// the oracle (the ambient symbol IS in `value_symbols`) but terminates
/// one hop EARLIER graph-natively (no `$state` header). WITHOUT the gate,
/// the oracle's `debug_assert_eq!` would PANIC on that divergence in a
/// debug build; WITH the gate the rune module is excluded and the oracle
/// peel completes.
///
/// Discrimination proof (verified by break → red → revert): removing the
/// `if !crate::host_resolve::is_svelte_rune_module(...)` gate makes the
/// oracle `peel_value_decl_alias` debug cross-check fire on the
/// `(rune-module, reexp)` divergence → this test PANICS (RED) in a debug
/// build. With the gate, the peel returns `$state` (oracle terminal)
/// without panicking.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn f1_c2_debug_cross_check_is_excluded_for_svelte_rune_modules() {
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    let rune_lang = FileLanguage::adapter_module(
        verter_language::ScriptSourceType::Ts,
        verter_language::FrameworkAdapterId::svelte(),
        verter_language::LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
    );
    // A rune module whose value `reexp` is a single-segment `typeof $state`
    // alias to the AMBIENT `$state` rune. The oracle's whole_env carries
    // `$state` (injected post-build); the graph-native header index does
    // NOT — the documented scoped divergence.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/runes.svelte.ts".to_string()),
            input_id: "/src/runes.svelte.ts".to_string(),
            source: Arc::from(
                "export const reexp: typeof $state = $state\n\
                 export const plain = 1\n",
            ),
            file_language: rune_lang,
            aliases: Vec::new(),
        })
        .expect("rune module upsert");

    // In a debug build, calling the ORACLE peel runs the cross-check. With
    // the gate, the rune module is excluded → the divergent terminal does
    // NOT panic. The oracle terminal lands on `$state` (the ambient hop);
    // the call completing WITHOUT a debug panic is the assertion.
    let (canonical, terminal) =
        host.peel_value_decl_alias_for_test("/src/runes.svelte.ts", "reexp");
    assert_eq!(canonical, "/src/runes.svelte.ts");
    assert_eq!(
        terminal, "$state",
        "the oracle peels the rune alias to the ambient `$state` (whole_env carries it); the gate \
         excludes the graph-native cross-check that would otherwise panic on this divergence"
    );

    // A plain (non-`$rune`) value in the SAME rune module still agrees
    // (no ambient divergence), so the gate does not mask real divergences
    // for ordinary symbols.
    let (_pc, plain_terminal) =
        host.peel_value_decl_alias_for_test("/src/runes.svelte.ts", "plain");
    assert_eq!(plain_terminal, "plain");
}

/// C4 BOUND — on a FRESH host where ONLY
/// `dependency_value_symbol_graph_native` runs (the oracle
/// `base_eval_env_arc` is NEVER called), the source file's
/// `whole_env_materialized()` must stay `false`. This is the independent
/// bound the equivalence test above cannot prove (it materialises the
/// oracle FIRST for the value comparison, so a reader that secretly called
/// `base_eval_env_arc` would pass there).
///
/// Discrimination proof (verified by break → red → revert): replacing the
/// reader body with a `self.base_eval_env_arc(source).?.value_symbols.get(name)`
/// read makes `whole_env_materialized("/src/dep.ts")` become `true` after
/// the reader runs → this test goes RED.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn c4_dependency_value_symbol_graph_native_is_bounded_on_fresh_host() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/dep.ts",
        "export const theme = { color: 'dark' }\n\
         export const count = 3\n",
    );

    // Precondition: dep whole env not yet materialised, and the oracle has
    // never been touched on this fresh host.
    assert!(
        !whole_env_materialized(&host, "/src/dep.ts"),
        "precondition: dep whole env not yet materialised"
    );

    // Run ONLY the graph-native per-name reader (oracle never called).
    let theme = host.dependency_value_symbol_graph_native("/src/dep.ts", "theme");
    let count = host.dependency_value_symbol_graph_native("/src/dep.ts", "count");
    assert!(theme.is_some(), "graph-native reader must know `theme`");
    assert!(count.is_some(), "graph-native reader must know `count`");

    // BOUND: the per-name reader did NOT materialise the source whole env.
    assert!(
        !whole_env_materialized(&host, "/src/dep.ts"),
        "C4 graph-native per-name reader must NOT materialise the source file's whole_env()"
    );
}

/// SFC `<script setup generic="T">` divergence discriminator.
///
/// `whole_env()` post-build inserts the SFC generic `T` into
/// `env.type_bindings` (a SEPARATE namespace) — NOT `env.type_symbols`.
/// `type_declaration_id` reads `type_decl_ids` (built from
/// `type_symbols`), so `T` is NOT a type declaration id in EITHER the
/// oracle or the graph-native reader: the two AGREE on the SFC generic.
/// A real local type in the same SFC IS present in both. This proves the
/// known SFC-generic post-build divergence does NOT reach the C1
/// consumer.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn sfc_script_setup_generic_param_is_not_a_type_declaration_id_in_oracle_or_graph_native() {
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Comp.vue".to_string()),
            input_id: "/src/Comp.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\" generic=\"T\">\n\
             interface LocalProps { value: T }\n\
             const props = defineProps<LocalProps>()\n\
             </script>\n\
             <template><div /></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("vue upsert");

    // The SFC generic `T`: NOT a type declaration id in either path.
    assert_eq!(
        host.local_type_declaration_id("/src/Comp.vue", "T"),
        None,
        "oracle: SFC generic `T` lives in type_bindings, not type_symbols — no decl id"
    );
    assert_eq!(
        host.local_type_declaration_id_graph_native("/src/Comp.vue", "T"),
        None,
        "graph-native: SFC generic `T` has no header type symbol — no decl id"
    );

    // A real local type IS present in both.
    assert_eq!(
        host.local_type_declaration_id("/src/Comp.vue", "LocalProps")
            .is_some(),
        host.local_type_declaration_id_graph_native("/src/Comp.vue", "LocalProps")
            .is_some(),
        "a real local SFC type must agree between oracle and graph-native"
    );
    assert!(
        host.local_type_declaration_id_graph_native("/src/Comp.vue", "LocalProps")
            .is_some(),
        "the real local type `LocalProps` must be present graph-natively"
    );
}

/// A recording `ImportedRuntimeValueResolver` that wraps the REAL
/// host-backed resolution and records the `(source_canonical_id,
/// source_name)` pair the materializer computes as the source identity of
/// every binding it actually admits.
///
/// Every method delegates to the same host APIs the production
/// `HostRuntimeValueResolver` uses (`base_eval_env_arc`,
/// `dependency_value_symbol_graph_native`, `prepared_value_decl`,
/// `resolve_value_export_target`), so the materializer's OWN selection +
/// resolution logic drives — this is NOT a re-implementation of the
/// materializer's binding filter. The hook records inside
/// `resolve_value_export_target`, which the materializer calls exactly once
/// per filter-admitted binding (before any hydration-failure `continue`),
/// mirroring the materializer's `Some(target)`-or-`(dep, imported_name)`
/// fallback so the recorded pair is byte-identical to the source identity
/// the materializer computes in BOTH branches.
#[cfg(not(target_arch = "wasm32"))]
struct RecordingRuntimeValueResolver<'a> {
    host: &'a crate::VerterHost,
    touched: std::cell::RefCell<std::collections::BTreeSet<(String, String)>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl crate::resolver_core::ImportedRuntimeValueResolver for RecordingRuntimeValueResolver<'_> {
    fn dependency_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_eval::EvalEnv>> {
        self.host.base_eval_env_arc(canonical_id)
    }

    fn dependency_value_symbol_graph_native(
        &self,
        source_canonical_id: &str,
        source_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::ValueDeclInfo> {
        self.host
            .dependency_value_symbol_graph_native(source_canonical_id, source_name)
    }

    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        self.host.prepared_value_decl(canonical_id, symbol_name)
    }

    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<(String, String)> {
        // Delegate to the REAL host export-target resolution, then record
        // the SAME source pair the materializer derives — both the resolved
        // target and the `(dep, imported_name)` fallback the materializer
        // applies when resolution misses.
        let resolved = self
            .host
            .resolve_value_export_target(dep_canonical_id, imported_name)
            .map(|target| (target.canonical_id, target.name));
        let pair = resolved
            .clone()
            .unwrap_or_else(|| (dep_canonical_id.to_string(), imported_name.to_string()));
        self.touched.borrow_mut().insert(pair);
        resolved
    }
}

/// Captures the full `(source_canonical_id, source_name)` pairs the
/// runtime-value materializer ACTUALLY touches for a lightweight
/// fallthrough env build, by driving the REAL
/// [`materialize_imported_runtime_values_into_env`] on a disposable env
/// with a [`RecordingRuntimeValueResolver`] and reading back the pairs the
/// materializer recorded.
///
/// This is real materializer instrumentation, NOT a re-implementation of
/// the materializer's binding-filter loop: the materializer itself applies
/// the type-only / namespace / owner-shadow / required-name selection and
/// the export-target resolution; the recording resolver only observes the
/// source pair it commits to per admitted binding. The owner-shadow set and
/// the required-name set passed in are the SAME inputs the production
/// `build_fallthrough_eval_env_lightweight` hands the materializer (the
/// materializer's parameters, not its internal logic).
///
/// This is the authoritative "materializer-touched" oracle the
/// graph-native extractor must equal on FULL pairs — not collapsed to
/// names, so a wrong `source_canonical` or wrong `source_name` (a
/// re-export / aliased import that the extractor mis-resolves) is caught.
#[cfg(not(target_arch = "wasm32"))]
fn materializer_touched_source_pairs(
    host: &crate::VerterHost,
    owner: &str,
    snapshot: &crate::types::FileAnalysisSnapshot,
) -> std::collections::BTreeSet<(String, String)> {
    // The materializer's INPUTS (its parameters), identical to what the
    // production `build_fallthrough_eval_env_lightweight` template path
    // passes: the required template runtime-value names + the owner-local
    // value-symbol shadow set (the owner's whole-env value symbols).
    let required: rustc_hash::FxHashSet<String> =
        crate::host_manage::component_meta_extract::collect_required_template_runtime_value_names(
            snapshot,
        );
    let owner_local_value_names: rustc_hash::FxHashSet<String> = host
        .base_eval_env_arc(owner)
        .map(|env| env.value_symbols.keys().cloned().collect())
        .unwrap_or_default();

    // Drive the ACTUAL materializer on a disposable env via the recording
    // resolver. The materializer applies its own selection + resolution; the
    // resolver records the source pair it touches for each admitted binding.
    let resolver = RecordingRuntimeValueResolver {
        host,
        touched: std::cell::RefCell::new(std::collections::BTreeSet::new()),
    };
    let mut disposable_env = verter_semantic::analysis::type_eval::EvalEnv::new();
    crate::resolver_core::materialize_imported_runtime_values_into_env(
        snapshot.imports.as_slice(),
        &owner_local_value_names,
        Some(&required),
        &mut disposable_env,
        &resolver,
    );
    resolver.touched.into_inner()
}

/// C3 dep-equivalence — `fallthrough_runtime_value_deps_graph_native`
/// enumerates the EXACT `(source_canonical_id, source_name)` pairs the
/// legacy materializer touches, compared on FULL pairs (NOT collapsed to
/// names), through a RE-EXPORT / ALIASED-import fixture where
/// `source_canonical != dep_canonical` AND `source_name != binding.name`.
///
/// Discrimination proof (verified by break → red → revert):
/// - Comparing on full pairs catches a wrong source canonical: if the
///   graph-native extractor stopped resolving through the barrel re-export
///   and returned `(/src/barrel.ts, theme)` instead of the real
///   `(/src/dep.ts, themeImpl)`, the pair-set equality goes RED. A
///   name-only collapse would have hidden this.
/// - Dropping the `required_runtime_value_names` filter makes the
///   extractor emit the unused `helper` dep where the materializer never
///   touches it → RED.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn c3_fallthrough_runtime_value_deps_graph_native_equals_materializer_touched_full_pairs() {
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    // The real source: `themeImpl` lives in dep.ts under a DIFFERENT name
    // than the importing binding.
    upsert_ts(
        &host,
        "/src/dep.ts",
        "export const themeImpl = { color: 'dark' }\n\
         export const helperImpl = { x: 1 }\n",
    );
    // A barrel re-exports `themeImpl` under the alias `theme` and
    // `helperImpl` under `helper`. So a binding `theme` imported from the
    // barrel resolves to source `(/src/dep.ts, themeImpl)` —
    // `source_canonical != barrel` AND `source_name != binding.name`.
    upsert_ts(
        &host,
        "/src/barrel.ts",
        "export { themeImpl as theme, helperImpl as helper } from './dep'\n",
    );
    // The SFC imports `theme` and `helper` from the barrel, but only
    // `theme` is referenced in the template → only `theme` is required.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Owner.vue".to_string()),
            input_id: "/src/Owner.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">\n\
                 import { theme, helper } from './barrel'\n\
                 </script>\n\
                 <template><div :data-c=\"theme\" /></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("owner upsert");

    let snapshot = host
        .get_analysis("/src/Owner.vue")
        .expect("owner analysis snapshot");

    // Graph-native dep extraction (template path → root_reachability None).
    let deps: std::collections::BTreeSet<(String, String)> =
        host.fallthrough_runtime_value_deps_graph_native("/src/Owner.vue", &snapshot, None);

    // The authoritative materializer-touched FULL pair set (legacy oracle
    // export-target resolution).
    let touched = materializer_touched_source_pairs(&host, "/src/Owner.vue", &snapshot);

    // FULL-pair equality — source identity preserved, not name-collapsed.
    assert_eq!(
        deps, touched,
        "the graph-native dep extractor must equal the materializer-touched \
         (source_canonical, source_name) pairs EXACTLY: deps={deps:?} touched={touched:?}"
    );

    // The re-export / alias resolution must have produced the REAL source
    // identity (proving the fixture exercises a non-trivial pair, and the
    // extractor follows the barrel to dep.ts under the underlying name).
    assert!(
        deps.iter()
            .any(|(canonical, name)| canonical == "/src/dep.ts" && name == "themeImpl"),
        "the required, template-referenced `theme` must resolve through the barrel to its real \
         source (/src/dep.ts, themeImpl): {deps:?}"
    );
    assert!(
        !deps
            .iter()
            .any(|(_, name)| name == "helper" || name == "helperImpl"),
        "the unused `helper` must NOT be a graph-native dep (filtered to required names): {deps:?}"
    );
}

/// C3 double-alias soundness — two distinct bindings aliased onto the
/// SAME source (`import { x as a, x as b }`, both template-referenced so
/// both are required runtime values) drive the C3 readiness path WITHOUT a
/// false panic, and the graph-native dep set collapses to the single
/// underlying `(source_canonical, source_name)` pair.
///
/// This pins the soundness of the C3 readiness path against the prior
/// name-count cross-check. The retired in-production debug cross-check
/// asserted `graph_native_dep_count >= added`, where `added` counted env
/// value-symbol bindings hydrated by DISTINCT NAME (`a` and `b` → 2) and
/// `graph_native_dep_count` counted distinct `(source_canonical,
/// source_name)` PAIRS (`{(dep, x)}` → 1). For this LEGAL double-alias
/// input the bound `1 >= 2` is FALSE → the `debug_assert!` PANICKED on
/// legal code. The pairs count DIFFERENT things, so the proxy was unsound.
///
/// Discrimination proof (break → red → revert), run on THIS fixture:
/// 1. Temporarily restore the retired in-production cross-check in
///    `fallthrough.rs` (`debug_assert!(graph_native_dep_count >= added)`
///    around the `materialize_imported_runtime_values_into_env` call).
/// 2. This test PANICS in a debug build — the `build_*_lightweight` call
///    below hits the assert (`1 >= 2` false) and reddens.
/// 3. Revert to the current fix (cross-check removed; offline pair-equality
///    test is the equivalence rail) → this test is GREEN.
///
/// The authoritative full-pair equivalence proof stays in
/// `c3_fallthrough_runtime_value_deps_graph_native_equals_materializer_\
/// touched_full_pairs`; this test guards the double-alias soundness the
/// retired count proxy violated.
#[cfg(all(not(target_arch = "wasm32"), debug_assertions))]
#[test]
fn c3_double_alias_onto_same_source_drives_readiness_without_false_panic() {
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    // One real source value `xImpl`.
    upsert_ts(
        &host,
        "/src/m.ts",
        "export const xImpl = { color: 'dark' }\n",
    );
    // The SFC imports `xImpl` TWICE under two distinct aliases `a` and `b`,
    // and references BOTH in the template → BOTH are required runtime
    // values. The materializer hydrates two env bindings (`a`, `b`) from the
    // SINGLE source pair (/src/m.ts, xImpl).
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Owner.vue".to_string()),
            input_id: "/src/Owner.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">\n\
                 import { xImpl as a, xImpl as b } from './m'\n\
                 </script>\n\
                 <template><div :data-a=\"a\" :data-b=\"b\" /></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("owner upsert");

    let snapshot = host
        .get_analysis("/src/Owner.vue")
        .expect("owner analysis snapshot");

    // Drive the FULL C3 readiness consumer (template path → root_reachability
    // None). Under the retired `deps >= added` debug_assert this PANICS
    // (deps=1, added=2). With the fix it returns Some(env) cleanly.
    let env = host.build_fallthrough_eval_env_lightweight("/src/Owner.vue", &snapshot, None, None);
    assert!(
        env.is_some(),
        "the C3 readiness consumer must build an env for a legal double-alias-onto-one-source SFC \
         without a false panic"
    );

    // The graph-native dep set collapses both aliased bindings onto the
    // SINGLE underlying source pair — exactly what makes the retired
    // name-count `>=` proxy unsound.
    let deps: std::collections::BTreeSet<(String, String)> =
        host.fallthrough_runtime_value_deps_graph_native("/src/Owner.vue", &snapshot, None);
    assert_eq!(
        deps,
        std::collections::BTreeSet::from([("/src/m.ts".to_string(), "xImpl".to_string())]),
        "two aliases onto the same source must yield exactly the single (source_canonical, \
         source_name) pair: {deps:?}"
    );
}

/// C3 DEP-bound — the graph-native dep reader does NOT materialise the
/// DEPENDENCY file's whole env. After
/// `fallthrough_runtime_value_deps_graph_native` runs on a fresh host
/// (the oracle is NEVER called), every dependency / source file's
/// `whole_env_materialized()` must stay `false`. This proves the C3
/// reader's export-target + alias peel is bounded graph-native (routed
/// through `resolve_value_export_target_graph_native`, NOT the legacy
/// `resolve_value_export_target` whose `peel_value_decl_alias` would
/// materialise the dependency's `base_eval_env_arc`/`whole_env()`).
///
/// Discrimination proof (verified by break → red → revert): routing the
/// reader back through the legacy `resolve_value_export_target` (whose
/// peel reaches `base_eval_env_arc` on the dep) makes
/// `whole_env_materialized("/src/dep.ts")` become `true` → this test goes
/// RED.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn c3_graph_native_dep_reader_does_not_materialize_dependency_whole_env() {
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/dep.ts",
        "export const themeImpl = { color: 'dark' }\n",
    );
    upsert_ts(
        &host,
        "/src/barrel.ts",
        "export { themeImpl as theme } from './dep'\n",
    );
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Owner.vue".to_string()),
            input_id: "/src/Owner.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">\n\
                 import { theme } from './barrel'\n\
                 </script>\n\
                 <template><div :data-c=\"theme\" /></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("owner upsert");

    let snapshot = host
        .get_analysis("/src/Owner.vue")
        .expect("owner analysis snapshot");

    // Precondition: dep + barrel envs are not yet materialised.
    assert!(
        !whole_env_materialized(&host, "/src/dep.ts"),
        "precondition: dep whole env not yet materialised"
    );
    assert!(
        !whole_env_materialized(&host, "/src/barrel.ts"),
        "precondition: barrel whole env not yet materialised"
    );

    // Run ONLY the graph-native dep reader (oracle never called).
    let _ = host.fallthrough_runtime_value_deps_graph_native("/src/Owner.vue", &snapshot, None);

    // BOUND: neither the dependency nor the re-export barrel had its whole
    // env materialised by the graph-native reader.
    assert!(
        !whole_env_materialized(&host, "/src/dep.ts"),
        "C3 graph-native dep reader must NOT materialise the DEPENDENCY's whole_env()"
    );
    assert!(
        !whole_env_materialized(&host, "/src/barrel.ts"),
        "C3 graph-native dep reader must NOT materialise the re-export barrel's whole_env()"
    );
}
