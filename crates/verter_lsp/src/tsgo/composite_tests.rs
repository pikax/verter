//! Discriminating unit tests for the composite's project-bound admission and
//! shared-session lifecycle.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use verter_semantic::resolver_core::ConfiguredMembership;
use verter_session::external_ts::{AmbiguityCause, CarrierOwnershipResolution};
use verter_session::{HostConfig, VerterHost};
use verter_type_runtime::protocol::TypeProviderError;
use verter_type_runtime::traits::ProviderFuture;
use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, WorkspaceAccess};

use super::{
    carrier_source_of, compose_establishment_discriminant, effective_javascript_check_policy,
    injection_shadow_safe, invoke_epoch_bound, leading_file_check_directive, observe_epoch_bound,
    real_file_occupies_injected_path, FileCheckDirective, LazyOverlayCore, OverlayPriority,
    OverlayTransport, SharedEngageFailureKind, SharedRendezvous, SharedTsgoOverlay,
};

// @ai-generated
#[test]
fn authored_file_check_directive_overrides_the_configured_check_js_policy() {
    assert_eq!(
        leading_file_check_directive("// @ts-check\nconst value = 1;"),
        Some(FileCheckDirective::Check)
    );
    assert_eq!(
        effective_javascript_check_policy(Some(false), Some(FileCheckDirective::Check)),
        Some(true)
    );
    assert_eq!(
        effective_javascript_check_policy(Some(true), Some(FileCheckDirective::NoCheck)),
        Some(false)
    );
    assert_eq!(
        effective_javascript_check_policy(Some(false), None),
        Some(false)
    );
}

struct InvocationTransport {
    alive: AtomicBool,
}

impl InvocationTransport {
    fn alive() -> Self {
        Self {
            alive: AtomicBool::new(true),
        }
    }

    fn set_dead(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

impl OverlayTransport for InvocationTransport {
    fn inject(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn retract(&self, _path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn is_live(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn teardown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

async fn establish_invocation_transport(
    core: &LazyOverlayCore<InvocationTransport>,
    generation: u64,
    nonce: &str,
) -> crate::tsgo::transport_cell::EstablishedTransport<InvocationTransport> {
    let nonce = nonce.to_string();
    core.ensure(
        Some(((), generation)),
        move |_| Some(nonce.clone()),
        |(), _| async { Some(Arc::new(InvocationTransport::alive())) },
        Duration::from_secs(5),
    )
    .await
    .expect("establish invocation transport")
}

async fn replace_dead_invocation_transport(
    core: &LazyOverlayCore<InvocationTransport>,
) -> crate::tsgo::transport_cell::EstablishedTransport<InvocationTransport> {
    for generation in 2..=6 {
        let nonce = format!("invoke-{generation}");
        if let Some(established) = core
            .ensure(
                Some(((), generation)),
                move |_| Some(nonce.clone()),
                |(), _| async { Some(Arc::new(InvocationTransport::alive())) },
                Duration::from_secs(5),
            )
            .await
        {
            return established;
        }
    }
    panic!("dead invocation transport was not replaced")
}

/// The SHARED-establishment re-arm discriminant depends on BOTH the
/// shim advertisement nonce AND the workspace/config generation — so a failed
/// establishment re-arms on a reconnect (fresh nonce) OR a fresh published snapshot
/// (fresh generation), never nonce-only. The pre-fix nonce-only discriminant could
/// not retry establishment under a later valid snapshot generation.
#[test]
fn establishment_discriminant_covers_nonce_and_generation() {
    let base = compose_establishment_discriminant("abc123", 5);
    // Same nonce, ADVANCED generation ⇒ a distinct discriminant (re-arms a prior
    // transient miss even under the SAME shim nonce — the poisoning-fix rail).
    assert_ne!(
        base,
        compose_establishment_discriminant("abc123", 6),
        "a fresh config generation must change the discriminant (re-arm under the same nonce)"
    );
    // Fresh nonce (reconnect), same generation ⇒ also a distinct discriminant.
    assert_ne!(
        base,
        compose_establishment_discriminant("def456", 5),
        "a reconnect (fresh nonce) must change the discriminant"
    );
    // Identical (nonce, generation) ⇒ the SAME discriminant (no per-query re-attempt).
    assert_eq!(
        base,
        compose_establishment_discriminant("abc123", 5),
        "an unchanged (nonce, generation) is a stable discriminant (no retry-storm)"
    );
}

/// The carrier SOURCE of a provider companion path — the shape classification the
/// shadow-safety gate resolves the source from. A `.vue.tsx` / `.vue.jsx` companion maps
/// to its `.vue` source; a plain `.ts` file is not a companion; a Windows backslash path
/// normalizes to the same forward-slashed source (cross-platform).
#[test]
fn carrier_source_of_maps_companion_to_source_cross_platform() {
    assert_eq!(
        carrier_source_of("d:/ws/src/Foo.vue.tsx").as_deref(),
        Some("d:/ws/src/Foo.vue")
    );
    assert_eq!(
        carrier_source_of("d:/ws/src/Foo.vue.jsx").as_deref(),
        Some("d:/ws/src/Foo.vue")
    );
    // Cross-platform: a backslash path normalizes to the same forward-slashed source.
    assert_eq!(
        carrier_source_of(r"d:\ws\src\Foo.vue.tsx").as_deref(),
        Some("d:/ws/src/Foo.vue")
    );
    // A plain `.ts` file (no carrier stem) is NOT a carrier companion — OWNED serves it.
    assert_eq!(carrier_source_of("d:/ws/src/plain.ts"), None);
}

/// The DECLARATION companion (`Foo.d.vue.ts` / `Foo.d.svelte.ts`) and the API
/// import-surface companion (`Foo.vue.verter.ts`) map back to the TRUE carrier source
/// through the descriptor authority — the declaration companion resolves to `Foo.vue`,
/// NOT the intermediate `.d.<ext>` stem a generic trailing-`.segment` strip lands on.
/// `Foo.d.vue.ts` is the declaration companion of `Foo.vue`; it is never attributed to a
/// fabricated `Foo.d.vue` source.
#[test]
fn carrier_source_of_maps_declaration_and_api_companions_to_source() {
    // Declaration companions (extension-middle `.d.<ext>.ts`) map to the carrier source.
    assert_eq!(
        carrier_source_of("d:/ws/src/Foo.d.vue.ts").as_deref(),
        Some("d:/ws/src/Foo.vue")
    );
    assert_eq!(
        carrier_source_of("d:/ws/src/Foo.d.svelte.ts").as_deref(),
        Some("d:/ws/src/Foo.svelte")
    );
    // The API import-surface companion (`.verter.ts`) maps to the carrier source.
    assert_eq!(
        carrier_source_of("d:/ws/src/Foo.vue.verter.ts").as_deref(),
        Some("d:/ws/src/Foo.vue")
    );
    // NEGATIVE: the declaration companion is NEVER attributed to the intermediate
    // `.d.<ext>` stem (`Foo.d.vue`) — that is not a real carrier source.
    assert_ne!(
        carrier_source_of("d:/ws/src/Foo.d.vue.ts").as_deref(),
        Some("d:/ws/src/Foo.d.vue")
    );
}

/// The shadow-safety decision over a resolved source. A real user file at a
/// carrier-companion path surfaces — through the resolver's UNCONDITIONAL carrier-path
/// conflict pass — as `Ambiguous(CarrierPathOccupiedByRealFile)` in EVERY
/// owner-resolution state (owned `Unique`, unowned `NoProject`, or multiply-owned
/// `MultipleOwners`), and is NEVER injected / overlay-shadowed (`false`)
/// (`carrier_never_shadows_real_user_file`); a same-stem rune module is likewise
/// rejected. A GENUINE generated companion — one with NO real file at its path, whose
/// source resolves to a clean binding, `NoProject`, `SyntheticScratch`, or a
/// `MultipleOwners` overlap — IS safe to inject (`true`). Discriminates the exact
/// shadow-cause match from a blanket `Ambiguous` reject or an unconditional allow.
///
/// The END-TO-END guarantee that a real file at the companion path is NOT injected in
/// the `NoProject` / `MultipleOwners` / `Unique` states is enforced by the resolver's
/// unconditional conflict pass and guarded at the resolver level by
/// `real_file_at_carrier_path_downgrades_unowned_source_to_ambiguous`,
/// `real_file_at_carrier_path_downgrades_multiply_owned_source_to_ambiguous`, and
/// `real_file_at_carrier_path_downgrades_to_ambiguous` — this unit asserts the pure
/// mapping over the resolutions those states produce.
#[test]
fn injection_shadow_safe_rejects_only_real_file_shadow_causes() {
    // A real user file at the companion path — which the resolver's UNCONDITIONAL
    // conflict pass surfaces as `Ambiguous(CarrierPathOccupiedByRealFile)` REGARDLESS of
    // whether the source is owned, unowned (`NoProject`), or multiply-owned
    // (`MultipleOwners`) — is NEVER injected: fail closed to OWNED, never shadow the real
    // file. This is the E2 correction: pre-fix, `NoProject`/`MultipleOwners` short-
    // circuited before the conflict pass, so a real file there resolved to
    // `NoProject`/`MultipleOwners` (admitted below) and WAS overlay-shadowed.
    assert!(
        !injection_shadow_safe(&CarrierOwnershipResolution::Ambiguous {
            candidates: Vec::new(),
            cause: AmbiguityCause::CarrierPathOccupiedByRealFile,
        }),
        "a real user file at the companion path must never be overlay-shadowed, in ANY \
         owner state (the resolver's unconditional pass makes it this cause)"
    );
    assert!(
        !injection_shadow_safe(&CarrierOwnershipResolution::Ambiguous {
            candidates: Vec::new(),
            cause: AmbiguityCause::SameStemRuneModule,
        }),
        "a same-stem rune module beside the source must never be overlay-shadowed"
    );
    // A GENUINE generated companion — NO real file at its path — is injectable. A
    // `MultipleOwners` overlap and a `NoProject` source are these no-real-file
    // resolutions: a real file at the companion path is instead
    // `CarrierPathOccupiedByRealFile` (rejected above), NEVER these states — so admitting
    // them can no longer overlay-shadow a real user file.
    assert!(
        injection_shadow_safe(&CarrierOwnershipResolution::Ambiguous {
            candidates: Vec::new(),
            cause: AmbiguityCause::MultipleOwners,
        }),
        "a MultipleOwners overlap with NO real file at the companion path is a genuine \
         virtual companion — injectable (a real file there is CarrierPathOccupiedByRealFile, \
         rejected above)"
    );
    assert!(
        injection_shadow_safe(&CarrierOwnershipResolution::NoProject),
        "a NoProject genuine companion with NO real file at its path is injectable as a \
         supporting import member (a real file there is CarrierPathOccupiedByRealFile, \
         rejected above)"
    );
}

// ── Shadow-safety at the injected path (`carrier_never_shadows_real_user_file`) ──
//
// These tests drive the PRODUCTION `injection_is_shadow_safe` gate — the predicate
// `inject_all_dirty` consults before injecting a recorded companion — over a REAL
// `VerterHost` whose live published snapshot OWNS `src/**/*` and whose VFS holds the
// given real user files. This exercises the full disk-occupancy + resolver decision
// end-to-end (the seam a pure `compose_*` test cannot reach).

const SHADOW_WS_ROOT: &str = "d:/ws";
const SHADOW_TSCONFIG: &str = "d:/ws/tsconfig.json";

/// The ownership snapshot: ONE configured project whose `include: ["src/**/*"]` OWNS
/// every carrier / `.ts` under `src/` (pattern-based membership), built through the SAME
/// production membership parse/expansion chain the resolver's own tests use. Ownership is
/// glob-pattern-based (empty `materialized_files` ⇒ bridge mode ⇒ `spec.matches`), so it
/// owns any `src/**/*.vue` / `.svelte` / `.ts` path whether or not a file sits there.
fn shadow_fixture_snapshot() -> WorkspaceSnapshot {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![SHADOW_WS_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(
        SHADOW_TSCONFIG.to_string(),
        Arc::<str>::from(r#"{ "include": ["src/**/*"] }"#),
    );
    let root = CanonicalPath::new(SHADOW_WS_ROOT);
    let raw_membership = load_project_membership(&ws, SHADOW_TSCONFIG);
    let compiler_options = load_compiler_options(&ws, SHADOW_TSCONFIG);
    let supported = supported_extensions_for(&compiler_options);
    let spec = membership_to_spec(&root, &raw_membership, &supported);
    let references = load_project_references(&ws, SHADOW_TSCONFIG)
        .into_iter()
        .map(|r| CanonicalPath::new(&r))
        .collect();
    let project = OwnershipProject {
        id: ProjectId(0),
        root: root.clone(),
        workspace_root: CanonicalPath::new(SHADOW_WS_ROOT),
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(SHADOW_TSCONFIG),
            membership: ConfiguredMembership {
                spec,
                materialized_files: Default::default(),
            },
            compiler_options,
            references,
            workspace_aliases: Vec::new(),
        },
    };
    build_workspace_snapshot_simple(vec![project], SnapshotGeneration(1))
}

/// A `SharedTsgoOverlay` over a real host whose VFS holds `real_files` (injected into the
/// same workspace `Arc` the host holds, so the resolver's `file_exists` probe and the
/// disk-occupancy gate both see them) and whose published snapshot owns `src/**/*`.
fn shadow_overlay_with(real_files: &[(&str, &str)]) -> SharedTsgoOverlay {
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    for (path, content) in real_files {
        ws.inject_file((*path).to_string(), Arc::<str>::from(*content));
    }
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        shadow_fixture_snapshot(),
    )));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);
    SharedTsgoOverlay::new(
        host,
        SharedRendezvous {
            control_dir: PathBuf::from("d:/ws/.verter"),
            session_key: "shadow-test".to_string(),
            workspace_root: SHADOW_WS_ROOT.to_string(),
        },
    )
}

/// G1 (`carrier_never_shadows_real_user_file`): the SHARED overlay must NEVER inject a
/// generated carrier over a REAL user file at the exact injected path — for the
/// DECLARATION carrier (`Foo.d.vue.ts` / `Foo.d.svelte.ts`) as well as the IDE carrier.
/// This test pins the exact-path disk-occupancy gate — `injection_is_shadow_safe` step 1
/// (`real_file_occupies_injected_path`) — which fails the injection closed the instant a
/// real user file occupies the exact injected declaration-carrier path, before the
/// source-resolution consultation that follows it. Defense-in-depth: the descriptor
/// reverse-map in `carrier_source_of` (via `classify_carrier_companion`) maps the
/// declaration companion back to the real `Foo.vue`, and the resolver's carrier-path
/// conflict pass (`carrier_path_conflict` over `carrier_companion_identities_for_source`)
/// enumerates it among every companion family, so the source side would flag it too — but
/// this guard closes the shadow uniformly at the injected path regardless.
///
/// RED before the fix: `injection_is_shadow_safe(Foo.d.vue.ts)` returned `true` (the real
/// user file WAS admitted for injection / overlay-shadowed). GREEN after: `false`
/// (skipped, fail-closed to OWNED/native). Asserted for `.vue` AND `.svelte`.
#[tokio::test]
async fn real_declaration_carrier_is_never_overlay_shadowed_vue_and_svelte() {
    for (source, decl) in [
        ("d:/ws/src/Foo.vue", "d:/ws/src/Foo.d.vue.ts"),
        ("d:/ws/src/Foo.svelte", "d:/ws/src/Foo.d.svelte.ts"),
    ] {
        let overlay = shadow_overlay_with(&[
            (source, "<template></template>"),
            (decl, "export const realUserDeclaration = 1;\n"),
        ]);
        assert!(
            !overlay.injection_is_shadow_safe(decl),
            "a REAL user file at the declaration-carrier path `{decl}` must NEVER be \
             overlay-shadowed by the SHARED overlay (carrier_never_shadows_real_user_file)"
        );
    }
}

/// A GENUINE generated declaration carrier — NO real file at `Foo.d.vue.ts` (only the
/// `.vue` / `.svelte` source exists) — is STILL injectable (the disk-occupancy gate never
/// over-suppresses a real virtual companion). Guards against a blanket declaration-carrier
/// reject. Asserted for `.vue` AND `.svelte`.
#[tokio::test]
async fn genuine_declaration_carrier_is_still_injectable_vue_and_svelte() {
    for (source, decl) in [
        ("d:/ws/src/Foo.vue", "d:/ws/src/Foo.d.vue.ts"),
        ("d:/ws/src/Foo.svelte", "d:/ws/src/Foo.d.svelte.ts"),
    ] {
        // Only the SOURCE is a real file; the declaration carrier is Verter-generated.
        let overlay = shadow_overlay_with(&[(source, "<template></template>")]);
        assert!(
            overlay.injection_is_shadow_safe(decl),
            "a GENUINE generated declaration carrier `{decl}` (no real file at its path) \
             must stay injectable as a supporting Program member"
        );
    }
}

/// A failed first attach is observable as a typed terminal refusal carrying the exact
/// carrier/project binding context. This pins the diagnostic surface that the VS Code
/// single-project failure previously collapsed into an undifferentiated `None`.
#[tokio::test]
async fn engage_transport_failure_preserves_source_project_and_generation() {
    let source = "d:/ws/src/Foo.vue";
    let companion = "d:/ws/src/Foo.vue.tsx";
    let overlay = shadow_overlay_with(&[(source, "<template></template>")]);
    overlay.record_content(
        companion,
        "export const foo = 1;",
        OverlayPriority::Interactive,
    );
    let carrier = crate::tsgo::project_binding::resolve_carrier_bound(&overlay.inner.host, source)
        .into_bound()
        .expect("the single configured project binds the carrier");

    let failure = match overlay.engage_provider(companion, &carrier).await {
        Ok(_) => panic!("a workspace with no relay advertisement cannot engage SHARED"),
        Err(failure) => failure,
    };
    assert_eq!(failure.kind, SharedEngageFailureKind::TransportUnavailable);
    assert_eq!(failure.source, source);
    assert_eq!(failure.config, SHADOW_TSCONFIG);
    assert_eq!(failure.generation, carrier.generation());
    assert_eq!(failure.transport_epoch, None);
    assert_eq!(failure.sync_state, None);
    let rendered = failure.to_string();
    assert!(rendered.contains("TransportUnavailable"));
    assert!(rendered.contains(source));
    assert!(rendered.contains(SHADOW_TSCONFIG));
}

/// Selection retains epoch A until the feature call boundary. If reconnect B lands
/// between selection and invocation, the stale A provider is never called and the
/// already-admitted carrier falls back to managed.
#[tokio::test]
async fn feature_invocation_revalidates_epoch_after_selection() {
    let path = "/ws/Foo.vue.tsx";
    let core = LazyOverlayCore::<InvocationTransport>::new();
    core.record_content(path, "export const value = 1;");

    let selected = establish_invocation_transport(&core, 1, "invoke-1").await;
    core.inject_dirty(&selected, path, 1).await;
    let selected_epoch = selected.identity.epoch;
    assert!(core.sync_state_for_epoch(path, selected_epoch).is_synced());

    // Reconnect B after selection but before the terminal feature invocation.
    selected.transport.set_dead();
    let replacement = replace_dead_invocation_transport(&core).await;
    core.inject_dirty(&replacement, path, 1).await;
    assert_ne!(replacement.identity.epoch, selected_epoch);

    let shared_calls = AtomicUsize::new(0);
    let managed_calls = AtomicUsize::new(0);
    let result = invoke_epoch_bound(
        &core,
        path,
        selected_epoch,
        || async {
            shared_calls.fetch_add(1, Ordering::SeqCst);
            Err::<&'static str, _>(TypeProviderError::new("stale epoch A"))
        },
        || async {
            managed_calls.fetch_add(1, Ordering::SeqCst);
            Ok("managed")
        },
    )
    .await
    .expect("epoch mismatch activates managed fallback");

    assert_eq!(result, "managed");
    assert_eq!(shared_calls.load(Ordering::SeqCst), 0);
    assert_eq!(managed_calls.load(Ordering::SeqCst), 1);
}

/// Revalidate again after the shared await: reconnect B can occur while an epoch-A
/// request is in flight. Its stale success or error is discarded and managed serves
/// the admitted carrier instead.
#[tokio::test]
async fn feature_invocation_discards_stale_error_after_inflight_reconnect() {
    let path = "/ws/Foo.vue.tsx";
    let core = LazyOverlayCore::<InvocationTransport>::new();
    core.record_content(path, "export const value = 1;");

    let selected = establish_invocation_transport(&core, 1, "invoke-1").await;
    core.inject_dirty(&selected, path, 1).await;
    let selected_epoch = selected.identity.epoch;
    let shared_calls = AtomicUsize::new(0);
    let managed_calls = AtomicUsize::new(0);

    let result = invoke_epoch_bound(
        &core,
        path,
        selected_epoch,
        || async {
            shared_calls.fetch_add(1, Ordering::SeqCst);
            selected.transport.set_dead();
            let replacement = replace_dead_invocation_transport(&core).await;
            core.inject_dirty(&replacement, path, 1).await;
            assert_ne!(replacement.identity.epoch, selected_epoch);
            Err::<&'static str, _>(TypeProviderError::new("stale epoch A"))
        },
        || async {
            managed_calls.fetch_add(1, Ordering::SeqCst);
            Ok("managed")
        },
    )
    .await
    .expect("post-call epoch mismatch activates managed fallback");

    assert_eq!(result, "managed");
    assert_eq!(shared_calls.load(Ordering::SeqCst), 1);
    assert_eq!(managed_calls.load(Ordering::SeqCst), 1);
}

/// Diagnostics uses its own typed-refusal route instead of the feature helper,
/// but it must retain the same post-await epoch fence. A reconnect while the
/// shared diagnostics request is in flight makes that result stale and admits
/// the managed diagnostics fallback.
#[tokio::test]
async fn diagnostics_observation_rejects_result_after_inflight_reconnect() {
    let path = "/ws/Foo.vue.tsx";
    let core = LazyOverlayCore::<InvocationTransport>::new();
    core.record_content(path, "export const value = 1;");

    let selected = establish_invocation_transport(&core, 1, "diagnostics-1").await;
    core.inject_dirty(&selected, path, 1).await;
    let selected_epoch = selected.identity.epoch;
    let shared_calls = AtomicUsize::new(0);

    let result = observe_epoch_bound(&core, path, selected_epoch, || async {
        shared_calls.fetch_add(1, Ordering::SeqCst);
        selected.transport.set_dead();
        let replacement = replace_dead_invocation_transport(&core).await;
        core.inject_dirty(&replacement, path, 1).await;
        assert_ne!(replacement.identity.epoch, selected_epoch);
        "stale shared diagnostics"
    })
    .await;

    assert!(result.is_err(), "epoch-A diagnostics must be discarded");
    assert_eq!(shared_calls.load(Ordering::SeqCst), 1);
}

/// The existing IDE-carrier shadow behavior is preserved under the disk-occupancy gate: a
/// REAL `Foo.vue.tsx` is never injected; a GENUINE one (no real file) still is.
#[tokio::test]
async fn ide_carrier_shadow_behavior_preserved() {
    // A real user file at the IDE-carrier path ⇒ never injected.
    let overlay_real = shadow_overlay_with(&[
        ("d:/ws/src/Foo.vue", "<template></template>"),
        ("d:/ws/src/Foo.vue.tsx", "export const realUserFile = 1;\n"),
    ]);
    assert!(
        !overlay_real.injection_is_shadow_safe("d:/ws/src/Foo.vue.tsx"),
        "a real user file at the IDE-carrier path must never be overlay-shadowed"
    );
    // A genuine generated IDE carrier (no real file) ⇒ still injected.
    let overlay_genuine = shadow_overlay_with(&[("d:/ws/src/Foo.vue", "<template></template>")]);
    assert!(
        overlay_genuine.injection_is_shadow_safe("d:/ws/src/Foo.vue.tsx"),
        "a genuine generated IDE carrier (no real file) stays injectable"
    );
}

/// A lightweight in-memory workspace holding the given real user files (canonical ids) —
/// exercises the disk-occupancy gate directly without a full host.
fn memory_ws_with(files: &[&str]) -> MemoryWorkspace {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![SHADOW_WS_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    for f in files {
        ws.inject_file((*f).to_string(), Arc::<str>::from("real user file"));
    }
    ws
}

/// The disk-occupancy gate distinguishes a REAL user file at the exact injected path from
/// a genuine (absent) generated companion — for EVERY companion family the built-in
/// descriptors project. The injected paths are ENUMERATED from the single descriptor
/// authority (`carrier_companion_identities_for_source`) for a Vue AND a Svelte carrier
/// source, so the coverage is exactly the descriptor-owned family set — IDE (Vue's `.tsx`
/// and `.jsx`, Svelte's `.svelte.tsx`), declaration (`.d.vue.ts` / `.d.svelte.ts`),
/// import-surface API (`.verter.ts`), and the Vue testing-API (`.__verter_test.ts`) — so
/// it cannot silently omit a family the way a hand-maintained path list can, and stays
/// honest as the descriptor families evolve.
#[test]
fn real_file_occupies_injected_path_covers_every_companion_type() {
    use verter_session::framework::descriptor::{
        carrier_companion_identities_for_source, CarrierCompanionKind,
    };

    let mut kinds_seen: Vec<CarrierCompanionKind> = Vec::new();
    for source in ["d:/ws/src/Foo.vue", "d:/ws/src/Foo.svelte"] {
        let companions = carrier_companion_identities_for_source(source);
        assert!(
            !companions.is_empty(),
            "the descriptor authority must project at least one companion for `{source}`"
        );
        for companion in &companions {
            if !kinds_seen.contains(&companion.kind) {
                kinds_seen.push(companion.kind);
            }
            // A real user file at the exact injected companion path ⇒ occupied (never
            // overlay-shadowed), for EVERY enumerated family.
            let occupied = memory_ws_with(&[source, companion.path.as_str()]);
            assert!(
                real_file_occupies_injected_path(&occupied, &companion.path),
                "a real user file at `{}` ({:?}) must be detected as occupied (never overlay-shadowed)",
                companion.path,
                companion.kind
            );
            // Genuine generated companion: only the source exists, not the companion path.
            let genuine = memory_ws_with(&[source]);
            assert!(
                !real_file_occupies_injected_path(&genuine, &companion.path),
                "a genuine generated companion `{}` ({:?}) (no real file at its path) stays injectable",
                companion.path,
                companion.kind
            );
        }
    }
    // The enumeration must span MORE THAN ONE family so the test cannot silently degrade to
    // a single-kind (or zero) case and still pass.
    assert!(
        kinds_seen.len() > 1,
        "the descriptor authority must project more than one companion family, got {kinds_seen:?}"
    );
    // ...and it must include EACH of the three `.ts`-tail families the descriptor authority
    // projects (a hand-maintained IDE-suffix-only path list would miss them): the Vue
    // testing-API (`.__verter_test.ts`), the declaration companion (`.d.vue.ts` /
    // `.d.svelte.ts`), and the import-surface API (`.verter.ts`). Enumerating through the
    // descriptor authority guarantees every emitted family is covered; asserting each one
    // makes the test fail if any single family silently stops being enumerated.
    assert!(
        kinds_seen.contains(&CarrierCompanionKind::TestingApi),
        "the enumerated families must include the Vue testing-API companion; got {kinds_seen:?}"
    );
    assert!(
        kinds_seen.contains(&CarrierCompanionKind::Declaration),
        "the enumerated families must include the declaration companion; got {kinds_seen:?}"
    );
    assert!(
        kinds_seen.contains(&CarrierCompanionKind::ImportSurface),
        "the enumerated families must include the import-surface API companion; got {kinds_seen:?}"
    );
}

/// The occupancy probe NORMALIZES the injected path (backslash → slash, drive
/// lowercased on every platform), so a non-canonical injected path cannot evade the
/// fail-closed gate on a case-insensitive FS.
#[test]
fn real_file_occupies_injected_path_is_normalized_cross_platform() {
    let ws = memory_ws_with(&["d:/ws/src/Foo.d.vue.ts"]);
    assert!(
        real_file_occupies_injected_path(&ws, r"d:\ws\src\Foo.d.vue.ts"),
        "a backslash path must normalize to the canonical id and detect the real file"
    );
    assert!(
        real_file_occupies_injected_path(&ws, "D:/ws/src/Foo.d.vue.ts"),
        "an uppercase-drive path must normalize to the canonical id and detect the real file"
    );
    assert!(
        !real_file_occupies_injected_path(&ws, "d:/ws/src/Other.d.vue.ts"),
        "a different path is not occupied"
    );
}

/// INV-5: the `verter(project)` diagnostics path OBSERVES the published root's
/// `ownership_ready`, so a COLD-bootstrap snapshot resolves `NotReady` (it defers —
/// no false no-owner warning), while the always-present OWNED admission gate keeps
/// treating a PRESENT snapshot as authoritative (`NoProject`). Same host + same
/// bootstrap root, two readiness modes — proving the gate stays authoritative (the
/// 15-OWNED-gate-test contract) while diagnostics no longer emit a spurious warning
/// during bootstrap.
#[test]
fn diagnostics_observe_readiness_while_owned_gate_stays_authoritative() {
    use crate::tsgo::project_binding::{resolve_carrier, OwnershipReadinessMode};

    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    ws.inject_file(
        "d:/ws/src/Foo.vue".to_string(),
        Arc::<str>::from("<template></template>"),
    );
    // A COLD-bootstrap published root (`new_vfs_only` ⇒ ownership_ready == false)
    // whose snapshot has NO configured project: an authoritative resolution is
    // `NoProject`; a readiness-observing resolution is `NotReady`.
    let snapshot = build_workspace_snapshot_simple(Vec::new(), SnapshotGeneration(1));
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(snapshot)));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);

    let source = "d:/ws/src/Foo.vue";

    // The OWNED gate treats the present snapshot as authoritative: NoProject (no
    // owner), NEVER NotReady — sourcing readiness from the bootstrap bool here would
    // regress the 15 OWNED-gate tests.
    let (authoritative, _) = resolve_carrier(
        host.as_ref(),
        source,
        Arc::from(""),
        OwnershipReadinessMode::PresentSnapshotAuthoritative,
    )
    .expect("a present snapshot resolves");
    assert_eq!(
        authoritative,
        CarrierOwnershipResolution::NoProject,
        "the OWNED gate treats a present snapshot as authoritative (NoProject, never NotReady)"
    );

    // The diagnostics path observes the cold `ownership_ready == false` ⇒ NotReady:
    // it DEFERS instead of resolving a premature terminal NoProject.
    let (observed, _) = resolve_carrier(
        host.as_ref(),
        source,
        Arc::from(""),
        OwnershipReadinessMode::ObservePublishedReadiness,
    )
    .expect("a present snapshot resolves");
    assert_eq!(
        observed,
        CarrierOwnershipResolution::NotReady,
        "a cold-bootstrap snapshot defers (NotReady) for the readiness-observing consumer"
    );
    // NotReady ⇒ NO `verter(project)` diagnostic (no spurious bootstrap warning).
    assert!(
        crate::external_ts::carrier_sync::project_ownership_diagnostic(&observed).is_none(),
        "a NotReady carrier must emit NO verter(project) diagnostic during bootstrap"
    );
}
