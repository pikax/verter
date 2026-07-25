use std::borrow::Cow;
use std::sync::Arc;

use dashmap::{DashMap, DashSet};

use crate::carrier_provider_projection::PreparedCarrierProviderContent;
use crate::type_provider::protocol::TypeProviderError;
use crate::type_provider::traits::TypeProvider;
use crate::{ProjectSyncMode, TypeProviderKind};

mod virtual_types;

const VERTER_TYPES_VIRTUAL_DTS: &str = include_str!("../verter_types_stub.d.ts");

/// Immutable evidence of the exact carrier bytes a provider operation
/// successfully delivered. Its fields are private: only `ProjectSync`, which
/// observes the successful provider result, can mint this value. The tsgo
/// post-open receipt consumes it so membership cannot attest pre-adaptation
/// compiler bytes while the engine serves an owner-specialized carrier.
#[derive(Clone)]
pub(crate) struct SyncedTsxSurface {
    path: Arc<str>,
    delivered: PreparedCarrierProviderContent,
}

impl SyncedTsxSurface {
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn content(&self) -> &Arc<str> {
        self.delivered.content()
    }

    /// The delivered bytes together with the mapper describing them — the value a
    /// recorder must carry forward whole.
    pub(crate) fn delivered(&self) -> &PreparedCarrierProviderContent {
        &self.delivered
    }
}

pub(super) struct PreparedTsxContent {
    prepared: PreparedCarrierProviderContent,
    virtual_verter_types_path: Option<String>,
}

#[derive(Clone, Copy)]
enum ProviderLane {
    Foreground,
    Background,
    Normal,
}

#[derive(Clone, Copy)]
enum ProviderFileVerb {
    Load,
    Open,
    Update,
}

/// Syncs project files to a `TypeProvider`.
///
/// In `FullProject` mode, `.vue` outputs and non-carrier source files are all sent.
///
/// For the tsserver engine a framework carrier (`.vue`/`.svelte`) reaches the
/// type provider as a member of its REAL configured project, served by the
/// `@verter/typescript-plugin` from the on-disk carrier-publish store — NOT by
/// opening the synthetic `.vue.tsx`/`.verter.ts` companions into tsserver. So
/// the contentful carrier-companion verbs (`open_tsx`/`sync_tsx`/`open_dts`/
/// `sync_dts`/`load_tsx`/`load_dts`) are SUPPRESSED here for tsserver: the plugin
/// is the sole carrier-content authority, and a second `provider.open_file(path,
/// content)` of a generated companion would be a competing carrier authority
/// that bypasses the store's content-hash/version/map identity. The `close_*`
/// verbs, the non-carrier real-file shadow verbs (`sync_file`/`load_file`/
/// `close_file`), and `configure_paths` stay active for every engine (a close /
/// a real-file shadow / a path config is not a carrier-companion content open).
///
/// tsgo is ALSO project-bound, not a single-file fallback: a tsgo carrier-companion's
/// CONTENT flows through the shared `--lsp` session (the contentful verbs are NOT
/// suppressed for tsgo), while the `--api` checker reaches the carrier as a member of
/// the configured project it OPENS — `update_snapshot_open_project(tsconfig)` selects
/// the configured project for that tsconfig and REQUIRES the carrier in
/// `project.root_files`, failing closed when it is absent. tsgo therefore routes
/// carrier membership through `open_project`, never an inferred/single-file Program.
#[derive(Clone)]
pub struct ProjectSync {
    provider: Arc<dyn TypeProvider>,
    mode: ProjectSyncMode,
    kind: TypeProviderKind,
    /// Last successfully delivered carrier bytes, keyed by provider path.
    /// Shared across clones because server/background producers must record the
    /// same immutable provider surface that the engine actually received.
    delivered_carrier_surfaces: Arc<DashMap<String, PreparedCarrierProviderContent>>,
    /// Adjacent provider-only `@verter/types` declaration overlays currently
    /// published for tsgo carriers.
    virtual_verter_types_paths: Arc<DashSet<String>>,
    /// Serializes each carrier with its adjacent declaration overlay so
    /// concurrent foreground/background publications cannot reorder the pair.
    virtual_verter_types_locks: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Published workspace policy used to project generated carrier imports.
    /// Absent in isolated unit tests; production binds the server's atomic VFS.
    workspace: Option<Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>>>,
}

impl ProjectSync {
    /// Build a `ProjectSync` whose carrier-companion content opens are NOT
    /// suppressed (the tsgo content-transport semantics: a carrier-companion's content
    /// flows through the shared `--lsp` session, while project membership is owned by
    /// the `--api` checker's `open_project`). Production code routes through
    /// [`Self::new_with_kind`] with the real engine kind; this remains for the
    /// in-process provider-sync unit tests that assert the raw `open_file`/
    /// `update_file` carrier verbs.
    pub fn new(provider: Arc<dyn TypeProvider>, mode: ProjectSyncMode) -> Self {
        Self {
            provider,
            mode,
            // Default to the tsgo content-transport semantics: the carrier-companion
            // content opens flow through (membership is owned separately by the
            // `--api` checker's configured-project `open_project`). tsserver's
            // store-publish suppression is opt-in via `new_with_kind`.
            kind: TypeProviderKind::Tsgo,
            delivered_carrier_surfaces: Arc::new(DashMap::new()),
            virtual_verter_types_paths: Arc::new(DashSet::new()),
            virtual_verter_types_locks: Arc::new(DashMap::new()),
            workspace: None,
        }
    }

    /// Build a `ProjectSync` bound to the active engine kind. For tsserver the
    /// contentful carrier-companion verbs are suppressed (the plugin serves the
    /// carrier from the publish store); for tsgo / no-provider they flow through.
    pub fn new_with_kind(
        provider: Arc<dyn TypeProvider>,
        mode: ProjectSyncMode,
        kind: TypeProviderKind,
    ) -> Self {
        Self {
            provider,
            mode,
            kind,
            delivered_carrier_surfaces: Arc::new(DashMap::new()),
            virtual_verter_types_paths: Arc::new(DashSet::new()),
            virtual_verter_types_locks: Arc::new(DashMap::new()),
            workspace: None,
        }
    }

    /// Build a production sync bound to the same atomic workspace snapshot
    /// used by ownership and import resolution.
    pub fn new_with_kind_and_workspace(
        provider: Arc<dyn TypeProvider>,
        mode: ProjectSyncMode,
        kind: TypeProviderKind,
        workspace: Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>>,
    ) -> Self {
        Self {
            provider,
            mode,
            kind,
            delivered_carrier_surfaces: Arc::new(DashMap::new()),
            virtual_verter_types_paths: Arc::new(DashSet::new()),
            virtual_verter_types_locks: Arc::new(DashMap::new()),
            workspace: Some(workspace),
        }
    }

    fn record_delivered_carrier_surface(
        &self,
        path: &str,
        delivered: PreparedCarrierProviderContent,
    ) {
        self.delivered_carrier_surfaces
            .insert(path.to_owned(), delivered);
    }

    fn retract_delivered_carrier_surface(&self, path: &str) {
        self.delivered_carrier_surfaces.remove(path);
    }

    /// The exact bytes AND rewrite mapper from the most recent successful carrier
    /// sync. A provider-surface snapshot must use this value rather than
    /// independently re-running provider specialization, which could observe a
    /// changed owner installation between the provider call and surface
    /// publication.
    pub(crate) fn delivered_carrier_surface(
        &self,
        path: &str,
    ) -> Option<PreparedCarrierProviderContent> {
        self.delivered_carrier_surfaces
            .get(path)
            .map(|entry| entry.value().clone())
    }

    /// Return the immutable bytes from the most recent successful carrier sync.
    #[cfg(test)]
    pub(crate) fn synced_tsx_content(&self, path: &str) -> Option<Arc<str>> {
        self.delivered_carrier_surface(path)
            .map(|delivered| Arc::clone(delivered.content()))
    }

    /// THE single answer to "what bytes does the provider hold for this carrier
    /// companion, and how do generated positions map into them".
    ///
    /// The delivered ledger wins when this sync actually published the path.
    /// Otherwise the answer is produced by [`Self::prepare_tsx_surface`] — the
    /// SAME function the publication runs — so a read taken before (or without)
    /// a publication models exactly the bytes a publication would deliver. A
    /// second, narrower re-derivation here would omit projections the delivery
    /// applies for this engine (the adjacent `@verter/types` overlay redirect,
    /// the managed-tsgo JSX specialization) and shift every offset past a
    /// rewritten import.
    ///
    /// `None` when the buffer cannot be modelled at all — fail closed: a caller
    /// that cannot say what the provider holds must record nothing rather than
    /// record bytes no engine has.
    pub(crate) fn carrier_provider_surface(
        &self,
        path: &str,
        generated: &str,
    ) -> Option<PreparedCarrierProviderContent> {
        match self.delivered_carrier_surface(path) {
            Some(delivered) => Some(delivered),
            None => self
                .prepare_tsx_surface(path, generated)
                .ok()
                .map(|prepared| prepared.prepared),
        }
    }

    /// Mint post-open evidence only for a path whose last provider operation
    /// completed successfully and remains live in the exact-byte ledger.
    pub(crate) fn synced_tsx_surface(&self, path: &str) -> Option<SyncedTsxSurface> {
        self.delivered_carrier_surface(path)
            .map(|delivered| SyncedTsxSurface {
                path: Arc::from(path),
                delivered,
            })
    }

    /// Whether a contentful carrier-companion open (`open_tsx`/`sync_tsx`/
    /// `open_dts`/`sync_dts`/`load_tsx`/`load_dts`) is suppressed for the active
    /// engine. True for tsserver: the carrier reaches tsserver as a
    /// configured-project member through the plugin's store-backed
    /// `getExternalFiles`, so the LSP must NOT open the synthetic companion as a
    /// competing content authority. The publish path (and `notify_carrier_changed`)
    /// is the membership + invalidation mechanism instead.
    #[inline]
    fn carrier_companion_open_suppressed(&self) -> bool {
        matches!(self.kind, TypeProviderKind::Tsserver)
    }

    /// Load a DTS file (.vue.ts) into the type provider for import resolution only.
    /// Unlike `open_dts`, this does NOT trigger diagnostics in providers that support it.
    ///
    /// Suppressed (no-op `Ok`) for tsserver: the carrier public-API companion is
    /// served from the publish store, never loaded as content here.
    pub async fn load_dts(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }
        self.provider.load_file(dts_path, dts_content).await
    }

    /// Open a new DTS file (.vue.ts) in the type provider.
    ///
    /// Suppressed (no-op `Ok`) for tsserver: the carrier public-API companion
    /// becomes a configured-project member via the plugin's store-backed
    /// `getExternalFiles`, never opened as content here.
    pub async fn open_dts(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }
        self.provider.open_file(dts_path, dts_content).await
    }

    /// Sync a Vue file's DTS representation (.vue.ts) to the type provider.
    ///
    /// Suppressed (no-op `Ok`) for tsserver: the carrier public-API companion's
    /// content flows to tsserver through the publish store, not a direct
    /// `provider.update_file`.
    pub async fn sync_dts(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }
        self.provider.update_file(dts_path, dts_content).await
    }

    /// Close a DTS file in the type provider.
    pub async fn close_dts(&self, dts_path: &str) -> Result<(), TypeProviderError> {
        self.provider.close_file(dts_path).await
    }

    /// Load a non-carrier file into the type provider for import resolution only.
    /// Unlike `sync_file`, this uses `load_file` (background semantics — no diagnostics).
    pub async fn load_file(&self, path: &str, content: &str) -> Result<(), TypeProviderError> {
        self.provider.load_file(path, content).await
    }

    /// Sync a non-carrier file to the type provider.
    pub async fn sync_file(&self, path: &str, content: &str) -> Result<(), TypeProviderError> {
        self.provider.update_file(path, content).await
    }

    /// Close a non-carrier provider file.
    pub async fn close_file(&self, path: &str) -> Result<(), TypeProviderError> {
        self.provider.close_file(path).await
    }

    pub fn mode(&self) -> ProjectSyncMode {
        self.mode
    }

    // ── Background-priority variants ─────────────────────────────────

    pub async fn load_tsx_background(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        // A carrier-companion content open is suppressed for tsserver in EVERY
        // priority lane (foreground/background/normal): the companion reaches
        // tsserver as a store-backed `getExternalFiles` member, never a competing
        // direct content open.
        self.publish_tsx(
            tsx_path,
            tsx_content,
            ProviderLane::Background,
            ProviderFileVerb::Load,
        )
        .await
    }

    pub async fn open_tsx_background(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.publish_tsx(
            tsx_path,
            tsx_content,
            ProviderLane::Background,
            ProviderFileVerb::Open,
        )
        .await
    }

    pub async fn sync_tsx_background(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.publish_tsx(
            tsx_path,
            tsx_content,
            ProviderLane::Background,
            ProviderFileVerb::Update,
        )
        .await
    }

    pub async fn close_tsx_background(&self, tsx_path: &str) -> Result<(), TypeProviderError> {
        self.close_tsx_in_lane(tsx_path, ProviderLane::Background)
            .await
    }

    pub async fn load_dts_background(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }
        self.provider
            .load_file_background(dts_path, dts_content)
            .await
    }

    pub async fn open_dts_background(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }
        self.provider
            .open_file_background(dts_path, dts_content)
            .await
    }

    pub async fn sync_dts_background(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }
        self.provider
            .update_file_background(dts_path, dts_content)
            .await
    }

    pub async fn close_dts_background(&self, dts_path: &str) -> Result<(), TypeProviderError> {
        self.provider.close_file_background(dts_path).await
    }

    pub async fn load_file_background(
        &self,
        path: &str,
        content: &str,
    ) -> Result<(), TypeProviderError> {
        self.provider.load_file_background(path, content).await
    }

    pub async fn sync_file_background(
        &self,
        path: &str,
        content: &str,
    ) -> Result<(), TypeProviderError> {
        self.provider.update_file_background(path, content).await
    }

    pub async fn close_file_background(&self, path: &str) -> Result<(), TypeProviderError> {
        self.provider.close_file_background(path).await
    }

    // ── Normal-priority variants ─────────────────────────────────────

    pub async fn load_tsx_normal(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.publish_tsx(
            tsx_path,
            tsx_content,
            ProviderLane::Normal,
            ProviderFileVerb::Load,
        )
        .await
    }

    pub async fn open_tsx_normal(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.publish_tsx(
            tsx_path,
            tsx_content,
            ProviderLane::Normal,
            ProviderFileVerb::Open,
        )
        .await
    }

    pub async fn sync_tsx_normal(
        &self,
        tsx_path: &str,
        tsx_content: &str,
    ) -> Result<(), TypeProviderError> {
        self.publish_tsx(
            tsx_path,
            tsx_content,
            ProviderLane::Normal,
            ProviderFileVerb::Update,
        )
        .await
    }

    pub async fn close_tsx_normal(&self, tsx_path: &str) -> Result<(), TypeProviderError> {
        self.close_tsx_in_lane(tsx_path, ProviderLane::Normal).await
    }

    pub async fn load_dts_normal(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }
        self.provider.load_file_normal(dts_path, dts_content).await
    }

    pub async fn open_dts_normal(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }
        self.provider.open_file_normal(dts_path, dts_content).await
    }

    pub async fn sync_dts_normal(
        &self,
        dts_path: &str,
        dts_content: &str,
    ) -> Result<(), TypeProviderError> {
        if self.carrier_companion_open_suppressed() {
            return Ok(());
        }
        self.provider
            .update_file_normal(dts_path, dts_content)
            .await
    }

    pub async fn close_dts_normal(&self, dts_path: &str) -> Result<(), TypeProviderError> {
        self.provider.close_file_normal(dts_path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_provider::mock::{FailingTypeProvider, MockCall, MockTypeProvider};

    #[tokio::test]
    async fn managed_tsgo_records_the_exact_svelte_carrier_delivered_to_the_provider() {
        let owner = tempfile::tempdir().expect("temporary Svelte owner");
        let svelte_dir = owner.path().join("node_modules/svelte");
        std::fs::create_dir_all(&svelte_dir).expect("Svelte package directory");
        std::fs::write(
            svelte_dir.join("package.json"),
            r#"{"name":"svelte","version":"5.0.0","types":"./index.d.ts","exports":{".":{"types":"./index.d.ts"},"./elements":{"types":"./elements.d.ts"}}}"#,
        )
            .expect("Svelte package marker");
        std::fs::write(
            svelte_dir.join("index.d.ts"),
            "export type Snippet = () => unknown;\n",
        )
        .expect("Svelte public declarations");
        std::fs::write(
            svelte_dir.join("elements.d.ts"),
            "export interface SvelteHTMLElements { div: {}; }\n",
        )
        .expect("Svelte elements declarations");
        let provider_path = owner.path().join("src/Component.svelte.tsx");
        std::fs::create_dir_all(provider_path.parent().expect("provider parent"))
            .expect("provider parent directory");
        let provider_path = provider_path.to_string_lossy().into_owned();
        let source = "/** @jsxImportSource @verter/svelte-jsx */\nconst view = <div />;\n";

        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );

        sync.open_tsx(&provider_path, source)
            .await
            .expect("managed tsgo open succeeds");
        let calls = mock.file_sync_calls();
        let delivered = match calls.as_slice() {
            [MockCall::OpenFile { path, content }] => {
                assert_eq!(path, &provider_path);
                content.as_str()
            }
            other => panic!("expected one provider open, got {other:?}"),
        };
        assert_ne!(delivered, source, "the managed tsgo carrier is specialized");
        assert_eq!(delivered.lines().count(), source.lines().count());
        assert_eq!(
            sync.synced_tsx_content(&provider_path).as_deref(),
            Some(delivered),
            "surface consumers observe the exact bytes delivered to tsgo"
        );

        sync.close_tsx(&provider_path)
            .await
            .expect("managed tsgo close succeeds");
        assert!(
            sync.synced_tsx_content(&provider_path).is_none(),
            "a closed provider buffer cannot remain an authoritative surface"
        );
    }

    /// A recorder that reads the carrier surface BEFORE this sync has published
    /// anything must model the SAME provider bytes the publication will deliver.
    ///
    /// The two reads answer one question — "what does the provider hold for this
    /// carrier" — so a pre-publication read that omits a projection the delivery
    /// applies is a second, narrower model of the same edits. It shifts every
    /// offset past a rewritten import (24 bytes for this specifier pair), so a
    /// surface recorded from it maps positions into a buffer no engine holds.
    #[tokio::test]
    async fn carrier_provider_surface_is_identical_before_and_after_delivery() {
        let owner = tempfile::tempdir().expect("temporary owner");
        let provider_path = owner.path().join("src/App.vue.tsx");
        std::fs::create_dir_all(provider_path.parent().expect("provider parent"))
            .expect("provider parent directory");
        let provider_path = provider_path.to_string_lossy().into_owned();
        let source = "import type { GlobalComponentType } from \"@verter/types\";\n\
                      type T = GlobalComponentType<\"X\">;\n";

        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );

        let before = sync
            .carrier_provider_surface(&provider_path, source)
            .expect("a projectable carrier models a provider surface before publication");
        sync.open_tsx(&provider_path, source)
            .await
            .expect("managed tsgo open succeeds");
        let after = sync
            .carrier_provider_surface(&provider_path, source)
            .expect("the delivered surface is readable");

        assert!(
            after
                .content()
                .contains("from \"./App.vue.tsx.__verter_types\""),
            "the delivered buffer redirects onto the adjacent overlay: {}",
            after.content()
        );
        assert!(
            !after.content().contains("from \"@verter/types\""),
            "the delivered buffer keeps no unresolvable package specifier: {}",
            after.content()
        );
        assert_eq!(
            before.content(),
            after.content(),
            "a pre-publication read must model the delivered bytes, not a narrower projection"
        );
        assert_eq!(
            before.rewrites(),
            after.rewrites(),
            "the mapper must describe the same rewrites in both reads"
        );
    }

    fn make_sync(mock: &MockTypeProvider, mode: ProjectSyncMode) -> ProjectSync {
        ProjectSync::new(Arc::new(mock.clone()), mode)
    }

    fn make_sync_failing(provider: &FailingTypeProvider, mode: ProjectSyncMode) -> ProjectSync {
        // FailingTypeProvider is not Clone, so wrap directly. These tests assert
        // the carrier verbs reach the provider (error propagation), so the kind
        // must NOT suppress the carrier-companion opens — use the tsgo default.
        ProjectSync {
            provider: Arc::new(FailingTypeProvider::new(&provider.error_message)),
            mode,
            kind: TypeProviderKind::Tsgo,
            delivered_carrier_surfaces: Arc::new(DashMap::new()),
            virtual_verter_types_paths: Arc::new(DashSet::new()),
            virtual_verter_types_locks: Arc::new(DashMap::new()),
            workspace: None,
        }
    }

    /// @ai-generated — TSX sync sends update_file in both modes
    #[tokio::test]
    async fn tsx_sync_sends_update_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.sync_tsx("App.vue.tsx", "export default {}")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::UpdateFile { path, content } => {
                assert_eq!(path, "App.vue.tsx");
                assert_eq!(content, "export default {}");
            }
            _ => panic!("expected UpdateFile"),
        }
    }

    /// @ai-generated — open_tsx sends open_file
    #[tokio::test]
    async fn open_tsx_sends_open_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.open_tsx("App.vue.tsx", "const x = 1;").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::OpenFile { path, content } => {
                assert_eq!(path, "App.vue.tsx");
                assert_eq!(content, "const x = 1;");
            }
            _ => panic!("expected OpenFile"),
        }
    }

    /// Missing `@verter/types` is served as a provider-only declaration overlay.
    #[tokio::test]
    async fn managed_tsgo_serves_verter_types_as_an_adjacent_virtual_module_when_absent() {
        let owner = tempfile::tempdir().expect("temporary owner");
        let provider_path = owner.path().join("src/App.vue.tsx");
        std::fs::create_dir_all(provider_path.parent().unwrap()).expect("provider parent");
        let provider_path = provider_path.to_string_lossy().into_owned();
        let source =
            "import type { GlobalComponentType } from \"@verter/types\";\ntype T = GlobalComponentType<\"X\">;\n";
        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );

        sync.open_tsx(&provider_path, source)
            .await
            .expect("managed tsgo open succeeds");

        let calls = mock.file_sync_calls();
        assert_eq!(
            calls.len(),
            2,
            "virtual declarations must open before the carrier"
        );
        let virtual_path = match &calls[0] {
            MockCall::OpenFile { path, content } => {
                assert!(path.ends_with("App.vue.tsx.__verter_types.d.ts"));
                assert_eq!(content, VERTER_TYPES_VIRTUAL_DTS);
                path
            }
            other => panic!("expected virtual @verter/types open, got {other:?}"),
        };
        match &calls[1] {
            MockCall::OpenFile { path, content } => {
                assert_eq!(path, &provider_path);
                assert!(content.contains("from \"./App.vue.tsx.__verter_types\""));
                assert!(!content.contains("from \"@verter/types\""));
            }
            other => panic!("expected carrier open, got {other:?}"),
        }
        assert_ne!(virtual_path, &provider_path);
        assert!(
            !std::path::Path::new(virtual_path).exists(),
            "the fallback must remain provider-virtual"
        );
    }

    #[tokio::test]
    async fn managed_tsgo_loads_virtual_verter_types_without_opening_it() {
        let owner = tempfile::tempdir().expect("temporary owner");
        let provider_path = owner.path().join("src/App.vue.tsx");
        std::fs::create_dir_all(provider_path.parent().unwrap()).expect("provider parent");
        let provider_path = provider_path.to_string_lossy().into_owned();
        let source = "import type { GlobalComponentType } from \"@verter/types\";\n";
        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );

        sync.load_tsx(&provider_path, source)
            .await
            .expect("managed tsgo load succeeds");

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 2);
        assert!(
            matches!(&calls[0], MockCall::LoadFile { path, .. } if path.ends_with("App.vue.tsx.__verter_types.d.ts")),
            "the dependency overlay must retain load-only semantics: {calls:?}"
        );
        assert!(
            matches!(&calls[1], MockCall::LoadFile { path, .. } if path == &provider_path),
            "the carrier must remain load-only: {calls:?}"
        );
    }

    #[tokio::test]
    async fn managed_tsgo_rolls_back_new_virtual_types_when_carrier_sync_fails() {
        let owner = tempfile::tempdir().expect("temporary owner");
        let provider_path = owner.path().join("src/App.vue.tsx");
        std::fs::create_dir_all(provider_path.parent().unwrap()).expect("provider parent");
        let provider_path = provider_path.to_string_lossy().into_owned();
        let virtual_path = format!("{provider_path}.__verter_types.d.ts");
        let source = "import type { GlobalComponentType } from \"@verter/types\";\n";
        let mock = MockTypeProvider::new();
        mock.set_fail_sync_path(&provider_path);
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );

        assert!(sync.open_tsx(&provider_path, source).await.is_err());

        let calls = mock.file_sync_calls();
        assert_eq!(
            calls.len(),
            3,
            "a failed carrier publication must close its newly-created dependency overlay"
        );
        assert!(matches!(
            &calls[2],
            MockCall::CloseFile { path } if path == &virtual_path
        ));
        assert!(
            !sync.virtual_verter_types_paths.contains(&virtual_path),
            "rolled-back overlays must not remain in the live ledger"
        );
    }

    #[tokio::test]
    async fn managed_tsgo_keeps_old_virtual_types_when_installed_transition_fails() {
        let owner = tempfile::tempdir().expect("temporary owner");
        let provider_path = owner.path().join("src/App.vue.tsx");
        std::fs::create_dir_all(provider_path.parent().unwrap()).expect("provider parent");
        let provider_path = provider_path.to_string_lossy().into_owned();
        let virtual_path = format!("{provider_path}.__verter_types.d.ts");
        let source = "import type { GlobalComponentType } from \"@verter/types\";\n";
        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );
        sync.open_tsx(&provider_path, source)
            .await
            .expect("initial virtual publication succeeds");

        let package = owner.path().join("node_modules/@verter/types");
        std::fs::create_dir_all(&package).expect("installed package directory");
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@verter/types","types":"index.d.ts"}"#,
        )
        .expect("installed package manifest");
        std::fs::write(
            package.join("index.d.ts"),
            "export type Installed = true;\n",
        )
        .expect("installed declarations");
        mock.set_fail_sync_path(&provider_path);

        assert!(sync.sync_tsx(&provider_path, source).await.is_err());

        let calls = mock.file_sync_calls();
        assert_eq!(
            calls.len(),
            3,
            "the old overlay must not close before the replacement carrier succeeds: {calls:?}"
        );
        assert!(matches!(
            &calls[2],
            MockCall::UpdateFile { path, .. } if path == &provider_path
        ));
        assert!(
            sync.virtual_verter_types_paths.contains(&virtual_path),
            "the previous live carrier still imports this overlay"
        );
    }

    #[tokio::test]
    async fn managed_tsgo_serializes_carrier_close_with_virtual_publication() {
        let owner = tempfile::tempdir().expect("temporary owner");
        let provider_path = owner.path().join("src/App.vue.tsx");
        std::fs::create_dir_all(provider_path.parent().unwrap()).expect("provider parent");
        let provider_path = provider_path.to_string_lossy().into_owned();
        let virtual_path = format!("{provider_path}.__verter_types.d.ts");
        let source = "import type { GlobalComponentType } from \"@verter/types\";\n";
        let mock = MockTypeProvider::new();
        let (virtual_arrived, release_virtual) = mock.block_open_file(&virtual_path);
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );

        let opening_sync = sync.clone();
        let opening_path = provider_path.clone();
        let opening =
            tokio::spawn(async move { opening_sync.open_tsx(&opening_path, source).await });
        virtual_arrived.notified().await;

        let closing_sync = sync.clone();
        let closing_path = provider_path.clone();
        let mut closing = tokio::spawn(async move { closing_sync.close_tsx(&closing_path).await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut closing)
                .await
                .is_err(),
            "close must wait until the carrier and its dependency finish publishing"
        );

        release_virtual.notify_one();
        opening
            .await
            .expect("opening task joins")
            .expect("opening succeeds");
        closing
            .await
            .expect("closing task joins")
            .expect("closing succeeds");

        let calls = mock.file_sync_calls();
        assert!(matches!(
            calls.as_slice(),
            [
                MockCall::OpenFile { path: opened_virtual, .. },
                MockCall::OpenFile { path: opened_carrier, .. },
                MockCall::CloseFile { path: closed_carrier },
                MockCall::CloseFile { path: closed_virtual },
            ] if opened_virtual == &virtual_path
                && opened_carrier == &provider_path
                && closed_carrier == &provider_path
                && closed_virtual == &virtual_path
        ));
    }

    /// A real package always wins over the provider fallback.
    #[tokio::test]
    async fn managed_tsgo_preserves_installed_verter_types_resolution() {
        let owner = tempfile::tempdir().expect("temporary owner");
        let package = owner.path().join("node_modules/@verter/types");
        std::fs::create_dir_all(package.join("dist")).expect("package directory");
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@verter/types","types":"dist/index.d.ts"}"#,
        )
        .expect("package manifest");
        std::fs::write(
            package.join("dist/index.d.ts"),
            "export type Installed = true;\n",
        )
        .expect("package declarations");
        let provider_path = owner.path().join("src/App.vue.tsx");
        std::fs::create_dir_all(provider_path.parent().unwrap()).expect("provider parent");
        let provider_path = provider_path.to_string_lossy().into_owned();
        let source =
            "import type { GlobalComponentType } from \"@verter/types\";\ntype T = GlobalComponentType<\"X\">;\n";
        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );

        sync.open_tsx(&provider_path, source)
            .await
            .expect("managed tsgo open succeeds");

        let calls = mock.file_sync_calls();
        assert_eq!(
            calls.len(),
            1,
            "installed packages must suppress the fallback"
        );
        match &calls[0] {
            MockCall::OpenFile { path, content } => {
                assert_eq!(path, &provider_path);
                assert_eq!(content, source);
            }
            other => panic!("expected carrier open, got {other:?}"),
        }
    }

    /// @ai-generated — load_tsx sends load_file (not open_file)
    #[tokio::test]
    async fn load_tsx_sends_load_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.load_tsx("App.vue.tsx", "const x = 1;").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::LoadFile { path, content } => {
                assert_eq!(path, "App.vue.tsx");
                assert_eq!(content, "const x = 1;");
            }
            _ => panic!("expected LoadFile, got {:?}", calls[0]),
        }
    }

    /// @ai-generated — close_tsx sends close_file
    #[tokio::test]
    async fn close_tsx_sends_close_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.close_tsx("App.vue.tsx").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::CloseFile { path } => {
                assert_eq!(path, "App.vue.tsx");
            }
            _ => panic!("expected CloseFile"),
        }
    }

    /// @ai-generated — open_dts sends open_file
    #[tokio::test]
    async fn open_dts_sends_open_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.open_dts("App.vue.ts", "export default App;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::OpenFile { path, content } => {
                assert_eq!(path, "App.vue.ts");
                assert_eq!(content, "export default App;");
            }
            _ => panic!("expected OpenFile"),
        }
    }

    /// @ai-generated — load_dts sends load_file (not open_file)
    #[tokio::test]
    async fn load_dts_sends_load_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.load_dts("App.vue.ts", "export default App;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::LoadFile { path, content } => {
                assert_eq!(path, "App.vue.ts");
                assert_eq!(content, "export default App;");
            }
            _ => panic!("expected LoadFile, got {:?}", calls[0]),
        }
    }

    /// @ai-generated — sync_dts sends update_file
    #[tokio::test]
    async fn sync_dts_sends_update_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.sync_dts("App.vue.ts", "export default App;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::UpdateFile { path, content } => {
                assert_eq!(path, "App.vue.ts");
                assert_eq!(content, "export default App;");
            }
            _ => panic!("expected UpdateFile"),
        }
    }

    /// @ai-generated — close_dts sends close_file
    #[tokio::test]
    async fn close_dts_sends_close_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.close_dts("App.vue.ts").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::CloseFile { path } => {
                assert_eq!(path, "App.vue.ts");
            }
            _ => panic!("expected CloseFile"),
        }
    }

    /// @ai-generated — Non-carrier files are synced only in FullProject mode
    #[tokio::test]
    async fn non_carrier_file_synced_in_full_project_mode() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.sync_file("utils.ts", "export const x = 1;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::UpdateFile { path, content } => {
                assert_eq!(path, "utils.ts");
                assert_eq!(content, "export const x = 1;");
            }
            _ => panic!("expected UpdateFile"),
        }
    }

    /// load_file sends load_file (background semantics, no diagnostics)
    #[tokio::test]
    async fn load_file_sends_load_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.load_file("utils.ts", "export const x = 1;")
            .await
            .unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::LoadFile { path, content } => {
                assert_eq!(path, "utils.ts");
                assert_eq!(content, "export const x = 1;");
            }
            _ => panic!("expected LoadFile, got {:?}", calls[0]),
        }
    }

    /// load_file propagates provider errors
    #[tokio::test]
    async fn load_file_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.load_file("utils.ts", "export const x = 1;").await;
        assert!(result.is_err(), "load_file should propagate provider error");
    }

    #[tokio::test]
    async fn non_carrier_file_close_sends_close_file() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.close_file("utils.ts").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            MockCall::CloseFile { path } => {
                assert_eq!(path, "utils.ts");
            }
            _ => panic!("expected CloseFile"),
        }
    }

    /// @ai-generated — Vue TSX is sent in resolver-managed mode
    #[tokio::test]
    async fn vue_tsx_sent_in_full_project_mode() {
        for mode in [ProjectSyncMode::FullProject] {
            let mock = MockTypeProvider::new();
            let sync = make_sync(&mock, mode);

            sync.sync_tsx("Comp.vue.tsx", "tsx content").await.unwrap();

            let calls = mock.file_sync_calls();
            assert_eq!(calls.len(), 1, "TSX should be sent in {:?} mode", mode);
        }
    }

    // ── Load vs Open contract tests ───────────

    /// @ai-generated — load_tsx uses load_file, open_tsx uses open_file — they must NOT overlap.
    /// This is the key contract: background-loaded files should not trigger diagnostics.
    #[tokio::test]
    async fn load_and_open_use_different_provider_methods() {
        let mock = MockTypeProvider::new();
        let sync = make_sync(&mock, ProjectSyncMode::FullProject);

        sync.load_tsx("A.vue.tsx", "load content").await.unwrap();
        sync.open_tsx("B.vue.tsx", "open content").await.unwrap();

        let calls = mock.file_sync_calls();
        assert_eq!(calls.len(), 2);

        // First call must be LoadFile (not OpenFile)
        assert!(
            matches!(&calls[0], MockCall::LoadFile { path, .. } if path == "A.vue.tsx"),
            "load_tsx should use LoadFile, got {:?}",
            calls[0]
        );
        // Second call must be OpenFile (not LoadFile)
        assert!(
            matches!(&calls[1], MockCall::OpenFile { path, .. } if path == "B.vue.tsx"),
            "open_tsx should use OpenFile, got {:?}",
            calls[1]
        );
    }

    // ── tsserver carrier-companion open suppression (the publish-only contract) ──

    /// For tsserver, the contentful carrier-companion verbs must NOT reach the
    /// provider — the carrier is served from the publish store via the plugin, so
    /// a `provider.open_file`/`update_file`/`load_file` of a generated companion
    /// would be a second carrier-content authority. This is the discriminating
    /// guard for the publish-only contract: reverting the suppression (routing
    /// tsserver through the tsgo open path) makes every assertion below fail.
    #[tokio::test]
    async fn tsserver_suppresses_carrier_companion_content_opens() {
        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsserver,
        );

        // Every contentful carrier-companion verb is a no-op `Ok` under tsserver.
        sync.open_tsx("App.vue.tsx", "tsx").await.unwrap();
        sync.sync_tsx("App.vue.tsx", "tsx").await.unwrap();
        sync.load_tsx("App.vue.tsx", "tsx").await.unwrap();
        sync.open_dts("App.vue.verter.ts", "ts").await.unwrap();
        sync.sync_dts("App.vue.verter.ts", "ts").await.unwrap();
        sync.load_dts("App.vue.verter.ts", "ts").await.unwrap();

        // NOT ONE open/update/load reached the provider — the carrier never
        // becomes a tsserver buffer via the open path.
        let content_calls: Vec<_> = mock
            .file_sync_calls()
            .into_iter()
            .filter(|c| {
                matches!(
                    c,
                    MockCall::OpenFile { .. }
                        | MockCall::UpdateFile { .. }
                        | MockCall::LoadFile { .. }
                )
            })
            .collect();
        assert!(
            content_calls.is_empty(),
            "tsserver must suppress every carrier-companion content open (publish-only); \
             got: {content_calls:?}"
        );

        // The close verb is still active (provider state cleanup is not a content
        // authority): a stale carrier path must still be retractable under tsserver.
        sync.close_tsx("App.vue.tsx").await.unwrap();
        sync.close_dts("App.vue.verter.ts").await.unwrap();
        let close_calls = mock
            .file_sync_calls()
            .into_iter()
            .filter(|c| matches!(c, MockCall::CloseFile { .. }))
            .count();
        assert_eq!(
            close_calls, 2,
            "close_tsx/close_dts must still reach the provider under tsserver (cleanup)"
        );
    }

    /// m8: the suppression must hold in EVERY priority lane. The background- and
    /// normal-priority carrier-companion verbs (currently unused, but they would
    /// send `fileContent`) must ALSO be no-op `Ok` under tsserver — otherwise a
    /// future caller routing a carrier companion through a priority lane would
    /// re-introduce a second carrier-content authority. RED before the fix: the
    /// `_background`/`_normal` variants bypassed `carrier_companion_open_suppressed`
    /// and reached the provider.
    #[tokio::test]
    async fn tsserver_suppresses_carrier_companion_background_and_normal_opens() {
        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsserver,
        );

        sync.open_tsx_background("App.vue.tsx", "tsx")
            .await
            .unwrap();
        sync.sync_tsx_background("App.vue.tsx", "tsx")
            .await
            .unwrap();
        sync.load_tsx_background("App.vue.tsx", "tsx")
            .await
            .unwrap();
        sync.open_dts_background("App.vue.verter.ts", "ts")
            .await
            .unwrap();
        sync.sync_dts_background("App.vue.verter.ts", "ts")
            .await
            .unwrap();
        sync.load_dts_background("App.vue.verter.ts", "ts")
            .await
            .unwrap();
        sync.open_tsx_normal("App.vue.tsx", "tsx").await.unwrap();
        sync.sync_tsx_normal("App.vue.tsx", "tsx").await.unwrap();
        sync.load_tsx_normal("App.vue.tsx", "tsx").await.unwrap();
        sync.open_dts_normal("App.vue.verter.ts", "ts")
            .await
            .unwrap();
        sync.sync_dts_normal("App.vue.verter.ts", "ts")
            .await
            .unwrap();
        sync.load_dts_normal("App.vue.verter.ts", "ts")
            .await
            .unwrap();

        let content_calls: Vec<_> = mock
            .file_sync_calls()
            .into_iter()
            .filter(|c| {
                matches!(
                    c,
                    MockCall::OpenFile { .. }
                        | MockCall::UpdateFile { .. }
                        | MockCall::LoadFile { .. }
                )
            })
            .collect();
        assert!(
            content_calls.is_empty(),
            "tsserver must suppress carrier-companion content opens in EVERY priority \
             lane (background + normal), not just the foreground verbs; got: \
             {content_calls:?}"
        );
    }

    /// tsgo control: the SAME background/normal carrier verbs DO reach the provider —
    /// proving the m8 suppression is gated to tsserver and does not regress the tsgo
    /// priority-lane open paths.
    #[tokio::test]
    async fn tsgo_still_opens_carrier_companions_in_priority_lanes() {
        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );

        sync.open_tsx_background("A.vue.tsx", "tsx").await.unwrap();
        sync.open_dts_normal("A.vue.verter.ts", "ts").await.unwrap();

        let calls = mock.file_sync_calls();
        let opens = calls
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    MockCall::OpenFile { .. } | MockCall::OpenFileBackground { .. }
                )
            })
            .count();
        assert_eq!(
            opens, 2,
            "tsgo must still open carrier companions in the background/normal lanes \
             (m8 suppression is tsserver-only), calls={calls:?}"
        );
        // …and each verb must land on ITS OWN lane: the background verb on the
        // background lane, the normal verb on the interactive one. The mock records
        // the lanes distinctly, so a verb that silently collapsed onto the wrong
        // priority is caught here rather than hidden by a lane-agnostic count.
        assert!(
            calls.iter().any(|c| matches!(
                c,
                MockCall::OpenFileBackground { path, .. } if path == "A.vue.tsx"
            )),
            "the background TSX verb must open on the background lane, calls={calls:?}"
        );
        assert!(
            calls.iter().any(|c| matches!(
                c,
                MockCall::OpenFile { path, .. } if path == "A.vue.verter.ts"
            )),
            "the normal DTS verb must open on the interactive lane, calls={calls:?}"
        );
    }

    /// tsgo (the content-transport engine — its carrier-companion content flows
    /// through the shared `--lsp` session) is the discriminating control: the SAME
    /// carrier verbs DO reach the provider. Proves the suppression is gated to
    /// tsserver only and does not regress the tsgo open path.
    #[tokio::test]
    async fn tsgo_still_opens_carrier_companions() {
        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsgo,
        );

        sync.open_tsx("App.vue.tsx", "tsx").await.unwrap();
        sync.open_dts("App.vue.verter.ts", "ts").await.unwrap();

        let opens = mock
            .file_sync_calls()
            .into_iter()
            .filter(|c| matches!(c, MockCall::OpenFile { .. }))
            .count();
        assert_eq!(
            opens, 2,
            "tsgo must still open the carrier companions (suppression is tsserver-only)"
        );
    }

    /// The suppression is scoped to the CARRIER-companion verbs: a NON-carrier
    /// real-file shadow (`sync_file`/`load_file`/`close_file`) still reaches
    /// tsserver. A real `.ts` shadow is not a generated carrier companion, so the
    /// plugin does not serve it — the LSP must still sync it directly.
    #[tokio::test]
    async fn tsserver_still_syncs_non_carrier_shadow_files() {
        let mock = MockTypeProvider::new();
        let sync = ProjectSync::new_with_kind(
            Arc::new(mock.clone()),
            ProjectSyncMode::FullProject,
            TypeProviderKind::Tsserver,
        );

        sync.sync_file("utils.ts", "export const x = 1;")
            .await
            .unwrap();
        sync.load_file("helpers.ts", "export const y = 2;")
            .await
            .unwrap();

        let content_calls = mock
            .file_sync_calls()
            .into_iter()
            .filter(|c| matches!(c, MockCall::UpdateFile { .. } | MockCall::LoadFile { .. }))
            .count();
        assert_eq!(
            content_calls, 2,
            "a non-carrier real-file shadow must still reach tsserver (not a carrier companion)"
        );
    }

    // ── Dead pipe / error propagation regression tests ───────────

    /// @ai-generated — Regression: sync_tsx propagates provider errors (dead pipe scenario).
    ///
    /// When tsgo crashes (e.g., OS error 232 "The pipe is being closed"),
    /// `sync_tsx` must return `Err`, not silently succeed or panic.
    #[tokio::test]
    async fn sync_tsx_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.sync_tsx("App.vue.tsx", "export default {}").await;
        assert!(result.is_err(), "sync_tsx should propagate provider error");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("pipe"),
            "error should mention pipe: {err}"
        );
    }

    /// @ai-generated — Regression: open_tsx propagates provider errors.
    #[tokio::test]
    async fn open_tsx_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.open_tsx("App.vue.tsx", "const x = 1;").await;
        assert!(result.is_err(), "open_tsx should propagate provider error");
    }

    /// @ai-generated — Regression: load_tsx propagates provider errors (dead pipe scenario).
    #[tokio::test]
    async fn load_tsx_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.load_tsx("App.vue.tsx", "const x = 1;").await;
        assert!(result.is_err(), "load_tsx should propagate provider error");
    }

    /// @ai-generated — Regression: load_dts propagates provider errors.
    #[tokio::test]
    async fn load_dts_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.load_dts("App.vue.ts", "export default App;").await;
        assert!(result.is_err(), "load_dts should propagate provider error");
    }

    /// @ai-generated — Regression: close_tsx propagates provider errors.
    #[tokio::test]
    async fn close_tsx_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.close_tsx("App.vue.tsx").await;
        assert!(result.is_err(), "close_tsx should propagate provider error");
    }

    /// @ai-generated — Regression: sync_file in FullProject mode propagates provider errors.
    #[tokio::test]
    async fn sync_file_full_project_propagates_provider_errors_duplicate() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.sync_file("utils.ts", "export const x = 1;").await;
        assert!(
            result.is_err(),
            "sync_file in FullProject mode should propagate error"
        );
    }

    /// @ai-generated — sync_file propagates provider errors for non-carrier files.
    #[tokio::test]
    async fn sync_file_full_project_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.sync_file("utils.ts", "export const x = 1;").await;
        assert!(
            result.is_err(),
            "sync_file should propagate provider errors"
        );
    }

    /// Regression: open_dts propagates provider errors.
    ///
    /// `sync_imported_carrier_api_lightweight` calls `open_dts` for TSGO when the file
    /// hasn't been background-loaded yet. The server must handle the returned error
    /// (log + queue for retry), not silently drop it.
    #[tokio::test]
    async fn open_dts_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.open_dts("App.vue.ts", "export default App;").await;
        assert!(
            result.is_err(),
            "open_dts should propagate provider error, not silently succeed"
        );
    }

    /// Regression: sync_dts propagates provider errors.
    ///
    /// `sync_imported_carrier_api_lightweight` calls `sync_dts` for non-TSGO or
    /// already-background-loaded files. The server must handle the returned error.
    #[tokio::test]
    async fn sync_dts_propagates_provider_errors() {
        let failing =
            FailingTypeProvider::new("flush error: The pipe is being closed. (os error 232)");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        let result = sync.sync_dts("App.vue.ts", "export default App;").await;
        assert!(
            result.is_err(),
            "sync_dts should propagate provider error, not silently succeed"
        );
    }

    /// @ai-generated — Multiple consecutive errors don't cause panic or unexpected state.
    /// Simulates repeated operations after tsgo crashes.
    #[tokio::test]
    async fn repeated_operations_after_provider_death() {
        let failing = FailingTypeProvider::new("write error: broken pipe");
        let sync = make_sync_failing(&failing, ProjectSyncMode::FullProject);

        // All operations should return errors, none should panic
        for i in 0..5 {
            let result = sync.sync_tsx("App.vue.tsx", &format!("version {i}")).await;
            assert!(
                result.is_err(),
                "operation {i} should still return error, not panic"
            );
        }
    }
}
