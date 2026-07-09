//! FAIL-CLOSED unit proof for the ALWAYS-present OWNED carrier-diagnostics gate and
//! the shared per-carrier project-binding helper — pure over an in-memory host +
//! published snapshot (NO live tsgo).
//!
//! The contract (REOPEN-A): production OWNED carrier diagnostics are gated on the
//! carrier's resolved `BoundProject`. A carrier companion whose owning project does
//! NOT resolve to a bound configured project (a not-yet-ready published snapshot, a
//! `NoProject` source, an `Ambiguous` / `SyntheticScratch` source) yields NO
//! external-TS diagnostics — fail closed, NEVER a `tsgo --lsp` inferred /
//! own-discovery fall-through. A BOUND carrier delegates to OWNED unchanged; a
//! NON-carrier path (plain `.ts`) is not gated at all.
//!
//! Discriminating: the OWNED baseline double surfaces a distinctive MARKER diagnostic
//! for EVERY path. So a gated-to-empty result proves the gate blocked OWNED's
//! `--lsp` fall-through (the pre-fix composite delegated `get_diagnostics` to OWNED
//! unconditionally, so the marker would leak for a non-bound carrier).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use verter_lsp::tsgo::composite::TsgoCompositeProvider;
use verter_lsp::tsgo::project_binding::{
    resolve_carrier, resolve_carrier_bound, AdmissionDenial, CarrierAdmissionCache, CarrierBinding,
};
use verter_lsp::type_provider::protocol::{
    Completion, CompletionResolveData, CompletionResolveResult, CompletionResult, HoverInfo,
    InlayHint, ProviderDiagnosticContext, RenameLocation, SemanticToken, SignatureHelp,
    TypeCodeAction, TypeDiagnostic, TypeDiagnosticSeverity, TypeDocumentHighlight, TypeLocation,
};
use verter_lsp::type_provider::traits::{ProviderFuture, TypeProvider};
use verter_session::external_ts::{AmbiguityCause, ProjectResolution};

use verter_session::{HostConfig, VerterHost};

use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::membership::ConfiguredMembership;
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, WorkspaceAccess, WorkspaceRead};

const WS_ROOT: &str = "/binding-gate-ws";
const TSCONFIG: &str = "/binding-gate-ws/tsconfig.json";

/// The distinctive OWNED-only marker diagnostic every double path surfaces — a gated
/// (empty) result proves the OWNED `--lsp` fall-through was blocked.
const MARKER_CODE: &str = "9999";

/// A minimal OWNED baseline double: EVERY diagnostics query surfaces the MARKER
/// diagnostic (foreground + background), and every GATED FEATURE method surfaces a
/// distinctive NON-EMPTY marker AND counts its invocations — so a gated feature result
/// that is empty/none AND a call counter of ZERO together prove the gate blocked OWNED's
/// `--lsp` fall-through (the pre-gate composite delegated features unconditionally, so
/// the marker would leak AND the counter would tick).
///
/// Counters are added per feature-group as each group is gated.
#[derive(Default)]
struct MarkerOwned {
    type_definition_calls: AtomicUsize,
    signature_help_calls: AtomicUsize,
    semantic_tokens_calls: AtomicUsize,
    hover_calls: AtomicUsize,
    definition_calls: AtomicUsize,
    references_calls: AtomicUsize,
    document_highlights_calls: AtomicUsize,
    inlay_hints_calls: AtomicUsize,
    completions_calls: AtomicUsize,
    completion_details_calls: AtomicUsize,
    resolve_completion_calls: AtomicUsize,
    rename_locations_calls: AtomicUsize,
    code_actions_calls: AtomicUsize,
}

fn marker_diag() -> TypeDiagnostic {
    TypeDiagnostic {
        message: "owned-lsp-fallthrough-marker".to_string(),
        severity: TypeDiagnosticSeverity::Error,
        start: 0,
        end: 1,
        code: Some(MARKER_CODE.to_string()),
        tags: Vec::new(),
        related_information: Vec::new(),
    }
}

/// A distinctive non-empty OWNED location marker (leaks iff the gate delegated).
fn marker_location() -> TypeLocation {
    TypeLocation {
        path: "owned-marker".to_string(),
        start: 0,
        end: 1,
    }
}

/// A distinctive non-empty OWNED hover marker.
fn marker_hover() -> HoverInfo {
    HoverInfo {
        contents: "owned-marker".to_string(),
        range_start: None,
        range_end: None,
    }
}

/// A distinctive non-empty OWNED document-highlight marker.
fn marker_highlight() -> TypeDocumentHighlight {
    TypeDocumentHighlight {
        start: 0,
        end: 1,
        kind: verter_lsp::type_provider::protocol::TypeDocumentHighlightKind::Text,
    }
}

/// A distinctive non-empty OWNED inlay-hint marker.
fn marker_inlay_hint() -> InlayHint {
    InlayHint {
        position: 0,
        label: "owned-marker".to_string(),
        kind: None,
        padding_left: None,
        padding_right: None,
    }
}

/// A distinctive non-empty OWNED code-action marker (leaks iff the gate delegated).
fn marker_code_action() -> TypeCodeAction {
    TypeCodeAction {
        title: "owned-marker".to_string(),
        kind: None,
        edits: Vec::new(),
    }
}

impl TypeProvider for MarkerOwned {
    fn provider_id(&self) -> &'static str {
        "tsgo"
    }
    fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }
    fn update_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }
    fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }
    fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async move { Ok(vec![marker_diag()]) })
    }
    fn get_diagnostics_background(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async move { Ok(vec![marker_diag()]) })
    }
    fn get_completions(
        &self,
        _path: &str,
        _offset: u32,
        _trigger: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        self.completions_calls.fetch_add(1, Ordering::SeqCst);
        // `is_incomplete: true` is the distinctive marker (the empty/gated default is
        // `false`), so a gated-to-default result is observably different from a delegated
        // one even with no items.
        Box::pin(async move {
            Ok(CompletionResult {
                items: Vec::new(),
                is_incomplete: true,
            })
        })
    }
    fn get_completion_details<'a>(
        &'a self,
        _path: &'a str,
        _offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        self.completion_details_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(items.to_vec()) })
    }
    fn resolve_completion(
        &self,
        _path: &str,
        _data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        self.resolve_completion_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(Some(CompletionResolveResult {
                detail: Some("owned-marker".to_string()),
                ..Default::default()
            }))
        })
    }
    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        self.hover_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(Some(marker_hover())) })
    }
    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.definition_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(vec![marker_location()]) })
    }
    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.type_definition_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(vec![marker_location()]) })
    }
    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.references_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(vec![marker_location()]) })
    }
    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        self.rename_locations_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(vec![RenameLocation {
                path: "owned-marker".to_string(),
                start: 0,
                end: 1,
            }])
        })
    }
    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        self.signature_help_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(Some(SignatureHelp {
                signatures: Vec::new(),
                active_signature: Some(0),
                active_parameter: None,
            }))
        })
    }
    fn get_code_actions(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
        _diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        self.code_actions_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(vec![marker_code_action()]) })
    }
    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        self.semantic_tokens_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(vec![SemanticToken {
                start: 0,
                length: 1,
                token_type: 0,
                token_modifiers: 0,
            }])
        })
    }
    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        self.document_highlights_calls
            .fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(vec![marker_highlight()]) })
    }
    fn get_inlay_hints(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        self.inlay_hints_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(vec![marker_inlay_hint()]) })
    }
}

/// A configured-project snapshot whose `include: ["src/**/*"]` OWNS `src/` under
/// [`TSCONFIG`], built through the SAME production membership parse/expansion chain
/// the resolver's own tests use, hermetically over an in-memory workspace.
fn owning_snapshot() -> WorkspaceSnapshot {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![WS_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(
        TSCONFIG.to_string(),
        Arc::<str>::from(r#"{ "include": ["src/**/*"] }"#),
    );
    let root = CanonicalPath::new(WS_ROOT);
    let raw_membership = load_project_membership(&ws, TSCONFIG);
    let compiler_options = load_compiler_options(&ws, TSCONFIG);
    let supported = supported_extensions_for(&compiler_options);
    let spec = membership_to_spec(&root, &raw_membership, &supported);
    let references = load_project_references(&ws, TSCONFIG)
        .into_iter()
        .map(|r| CanonicalPath::new(&r))
        .collect();
    let project = OwnershipProject {
        id: ProjectId(0),
        root: root.clone(),
        workspace_root: CanonicalPath::new(WS_ROOT),
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(TSCONFIG),
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

/// A host with the owning snapshot published (a bound source under `src/` resolves).
fn host_with_snapshot() -> Arc<VerterHost> {
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(owning_snapshot())));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);
    host
}

/// A host over a fresh in-memory workspace carrying an EMPTY published snapshot (roots
/// declared, no configured projects). `MemoryWorkspace::new` publishes that empty
/// snapshot, so `published_root()` is `Some(empty)` — NOT `None` — and the resolver
/// runs over it and yields the DISTINCT `NoProject` fail-closed variant (a genuine
/// resolver-driven "no owning tsconfig" state), NOT the `published_root()==None`
/// PreSnapshot / not-yet-ready state its name might suggest. (The PreSnapshot state is
/// covered separately by `every_non_bound_carrier_binding_variant_is_fail_closed_none`.)
fn host_without_snapshot() -> Arc<VerterHost> {
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions {
        roots: vec![WS_ROOT.to_string()],
        default_resolve_extensions: None,
    }));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);
    host
}

fn codes(diags: &[TypeDiagnostic]) -> Vec<String> {
    diags.iter().filter_map(|d| d.code.clone()).collect()
}

fn composite(host: Arc<VerterHost>) -> TsgoCompositeProvider {
    // The always-present admission layer with NO SHARED overlay (bare host-aware
    // OWNED). The gate is what is under test — SHARED is not required to prove it.
    TsgoCompositeProvider::new(Arc::new(MarkerOwned::default()), host, None)
}

/// A composite over a MARKER OWNED double whose `Arc` is ALSO returned, so a feature
/// test can inspect the OWNED per-method call counters (a denied carrier must leave the
/// counter at ZERO — the gate served the external default WITHOUT delegating to OWNED).
fn composite_with_owned(host: Arc<VerterHost>) -> (TsgoCompositeProvider, Arc<MarkerOwned>) {
    let owned = Arc::new(MarkerOwned::default());
    let composite =
        TsgoCompositeProvider::new(Arc::clone(&owned) as Arc<dyn TypeProvider>, host, None);
    (composite, owned)
}

// ── The shared project-binding helper: the fail-closed states are DISTINCT and none
//    admits a BoundProject. ──

/// EVERY non-bound `CarrierBinding` variant — the pre-published-snapshot, `NoProject`,
/// `Ambiguous`, `SyntheticScratch`, and `EnsureFailed` fail-closed states — is DISTINCT
/// and collapses to `!is_bound()` + `into_bound() == None`, so the caller serves NO
/// external-TS result. This directly covers the pre-snapshot + ambiguous + scratch
/// cases the resolver rarely produces in a hermetic unit.
#[test]
fn every_non_bound_carrier_binding_variant_is_fail_closed_none() {
    for variant in [
        CarrierBinding::PreSnapshot,
        CarrierBinding::NoProject,
        CarrierBinding::Ambiguous(AmbiguityCause::CarrierPathOccupiedByRealFile),
        CarrierBinding::SyntheticScratch,
        CarrierBinding::EnsureFailed,
    ] {
        assert!(
            !variant.is_bound(),
            "a non-bound CarrierBinding variant must not be bound: {variant:?}"
        );
        assert!(
            variant.into_bound().is_none(),
            "every non-bound CarrierBinding variant collapses to the fail-closed None"
        );
    }
    // And the Bound variant is the ONLY admitting state (via the resolver below).
}

#[test]
fn helper_bound_source_under_include_resolves_to_bound() {
    let carrier =
        resolve_carrier_bound(&host_with_snapshot(), &format!("{WS_ROOT}/src/Widget.vue"));
    assert!(
        carrier.is_bound(),
        "a source under the owning `src/**/*` include must resolve to a BoundProject"
    );
    assert!(carrier.into_bound().is_some());
}

#[test]
fn helper_no_project_source_is_distinct_fail_closed_none() {
    // A source OUTSIDE the owning `src/` include ⇒ NoProject (no owning tsconfig).
    let carrier = resolve_carrier_bound(
        &host_with_snapshot(),
        &format!("{WS_ROOT}/outside/Widget.vue"),
    );
    assert!(
        matches!(carrier, CarrierBinding::NoProject),
        "a source outside every configured include ⇒ NoProject, not a bound witness; got {carrier:?}"
    );
    assert!(!carrier.is_bound());
    assert!(carrier.into_bound().is_none());
}

#[test]
fn helper_unconfigured_workspace_source_resolves_no_project() {
    // A host whose published snapshot carries NO configured projects (the
    // empty-snapshot workspace) resolves ANY source to the DISTINCT `NoProject`
    // fail-closed variant — the resolver runs over the empty snapshot and finds no
    // owning tsconfig — discriminated from a bound witness, never collapsed to a bare
    // `None`. (See `host_without_snapshot()` for why this resolves to NoProject and not
    // the `published_root()==None` PreSnapshot state.) A different scenario from
    // `helper_no_project_source_is_distinct_fail_closed_none`, which resolves a source
    // OUTSIDE an EXISTING project's include over a POPULATED snapshot.
    let carrier = resolve_carrier_bound(
        &host_without_snapshot(),
        &format!("{WS_ROOT}/src/Widget.vue"),
    );
    assert!(
        matches!(carrier, CarrierBinding::NoProject),
        "an unconfigured-workspace source resolves to the NoProject fail-closed variant \
         (empty published snapshot, no owning tsconfig), distinct from a bound witness; \
         got {carrier:?}"
    );
    assert!(!carrier.is_bound());
    assert!(carrier.into_bound().is_none());
}

// ── The composite OWNED carrier-diagnostics gate. ──

#[tokio::test]
async fn gate_bound_carrier_delegates_to_owned() {
    let c = composite(host_with_snapshot());
    let diags = c
        .get_diagnostics(&format!("{WS_ROOT}/src/Widget.vue.tsx"))
        .await
        .expect("composite diagnostics");
    assert_eq!(
        codes(&diags),
        vec![MARKER_CODE.to_string()],
        "a BOUND carrier delegates to OWNED — the OWNED `--lsp` marker surfaces"
    );
}

#[tokio::test]
async fn gate_empty_snapshot_carrier_fails_closed_to_empty() {
    // An empty/not-yet-populated published snapshot resolves the carrier to no bound
    // configured project — the gate fails closed (the same as a genuine pre-snapshot).
    let c = composite(host_without_snapshot());
    let diags = c
        .get_diagnostics(&format!("{WS_ROOT}/src/Widget.vue.tsx"))
        .await
        .expect("composite diagnostics");
    assert!(
        diags.is_empty(),
        "a carrier with no bound configured project must yield NO external-TS diagnostics — \
         fail closed, NEVER the OWNED `--lsp` marker fall-through; got {:?}",
        codes(&diags)
    );
}

#[tokio::test]
async fn gate_no_project_carrier_fails_closed_to_empty() {
    let c = composite(host_with_snapshot());
    // A carrier OUTSIDE the owning include resolves to NoProject ⇒ no external-TS result.
    let diags = c
        .get_diagnostics(&format!("{WS_ROOT}/outside/Widget.vue.tsx"))
        .await
        .expect("composite diagnostics");
    assert!(
        diags.is_empty(),
        "a NoProject carrier must yield NO external-TS diagnostics — fail closed, never the \
         OWNED `--lsp` marker fall-through; got {:?}",
        codes(&diags)
    );
}

#[tokio::test]
async fn gate_non_carrier_path_is_not_gated() {
    // A plain `.ts` (NOT a carrier companion) is NOT gated — it delegates to OWNED
    // even with NO published snapshot (the gate is carrier-scoped only).
    let c = composite(host_without_snapshot());
    let diags = c
        .get_diagnostics(&format!("{WS_ROOT}/src/plain.ts"))
        .await
        .expect("composite diagnostics");
    assert_eq!(
        codes(&diags),
        vec![MARKER_CODE.to_string()],
        "a non-carrier `.ts` path is NOT gated — it delegates to OWNED unchanged, even \
         pre-snapshot"
    );
}

// ── The BACKGROUND diagnostics lane shares the SAME gate as foreground
//    (`diagnostics_gated(path, background=true)`): a non-bound carrier fails closed on
//    the background lane too, and a bound carrier still delegates to OWNED. ──

#[tokio::test]
async fn gate_bound_carrier_delegates_to_owned_background() {
    // A BOUND carrier on the BACKGROUND lane delegates to OWNED — the OWNED `--lsp`
    // marker surfaces. This is the discriminator proving the background lane PRODUCES the
    // marker for a bound carrier, so the empty result in the sibling test below is the
    // GATE blocking a non-bound carrier, not an always-empty background lane.
    let c = composite(host_with_snapshot());
    let diags = c
        .get_diagnostics_background(&format!("{WS_ROOT}/src/Widget.vue.tsx"))
        .await
        .expect("composite background diagnostics");
    assert_eq!(
        codes(&diags),
        vec![MARKER_CODE.to_string()],
        "a BOUND carrier delegates to OWNED on the BACKGROUND lane — the OWNED marker surfaces"
    );
}

#[tokio::test]
async fn gate_non_bound_carrier_fails_closed_to_empty_background() {
    // A NON-bound (NoProject) carrier on the BACKGROUND lane fails closed to empty —
    // symmetric with the foreground gate. RED on a hypothetical ungated background lane:
    // an impl that delegated `get_diagnostics_background` to OWNED unconditionally would
    // leak the OWNED `--lsp` marker here. Exercising the background lane confirms
    // `diagnostics_gated(path, background=true)` is gated, not just the foreground path.
    let c = composite(host_with_snapshot());
    let diags = c
        .get_diagnostics_background(&format!("{WS_ROOT}/outside/Widget.vue.tsx"))
        .await
        .expect("composite background diagnostics");
    assert!(
        diags.is_empty(),
        "a NoProject carrier must yield NO external-TS diagnostics on the BACKGROUND lane \
         too — fail closed, never the OWNED `--lsp` marker fall-through; got {:?}",
        codes(&diags)
    );
}

// ── Version-independence of the OWNED gate's bootstrap `ts_version`. ──

/// VERSION-INDEPENDENCE PIN: `resolve_carrier` for the SAME bound source with the empty
/// bootstrap `ts_version` (`""`, the OWNED gate's `OWNED_GATE_BOOTSTRAP_VERSION`) vs a
/// non-empty version yields the SAME owning-project identity + tsconfig. `ts_version` is
/// carried onto the resolved binding's metadata but is NOT load-bearing for the witness
/// identity, the project identity, or the owning tsconfig — so the coarse bound-or-not
/// gate decision (and the tsconfig the SHARED path reuses from the witness) is
/// version-independent, which is exactly why the empty bootstrap is safe.
///
/// This PINS that invariant: a future edit that made `ts_version` load-bearing for the
/// project identity / tsconfig would diverge the two resolutions and fail here — closing
/// the (verified-false) "empty version corrupts version-keyed state" concern against a
/// silent regression.
#[test]
fn resolve_carrier_project_identity_is_version_independent() {
    let host = host_with_snapshot();
    let source = format!("{WS_ROOT}/src/Widget.vue");

    let resolve_binding = |v: &str| match resolve_carrier(&host, &source, Arc::from(v)) {
        Some((ProjectResolution::ProjectBinding(binding), _generation)) => binding,
        other => panic!(
            "expected a ProjectBinding for the bound source with ts_version {v:?}; got {other:?}"
        ),
    };
    let empty = resolve_binding("");
    let versioned = resolve_binding("5.6.0");

    assert_eq!(
        empty.tsconfig_uri(),
        versioned.tsconfig_uri(),
        "the owning tsconfig must be ts_version-independent (empty bootstrap == non-empty)"
    );
    assert_eq!(
        empty.env_dims().project_identity,
        versioned.env_dims().project_identity,
        "the owning-project identity must be ts_version-independent — a future edit making \
         ts_version load-bearing for project identity would break the safe empty bootstrap"
    );
}

// ── The generation-scoped OWNED carrier FEATURE admission cache
//    (`CarrierAdmissionCache`) — the substrate the composite feature gate consults. It
//    MEMOIZES the ONE shared `resolve_carrier_bound` resolver per (source, 3-dim
//    generation tuple), never a second binding engine, and fails closed on every
//    non-bound state. ──

/// A NON-owning published snapshot at generation 1 (no configured projects), used to
/// RESET the published generation to the same value across a reconfigure while the
/// resolver now finds no owning tsconfig for any source.
fn non_owning_snapshot() -> WorkspaceSnapshot {
    build_workspace_snapshot_simple(Vec::new(), SnapshotGeneration(1))
}

/// The cache admits a source that resolves to a bound configured project and fails
/// closed (with the DISTINCT reason) for every non-bound source — the same fail-closed
/// contract as `resolve_carrier_bound`, now memoized.
#[test]
fn admission_cache_admits_bound_and_denies_non_bound() {
    let cache = CarrierAdmissionCache::new();
    let host = host_with_snapshot();

    // A source under the owning `src/**/*` include is ADMITTED, and the resolved witness
    // is RETAINED for reuse.
    let admitted = cache.admit(&host, &format!("{WS_ROOT}/src/Widget.vue"));
    assert!(
        admitted.is_admitted(),
        "a source under the owning include must be ADMITTED by the cache"
    );
    assert!(
        admitted.bound_carrier().is_some(),
        "an admitted admission retains the resolved BoundCarrier witness"
    );
    assert_eq!(
        admitted.denial(),
        None,
        "an admitted admission has no denial reason"
    );

    // A source OUTSIDE every include fails closed to NoProject — no owned delegation.
    let denied = cache.admit(&host, &format!("{WS_ROOT}/outside/Widget.vue"));
    assert!(
        !denied.is_admitted(),
        "a source outside every configured include must be DENIED (fail closed)"
    );
    assert_eq!(
        denied.denial(),
        Some(AdmissionDenial::NoProject),
        "the exact fail-closed reason is preserved: NoProject"
    );
    assert!(
        denied.bound_carrier().is_none(),
        "a denied admission carries no witness"
    );

    // An unconfigured workspace (empty published snapshot) resolves any source to the
    // NoProject fail-closed variant too.
    let unconfigured = CarrierAdmissionCache::new().admit(
        &host_without_snapshot(),
        &format!("{WS_ROOT}/src/Widget.vue"),
    );
    assert_eq!(
        unconfigured.denial(),
        Some(AdmissionDenial::NoProject),
        "an unconfigured-workspace source fails closed to NoProject"
    );
}

/// MEMOIZATION within ONE publication epoch: at the SAME epoch (the SAME published
/// `Arc<PublishedRoot>`, no republish and no generation change) a repeat admit is a warm
/// memo hit — it returns the SAME retained `BoundCarrier` witness by `Arc` identity, NOT a
/// fresh per-call re-resolve. Discriminates a real epoch-scoped memo from a per-call
/// resolver: a per-call resolver would mint a distinct witness on every call, failing the
/// identity assertion below.
///
/// (A republish that RESETS the published generation to the same scalar value is NOT the
/// same epoch — the publication `Arc` differs — so it is a cold miss, covered by
/// `admission_cache_reconfigure_window_denies_stale_admit_by_publication_identity`.)
#[test]
fn admission_cache_memoizes_within_one_publication_epoch() {
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(owning_snapshot())));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);

    let cache = CarrierAdmissionCache::new();
    let source = format!("{WS_ROOT}/src/Widget.vue");

    let cold = cache.admit(&host, &source);
    assert!(
        cold.is_admitted(),
        "cold admit over the owning snapshot is Admitted"
    );
    let cold_witness: *const _ = cold.bound_carrier().expect("cold admit is admitted");

    // A second admit at the UNCHANGED epoch (no republish, no content bump, no project
    // bump) is a warm memo hit that reuses the SAME retained witness — proving the cache is
    // a real epoch-scoped memo, not a per-call re-resolve (which would mint a distinct
    // witness). `cold` is kept alive so its retained witness `Arc` is not freed underneath
    // the identity comparison.
    let warm = cache.admit(&host, &source);
    assert!(
        warm.is_admitted(),
        "the second admit at the same epoch is Admitted"
    );
    let warm_witness: *const _ = warm.bound_carrier().expect("warm admit is admitted");
    assert!(
        std::ptr::eq(cold_witness, warm_witness),
        "a warm hit within one publication epoch reuses the SAME retained BoundCarrier \
         witness (memoized) — not a per-call re-resolve, which would mint a distinct witness"
    );
}

/// SECURITY-CRITICAL regression (the reconfigure publish-before-bump window): a
/// production `configure_projects` PUBLISHES the new (now non-owning) project graph BEFORE
/// it bumps the monotonic project generation (`host_lifecycle.rs`: publish at ~452, bump
/// at ~489). `ProjectGraph::from_configs` hard-codes every rebuilt graph to generation 1,
/// so the republish RESETS the published `snapshot.generation.0` back to the SAME value
/// (1) with a DISTINCT `Arc<PublishedRoot>` and NO content bump. An `admit` racing INSIDE
/// that window reconstructs a scalar generation tuple byte-identical to the prior OWNING
/// epoch's — yet the carrier is now NON-owning and MUST be denied.
///
/// The admission epoch keys on the UNREPEATABLE publication `Arc<PublishedRoot>` identity
/// (retained, so the pointer cannot be freed-and-reused — the ABA guard), NOT the
/// repeatable `snapshot.generation.0` scalar. So the in-window warm-hit lookup MISSES the
/// stale `Admitted` and re-resolves to the non-owning decision — fail closed. Keying on
/// the scalar (the prior design) served the stale `Admitted` warm from the WARM-HIT
/// lookup path: a fail-OPEN cross-epoch privilege bleed. The prior test hid the defect by
/// bumping BEFORE the republish (the reverse of production ordering).
#[test]
fn admission_cache_reconfigure_window_denies_stale_admit_by_publication_identity() {
    // Epoch A: an OWNING snapshot at published generation 1 — the source is ADMITTED and
    // the decision is warmed into the cache.
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(owning_snapshot())));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);

    let cache = CarrierAdmissionCache::new();
    let source = format!("{WS_ROOT}/src/Widget.vue");

    let published_a = ws.published_root().unwrap().snapshot.generation.0;
    let content_a = ws.content_generation();
    let project_a = host.project_type_store().current_project_generation();

    // `epoch_a` is retained for the whole window: its held witness `Arc` keeps epoch A's
    // allocation alive (modelling the cache's own ABA-guard retention), so the identity
    // comparison below is deterministic.
    let epoch_a = cache.admit(&host, &source);
    assert!(
        epoch_a.is_admitted(),
        "epoch A: the owning-snapshot source is admitted (and warmed into the cache)"
    );
    let epoch_a_witness: *const _ = epoch_a.bound_carrier().expect("epoch A is admitted");

    // PRODUCTION ORDERING — PUBLISH FIRST: republish a NON-owning graph. This RESETS the
    // published generation back to 1 (a DISTINCT `Arc<PublishedRoot>`) with NO content
    // bump, and the monotonic project generation is NOT yet bumped — exactly the window
    // `configure_projects` opens between its publish (~452) and its bump (~489).
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(non_owning_snapshot())));

    // Every scalar dim is byte-identical to epoch A across the reset — the tuple the prior
    // design keyed on COLLIDES with epoch A. ONLY the publication `Arc` identity differs.
    assert_eq!(
        ws.published_root().unwrap().snapshot.generation.0,
        published_a,
        "the republish RESET the published generation to the same value (1)"
    );
    assert_eq!(
        ws.content_generation(),
        content_a,
        "no content bump across the republish"
    );
    assert_eq!(
        host.project_type_store().current_project_generation(),
        project_a,
        "the monotonic project generation is NOT yet bumped — the pre-bump window"
    );

    // QUERY admit INSIDE the window — exercises the WARM-HIT lookup path against the exact
    // colliding scalar tuple. The publication-identity epoch makes it a MISS → cold
    // re-resolve → the non-owning NoProject decision.
    let in_window = cache.admit(&host, &source);

    // Complete the reconfigure: bump the monotonic project generation SECOND (models the
    // full production sequence; the in-window decision above was already taken pre-bump).
    host.project_type_store().bump_project_generation();

    // The in-window admit MUST fail closed — NOT the stale epoch-A `Admitted`.
    assert!(
        !in_window.is_admitted(),
        "the in-window admit must NOT serve the stale epoch-A Admitted — fail closed \
         (the unrepeatable publication identity disambiguates the reset published generation)"
    );
    assert_eq!(
        in_window.denial(),
        Some(AdmissionDenial::NoProject),
        "the reset (now non-owning) epoch re-resolves the source to NoProject"
    );
    // NEGATIVE (witness identity): the fail-closed in-window decision carries NO witness —
    // in particular it is NOT the retained epoch-A `BoundCarrier` witness the stale warm
    // hit would have served by `Arc` identity.
    match in_window.bound_carrier() {
        None => {}
        Some(w) => {
            let w_ptr: *const _ = w;
            assert!(
                !std::ptr::eq(w_ptr, epoch_a_witness),
                "the in-window admit must not serve the stale epoch-A witness by identity"
            );
        }
    }
    assert!(
        in_window.bound_carrier().is_none(),
        "a fail-closed in-window admit carries no witness (not the stale epoch-A witness)"
    );
}

/// The monotonic project generation is STILL a live epoch dimension alongside the
/// publication identity: a project-generation bump WITHOUT a republish (the SAME published
/// `Arc<PublishedRoot>`, same content) is a DISTINCT epoch, so a prior warm `Admitted` is
/// NOT reused — the admit re-resolves at the new epoch (a FRESH witness, not the memoized
/// one). This isolates the project-generation dim from the publication-identity dim the
/// sibling window regression exercises.
#[test]
fn admission_cache_project_generation_bump_is_a_distinct_epoch() {
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(owning_snapshot())));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);

    let cache = CarrierAdmissionCache::new();
    let source = format!("{WS_ROOT}/src/Widget.vue");

    let first = cache.admit(&host, &source);
    assert!(
        first.is_admitted(),
        "the owning source is admitted at epoch A"
    );
    let first_witness: *const _ = first.bound_carrier().expect("epoch A is admitted");

    // Bump the monotonic project generation with NO republish — the published root `Arc`
    // and content are unchanged, so ONLY the project-generation dim advances.
    host.project_type_store().bump_project_generation();

    let after = cache.admit(&host, &source);
    // The owning snapshot is unchanged, so the fresh resolve is STILL Admitted...
    assert!(
        after.is_admitted(),
        "the owning source re-resolves to admitted at the new epoch"
    );
    // ...but it is a COLD re-resolve at the new epoch, NOT the memoized epoch-A witness:
    // the project-generation dim invalidated the prior entry. `first` is retained so its
    // witness allocation is not freed underneath the comparison.
    let after_witness: *const _ = after.bound_carrier().expect("re-resolve is admitted");
    assert!(
        !std::ptr::eq(first_witness, after_witness),
        "a project-generation bump is a distinct epoch — the admit re-resolves (a fresh \
         witness), not a memoized warm hit; the project-generation dim still participates"
    );
}

// ── EXTERNAL-ONLY carrier features (type_definition, signature_help, semantic_tokens):
//    a denied carrier serves the empty/none external default with NO owned delegation
//    (counter == 0); a bound carrier delegates (the OWNED marker surfaces, counter == 1);
//    a plain `.ts` is UNGATED (delegates even pre-snapshot). ──

#[tokio::test]
async fn feature_external_only_denied_carrier_serves_empty_no_owned_call() {
    let (c, owned) = composite_with_owned(host_without_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    let type_def = c
        .get_type_definition(&carrier, 0)
        .await
        .expect("type_definition");
    assert!(
        type_def.is_empty(),
        "a denied carrier's type_definition must be the empty external default (no marker \
         leak); got {type_def:?}"
    );
    let sig = c
        .get_signature_help(&carrier, 0)
        .await
        .expect("signature_help");
    assert!(
        sig.is_none(),
        "a denied carrier's signature_help must be None"
    );
    let toks = c
        .get_semantic_tokens(&carrier)
        .await
        .expect("semantic_tokens");
    assert!(
        toks.is_empty(),
        "a denied carrier's semantic_tokens must be the empty external default"
    );

    assert_eq!(
        owned.type_definition_calls.load(Ordering::SeqCst),
        0,
        "a denied carrier must NOT delegate type_definition to OWNED (no `--lsp` fall-through)"
    );
    assert_eq!(
        owned.signature_help_calls.load(Ordering::SeqCst),
        0,
        "a denied carrier must NOT delegate signature_help to OWNED"
    );
    assert_eq!(
        owned.semantic_tokens_calls.load(Ordering::SeqCst),
        0,
        "a denied carrier must NOT delegate semantic_tokens to OWNED"
    );
}

#[tokio::test]
async fn feature_external_only_bound_carrier_delegates_to_owned() {
    let (c, owned) = composite_with_owned(host_with_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    assert!(
        !c.get_type_definition(&carrier, 0).await.unwrap().is_empty(),
        "a BOUND carrier delegates type_definition — the OWNED marker surfaces"
    );
    assert!(
        c.get_signature_help(&carrier, 0).await.unwrap().is_some(),
        "a BOUND carrier delegates signature_help — the OWNED marker surfaces"
    );
    assert!(
        !c.get_semantic_tokens(&carrier).await.unwrap().is_empty(),
        "a BOUND carrier delegates semantic_tokens — the OWNED marker surfaces"
    );

    assert_eq!(owned.type_definition_calls.load(Ordering::SeqCst), 1);
    assert_eq!(owned.signature_help_calls.load(Ordering::SeqCst), 1);
    assert_eq!(owned.semantic_tokens_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn feature_external_only_plain_ts_is_ungated() {
    // A plain `.ts` (NOT a carrier companion) is NOT gated — it delegates to OWNED even
    // with NO published snapshot (the feature gate is carrier-scoped, exactly like the
    // diagnostics gate).
    let (c, owned) = composite_with_owned(host_without_snapshot());
    let plain = format!("{WS_ROOT}/src/plain.ts");

    assert!(
        !c.get_type_definition(&plain, 0).await.unwrap().is_empty(),
        "a plain `.ts` type_definition is UNGATED — it delegates to OWNED (marker surfaces)"
    );
    assert_eq!(
        owned.type_definition_calls.load(Ordering::SeqCst),
        1,
        "a plain `.ts` delegates to OWNED (ungated) even pre-snapshot"
    );
}

// ── MIXED-READ carrier features (hover, definition, references, document_highlights,
//    inlay_hints): a denied carrier serves the empty/none EXTERNAL DEFAULT with NO owned
//    delegation (counter == 0) — the LSP handler then preserves the native sub-answer via
//    its merge, never a `--lsp` self-discovery fall-through; a bound carrier delegates. ──

#[tokio::test]
async fn feature_mixed_read_denied_carrier_serves_external_default_no_owned_call() {
    let (c, owned) = composite_with_owned(host_without_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    assert!(
        c.get_hover(&carrier, 0).await.expect("hover").is_none(),
        "a denied carrier's hover must be the None external default (native preserved by \
         the handler merge)"
    );
    assert!(
        c.get_definition(&carrier, 0)
            .await
            .expect("definition")
            .is_empty(),
        "a denied carrier's definition must be the empty external default"
    );
    assert!(
        c.get_references(&carrier, 0)
            .await
            .expect("references")
            .is_empty(),
        "a denied carrier's references must be the empty external default"
    );
    assert!(
        c.get_document_highlights(&carrier, 0)
            .await
            .expect("document_highlights")
            .is_empty(),
        "a denied carrier's document_highlights must be the empty external default"
    );
    assert!(
        c.get_inlay_hints(&carrier, 0, 1)
            .await
            .expect("inlay_hints")
            .is_empty(),
        "a denied carrier's inlay_hints must be the empty external default"
    );

    assert_eq!(
        owned.hover_calls.load(Ordering::SeqCst),
        0,
        "denied: no owned hover"
    );
    assert_eq!(
        owned.definition_calls.load(Ordering::SeqCst),
        0,
        "denied: no owned definition"
    );
    assert_eq!(
        owned.references_calls.load(Ordering::SeqCst),
        0,
        "denied: no owned references"
    );
    assert_eq!(
        owned.document_highlights_calls.load(Ordering::SeqCst),
        0,
        "denied: no owned document_highlights"
    );
    assert_eq!(
        owned.inlay_hints_calls.load(Ordering::SeqCst),
        0,
        "denied: no owned inlay_hints"
    );
}

#[tokio::test]
async fn feature_mixed_read_bound_carrier_delegates_to_owned() {
    let (c, owned) = composite_with_owned(host_with_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    assert!(
        c.get_hover(&carrier, 0).await.unwrap().is_some(),
        "a BOUND carrier delegates hover — the OWNED marker surfaces"
    );
    assert!(
        !c.get_definition(&carrier, 0).await.unwrap().is_empty(),
        "a BOUND carrier delegates definition"
    );
    assert!(
        !c.get_references(&carrier, 0).await.unwrap().is_empty(),
        "a BOUND carrier delegates references"
    );
    assert!(
        !c.get_document_highlights(&carrier, 0)
            .await
            .unwrap()
            .is_empty(),
        "a BOUND carrier delegates document_highlights"
    );
    assert!(
        !c.get_inlay_hints(&carrier, 0, 1).await.unwrap().is_empty(),
        "a BOUND carrier delegates inlay_hints"
    );

    assert_eq!(owned.hover_calls.load(Ordering::SeqCst), 1);
    assert_eq!(owned.definition_calls.load(Ordering::SeqCst), 1);
    assert_eq!(owned.references_calls.load(Ordering::SeqCst), 1);
    assert_eq!(owned.document_highlights_calls.load(Ordering::SeqCst), 1);
    assert_eq!(owned.inlay_hints_calls.load(Ordering::SeqCst), 1);
}

// ── COMPLETION + RESOLVE carrier features (get_completions, get_completion_details,
//    resolve_completion): a denied carrier serves native completions only — the empty
//    external default with NO owned delegation, and resolve enrichment is SUPPRESSED
//    (None) when admission is absent/stale; a bound carrier delegates. ──

fn lsp_resolve_data() -> CompletionResolveData {
    CompletionResolveData::Lsp {
        label: "x".to_string(),
        data: serde_json::Value::Null,
    }
}

#[tokio::test]
async fn feature_completion_denied_carrier_serves_native_only_no_owned_call() {
    let (c, owned) = composite_with_owned(host_without_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    let completions = c
        .get_completions(&carrier, 0, None)
        .await
        .expect("completions");
    assert!(
        !completions.is_incomplete && completions.items.is_empty(),
        "a denied carrier's completions must be the empty external default (the OWNED \
         `is_incomplete: true` marker must NOT leak)"
    );
    let details = c
        .get_completion_details(&carrier, 0, &[])
        .await
        .expect("completion_details");
    assert!(
        details.is_empty(),
        "a denied carrier's completion_details must be the empty external default"
    );
    // resolve enrichment is SUPPRESSED (None) when admission is absent.
    let resolved = c
        .resolve_completion(&carrier, lsp_resolve_data())
        .await
        .expect("resolve_completion");
    assert!(
        resolved.is_none(),
        "a denied carrier's resolve_completion must suppress provider enrichment (None)"
    );

    assert_eq!(
        owned.completions_calls.load(Ordering::SeqCst),
        0,
        "denied: no owned completions"
    );
    assert_eq!(
        owned.completion_details_calls.load(Ordering::SeqCst),
        0,
        "denied: no owned completion_details"
    );
    assert_eq!(
        owned.resolve_completion_calls.load(Ordering::SeqCst),
        0,
        "denied: no owned resolve_completion enrichment"
    );
}

#[tokio::test]
async fn feature_completion_bound_carrier_delegates_to_owned() {
    let (c, owned) = composite_with_owned(host_with_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    assert!(
        c.get_completions(&carrier, 0, None)
            .await
            .unwrap()
            .is_incomplete,
        "a BOUND carrier delegates completions — the OWNED `is_incomplete` marker surfaces"
    );
    // completion_details echoes the input items; delegation is proven by the counter.
    let _ = c.get_completion_details(&carrier, 0, &[]).await.unwrap();
    assert!(
        c.resolve_completion(&carrier, lsp_resolve_data())
            .await
            .unwrap()
            .is_some(),
        "a BOUND carrier delegates resolve_completion — the OWNED enrichment surfaces"
    );

    assert_eq!(owned.completions_calls.load(Ordering::SeqCst), 1);
    assert_eq!(owned.completion_details_calls.load(Ordering::SeqCst), 1);
    assert_eq!(owned.resolve_completion_calls.load(Ordering::SeqCst), 1);
}

// ── RENAME carrier feature (get_rename_locations): a denied carrier serves the empty
//    external default with NO owned delegation (native-only where safe) — the LSP
//    handler's existing incomplete-rename safety gates (a SEPARATE layer this gate does
//    not touch) still block unsafe partial edits; a bound carrier delegates. ──

#[tokio::test]
async fn feature_rename_denied_carrier_serves_native_only_no_owned_call() {
    let (c, owned) = composite_with_owned(host_without_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    let renames = c
        .get_rename_locations(&carrier, 0)
        .await
        .expect("rename_locations");
    assert!(
        renames.is_empty(),
        "a denied carrier's rename_locations must be the empty external default (native \
         only) — the OWNED provider rename must NOT leak (never a `--lsp` self-discovery \
         fall-through)"
    );
    assert_eq!(
        owned.rename_locations_calls.load(Ordering::SeqCst),
        0,
        "a denied carrier must NOT delegate rename_locations to OWNED"
    );
}

#[tokio::test]
async fn feature_rename_bound_carrier_delegates_to_owned() {
    let (c, owned) = composite_with_owned(host_with_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    assert!(
        !c.get_rename_locations(&carrier, 0)
            .await
            .unwrap()
            .is_empty(),
        "a BOUND carrier delegates rename_locations — the OWNED marker surfaces"
    );
    assert_eq!(owned.rename_locations_calls.load(Ordering::SeqCst), 1);
}

// ── CODE-ACTIONS carrier feature (get_code_actions): MIXED — the LSP handler
//    (`handle_code_action`) contributes native Verter carrier code-actions (organize
//    imports, extract component, macro/component/event actions, action-engine fixes) and
//    MERGES the provider's `get_code_actions` (`getCodeFixes` quickfixes) over them. A
//    denied carrier serves the empty EXTERNAL DEFAULT with NO owned delegation (counter
//    == 0) — the handler merge preserves the native side, never a `--lsp` self-discovery
//    fall-through; a bound carrier delegates (the OWNED marker surfaces, counter == 1). ──

#[tokio::test]
async fn feature_code_actions_denied_carrier_serves_native_only_no_owned_call() {
    let (c, owned) = composite_with_owned(host_without_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    let actions = c
        .get_code_actions(&carrier, 0, 1, &[])
        .await
        .expect("code_actions");
    assert!(
        actions.is_empty(),
        "a denied carrier's code_actions must be the empty external default (native \
         preserved by the handler merge) — the OWNED provider marker must NOT leak (never a \
         `--lsp` self-discovery fall-through); got {actions:?}"
    );
    assert_eq!(
        owned.code_actions_calls.load(Ordering::SeqCst),
        0,
        "a denied carrier must NOT delegate code_actions to OWNED (no `--lsp` fall-through)"
    );
}

#[tokio::test]
async fn feature_code_actions_bound_carrier_delegates_to_owned() {
    let (c, owned) = composite_with_owned(host_with_snapshot());
    let carrier = format!("{WS_ROOT}/src/Widget.vue.tsx");

    assert!(
        !c.get_code_actions(&carrier, 0, 1, &[])
            .await
            .unwrap()
            .is_empty(),
        "a BOUND carrier delegates code_actions — the OWNED marker surfaces"
    );
    assert_eq!(owned.code_actions_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn feature_code_actions_plain_ts_is_ungated() {
    // A plain `.ts` (NOT a carrier companion) is NOT gated — it delegates to OWNED even
    // with NO published snapshot (the feature gate is carrier-scoped).
    let (c, owned) = composite_with_owned(host_without_snapshot());
    let plain = format!("{WS_ROOT}/src/plain.ts");

    assert!(
        !c.get_code_actions(&plain, 0, 1, &[])
            .await
            .unwrap()
            .is_empty(),
        "a plain `.ts` code_actions is UNGATED — it delegates to OWNED (marker surfaces)"
    );
    assert_eq!(
        owned.code_actions_calls.load(Ordering::SeqCst),
        1,
        "a plain `.ts` delegates to OWNED (ungated) even pre-snapshot"
    );
}
