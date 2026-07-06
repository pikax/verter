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

use std::sync::Arc;

use verter_lsp::tsgo::composite::TsgoCompositeProvider;
use verter_lsp::tsgo::project_binding::{resolve_carrier, resolve_carrier_bound, CarrierBinding};
use verter_lsp::type_provider::protocol::{
    CompletionResult, HoverInfo, InlayHint, ProviderDiagnosticContext, RenameLocation,
    SemanticToken, SignatureHelp, TypeCodeAction, TypeDiagnostic, TypeDiagnosticSeverity,
    TypeDocumentHighlight, TypeLocation,
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
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, WorkspaceAccess};

const WS_ROOT: &str = "/binding-gate-ws";
const TSCONFIG: &str = "/binding-gate-ws/tsconfig.json";

/// The distinctive OWNED-only marker diagnostic every double path surfaces — a gated
/// (empty) result proves the OWNED `--lsp` fall-through was blocked.
const MARKER_CODE: &str = "9999";

/// A minimal OWNED baseline double: EVERY diagnostics query surfaces the MARKER
/// diagnostic (foreground + background), so a gated-to-empty composite result can
/// only mean the gate blocked OWNED's fall-through.
struct MarkerOwned;

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
        Box::pin(async move {
            Ok(CompletionResult {
                items: Vec::new(),
                is_incomplete: false,
            })
        })
    }
    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        Box::pin(async move { Ok(None) })
    }
    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async move { Ok(None) })
    }
    fn get_code_actions(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
        _diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn get_inlay_hints(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async move { Ok(Vec::new()) })
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
    TsgoCompositeProvider::new(Arc::new(MarkerOwned), host, None)
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
