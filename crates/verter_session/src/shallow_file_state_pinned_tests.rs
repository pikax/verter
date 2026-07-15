//! Current-content-pinned `shallow_file_state` discriminators.
//!
//! `shallow_file_state` is the canonical shallow-type-file-state reader
//! the frontier engine and the provenance-pure signature builders draw
//! the observed content identity from. A non-content-pinned reader
//! reads `IndexedReady` through `FileArtifactStore::get_any` — a
//! content-agnostic, canonical-only lookup. After a same-canonical
//! upsert the pre-edit artifact physically lingers in
//! `FileArtifactStore` (the upsert performs no own-canonical drain), so
//! a `get_any` read surfaces it and feeds a stale observed-content hash
//! to every signature builder. Self-version-rooting would be defeated
//! at the root: the builders are provenance-pure but the version they
//! are fed would be stale.
//!
//! Content-pinned `shallow_file_state` resolves the canonical's
//! authoritative current content hash and reads `FileArtifactStore`
//! pinned to that hash. A stale older-content artifact yields a miss;
//! the read falls through to the route-surface accessor
//! (`routed_shallow_state_with_context`), whose base fall-through joins
//! the canonical `IndexedReady` build (`ensure_indexed_ready`) — so the
//! current content is re-materialised, never served from the stale
//! artifact.
//!
//! Discrimination property of every test below: the mutation is driven
//! through the production [`crate::VerterHost::upsert`], which performs
//! no own-canonical cache drain, so the stale `IndexedReady` physically
//! survives in `FileArtifactStore`. A non-content-pinned `get_any` read
//! would return that stale artifact (asserted RED); the content-pinned
//! read observes the edited content (asserted GREEN). The assertions
//! are on observed hash-level and shallow-symbol freshness — never on
//! physical cache emptiness.

use std::sync::Arc;

use crate::{HostConfig, UpsertRequest, VerterHost};

/// Upsert a file through the production [`VerterHost::upsert`] path.
/// The upsert performs no own-canonical query-identity cache drain, so
/// the pre-edit `IndexedReady` physically survives in
/// `FileArtifactStore` while the scheduler tracks the new content.
fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

/// Core discriminator — a content edit to a directly resolvable `.ts`
/// dependency must make `shallow_file_state` return the CURRENT
/// content's shallow state, not the stale pre-edit one.
///
/// - **Non-content-pinned read:** `shallow_file_state` reads `get_any`.
///   The edit left the pre-edit `IndexedReady` in `FileArtifactStore`
///   (the upsert performs no own-canonical drain); `get_any` returns
///   it, so the shallow state's `whole_hash` is the pre-edit hash and
///   the new `Renamed` type symbol is absent. Both assertions FAIL.
/// - **Content-pinned read:** `shallow_file_state` resolves the
///   authoritative current content hash (the scheduler's post-edit
///   `parse.whole_hash`) and reads `FileArtifactStore` pinned to it. The
///   stale artifact misses; the content-pinned read (or the
///   IndexedReady-backed fallback) observes the edited content, so the
///   `whole_hash` is the post-edit hash and `Renamed` is present.
#[test]
fn shallow_file_state_observes_current_content_after_dependency_edit() {
    let canonical = "/pinned_shallow/dep.ts";
    let host = VerterHost::new_standalone(HostConfig::default());

    // Seed the dependency and materialise its `IndexedReady` so a real
    // pre-edit artifact lives in `FileArtifactStore` — that artifact is
    // the one the edit must NOT leave masking the new content.
    upsert(
        &host,
        canonical,
        "export interface Original { a: number; }\n",
    );
    let pre_edit = host
        .ensure_indexed_ready(canonical)
        .expect("pre-edit IndexedReady must materialise for the seeded dep");
    let pre_edit_hash = pre_edit.whole_hash;
    assert!(
        pre_edit.shallow_state.has_type_symbol("Original"),
        "fixture invariant: the pre-edit shallow state must expose the \
         `Original` type symbol — got {:?}",
        pre_edit
            .shallow_state
            .type_symbol_names()
            .collect::<Vec<_>>()
    );

    // Edit the dependency: the upsert performs no own-canonical drain,
    // so the pre-edit `IndexedReady` survives in `FileArtifactStore`
    // while the scheduler tracks the new content.
    let edited = "export interface Renamed { b: string; }\n";
    upsert(&host, canonical, edited);

    // Fixture invariant: the pre-edit artifact genuinely lingers — a
    // permissive `get_any` (the non-content-pinned read shape) still
    // returns it.
    let lingering = host
        .project_type_store()
        .indexed()
        .get_any(canonical)
        .expect("upsert must leave the pre-edit IndexedReady in FileArtifactStore");
    assert_eq!(
        lingering.whole_hash, pre_edit_hash,
        "fixture invariant: the lingering artifact keeps its pre-edit \
         content hash — a non-content-pinned `get_any` read of \
         `shallow_file_state` would surface THIS stale artifact"
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
         after the edit — a non-content-pinned `get_any` read returns the \
         lingering pre-edit artifact, so its whole_hash is stale. The \
         observed content identity feeds every provenance-pure signature \
         builder; a stale hash defeats self-version-rooting at the root."
    );

    // Discriminating assertion 2 — the observed shallow surface carries
    // the edited content (the `Renamed` symbol), not the stale one.
    assert!(
        after.has_type_symbol("Renamed"),
        "shallow_file_state MUST observe the edited content — the post-edit \
         shallow surface must expose the `Renamed` type symbol. Got {:?}",
        after.type_symbol_names().collect::<Vec<_>>()
    );
    assert!(
        !after.has_type_symbol("Original"),
        "shallow_file_state MUST NOT observe the stale pre-edit content — \
         the `Original` symbol was renamed away and must be absent. A \
         non-content-pinned `get_any` read surfaces the stale artifact \
         and still reports `Original`. Got {:?}",
        after.type_symbol_names().collect::<Vec<_>>()
    );
}

/// Companion discriminator — a content edit that *renames* the
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
/// - **Non-content-pinned read:** `shallow_file_state` reads `get_any`,
///   surfaces the lingering pre-edit `IndexedReady`, and returns its
///   `shallow_state` — which still carries `Probe` and lacks
///   `RenamedProbe`. Both symbol assertions FAIL.
/// - **Content-pinned read:** the content-pinned read misses the stale
///   artifact (its content hash is not the scheduler's current hash);
///   the IndexedReady-backed fallback (`ensure_indexed_ready`) recomputes
///   the current shallow surface, which carries `RenamedProbe`.
#[test]
fn shallow_file_state_observes_renamed_symbol_after_dependency_edit() {
    let canonical = "/pinned_shallow/probe.ts";
    let host = VerterHost::new_standalone(HostConfig::default());

    upsert(
        &host,
        canonical,
        "export interface Probe { kept: number; }\n",
    );
    let pre_edit = host
        .ensure_indexed_ready(canonical)
        .expect("pre-edit IndexedReady must materialise");
    assert!(
        pre_edit.shallow_state.has_type_symbol("Probe"),
        "fixture invariant: the pre-edit shallow surface must expose `Probe`"
    );

    // Rename the exported interface. The upsert performs no
    // own-canonical drain, so the pre-edit `IndexedReady` (carrying the
    // `Probe` shallow surface) survives in `FileArtifactStore`; the
    // scheduler tracks the renamed content.
    let edited = "export interface RenamedProbe { kept: number; }\n";
    upsert(&host, canonical, edited);

    // Fixture invariant: the pre-edit artifact's shallow surface still
    // lingers — its `shallow_state` carries the pre-edit `Probe` symbol.
    let lingering = host
        .project_type_store()
        .indexed()
        .get_any(canonical)
        .expect("upsert must leave the pre-edit IndexedReady in FileArtifactStore");
    assert!(
        lingering.shallow_state.has_type_symbol("Probe"),
        "fixture invariant: the lingering artifact's shallow surface still \
         carries the pre-edit `Probe` symbol — a non-content-pinned \
         `get_any` read of `shallow_file_state` would surface THIS stale \
         surface"
    );

    // The discriminating read.
    let observed = host
        .shallow_file_state(canonical)
        .expect("post-edit shallow_file_state must resolve via the content-pinned path");
    assert!(
        observed.has_type_symbol("RenamedProbe"),
        "shallow_file_state MUST observe the renamed content — the \
         post-edit shallow surface must expose `RenamedProbe`. A \
         non-content-pinned `get_any` read surfaces the lingering \
         pre-edit artifact's surface (only `Probe`). Got {:?}",
        observed.type_symbol_names().collect::<Vec<_>>()
    );
    assert!(
        !observed.has_type_symbol("Probe"),
        "shallow_file_state MUST NOT surface the stale pre-edit `Probe` \
         symbol — it was renamed away. Got {:?}",
        observed.type_symbol_names().collect::<Vec<_>>()
    );
}

// ── Content-pinned component-meta cross-file scenarios ──────────────
//
// The two scenarios below mirror `block_2_canary_component_meta.rs`'s
// `imported_prop_type_edit_misses_warm_component_meta` and
// `route_surface_dep_edit_misses_warm_component_meta`, focused on the
// `shallow_file_state` content-pinning property. A non-content-pinned
// `shallow_file_state` reads the lingering pre-edit `IndexedReady` via
// `get_any` and feeds a stale observed-content hash to the
// component-meta signature builders — so the warm `ComponentMetaResultDb`
// entry's `fact_dep_signature` would be rooted on the stale hash and
// validate against post-edit content (a false warm hit). Content-pinned
// `shallow_file_state` roots the signature on the dependency's current
// content, so the warm entry misses and the recompute observes the
// edit.

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

/// Imported prop type edit misses the owner's warm component-meta.
///
/// `defineProps<Foo>` over a `Foo` interface imported from a workspace
/// `.ts`. Editing a `Foo` member's type must MISS the owner's warm
/// `ComponentMetaResultDb` entry and the recomputed prop must carry the
/// new member type.
///
/// Discrimination property: `ComponentMetaResultDb::get_with_view` runs
/// `validates_fact_signature` on the warm-hit path. The owner entry's
/// signature records the dep's parse facts pinned to the observed
/// content hash that `shallow_file_state` reported when the value was
/// computed. The upsert performs no own-canonical drain, so the
/// pre-edit `IndexedReady` lingers; a non-content-pinned
/// `shallow_file_state` reading `get_any` would report a stale observed
/// hash and the warm entry would validate against post-edit content — a
/// false warm hit, the miss-delta never materialises, and the
/// recomputed prop would report the stale `number` type. Content-pinned
/// `shallow_file_state` makes the observed hash current, so the warm
/// entry misses and the recompute observes the edited `string` type.
#[test]
fn imported_prop_type_edit_misses_warm_component_meta() {
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

    // Edit the imported member's type. The upsert performs no
    // own-canonical drain, so the owner's warm ComponentMetaResultDb
    // entry survives, and so does the dependency's pre-edit
    // `IndexedReady` in `FileArtifactStore`.
    let edited = "export interface Foo { a: string; }\n";
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        std::sync::Arc::from(edited),
    );
    upsert(&host, "/workspace/src/types.ts", edited);

    let after = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    let misses_after = meta_misses(&host);
    assert!(
        misses_after > misses_before,
        "an imported prop-type edit MUST miss the owner's warm \
         ComponentMetaResultDb entry — the component-meta signature must \
         root on the dep's CURRENT observed content (misses \
         {misses_before} -> {misses_after}). A stale `shallow_file_state` \
         read roots the signature on the pre-edit hash and the warm entry \
         validates falsely."
    );

    // User-visible output: the recomputed `a` prop is `string`.
    let a_prop = after
        .props
        .iter()
        .find(|p| p.name == "a")
        .expect("recomputed meta must publish prop `a`");
    let a_type = crate::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/workspace/src/Comp.vue",
        a_prop
            .type_source
            .present()
            .expect("recomputed prop `a` must publish a typed source"),
    )
    .unwrap_or_else(|| panic!("`a`'s published source must demand-materialize"));
    assert!(
        matches!(
            a_type,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the recomputed `a` prop MUST carry the edited `string` type — a \
         stale warm hit would still report `number`. Got {a_type:?}"
    );
    assert!(
        !matches!(
            a_type,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the recomputed `a` prop must NOT be the stale `number` type — \
         got {a_type:?}"
    );
}

/// Route-surface dependency edit misses the owner's warm component-meta.
///
/// `defineProps<RProps>()` over an imported type. Resolving the macro
/// root walks the named-type export route — the route walk observes the
/// route DEP's `DerivedFactHash{Route}` participant facts into the
/// published signature. Editing the route source type must MISS the
/// owner's warm `ComponentMetaResultDb` entry and the recomputed prop
/// set must carry the new route-surface shape.
///
/// Discrimination property: the route-fact producer reads the dep's
/// route surface through the routed-shallow path. The upsert
/// performs no own-canonical drain, so a stale pre-edit `IndexedReady`
/// lingers; a non-content-pinned `shallow_file_state` /
/// `route_shallow_state` reading `get_any` would let that stale
/// artifact shadow the freshly-published current-content artifact, so
/// the route fact (and the owner's published signature) would be rooted
/// on the stale surface and the warm entry would validate falsely.
/// Content-pinning the routed-shallow indexed fast path makes the route
/// fact observe the edited surface, so the warm entry misses and the
/// recompute reports both `a` and `b`.
#[test]
fn route_surface_dep_edit_misses_warm_component_meta() {
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

    // Edit the route source type — `RProps` gains `b`.
    let edited = "export interface RProps { a: number; b: string; }\n";
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        std::sync::Arc::from(edited),
    );
    upsert(&host, "/workspace/src/types.ts", edited);

    let after = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    let misses_after = meta_misses(&host);
    assert!(
        misses_after > misses_before,
        "a route-surface dependency edit MUST miss the owner's warm \
         ComponentMetaResultDb entry — the cross-file route facts must \
         root on the dep's CURRENT route surface (misses \
         {misses_before} -> {misses_after})"
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
