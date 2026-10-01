//! Discriminating unit tests for the composite's project-bound admission and
//! shared-session lifecycle.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

use verter_semantic::resolver_core::ConfiguredMembership;
use verter_session::external_ts::{
    AmbiguityCause, CarrierOwnershipResolution, GeneratedUnitAdmissionFact,
};
use verter_session::{HostConfig, VerterHost};
use verter_type_runtime::protocol::*;
use verter_type_runtime::provider_hub::{
    HubPolicy, ProviderEstablisher, ProviderHub, TracingNotifier,
};
use verter_type_runtime::traits::{ProviderFuture, TypeProvider};
use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::traits::WorkspaceRead;
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, WorkspaceAccess};

use crate::tsgo::shared::EstablishSharedParams;
use verter_type_runtime::provider_hub::overlay::{HubAdmittedTransport, ServingTransport};

use super::{
    carrier_source_of, compose_establishment_discriminant, effective_javascript_check_policy,
    injection_shadow_safe, leading_file_check_directive, real_file_occupies_injected_path,
    FeatureProviderSelection, FileCheckDirective, OverlayPriority, SharedAttach,
    SharedEngageFailureKind, SharedRendezvous, SharedTsgoOverlay,
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

// ── Test attach doubles: drive the REAL production gates (hub-owned
//    establishment, hub-issued admission, epoch fencing) through a real
//    ProviderHub whose serving provider is a recording double. ──

/// A recording [`SharedAttach`] double: every provider-visible operation that
/// reached the "editor engine" is recorded, with an optional hover gate that
/// models a request in flight while its serving incarnation is replaced.
struct RecordingAttach {
    ops: parking_lot::Mutex<Vec<String>>,
    applied: parking_lot::Mutex<std::collections::HashMap<String, Arc<str>>>,
    hover_reached: tokio::sync::Notify,
    hover_release: tokio::sync::Notify,
    hover_gated: std::sync::atomic::AtomicBool,
}

impl RecordingAttach {
    fn new() -> Self {
        Self {
            ops: parking_lot::Mutex::new(Vec::new()),
            applied: parking_lot::Mutex::default(),
            hover_reached: tokio::sync::Notify::new(),
            hover_release: tokio::sync::Notify::new(),
            hover_gated: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn ops(&self) -> Vec<String> {
        let mut ops = self.ops.lock().clone();
        ops.sort();
        ops
    }

    fn clear_ops(&self) {
        self.ops.lock().clear();
    }

    fn count(&self, prefix: &str) -> usize {
        self.ops
            .lock()
            .iter()
            .filter(|op| op.starts_with(prefix))
            .count()
    }

    /// Arm the hover gate: the next hover signals it is in flight and BLOCKS
    /// until `release_hover` — a request mid-await while its incarnation is
    /// retired underneath it.
    fn arm_hover_gate(&self) {
        self.hover_gated
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn release_hover(&self) {
        self.hover_release.notify_one();
    }
}

impl TypeProvider for RecordingAttach {
    fn provider_id(&self) -> &'static str {
        "tsgo"
    }

    fn applied_content(&self, path: &str) -> verter_type_runtime::traits::AppliedContent {
        self.applied
            .lock()
            .get(path)
            .cloned()
            .map(verter_type_runtime::traits::AppliedContent::Applied)
            .unwrap_or(verter_type_runtime::traits::AppliedContent::NotApplied)
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.ops.lock().push(format!("write:{path}"));
        let path = path.to_string();
        let content = Arc::from(content);
        Box::pin(async move {
            self.applied.lock().insert(path, content);
            Ok(())
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.ops.lock().push(format!("retract:{path}"));
        let path = path.to_string();
        Box::pin(async move {
            self.applied.lock().remove(&path);
            Ok(())
        })
    }

    fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        self.get_diagnostics(path)
    }

    fn get_diagnostics_in_project<'a>(
        &'a self,
        _path: &'a str,
        _configured_project: &'a str,
    ) -> ProviderFuture<'a, Option<Vec<TypeDiagnostic>>> {
        Box::pin(async { Ok(Some(Vec::new())) })
    }

    fn get_completions(
        &self,
        _path: &str,
        _offset: u32,
        _trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        Box::pin(async {
            Ok(CompletionResult {
                items: Vec::new(),
                is_incomplete: false,
            })
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        _path: &'a str,
        _offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        let items = items.to_vec();
        Box::pin(async move { Ok(items) })
    }

    fn resolve_completion(
        &self,
        _path: &str,
        _data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        Box::pin(async { Ok(None) })
    }

    fn get_hover(&self, path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let gated = self.hover_gated.load(std::sync::atomic::Ordering::SeqCst);
        self.ops.lock().push(format!("hover:{path}"));
        Box::pin(async move {
            if gated {
                self.hover_reached.notify_one();
                self.hover_release.notified().await;
            }
            Ok(None)
        })
    }

    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async { Ok(None) })
    }

    fn get_code_actions(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
        _diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_inlay_hints(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn register_carrier_member(
        &self,
        _source_path: &str,
        companion_path: &str,
        _content: &str,
        _project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        self.ops.lock().push(format!("register:{companion_path}"));
        Box::pin(async { Ok(()) })
    }

    fn register_carrier_metadata<'a>(
        &'a self,
        _source_path: &'a str,
        companion_path: &'a str,
        _content: &'a str,
        _project_file_name: &'a str,
    ) -> ProviderFuture<'a, ()> {
        self.ops.lock().push(format!("metadata:{companion_path}"));
        Box::pin(async { Ok(()) })
    }

    fn activate_carrier_member(
        &self,
        _source_path: &str,
        companion_path: &str,
        _project_file_name: &str,
        _script_kind: verter_type_runtime::CarrierScriptKind,
    ) -> ProviderFuture<'_, ()> {
        self.ops.lock().push(format!("activate:{companion_path}"));
        Box::pin(async { Ok(()) })
    }

    fn activate_carrier_members<'a>(
        &'a self,
        members: &'a [verter_type_runtime::CarrierActivation],
    ) -> ProviderFuture<'a, ()> {
        let ops: Vec<String> = members
            .iter()
            .map(|m| format!("activate:{}", m.companion_path))
            .collect();
        self.ops.lock().extend(ops);
        Box::pin(async { Ok(()) })
    }

    fn configure_paths(
        &self,
        _base_url: &str,
        _paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn update_workspace_folders(
        &self,
        _added: Vec<serde_json::Value>,
        _removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

impl SharedAttach for RecordingAttach {
    fn establish_shared_attach<'a>(
        _params: EstablishSharedParams<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Self, crate::tsgo::shared::EstablishError>> + Send + 'a>>
    {
        Box::pin(async {
            Err(crate::tsgo::shared::EstablishError::NoShim(
                "test double".into(),
            ))
        })
    }

    fn redecide_for_binding(
        &self,
        _binding: &verter_session::external_ts::ProjectBinding,
        _generated_units: GeneratedUnitAdmissionFact,
        _generation: u64,
    ) -> verter_session::external_ts::LiveDecision {
        unreachable!("the recording double is established directly, never through engage")
    }

    fn overlay_diagnostics_in_project<'a>(
        &'a self,
        _path: &'a str,
        _tsconfig: &'a str,
    ) -> ProviderFuture<'a, Option<Vec<TypeDiagnostic>>> {
        Box::pin(async { Ok(Some(Vec::new())) })
    }

    fn attach_is_alive(&self) -> bool {
        true
    }
}

/// The test attach establisher: hands the recording double to a REAL
/// [`ProviderHub`] and exposes the crash signal the hub handed it, so a test
/// can retire the serving incarnation deterministically.
struct TestAttachBackend {
    provider: Arc<RecordingAttach>,
    crash_notify: parking_lot::Mutex<Option<Arc<Notify>>>,
}

impl TestAttachBackend {
    fn new(provider: Arc<RecordingAttach>) -> Self {
        Self {
            provider,
            crash_notify: parking_lot::Mutex::new(None),
        }
    }

    /// Retire the serving incarnation: notify the crash signal the hub handed
    /// the establishment and spin until the hub reports nothing serving.
    async fn retire_serving(&self, hub: &ProviderHub<RecordingAttach>) {
        self.crash_notify
            .lock()
            .as_ref()
            .expect("the establishment received its crash signal")
            .notify_one();
        for _ in 0..100_000 {
            if !hub.is_serving() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the recording attach was never retired");
    }
}

impl ProviderEstablisher<RecordingAttach> for TestAttachBackend {
    fn log_name(&self) -> &'static str {
        "test-attach"
    }

    fn user_label(&self) -> &'static str {
        "test"
    }

    fn restarting_error(&self) -> &'static str {
        "test attach is re-arming"
    }

    fn supports_completion_resolve(&self) -> bool {
        false
    }

    fn establish<'a>(
        &'a self,
        crash_notify: Arc<Notify>,
    ) -> verter_type_runtime::provider_hub::EstablishFuture<'a, RecordingAttach> {
        *self.crash_notify.lock() = Some(Arc::clone(&crash_notify));
        let provider = Arc::clone(&self.provider);
        Box::pin(async move { Ok(provider) })
    }
}

/// An overlay over a REAL hub serving the recording double, on the same host /
/// published-snapshot fixture shape as [`overlay_over`].
async fn recording_overlay_over(
    real_files: &[(&str, &str)],
    snapshot: WorkspaceSnapshot,
) -> (
    SharedTsgoOverlay<RecordingAttach>,
    Arc<FilesystemWorkspace>,
    Arc<RecordingAttach>,
    Arc<TestAttachBackend>,
) {
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    for (path, content) in real_files {
        ws.inject_file((*path).to_string(), Arc::<str>::from(*content));
    }
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(snapshot)));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);
    let attach = Arc::new(RecordingAttach::new());
    let backend = Arc::new(TestAttachBackend::new(Arc::clone(&attach)));
    let hub = Arc::new(ProviderHub::new(
        TestAttachShim(Arc::clone(&backend)),
        Arc::new(TracingNotifier),
        HubPolicy::lazy_attach(Duration::from_secs(5)),
    ));
    let overlay = SharedTsgoOverlay::over_test_hub(host, hub);
    (overlay, ws, attach, backend)
}

/// [`ProviderHub`] owns its establisher behind an `Arc`-free `Box`; the shim
/// shares the backend with the test.
struct TestAttachShim(Arc<TestAttachBackend>);

impl ProviderEstablisher<RecordingAttach> for TestAttachShim {
    fn log_name(&self) -> &'static str {
        self.0.log_name()
    }

    fn user_label(&self) -> &'static str {
        self.0.user_label()
    }

    fn restarting_error(&self) -> &'static str {
        self.0.restarting_error()
    }

    fn supports_completion_resolve(&self) -> bool {
        self.0.supports_completion_resolve()
    }

    fn establish<'a>(
        &'a self,
        crash_notify: Arc<Notify>,
    ) -> verter_type_runtime::provider_hub::EstablishFuture<'a, RecordingAttach> {
        self.0.establish(crash_notify)
    }
}

/// Establish the recording overlay's hub and hand the core the hub-minted
/// serving incarnation (the same observe the production ensure path performs).
async fn serve_recording(
    overlay: &SharedTsgoOverlay<RecordingAttach>,
) -> ServingTransport<HubAdmittedTransport<RecordingAttach>> {
    overlay
        .inner
        .hub
        .establish()
        .await
        .expect("the test attach establishes");
    let (provider, epoch) = overlay.inner.hub.serving().expect("serving");
    ServingTransport {
        transport: Arc::new(HubAdmittedTransport::new(
            provider,
            Arc::clone(&overlay.inner.hub),
            epoch,
        )),
        epoch,
    }
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
    fixture_snapshot(r#"{ "include": ["src/**/*"] }"#)
}

/// [`shadow_fixture_snapshot`] over an arbitrary `tsconfig.json` body — the membership
/// shape under test.
fn fixture_snapshot(tsconfig_body: &str) -> WorkspaceSnapshot {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![SHADOW_WS_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(SHADOW_TSCONFIG.to_string(), Arc::<str>::from(tsconfig_body));
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
    overlay_over(real_files, shadow_fixture_snapshot()).0
}

/// [`shadow_overlay_with`] over an arbitrary published `snapshot`, also handing back the
/// workspace so a test can publish a LATER snapshot into the same host.
fn overlay_over(
    real_files: &[(&str, &str)],
    snapshot: WorkspaceSnapshot,
) -> (SharedTsgoOverlay, Arc<FilesystemWorkspace>) {
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    for (path, content) in real_files {
        ws.inject_file((*path).to_string(), Arc::<str>::from(*content));
    }
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(snapshot)));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);
    let overlay = SharedTsgoOverlay::new(
        host,
        SharedRendezvous {
            control_dir: PathBuf::from("d:/ws/.verter"),
            session_key: "shadow-test".to_string(),
            workspace_root: SHADOW_WS_ROOT.to_string(),
        },
    );
    (overlay, ws)
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

// ── Generated-unit admission at the SHARED write boundary ──
//
// A configured project can OWN a carrier source while admitting none of the units
// generated for it. These tests drive the PRODUCTION admission gate and the PRODUCTION
// sweep (`inject_editor_demand`) over a real host + published snapshot, with a recording
// transport double standing in for the editor's engine, and assert on what reached it.

/// A project that owns the carrier source (the directory include admits
/// `Foo.vue`) while its `exclude` removes the companion's own form — the
/// configuration-excluded shape that stays refused under the carrier-basis
/// membership model.
const COMPANION_EXCLUDING_TSCONFIG: &str =
    r#"{ "include": ["src"], "exclude": ["src/**/*.vue.tsx"] }"#;

/// A carrier whose project owns the source but CONFIGURATION-EXCLUDES its
/// generated units is refused with the typed reason BEFORE the transport is
/// established — so nothing can have been written — and the refusal is
/// reported once per (carrier, generation).
#[tokio::test]
async fn unadmitted_carrier_is_refused_before_the_transport_is_touched() {
    let source = "d:/ws/src/Foo.vue";
    let companion = "d:/ws/src/Foo.vue.tsx";
    let (overlay, _ws) = overlay_over(
        &[(source, "<template></template>")],
        fixture_snapshot(COMPANION_EXCLUDING_TSCONFIG),
    );
    overlay.record_content(companion, "export {}", OverlayPriority::Interactive);
    overlay.record_content(
        "d:/ws/src/Foo.vue.verter.ts",
        "export {}",
        OverlayPriority::Interactive,
    );
    let carrier = crate::tsgo::project_binding::resolve_carrier_bound(&overlay.inner.host, source)
        .into_bound()
        .expect("the directory include OWNS the carrier source");

    let failure = match overlay.engage_provider(companion, &carrier).await {
        Ok(_) => panic!("an unadmitted carrier cannot engage SHARED"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.kind,
        SharedEngageFailureKind::GeneratedUnitsNotAdmitted {
            reason: verter_workspace::GeneratedUnitNonAdmissionReason::Excluded,
            // Only the offender: the `.verter.ts` sibling is neither excluded
            // nor refused through the carrier basis.
            units: vec![companion.to_string()],
        }
    );
    assert_eq!(failure.transport_epoch, None, "no transport was reached");
    assert!(
        !overlay.inner.hub.is_serving(),
        "the refusal precedes establishment: no attach exists to have been written to"
    );

    let generation = overlay.sweep_generation();
    assert!(
        !overlay.first_refusal_report(source, generation),
        "the engage already reported this carrier at this generation"
    );
}

/// A carrier with no recorded generated unit has no write set to prove admitted.
#[tokio::test]
async fn carrier_without_a_recorded_write_set_is_refused_as_unproven() {
    let source = "d:/ws/src/Foo.vue";
    let (overlay, _ws) = overlay_over(
        &[(source, "<template></template>")],
        fixture_snapshot(r#"{ "include": ["src"] }"#),
    );
    let carrier = crate::tsgo::project_binding::resolve_carrier_bound(&overlay.inner.host, source)
        .into_bound()
        .expect("the directory include owns the carrier");
    let failure = match overlay
        .engage_provider("d:/ws/src/Foo.vue.tsx", &carrier)
        .await
    {
        Ok(_) => panic!("nothing is recorded for the carrier"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.kind,
        SharedEngageFailureKind::GeneratedUnitAdmissionUnproven
    );
    assert!(!overlay.inner.hub.is_serving());
}

/// ONE sweep, three carriers, all recorded on an editor lane (in scope). Only the carrier
/// whose WHOLE generated family is admitted reaches the transport:
///
/// - `admitted/Ok.vue` — every unit admitted (`admitted/**/*.tsx` matches the
///   `.vue.tsx` directly, `src/**/*.ts` the `.verter.ts`) ⇒ written;
/// - `Neighbour.vue` — owned, in scope, but its `.vue.tsx` form is
///   configuration-EXCLUDED ⇒ NOT written, and neither is its `.verter.ts`
///   sibling even though `src/**/*.ts` matches that one (admission is
///   all-or-nothing per carrier);
/// - `outside/Stray.vue` — no owning configured project at all ⇒ NOT written.
#[tokio::test]
async fn sweep_writes_only_carriers_whose_generated_units_are_admitted() {
    let (overlay, _ws, attach, _backend) = recording_overlay_over(
        &[
            ("d:/ws/src/admitted/Ok.vue", "<template></template>"),
            ("d:/ws/src/Neighbour.vue", "<template></template>"),
            ("d:/ws/outside/Stray.vue", "<template></template>"),
        ],
        fixture_snapshot(
            r#"{ "include": ["src/**/*.ts", "src/**/*.vue", "src/**/*.tsx"], "exclude": ["src/Neighbour.vue.tsx"] }"#,
        ),
    )
    .await;
    for unit in [
        "d:/ws/src/admitted/Ok.vue.tsx",
        "d:/ws/src/admitted/Ok.vue.verter.ts",
        "d:/ws/src/Neighbour.vue.tsx",
        "d:/ws/src/Neighbour.vue.verter.ts",
        "d:/ws/outside/Stray.vue.tsx",
    ] {
        overlay
            .inner
            .hub
            .overlay_state()
            .record_content_at_priority(unit, "export {}", OverlayPriority::Normal);
    }
    let serving = serve_recording(&overlay).await;

    overlay
        .inject_editor_demand(
            overlay.inner.hub.overlay_state(),
            &serving,
            "d:/ws/src/admitted/Ok.vue.tsx",
            overlay.sweep_generation(),
        )
        .await;

    assert_eq!(
        attach.ops(),
        vec![
            "write:d:/ws/src/admitted/Ok.vue.tsx".to_string(),
            "write:d:/ws/src/admitted/Ok.vue.verter.ts".to_string(),
        ],
        "only the admitted carrier's family reaches the editor-owned engine"
    );
}

/// Admission follows the published membership. Narrowing `include` (a new publication)
/// re-decides: the units a previous sweep wrote are RETRACTED and nothing is written;
/// widening it again writes them back. A decision is never carried across a publication.
#[tokio::test]
async fn a_new_publication_re_evaluates_generated_unit_admission() {
    let source = "d:/ws/src/Foo.vue";
    let companion = "d:/ws/src/Foo.vue.tsx";
    let (overlay, ws, attach, _backend) = recording_overlay_over(
        &[(source, "<template></template>")],
        fixture_snapshot(r#"{ "include": ["src"] }"#),
    )
    .await;
    let core = overlay.inner.hub.overlay_state();
    core.record_content_at_priority(companion, "export {}", OverlayPriority::Interactive);
    let serving = serve_recording(&overlay).await;
    let sweep = || async {
        overlay
            .inject_editor_demand(core, &serving, companion, overlay.sweep_generation())
            .await;
        let ops = attach.ops();
        attach.clear_ops();
        ops
    };

    assert_eq!(sweep().await, vec![format!("write:{companion}")]);

    // Narrow the membership: the project still OWNS `Foo.vue`, but its
    // `exclude` now removes the companion's own form.
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(fixture_snapshot(
        COMPANION_EXCLUDING_TSCONFIG,
    ))));
    assert_eq!(
        sweep().await,
        vec![format!("retract:{companion}")],
        "the unit written under the earlier membership leaves the editor-owned engine"
    );
    assert!(
        !core
            .sync_state_for_epoch(companion, serving.epoch)
            .is_synced(),
        "an unadmitted unit is never reported synced"
    );

    // Widen it again.
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(fixture_snapshot(
        r#"{ "include": ["src"] }"#,
    ))));
    assert_eq!(sweep().await, vec![format!("write:{companion}")]);
}

/// Selection retains epoch A until the feature call boundary. If the hub retires
/// that incarnation (a replacement landed) between selection and invocation, the
/// stale shared engine is never called and the already-admitted carrier falls
/// back to managed.
#[tokio::test]
async fn feature_invocation_revalidates_epoch_after_selection() {
    let source = "d:/ws/src/Foo.vue";
    let path = "d:/ws/src/Foo.vue.tsx";
    let (overlay, _ws, attach, backend) = recording_overlay_over(
        &[(source, "<template></template>")],
        fixture_snapshot(r#"{ "include": ["src"] }"#),
    )
    .await;
    let core = overlay.inner.hub.overlay_state();
    core.record_content_at_priority(
        path,
        "export const value = 1;",
        OverlayPriority::Interactive,
    );
    let serving = serve_recording(&overlay).await;
    // Sync the carrier through the REAL write gate (a hub-issued admission).
    let permit = overlay
        .generated_unit_write_permit(core, path)
        .expect("the carrier's generated units are admitted");
    overlay
        .inner
        .hub
        .synchronize(
            serving.epoch,
            overlay.sweep_generation(),
            |candidate, _| candidate == path,
            |_| {
                Some(
                    verter_type_runtime::provider_hub::overlay::GeneratedUnitWritePermit::admitted(
                        permit.admission().unwrap().clone(),
                    ),
                )
            },
        )
        .await
        .unwrap();
    assert!(core.sync_state_for_epoch(path, serving.epoch).is_synced());

    // The serving incarnation is retired AFTER selection but BEFORE the
    // terminal feature invocation.
    backend.retire_serving(&overlay.inner.hub).await;

    let managed = Arc::new(RecordingAttach::new());
    let selection = FeatureProviderSelection::Shared {
        hub: Arc::clone(&overlay.inner.hub),
        managed: Arc::clone(&managed) as Arc<dyn TypeProvider>,
        core: Arc::clone(&overlay.inner),
        provider_path: path.to_string(),
        transport_epoch: serving.epoch,
    };
    let hover = selection
        .invoke(|provider| {
            let path = path.to_string();
            async move { provider.get_hover(&path, 0).await }
        })
        .await
        .expect("epoch mismatch activates managed fallback");

    assert!(
        hover.is_none() || hover.is_some(),
        "managed served the call"
    );
    assert_eq!(
        attach.count("hover:"),
        0,
        "the retired shared engine is never invoked after the epoch moved"
    );
    assert_eq!(
        managed.count("hover:"),
        1,
        "managed served the admitted carrier"
    );
}

/// Revalidate again after the shared await: the incarnation can be retired WHILE an
/// epoch-A request is in flight. Its stale answer is discarded by the hub's epoch
/// settlement and managed serves the admitted carrier instead.
#[tokio::test]
async fn feature_invocation_discards_stale_error_after_inflight_reconnect() {
    let source = "d:/ws/src/Foo.vue";
    let path = "d:/ws/src/Foo.vue.tsx";
    let (overlay, _ws, attach, backend) = recording_overlay_over(
        &[(source, "<template></template>")],
        fixture_snapshot(r#"{ "include": ["src"] }"#),
    )
    .await;
    let core = overlay.inner.hub.overlay_state();
    core.record_content_at_priority(
        path,
        "export const value = 1;",
        OverlayPriority::Interactive,
    );
    let serving = serve_recording(&overlay).await;
    let permit = overlay
        .generated_unit_write_permit(core, path)
        .expect("the carrier's generated units are admitted");
    overlay
        .inner
        .hub
        .synchronize(
            serving.epoch,
            overlay.sweep_generation(),
            |candidate, _| candidate == path,
            |_| {
                Some(
                    verter_type_runtime::provider_hub::overlay::GeneratedUnitWritePermit::admitted(
                        permit.admission().unwrap().clone(),
                    ),
                )
            },
        )
        .await
        .unwrap();
    assert!(core.sync_state_for_epoch(path, serving.epoch).is_synced());

    // The shared hover blocks mid-flight; the retirement lands UNDERNEATH it.
    attach.arm_hover_gate();
    let managed = Arc::new(RecordingAttach::new());
    let managed_for_invoke = Arc::clone(&managed);
    let hub = Arc::clone(&overlay.inner.hub);
    let inner = Arc::clone(&overlay.inner);
    let invoke = tokio::spawn(async move {
        let selection = FeatureProviderSelection::<RecordingAttach>::Shared {
            hub,
            managed: managed_for_invoke as Arc<dyn TypeProvider>,
            core: inner,
            provider_path: path.to_string(),
            transport_epoch: serving.epoch,
        };
        selection
            .invoke(|provider| {
                let path = path.to_string();
                async move { provider.get_hover(&path, 0).await }
            })
            .await
    });
    // The shared hover is in flight (the attach reached it) ...
    attach.hover_reached.notified().await;
    // ... the incarnation is retired underneath it ...
    backend.retire_serving(&overlay.inner.hub).await;
    attach.release_hover();
    let hover = invoke
        .await
        .expect("the invoke task completes")
        .expect("post-call epoch mismatch activates managed fallback");

    assert!(
        hover.is_none() || hover.is_some(),
        "managed served the call"
    );
    assert_eq!(
        attach.count("hover:"),
        1,
        "the shared engine answered once, in flight, before the retirement"
    );
    assert_eq!(
        managed.count("hover:"),
        1,
        "the retired epoch's answer is discarded and managed serves the carrier"
    );
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

// ── The hub-issued binding warmth and the reconfigure publish-before-bump
//    window — the semantics the deleted per-composite admission cache used to
//    carry, now pinned against the REAL hub witness path. ──

/// Within ONE publication the hub-issued binding witness is WARM: a repeated
/// bind of the same source at the unchanged basis reuses the SAME witness
/// (identity, not a re-mint) — the hub's `bindings` map is the one warm
/// binding memo, no second cache beside it.
#[tokio::test]
async fn hub_binding_warmth_reuses_the_same_witness_within_one_publication() {
    let source = "d:/ws/src/Foo.vue";
    let (overlay, _ws, _attach, _backend) = recording_overlay_over(
        &[(source, "<template></template>")],
        fixture_snapshot(r#"{ "include": ["src"] }"#),
    )
    .await;
    let _serving = serve_recording(&overlay).await;
    let first = overlay
        .bind_carrier_witness(source)
        .expect("the owning snapshot binds the carrier");
    let second = overlay
        .bind_carrier_witness(source)
        .expect("an unchanged basis re-binds");
    assert!(
        first.same_binding(&second),
        "an unchanged publication reuses the SAME hub-issued witness (memoized), not a \
         per-call re-mint"
    );
    // The warm path (`bound_project`) serves the same current witness.
    assert!(overlay
        .inner
        .hub
        .bound_project(&verter_semantic::resolver_core::normalize_canonical_id(
            source
        ))
        .is_some_and(|warm| warm.same_binding(&first)));
}

/// SECURITY-CRITICAL regression (the reconfigure publish-before-bump window):
/// `configure_projects` PUBLISHES the new (now non-owning) project graph BEFORE
/// it bumps the monotonic project generation, and `ProjectGraph::from_configs`
/// resets the published scalar generation to the same value (1) with a DISTINCT
/// `Arc<PublishedRoot>` and NO content bump. A write gate racing inside that
/// window must NOT reuse the prior epoch's warm witness: the hub basis keys on
/// the UNREPEATABLE publication identity, so the warm witness retires, the
/// re-resolve fails closed to the non-owning answer, and NOTHING is written —
/// never a fail-OPEN cross-epoch privilege bleed.
#[tokio::test]
async fn republished_generation_scalar_never_reuses_a_prior_epoch_binding() {
    let source = "d:/ws/src/Foo.vue";
    let companion = "d:/ws/src/Foo.vue.tsx";
    let (overlay, ws, attach, _backend) = recording_overlay_over(
        &[(source, "<template></template>")],
        fixture_snapshot(r#"{ "include": ["src"] }"#),
    )
    .await;
    let core = overlay.inner.hub.overlay_state();
    core.record_content_at_priority(companion, "export {}", OverlayPriority::Interactive);
    let serving = serve_recording(&overlay).await;

    // Epoch A: the owning snapshot binds the carrier and its write set admits.
    let witness_a = overlay
        .bind_carrier_witness(source)
        .expect("epoch A: the owning snapshot binds the carrier");
    let permit_a = overlay
        .generated_unit_write_permit(core, companion)
        .expect("epoch A: the write set is admitted");
    overlay
        .inner
        .hub
        .synchronize(
            serving.epoch,
            overlay.sweep_generation(),
            |candidate, _| candidate == companion,
            |_| {
                Some(
                    verter_type_runtime::provider_hub::overlay::GeneratedUnitWritePermit::admitted(
                        permit_a.admission().unwrap().clone(),
                    ),
                )
            },
        )
        .await
        .unwrap();
    assert_eq!(attach.ops(), vec![format!("write:{companion}")]);
    attach.clear_ops();

    let published_a = ws.published_root().unwrap().snapshot.generation.0;
    let content_a = ws.content_generation();
    let project_a = overlay
        .inner
        .host
        .project_type_store()
        .current_project_generation();

    // PRODUCTION ORDERING — PUBLISH FIRST: a NON-owning graph that RESETS the
    // published generation to the SAME scalar (a DISTINCT Arc) with no content
    // bump and no project-generation bump — the pre-bump window.
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        build_workspace_snapshot_simple(Vec::new(), SnapshotGeneration(1)),
    )));
    assert_eq!(
        ws.published_root().unwrap().snapshot.generation.0,
        published_a,
        "the republish RESET the published generation to the same scalar"
    );
    assert_eq!(ws.content_generation(), content_a, "no content bump");
    assert_eq!(
        overlay
            .inner
            .host
            .project_type_store()
            .current_project_generation(),
        project_a,
        "the monotonic project generation is NOT yet bumped — the pre-bump window"
    );

    // The warm witness RETIRES on the publication-identity drift alone.
    assert!(
        !overlay
            .inner
            .hub
            .bound_project(&verter_semantic::resolver_core::normalize_canonical_id(
                source
            ))
            .is_some_and(|warm| warm.same_binding(&witness_a)),
        "the in-window warm lookup must MISS the prior epoch's witness — the \
         unrepeatable publication identity disambiguates the reset scalar"
    );

    // The write gate inside the window FAILS CLOSED: re-resolve finds no owner,
    // no witness, no permit — and the previously written unit is RETRACTED.
    assert!(
        overlay
            .generated_unit_write_permit(core, companion)
            .is_none(),
        "the in-window gate must not write: the reset (now non-owning) epoch \
         re-resolves fail-closed, never serving epoch A's admission"
    );
    overlay
        .inject_editor_demand(core, &serving, companion, overlay.sweep_generation())
        .await;
    assert_eq!(
        attach.ops(),
        vec![format!("retract:{companion}")],
        "the unit written under epoch A leaves the editor-owned engine inside the \
         window — zero writes on the stale authorization"
    );
}
