//! Current-content-pinned `shallow_file_state` discriminators.
//!
//! `shallow_file_state` is the canonical shallow-type-file-state reader
//! the frontier engine and the provenance-pure signature builders draw
//! the observed content identity from. Pre-fix it read `IndexedReady`
//! through `FileArtifactStore::get_any` — a content-agnostic,
//! canonical-only lookup. The own-canonical drain at upsert masked the
//! defect (`evict_canonical` → `indexed.remove` deletes the stale
//! artifact); under the skip-own-drain hook the stale artifact lingers
//! and `get_any` surfaces it, feeding a stale observed-content hash to
//! every signature builder. Self-version-rooting is defeated at the
//! root: the builders are provenance-pure but the version they are fed
//! is stale.
//!
//! Post-fix `shallow_file_state` resolves the canonical's authoritative
//! current content hash and reads `FileArtifactStore` pinned to that
//! hash. A stale older-content artifact yields a miss; the read either
//! returns `None` for a live scheduler-tracked canonical or falls
//! through to a content-pinned route-owned fallback. It never
//! materialises (`ensure_indexed_ready` is the recursion the function's
//! own comment guards against).
//!
//! Discrimination property of every test below: the mutation is driven
//! through [`crate::VerterHost::upsert_skipping_own_canonical_drain_for_tests`]
//! so the stale `IndexedReady` physically survives in
//! `FileArtifactStore`. A pre-fix `get_any` read returns that stale
//! artifact (asserted RED); a content-pinned read observes the edited
//! content (asserted GREEN). The assertions are on observed hash-level
//! and shallow-symbol freshness — never on physical cache emptiness.

use std::sync::Arc;

use crate::{FileKind, HostConfig, UpsertRequest, VerterHost};

/// Upsert a file through the plain [`VerterHost::upsert`] path (runs
/// the own-canonical drain — production semantics).
fn upsert_plain(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::from_path(canonical),
            aliases: Vec::new(),
        })
        .expect("plain upsert succeeds");
}

/// Edit a file through the skip-own-canonical-drain hook — runs the
/// full upsert pipeline but suppresses the post-commit own-canonical
/// query-identity cache drain, so the pre-edit `IndexedReady` survives
/// in `FileArtifactStore` while the scheduler tracks the new content.
fn upsert_skipping_drain(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert_skipping_own_canonical_drain_for_tests(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::from_path(canonical),
            aliases: Vec::new(),
        })
        .expect("skip-drain upsert succeeds");
}

/// Core deliverable-A discriminator — a skip-drain edit of a directly
/// resolvable `.ts` dependency must make `shallow_file_state` return the
/// CURRENT content's shallow state, not the stale pre-edit one.
///
/// - **Pre-fix tree:** `shallow_file_state` reads `get_any`. The
///   skip-drain edit left the pre-edit `IndexedReady` in
///   `FileArtifactStore`; `get_any` returns it, so the shallow state's
///   `whole_hash` is the pre-edit hash and the new `Renamed` type
///   symbol is absent. Both assertions FAIL.
/// - **Post-fix tree:** `shallow_file_state` resolves the authoritative
///   current content hash (the scheduler's post-edit
///   `parse.whole_hash`) and reads `FileArtifactStore` pinned to it. The
///   stale artifact misses; the content-pinned read (or route-owned
///   fallback) observes the edited content, so the `whole_hash` is the
///   post-edit hash and `Renamed` is present.
#[test]
fn shallow_file_state_observes_current_content_after_skip_drain_edit() {
    let canonical = "/pinned_shallow/dep.ts";
    let host = VerterHost::new_standalone(HostConfig::default());

    // Seed the dependency and materialise its `IndexedReady` so a real
    // pre-edit artifact lives in `FileArtifactStore` — that artifact is
    // the one the skip-drain edit must NOT leave masking the new
    // content.
    upsert_plain(
        &host,
        canonical,
        "export interface Original { a: number; }\n",
    );
    let pre_edit = host
        .ensure_indexed_ready(canonical)
        .expect("pre-edit IndexedReady must materialise for the seeded dep");
    let pre_edit_hash = pre_edit.whole_hash;
    assert!(
        pre_edit.shallow_state.symbols.contains_key("Original"),
        "fixture invariant: the pre-edit shallow state must expose the \
         `Original` type symbol — got {:?}",
        pre_edit.shallow_state.symbols.keys().collect::<Vec<_>>()
    );

    // Edit the dependency through the skip-own-drain hook: the pre-edit
    // `IndexedReady` survives in `FileArtifactStore`, while the
    // scheduler tracks the new content.
    let edited = "export interface Renamed { b: string; }\n";
    upsert_skipping_drain(&host, canonical, edited);

    // Fixture invariant: the pre-edit artifact genuinely lingers — a
    // permissive `get_any` (the pre-fix read shape) still returns it.
    let lingering = host
        .project_type_store()
        .indexed()
        .get_any(canonical)
        .expect("skip-drain upsert must leave the pre-edit IndexedReady in FileArtifactStore");
    assert_eq!(
        lingering.whole_hash, pre_edit_hash,
        "fixture invariant: the lingering artifact keeps its pre-edit \
         content hash — a pre-fix `get_any` read of `shallow_file_state` \
         would surface THIS stale artifact"
    );

    // The discriminating read.
    let after = host
        .shallow_file_state(canonical)
        .expect("post-edit shallow_file_state must resolve for the edited dep");

    // Discriminating assertion 1 — the observed `whole_hash` is the
    // CURRENT content's hash, not the stale pre-edit hash.
    assert_ne!(
        after.whole_hash, pre_edit_hash,
        "shallow_file_state MUST NOT report the stale pre-edit whole_hash \
         after a skip-drain edit — a pre-fix `get_any` read returns the \
         lingering pre-edit artifact, so its whole_hash is stale. The \
         observed content identity feeds every provenance-pure signature \
         builder; a stale hash defeats self-version-rooting at the root."
    );

    // Discriminating assertion 2 — the observed shallow surface carries
    // the edited content (the `Renamed` symbol), not the stale one.
    assert!(
        after.symbols.contains_key("Renamed"),
        "shallow_file_state MUST observe the edited content — the post-edit \
         shallow surface must expose the `Renamed` type symbol. Got {:?}",
        after.symbols.keys().collect::<Vec<_>>()
    );
    assert!(
        !after.symbols.contains_key("Original"),
        "shallow_file_state MUST NOT observe the stale pre-edit content — \
         the `Original` symbol was renamed away and must be absent. A \
         pre-fix `get_any` read surfaces the stale artifact and still \
         reports `Original`. Got {:?}",
        after.symbols.keys().collect::<Vec<_>>()
    );
}

/// Companion discriminator — a skip-drain edit that *renames* the
/// dependency's type symbol must surface the renamed symbol through
/// `shallow_file_state`, not the stale pre-edit symbol carried by the
/// lingering `IndexedReady`.
///
/// This isolates the discrimination to the shallow *surface* the
/// builders read (`ShallowFileState::symbols`), independent of the
/// outer `IndexedReady::whole_hash`: the lingering artifact's
/// `shallow_state` is the genuine pre-edit surface with the pre-edit
/// `Probe` symbol, while the scheduler tracks the renamed content.
///
/// - **Pre-fix tree:** `shallow_file_state` reads `get_any`, surfaces
///   the lingering pre-edit `IndexedReady`, and returns its
///   `shallow_state` — which still carries `Probe` and lacks
///   `RenamedProbe`. Both symbol assertions FAIL.
/// - **Post-fix tree:** the content-pinned read misses the stale
///   artifact (its content hash is not the scheduler's current hash);
///   the route-owned fallback recomputes the current shallow surface,
///   which carries `RenamedProbe`.
#[test]
fn shallow_file_state_observes_renamed_symbol_after_skip_drain_edit() {
    let canonical = "/pinned_shallow/probe.ts";
    let host = VerterHost::new_standalone(HostConfig::default());

    upsert_plain(
        &host,
        canonical,
        "export interface Probe { kept: number; }\n",
    );
    let pre_edit = host
        .ensure_indexed_ready(canonical)
        .expect("pre-edit IndexedReady must materialise");
    assert!(
        pre_edit.shallow_state.symbols.contains_key("Probe"),
        "fixture invariant: the pre-edit shallow surface must expose `Probe`"
    );

    // Skip-drain edit: rename the exported interface. The pre-edit
    // `IndexedReady` (carrying the `Probe` shallow surface) survives in
    // `FileArtifactStore`; the scheduler tracks the renamed content.
    let edited = "export interface RenamedProbe { kept: number; }\n";
    upsert_skipping_drain(&host, canonical, edited);

    // Fixture invariant: the pre-edit artifact's shallow surface still
    // lingers — its `shallow_state` carries the pre-edit `Probe` symbol.
    let lingering = host
        .project_type_store()
        .indexed()
        .get_any(canonical)
        .expect("skip-drain upsert must leave the pre-edit IndexedReady in FileArtifactStore");
    assert!(
        lingering.shallow_state.symbols.contains_key("Probe"),
        "fixture invariant: the lingering artifact's shallow surface still \
         carries the pre-edit `Probe` symbol — a pre-fix `get_any` read of \
         `shallow_file_state` would surface THIS stale surface"
    );

    // The discriminating read.
    let observed = host
        .shallow_file_state(canonical)
        .expect("post-edit shallow_file_state must resolve via the content-pinned path");
    assert!(
        observed.symbols.contains_key("RenamedProbe"),
        "shallow_file_state MUST observe the renamed content — the \
         post-edit shallow surface must expose `RenamedProbe`. A pre-fix \
         `get_any` read surfaces the lingering pre-edit artifact's surface \
         (only `Probe`). Got {:?}",
        observed.symbols.keys().collect::<Vec<_>>()
    );
    assert!(
        !observed.symbols.contains_key("Probe"),
        "shallow_file_state MUST NOT surface the stale pre-edit `Probe` \
         symbol — it was renamed away. Got {:?}",
        observed.symbols.keys().collect::<Vec<_>>()
    );
}

// ── Block 2 canary scenarios under the skip-own-drain hook ──────────
//
// The two scenarios below mirror `block_2_canary_component_meta.rs`'s
// `imported_prop_type_edit_misses_warm_component_meta` and
// `route_surface_dep_edit_misses_warm_component_meta`, but drive the
// dependency edit through `upsert_skipping_own_canonical_drain_for_tests`.
// Block 2.S-E found those two scenarios fail under the skip-drain hook
// because `shallow_file_state` read the stale pre-edit `IndexedReady`
// via `get_any` and fed a stale observed-content hash to the
// component-meta signature builders — so the warm `ComponentMetaResultDb`
// entry's `fact_dep_signature` was rooted on the stale hash and
// validated against post-edit content (a false warm hit). They are
// focused 2.S-F tests — the shared canary harness is NOT rewired (that
// is Block 2.S-E's deliverable).

/// `ComponentMetaResultDb` warm-hit miss counter.
fn meta_misses(host: &VerterHost) -> u64 {
    host.provenance()
        .component_meta_result_cache_misses
        .load(std::sync::atomic::Ordering::Relaxed)
}

/// `ComponentMetaResultDb` warm-hit hit counter.
fn meta_hits(host: &VerterHost) -> u64 {
    host.provenance()
        .component_meta_result_cache_hits
        .load(std::sync::atomic::Ordering::Relaxed)
}

/// Build a workspace-backed host rooted at `/workspace` with `files`
/// injected into the in-memory overlay.
fn workspace_host(
    files: &[(&str, &str)],
) -> (Arc<verter_workspace::MemoryWorkspace>, Arc<VerterHost>) {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn verter_workspace::WorkspaceAccess> = workspace.clone();
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws_access));
    (workspace, host)
}

/// Canary (skip-drain) — imported prop type edit.
///
/// `defineProps<Foo>` over a `Foo` interface imported from a workspace
/// `.ts`. Editing a `Foo` member's type — through the skip-own-drain
/// hook — must MISS the owner's warm `ComponentMetaResultDb` entry and
/// the recomputed prop must carry the new member type.
///
/// Discrimination property: `ComponentMetaResultDb::get_with_view` runs
/// `validates_fact_signature` on the warm-hit path. The owner entry's
/// signature records the dep's parse facts pinned to the observed
/// content hash that `shallow_file_state` reported when the value was
/// computed. With `shallow_file_state` reading `get_any`, the skip-drain
/// edit leaves the stale pre-edit `IndexedReady` lingering, so the
/// observed hash is stale and the warm entry validates against post-edit
/// content — a false warm hit, the miss-delta never materialises, and
/// the recomputed prop reports the stale `number` type. Content-pinning
/// `shallow_file_state` makes the observed hash current, so the warm
/// entry misses and the recompute observes the edited `string` type.
#[test]
fn skip_drain_imported_prop_type_edit_misses_warm_component_meta() {
    let (workspace, host) = workspace_host(&[
        (
            "/workspace/src/types.ts",
            "export interface Foo { a: number; }\n",
        ),
        (
            "/workspace/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Foo } from '/workspace/src/types'\n\
             defineProps<Foo>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    let prime = host.get_component_meta("/workspace/src/Comp.vue");
    assert!(prime.is_some(), "prime get_component_meta must resolve");

    // Warm sanity — an unedited second query must round-trip a warm hit
    // so the post-edit miss-delta is a discriminating signal.
    let hits_before = meta_hits(&host);
    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    assert!(
        meta_hits(&host) > hits_before,
        "warm sanity: an unedited second get_component_meta must hit the \
         warm cache — without a round-tripping warm hit the post-edit \
         miss-delta is not discriminating"
    );
    let misses_before = meta_misses(&host);

    // Edit the imported member's type THROUGH THE SKIP-OWN-DRAIN HOOK:
    // the owner's warm ComponentMetaResultDb entry survives, and so does
    // the dependency's pre-edit `IndexedReady` in `FileArtifactStore`.
    let edited = "export interface Foo { a: string; }\n";
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        std::sync::Arc::from(edited),
    );
    upsert_skipping_drain(&host, "/workspace/src/types.ts", edited);

    let after = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    let misses_after = meta_misses(&host);
    assert!(
        misses_after > misses_before,
        "an imported prop-type edit driven through the skip-own-drain hook \
         MUST miss the owner's warm ComponentMetaResultDb entry — the \
         component-meta signature must root on the dep's CURRENT observed \
         content (misses {misses_before} -> {misses_after}). A stale \
         `shallow_file_state` read roots the signature on the pre-edit \
         hash and the warm entry validates falsely."
    );

    // User-visible output: the recomputed `a` prop is `string`.
    let a_prop = after
        .props
        .iter()
        .find(|p| p.name == "a")
        .expect("recomputed meta must publish prop `a`");
    assert!(
        matches!(
            a_prop.type_expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the recomputed `a` prop MUST carry the edited `string` type — a \
         stale warm hit would still report `number`. Got {:?}",
        a_prop.type_expr
    );
    assert!(
        !matches!(
            a_prop.type_expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the recomputed `a` prop must NOT be the stale `number` type — \
         got {:?}",
        a_prop.type_expr
    );
}

/// Canary (skip-drain) — route-surface dependency edit.
///
/// `defineProps<RProps>()` over an imported type. Resolving the macro
/// root walks the named-type export route — the route walk observes the
/// route DEP's `DerivedFactHash{Route}` participant facts into the
/// published signature. Editing the route source type — through the
/// skip-own-drain hook — must MISS the owner's warm
/// `ComponentMetaResultDb` entry and the recomputed prop set must carry
/// the new route-surface shape.
///
/// Discrimination property: the route-fact producer reads the dep's
/// route surface through the route-owned shallow path. With
/// `shallow_file_state` / `route_shallow_state` reading `get_any`, the
/// skip-drain edit leaves a stale pre-edit `IndexedReady` that shadows
/// the freshly-published route-owned entry, so the route fact (and the
/// owner's published signature) is rooted on the stale surface and the
/// warm entry validates falsely. Content-pinning the route-owned indexed
/// fast path makes the route fact observe the edited surface, so the
/// warm entry misses and the recompute reports both `a` and `b`.
#[test]
fn skip_drain_route_surface_dep_edit_misses_warm_component_meta() {
    let (workspace, host) = workspace_host(&[
        (
            "/workspace/src/types.ts",
            "export interface RProps { a: number; }\n",
        ),
        (
            "/workspace/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { RProps } from '/workspace/src/types'\n\
             defineProps<RProps>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    let prime = host.get_component_meta("/workspace/src/Comp.vue");
    assert!(prime.is_some(), "prime get_component_meta must resolve");

    let hits_before = meta_hits(&host);
    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    assert!(
        meta_hits(&host) > hits_before,
        "warm sanity: an unedited second get_component_meta must hit the \
         warm cache"
    );
    let misses_before = meta_misses(&host);

    // Edit the route source type — `RProps` gains `b` — THROUGH THE
    // SKIP-OWN-DRAIN HOOK.
    let edited = "export interface RProps { a: number; b: string; }\n";
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        std::sync::Arc::from(edited),
    );
    upsert_skipping_drain(&host, "/workspace/src/types.ts", edited);

    let after = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    let misses_after = meta_misses(&host);
    assert!(
        misses_after > misses_before,
        "a route-surface dependency edit driven through the skip-own-drain \
         hook MUST miss the owner's warm ComponentMetaResultDb entry — the \
         cross-file route facts must root on the dep's CURRENT route \
         surface (misses {misses_before} -> {misses_after})"
    );

    // User-visible output: the recomputed prop set carries `a` + `b`.
    let after_names: Vec<String> = after.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        after_names.iter().any(|n| n == "a") && after_names.iter().any(|n| n == "b"),
        "the recomputed props MUST reflect the new `RProps` route surface \
         (`a` + `b`) — a stale warm hit would report only `a`. Got \
         {after_names:?}"
    );
}
