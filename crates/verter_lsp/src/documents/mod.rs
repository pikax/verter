mod analysis;
pub(crate) use analysis::SemanticReady;
pub mod carrier_structure;
pub mod line_index;
pub mod position_map;
pub mod provider_projection;
pub mod registration_signal;

use std::sync::Arc;

use parking_lot::RwLock;

use dashmap::DashMap;
use tower_lsp_server::ls_types::*;
use verter_session::{
    carrier_publication_store::{HostSourceRevisionToken, RegisteredFileStructure},
    CompileProfile, FileLanguage, HostUpdateResult, IdeResponse, StyleOverrideEntry,
    StyleOverrideRequest, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

use crate::server::{CodeBlock, VirtualFileEntry, VirtualFilesResponse};
use crate::uri::{file_uri_to_path, percent_decode};

use line_index::LineIndex;
use position_map::PositionMapper;
use provider_projection::{
    DocumentProviderProjection, ProviderPositionMapper, SelfFileProviderMapper,
};

/// Manages open documents and their relationship to verter_session.
pub struct DocumentRegistry {
    pub(crate) host: Arc<VerterHost>,
    /// Map from document URI to document state.
    documents: DashMap<String, DocumentState>,
    next_open_incarnation: std::sync::atomic::AtomicU64,
    /// Default compile profile for TSX generation (LSP mode).
    pub(crate) tsx_profile: Arc<RwLock<CompileProfile>>,
    /// Negotiated position encoding from the client (LSP 3.17).
    /// Set once during `initialize()`, before any documents are opened.
    encoding: RwLock<PositionEncodingKind>,
    /// The authoritative, generation-stamped store of provider surfaces synced
    /// to the type provider. Owned here — the single shared document/host facade
    /// reached by the server, the sync coordinator, AND the background-drain free
    /// functions (which already receive `&DocumentRegistry`) — so EVERY sync/close
    /// path records/forgets a generation through one owner. A cross-file rename
    /// captures the current snapshot set under a fence and maps a returned offset
    /// only against the exact generation it captured.
    provider_surfaces: crate::provider_surface_store::ProviderSurfaceStore,
    /// Signalled on every document registration (a request racing `did_open` waits on it).
    pub(crate) registration: registration_signal::RegistrationSignal,
    /// Full Verter semantic enrichment is deliberately isolated from the
    /// editor-critical projection host. The host is created lazily only when the
    /// client opts in, owns a single CPU worker, and is never queried inline by an
    /// LSP handler.
    semantic_host: RwLock<Option<Arc<VerterHost>>>,
    semantic_workspace: RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    semantic_enabled: std::sync::atomic::AtomicBool,
    semantic_snapshots: DashMap<String, analysis::SemanticSnapshot>,
    semantic_serial: Arc<tokio::sync::Mutex<()>>,
    semantic_ready_tx: tokio::sync::broadcast::Sender<analysis::SemanticReady>,
    /// Last deterministic carrier IDE compile verdict, keyed by source bytes.
    ///
    /// This is deliberately narrower than a failed-attempt memo: unavailable
    /// input and every host/scheduler transient record nothing, so cancellation
    /// can recover on a later request. Concurrent transient requests are
    /// bounded by the server's repair sequence instead.
    failed_carrier_ide_content_verdicts:
        DashMap<String, crate::provider_surface_store::ContentHash>,
    /// TEST SEAM: a one-shot callback run inside
    /// [`Self::recompile_and_refresh_mapper`] AFTER the compile and BEFORE the
    /// source-identity fence, so a test can commit the interleaved `didChange`
    /// at exactly the point production can. The compile is synchronous and
    /// holds no registry guard there, so the callback may re-enter the registry.
    #[cfg(test)]
    after_compile_hook: parking_lot::Mutex<Option<AfterCompileHook>>,
    /// TEST SEAM: a one-shot callback after the retained-source check and
    /// immediately before projection installation.
    #[cfg(test)]
    before_projection_install_hook: parking_lot::Mutex<Option<AfterCompileHook>>,
    /// TEST SEAM: a one-shot callback after semantic work completes and before
    /// its captured document/envelope identity is admitted for publication.
    #[cfg(test)]
    before_semantic_publish_hook: parking_lot::Mutex<Option<BeforeSemanticPublishHook>>,
    /// TEST SEAM: a one-shot callback immediately after the document read that
    /// followed the former early semantic-cache invalidation in `did_change`.
    #[cfg(test)]
    after_early_semantic_invalidation_window_hook: parking_lot::Mutex<Option<AfterCompileHook>>,
    /// TEST SEAM: a one-shot callback immediately before `did_change`
    /// reacquires the document shard while preparing its revision commit.
    #[cfg(test)]
    before_change_document_reacquire_hook: parking_lot::Mutex<Option<AfterCompileHook>>,
    /// TEST SEAM: a one-shot callback after semantic admission succeeds while
    /// the document shard is still held.
    #[cfg(test)]
    after_semantic_admission_hook: parking_lot::Mutex<Option<AfterCompileHook>>,
    /// TEST SEAM: a one-shot callback immediately after `did_change` commits
    /// the document entry and releases its shard write guard. Tests use it to
    /// probe the commit→semantic-invalidation window.
    #[cfg(test)]
    after_change_commit_hook: parking_lot::Mutex<Option<AfterCompileHook>>,
    /// TEST SEAM: a one-shot callback immediately before semantic cache
    /// publication. Tests use it to race an admitted result against an edit.
    #[cfg(test)]
    before_semantic_cache_publication_hook: parking_lot::Mutex<Option<AfterCompileHook>>,
    /// TEST SEAM: a one-shot callback inside [`Self::apply_style_overrides`]
    /// AFTER the revision/artifact token validation and BEFORE the host
    /// apply — the exact window the document-commit fence must close.
    #[cfg(test)]
    before_style_override_host_apply_hook: parking_lot::Mutex<Option<AfterCompileHook>>,
}

/// TEST SEAM callback type — see [`DocumentRegistry::after_compile_hook`].
#[cfg(test)]
pub(crate) type AfterCompileHook = Box<dyn FnOnce(&DocumentRegistry, &Uri) + Send>;

/// TEST SEAM callback type — `Some` plants a structure candidate at the final
/// publication join so one identity fence can be discriminated independently.
#[cfg(test)]
pub(crate) type BeforeSemanticPublishHook =
    Box<dyn FnOnce(&DocumentRegistry, &Uri) -> Option<RegisteredFileStructure> + Send>;

/// Tracked state for an open document.
///
/// `Clone` exists so a caller can take an owned snapshot and release the
/// registry's shard read guard — see [`DocumentRegistry::get`].
#[derive(Clone)]
pub struct DocumentState {
    /// The canonical ID used with verter_session.
    pub canonical_id: String,
    /// Current document version (from LSP client).
    pub version: i32,
    pub document_revision: DocumentRevisionId,
    /// Current source text.
    pub source: Arc<str>,
    /// Precomputed line index for byte-offset ↔ LSP Position conversion.
    pub line_index: Arc<LineIndex>,
    pub feature_snapshot: Option<Arc<DocumentFeatureSnapshot>>,
    /// The document's provider projection (rebuilt on each document change):
    /// the source↔provider position mapper plus the discriminant of which
    /// provider buffer this document projects into (`CarrierIde` for a Vue /
    /// Svelte carrier, `SelfFile` for a `.svelte.ts` / `.svelte.js` rune
    /// module). `None` for a plain script / virtual file (no provider
    /// projection). The `provider_path` is NOT stored here — it is DERIVED from
    /// the committed provider-sync state (carrier IDE path) or IS the canonical
    /// id (self-file).
    pub projection: Option<DocumentProviderProjection>,
    /// Language ID (e.g., "vue", "typescript").
    pub language_id: String,
    /// For virtual files (`verter-virtual://`): the source .vue file URI.
    /// When set, this document is a virtual file and feature requests should
    /// be routed through the source file's TSX for TSGO queries.
    pub virtual_source_uri: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DocumentOpenIncarnation(u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DocumentEditGeneration(u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DocumentRevisionId {
    open_incarnation: DocumentOpenIncarnation,
    edit_generation: DocumentEditGeneration,
}

impl DocumentRevisionId {
    fn opened(open_incarnation: u64) -> Self {
        Self {
            open_incarnation: DocumentOpenIncarnation(open_incarnation),
            edit_generation: DocumentEditGeneration(0),
        }
    }

    fn edited(self) -> Self {
        Self {
            open_incarnation: self.open_incarnation,
            edit_generation: DocumentEditGeneration(self.edit_generation.0.wrapping_add(1)),
        }
    }

    /// Stable opaque token for this exact open-incarnation/edit generation.
    pub fn public_token(self) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut input = [0u8; 16];
        input[..8].copy_from_slice(&self.open_incarnation.0.to_be_bytes());
        input[8..].copy_from_slice(&self.edit_generation.0.to_be_bytes());
        let digest = blake3::derive_key("verter.document-revision-token.v1", &input);
        let mut out = String::with_capacity(43);
        for chunk in digest.chunks_exact(3) {
            out.push(ALPHABET[(chunk[0] >> 2) as usize] as char);
            out.push(ALPHABET[(((chunk[0] & 0x03) << 4) | (chunk[1] >> 4)) as usize] as char);
            out.push(ALPHABET[(((chunk[1] & 0x0f) << 2) | (chunk[2] >> 6)) as usize] as char);
            out.push(ALPHABET[(chunk[2] & 0x3f) as usize] as char);
        }
        let tail = &digest[30..];
        out.push(ALPHABET[(tail[0] >> 2) as usize] as char);
        out.push(ALPHABET[(((tail[0] & 0x03) << 4) | (tail[1] >> 4)) as usize] as char);
        out.push(ALPHABET[((tail[1] & 0x0f) << 2) as usize] as char);
        out
    }
}

#[derive(Clone)]
pub struct SemanticAnalysisEnvelope {
    document_revision: DocumentRevisionId,
    semantic_host_revision: HostSourceRevisionToken,
    structure: RegisteredFileStructure,
    analysis: Arc<verter_session::FileAnalysisSnapshot>,
}

impl SemanticAnalysisEnvelope {
    pub fn document_revision(&self) -> DocumentRevisionId {
        self.document_revision
    }

    pub fn semantic_host_revision(&self) -> HostSourceRevisionToken {
        self.semantic_host_revision
    }

    pub fn structure(&self) -> &RegisteredFileStructure {
        &self.structure
    }

    pub fn analysis(&self) -> &Arc<verter_session::FileAnalysisSnapshot> {
        &self.analysis
    }
}

#[derive(Clone)]
pub struct DocumentFeatureSnapshot {
    document_revision: DocumentRevisionId,
    client_version: i32,
    line_index: Arc<LineIndex>,
    structure: RegisteredFileStructure,
    projection_host_revision: HostSourceRevisionToken,
    analysis: Option<SemanticAnalysisEnvelope>,
}

impl DocumentFeatureSnapshot {
    pub fn document_revision(&self) -> DocumentRevisionId {
        self.document_revision
    }

    pub fn client_version(&self) -> i32 {
        self.client_version
    }

    pub fn line_index(&self) -> &Arc<LineIndex> {
        &self.line_index
    }

    pub fn structure(&self) -> &RegisteredFileStructure {
        &self.structure
    }

    pub fn source(&self) -> &str {
        self.structure.source().bytes()
    }

    pub fn source_arc(&self) -> &Arc<str> {
        self.structure.source().source_arc()
    }

    pub fn projection_host_revision(&self) -> HostSourceRevisionToken {
        self.projection_host_revision
    }

    pub fn analysis(&self) -> Option<&SemanticAnalysisEnvelope> {
        self.analysis.as_ref()
    }
}

/// Immutable identity for one open-document revision.
///
/// LSP versions are not globally unique: clients may reuse a version after a
/// close/reopen and buggy clients may replace text without incrementing it.
/// The registry-owned revision distinguishes both cases independently of bytes.
#[derive(Clone)]
pub(crate) struct DocumentSnapshotIdentity {
    pub(crate) version: i32,
    revision: DocumentRevisionId,
    source: Arc<str>,
}

impl DocumentRegistry {
    fn next_document_revision(&self) -> DocumentRevisionId {
        DocumentRevisionId::opened(
            self.next_open_incarnation
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        )
    }

    pub fn new(host: Arc<VerterHost>) -> Self {
        let (semantic_ready_tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            host,
            documents: DashMap::new(),
            next_open_incarnation: std::sync::atomic::AtomicU64::new(1),
            tsx_profile: Arc::new(RwLock::new(CompileProfile {
                source_map: true,
                target: verter_session::CompileTarget::IDE
                    | verter_session::CompileTarget::TEMPLATE_DATA,
                ..CompileProfile::default()
            })),
            encoding: RwLock::new(PositionEncodingKind::UTF16),
            provider_surfaces: crate::provider_surface_store::ProviderSurfaceStore::new(),
            registration: registration_signal::RegistrationSignal::default(),
            semantic_host: RwLock::new(None),
            semantic_workspace: RwLock::new(None),
            semantic_enabled: std::sync::atomic::AtomicBool::new(false),
            semantic_snapshots: DashMap::new(),
            semantic_serial: Arc::new(tokio::sync::Mutex::new(())),
            semantic_ready_tx,
            failed_carrier_ide_content_verdicts: DashMap::new(),
            #[cfg(test)]
            after_compile_hook: parking_lot::Mutex::new(None),
            #[cfg(test)]
            before_projection_install_hook: parking_lot::Mutex::new(None),
            #[cfg(test)]
            before_semantic_publish_hook: parking_lot::Mutex::new(None),
            #[cfg(test)]
            after_early_semantic_invalidation_window_hook: parking_lot::Mutex::new(None),
            #[cfg(test)]
            before_change_document_reacquire_hook: parking_lot::Mutex::new(None),
            #[cfg(test)]
            after_semantic_admission_hook: parking_lot::Mutex::new(None),
            #[cfg(test)]
            before_semantic_cache_publication_hook: parking_lot::Mutex::new(None),
            #[cfg(test)]
            after_change_commit_hook: parking_lot::Mutex::new(None),
            #[cfg(test)]
            before_style_override_host_apply_hook: parking_lot::Mutex::new(None),
        }
    }

    /// TEST SEAM: arm the one-shot validation→host-apply window callback (see
    /// [`Self::before_style_override_host_apply_hook`]).
    #[cfg(test)]
    pub(crate) fn set_before_style_override_host_apply_hook(&self, hook: AfterCompileHook) {
        *self.before_style_override_host_apply_hook.lock() = Some(hook);
    }

    /// TEST SEAM: arm the one-shot post-compile callback (see
    /// [`Self::after_compile_hook`]).
    #[cfg(test)]
    pub(crate) fn set_after_compile_hook(&self, hook: AfterCompileHook) {
        *self.after_compile_hook.lock() = Some(hook);
    }

    #[cfg(test)]
    fn run_after_compile_hook(&self, uri: &Uri) {
        let hook = self.after_compile_hook.lock().take();
        if let Some(hook) = hook {
            hook(self, uri);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_before_projection_install_hook(&self, hook: AfterCompileHook) {
        *self.before_projection_install_hook.lock() = Some(hook);
    }

    #[cfg(test)]
    pub(crate) fn set_after_change_commit_hook(&self, hook: AfterCompileHook) {
        *self.after_change_commit_hook.lock() = Some(hook);
    }

    #[cfg(test)]
    fn run_after_change_commit_hook(&self, uri: &Uri) {
        let hook = self.after_change_commit_hook.lock().take();
        if let Some(hook) = hook {
            hook(self, uri);
        }
    }

    #[cfg(test)]
    fn run_before_projection_install_hook(&self, uri: &Uri) {
        let hook = self.before_projection_install_hook.lock().take();
        if let Some(hook) = hook {
            hook(self, uri);
        }
    }

    /// Whether these exact bytes already produced a deterministic compile
    /// verdict. Unavailable input and host/scheduler transients never enter
    /// this map.
    pub(crate) fn carrier_ide_compile_has_content_verdict(
        &self,
        canonical_id: &str,
        source: &str,
    ) -> bool {
        let content = crate::provider_surface_store::ContentHash::of(source);
        self.failed_carrier_ide_content_verdicts
            .get(canonical_id)
            .is_some_and(|failed| *failed == content)
    }

    /// Account only deterministic, source-byte verdicts.
    ///
    /// A blocked compile (`blocked_on_unavailable_input`) includes missing
    /// files and both transient macro-semantic codes; scheduler cancellation
    /// also arrives through the host-transient arms. All record nothing and
    /// remove a same-content verdict so a later request remains live.
    fn account_carrier_ide_content_verdict(
        &self,
        canonical_id: &str,
        attempted_source: &str,
        outcome: &Result<bool, verter_session::HostError>,
        ide_read_succeeded: bool,
        projection_installed: bool,
    ) {
        if projection_installed {
            self.failed_carrier_ide_content_verdicts
                .remove(canonical_id);
            return;
        }
        let deterministic = match outcome {
            Ok(true) if !ide_read_succeeded => false,
            Ok(_) => true,
            Err(verter_session::HostError::CompileError(failure)) => {
                !failure.blocked_on_unavailable_input()
            }
            Err(
                verter_session::HostError::InvalidQuery
                | verter_session::HostError::MissingVirtualNode { .. }
                | verter_session::HostError::RuntimeSurfaceRefused { .. }
                | verter_session::HostError::GrammarMismatch(_),
            ) => true,
            Err(
                verter_session::HostError::MissingSource { .. }
                | verter_session::HostError::Scheduler(_)
                | verter_session::HostError::Superseded
                | verter_session::HostError::Shutdown
                | verter_session::HostError::ExternalBlockContentDeferred(_),
            ) => false,
        };
        let content = crate::provider_surface_store::ContentHash::of(attempted_source);
        if deterministic {
            self.failed_carrier_ide_content_verdicts
                .insert(canonical_id.to_string(), content);
        } else {
            self.failed_carrier_ide_content_verdicts
                .remove_if(canonical_id, |_, failed| *failed == content);
        }
    }

    /// The generation-stamped provider-surface store (the authority behind
    /// fail-closed cross-file rename mapping).
    pub fn provider_surfaces(&self) -> &crate::provider_surface_store::ProviderSurfaceStore {
        &self.provider_surfaces
    }

    /// Set the negotiated position encoding. Called once during `initialize()`,
    /// before any documents are opened.
    pub fn set_encoding(&self, encoding: PositionEncodingKind) {
        *self.encoding.write() = encoding;
    }

    #[cfg(test)]
    pub fn set_embed_ambient_types(&self, embed: bool) {
        self.tsx_profile.write().embed_ambient_types = embed;
    }

    /// TEST SEAM: drop an open document's provider projection, reconstructing
    /// the startup-race state a carrier occupies when `did_open` runs before
    /// the host can serve its IDE compile (see [`Self::get_ide`]'s lazy-rebuild
    /// comment). Needed because some carriers (Svelte) have error-recovering
    /// IDE lowering — a malformed source still compiles — so tests cannot reach
    /// the projection-less state through a broken fixture the way Vue tests do.
    ///
    /// The interactive repair admits every open projection-less carrier; its
    /// generation-local single-flight collapses concurrent requests onto one
    /// compile while a genuinely later request may retry transient failure.
    #[cfg(test)]
    pub fn clear_projection_for_test(&self, uri: &Uri) {
        if let Some(mut entry) = self.documents.get_mut(uri.as_str()) {
            entry.projection = None;
        }
    }

    /// Build the self-file provider projection for an own-path provider
    /// document: a Svelte rune module (`.svelte.ts` / `.svelte.js`) gets the
    /// rune-prelude line count plus the import-specifier rewrite segments; a
    /// plain TS-family script gets a zero-prelude mapper over the same
    /// rewrite segments (its provider buffer is the source verbatim).
    /// `None` for framework carriers and for unknown extensions (a `.md` /
    /// extensionless document is never served to the TypeScript provider).
    ///
    /// `replacements` are the `(byte_start, byte_end, replacement)` import-
    /// specifier rewrites computed against `source` (the resolver-backed
    /// `compute_specifier_replacements`). They may be EMPTY before resolver
    /// ownership is ready — the prelude offset alone is a correct uniform line
    /// shift for every position; the rewrite column shifts refine import lines
    /// once the resolver lands.
    fn build_self_file_projection(
        canonical_id: &str,
        source: &str,
        replacements: &[(usize, usize, String)],
        line_index: &LineIndex,
    ) -> Option<DocumentProviderProjection> {
        // Path-gated: the registry extension table is the authority, so an
        // unknown extension (which the host classifier's catch-all would
        // report as a TS script) builds NO projection.
        let file_language = crate::server::self_file_language_for(canonical_id)?;
        let built = verter_session::framework::self_file_provider_content(&file_language, source)?;
        let mapper =
            SelfFileProviderMapper::new(built.prelude_line_count, replacements, line_index);
        Some(DocumentProviderProjection::SelfFile { mapper })
    }

    /// Get the negotiated encoding.
    pub fn encoding(&self) -> PositionEncodingKind {
        self.encoding.read().clone()
    }

    /// Resolve the [`FileLanguage`] row for an editor document.
    ///
    /// The client's `language_id` is authoritative for a framework CARRIER
    /// (an in-memory carrier document may not carry its `.vue` / `.svelte`
    /// path); every other document classifies by canonical path through the
    /// host's language classifier — the same authority the workspace-scan
    /// ingress uses, so one file resolves one `FileLanguage` row regardless of
    /// which ingress loaded it. The carrier mapping is REGISTRY-driven
    /// (`carrier_for_editor_language_id`), not a hardcoded `== "vue"` branch:
    /// any registered carrier (`vue`, `svelte`, …) resolves to its framework
    /// row here and the host upsert parses it through the registered carrier —
    /// never silently as a plain script.
    fn document_file_language(&self, language_id: &str, canonical_id: &str) -> FileLanguage {
        verter_session::LanguageRegistry::global()
            .carrier_for_editor_language_id(language_id)
            .unwrap_or_else(|| self.host.language_classifier().classify(canonical_id))
    }

    /// Handle a document being opened in the editor.
    pub fn did_open(&self, params: &TextDocumentItem) -> HostUpdateResult {
        let uri_str = params.uri.as_str().to_string();

        // Handle virtual files (verter-virtual:// scheme) — lightweight state only
        if let Some(source_uri) = parse_virtual_uri(&uri_str) {
            let source: Arc<str> = Arc::from(params.text.as_str());
            let state = DocumentState {
                canonical_id: uri_str.clone(),
                version: params.version,
                document_revision: self.next_document_revision(),
                line_index: Arc::new(LineIndex::new(&source, self.encoding())),
                feature_snapshot: None,
                source,
                projection: None,
                language_id: params.language_id.clone(),
                virtual_source_uri: Some(source_uri),
            };
            self.documents.insert(uri_str.clone(), state);
            self.registration.signal();
            return HostUpdateResult::no_change(uri_str);
        }

        let canonical_id = uri_to_canonical_id(&params.uri);
        self.semantic_snapshots.remove(&canonical_id);
        let submitted_source: Arc<str> = Arc::from(params.text.as_str());

        let file_language = self.document_file_language(&params.language_id, &canonical_id);
        // Every framework CARRIER (Vue OR Svelte) projects an IDE TSX file +
        // source map — the compile + position-mapper paths are carrier-general.
        let is_carrier = file_language.is_framework_carrier();

        let result = self.host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: submitted_source.clone(),
            file_language: file_language.clone(),
            aliases: vec![],
        });

        // Every did_open is a new document lifetime, even when the client reuses
        // its version. Restore the VFS overlay membership removed by did_close so
        // an interactive repair cannot reload the retired incarnation from disk.
        #[cfg(not(target_arch = "wasm32"))]
        self.host
            .notify_upsert(&canonical_id, Arc::clone(&submitted_source));

        // Trigger compilation to populate the TSX cache (upsert only parses).
        // IDE-sync: drive the IDE/TSX surface, NOT the runtime `Main` node — a
        // Main-less carrier (Svelte) projects only `CachedTsx`, so
        // `ensure_ide_compiled` populates it where `ensure_compiled` (which
        // demands `Main`) would not. `get_ide` below then reads the source map.
        let carrier_compile = is_carrier.then(|| {
            self.host
                .ensure_ide_compiled(&canonical_id, &self.tsx_profile.read())
        });

        let registered = is_carrier
            .then(|| self.host.registered_file_structure_snapshot(&canonical_id))
            .flatten();
        let source = registered
            .as_ref()
            .map(|(structure, _)| Arc::clone(structure.source().source_arc()))
            .unwrap_or(submitted_source);
        let line_index = Arc::new(LineIndex::new(&source, self.encoding()));
        let document_revision = self.next_document_revision();

        // Build the document's provider projection.
        // Always build on did_open — even if the host reports `changed: false`
        // (e.g., because scan_workspace already loaded the same content), we still
        // need the mapper for hover/definition/diagnostics.
        //  - CARRIER (`.vue` / `.svelte`): the IDE TSX source-map projection.
        //  - SELF-FILE (rune module OR plain TS-family script): the line-only
        //    rewrite-aware projection (prelude-offset-only at did_open; the
        //    server refines it with resolver-backed rewrite segments once
        //    ownership is ready). Plain scripts carry a zero-line prelude —
        //    their provider buffer is the source verbatim.
        let carrier_ide_read = if is_carrier {
            self.host.get_ide(&canonical_id, &self.tsx_profile.read())
        } else {
            None
        };
        let projection = if is_carrier {
            carrier_ide_read
                .as_ref()
                .and_then(|tsx| PositionMapper::from_json(tsx.source_map.as_ref()?).ok())
                .map(DocumentProviderProjection::carrier_ide)
        } else {
            Self::build_self_file_projection(&canonical_id, &source, &[], line_index.as_ref())
        };
        if let Some(outcome) = &carrier_compile {
            self.account_carrier_ide_content_verdict(
                &canonical_id,
                &source,
                outcome,
                carrier_ide_read.is_some(),
                projection.is_some(),
            );
        }

        let state = DocumentState {
            canonical_id,
            version: params.version,
            document_revision,
            line_index: Arc::clone(&line_index),
            source,
            feature_snapshot: registered.map(|(structure, projection_host_revision)| {
                Arc::new(DocumentFeatureSnapshot {
                    document_revision,
                    client_version: params.version,
                    line_index,
                    structure,
                    projection_host_revision,
                    analysis: None,
                })
            }),
            projection,
            language_id: params.language_id.clone(),
            virtual_source_uri: None,
        };

        self.documents.insert(uri_str.clone(), state);
        self.registration.signal();

        result.unwrap_or_else(|e| {
            tracing::error!("upsert failed for {}: {:?}", uri_str, e);
            HostUpdateResult::no_change(uri_to_canonical_id(&params.uri))
        })
    }

    /// Handle a document being changed.
    ///
    pub fn did_change(&self, uri: &Uri, version: i32, text: &str) -> HostUpdateResult {
        let uri_str = uri.as_str().to_string();
        tracing::info!(
            "DocumentRegistry::did_change UPSERT_START v{version} thread={:?}",
            std::thread::current().id()
        );

        // Virtual files: just update text + line index, skip host upsert
        if let Some(mut entry) = self.documents.get_mut(&uri_str) {
            if entry.virtual_source_uri.is_some() {
                let source: Arc<str> = Arc::from(text);
                entry.version = version;
                entry.document_revision = entry.document_revision.edited();
                entry.line_index = Arc::new(LineIndex::new(&source, self.encoding()));
                entry.source = source;
                entry.feature_snapshot = None;
                return HostUpdateResult::no_change(entry.canonical_id.clone());
            }
        }

        let canonical_id = uri_to_canonical_id(uri);
        let submitted_source: Arc<str> = Arc::from(text);

        let stored_language_id = self
            .documents
            .get(&uri_str)
            .map(|d| d.language_id.clone())
            .unwrap_or_default();
        #[cfg(test)]
        if let Some(hook) = self
            .after_early_semantic_invalidation_window_hook
            .lock()
            .take()
        {
            hook(self, uri);
        }
        let file_language = self.document_file_language(&stored_language_id, &canonical_id);
        // Carrier-general: every framework carrier (Vue OR Svelte) re-compiles
        // its IDE TSX on change.
        let is_carrier = file_language.is_framework_carrier();

        let upsert_start = std::time::Instant::now();
        let result = self.host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: submitted_source.clone(),
            file_language,
            aliases: vec![],
        });
        tracing::info!(
            "DocumentRegistry::did_change HOST_UPSERT_DONE elapsed={:?} thread={:?}",
            upsert_start.elapsed(),
            std::thread::current().id()
        );

        // Update VFS overlay with latest buffer content. sub-plan
        // §6b.D2b — route through `host.notify_upsert` so the route-only
        // shallow cache is evicted alongside the workspace overlay write.
        #[cfg(not(target_arch = "wasm32"))]
        self.host
            .notify_upsert(&canonical_id, submitted_source.clone());

        // A document commit owes the document's TEXT. It does not owe the IDE
        // TSX, and it must not pay for it per keystroke.
        //
        // `tower-lsp-server` does not spawn a task per notification: `Server::serve`
        // queues handler futures and polls them INLINE on the serve thread (see
        // `crate::SERVE_THREAD_STACK_BYTES`), and `handle_did_change` runs from
        // entry through this commit without ever pending — an uncontended
        // `did_change_mutex.lock()` completes in the current poll and the commit
        // itself is synchronous. Handler k therefore finishes before handler k+1
        // is polled at all, so a compile here is a strictly serialized queue whose
        // length is the user's typing speed. That is the ~9s of
        // https://github.com/pikax/verter/issues/96.
        //
        // Nor can it be coalesced from inside the handler: a notification still
        // sitting in the serve loop's channel has not been polled, so nothing on
        // this path can know that a newer revision is already on the wire.
        //
        // The IDE TSX is produced where it is DEMANDED — the debounced
        // coordinator's `sync_file`, the interactive repair path
        // (`ensure_current_file_synced` → `recompile_and_refresh_mapper`), and
        // `DocumentRegistry::get_ide`'s slow path. Each drives
        // `ensure_ide_compiled` itself, and each is already coalesced or
        // singleflighted.
        //
        // There is NO exception for a carrier that has no projection yet. Making
        // the commit compile in that case looks bounded ("only until the first
        // projection exists") but is not: a compile that FAILS installs no
        // projection, so the next edit compiles again — and a malformed
        // intermediate revision is the ordinary state of a file being typed, not
        // an edge case. That reinstates exactly the serialized per-keystroke
        // queue this method exists to avoid. A missing projection is instead
        // recovered on the paths that compile on their own cadence: the
        // debounced coordinator tick and the pending-snapshot drain (each
        // follows its compile with
        // [`Self::install_missing_carrier_projection`]), and — for the
        // interactive request already in hand — the foreground repair, whose
        // generation-local single-flight collapses concurrent requests onto
        // one compile. A genuinely later request retries, which is required
        // for scheduler cancellation and other transient failures to recover.
        let registered = is_carrier
            .then(|| self.host.registered_file_structure_snapshot(&canonical_id))
            .flatten();
        let source = registered
            .as_ref()
            .map(|(structure, _)| Arc::clone(structure.source().source_arc()))
            .unwrap_or(submitted_source);
        let new_line_index = Arc::new(LineIndex::new(&source, self.encoding()));

        #[cfg(test)]
        if let Some(hook) = self.before_change_document_reacquire_hook.lock().take() {
            hook(self, uri);
        }

        // Rebuild the document's provider projection.
        let prior_projection = || {
            self.documents
                .get(&uri_str)
                .and_then(|d| d.projection.clone())
        };
        let projection = if is_carrier {
            // Always the prior projection: the commit compiles nothing, so
            // `get_ide` (a pure cached read) has no fresh source map to rebuild
            // from. This is the same carry a failed compile has always used. The
            // demand-side rebuild installs the mapper that matches the text
            // readers see.
            prior_projection()
        } else {
            // Self-file (rune module or plain TS-family script; unknown
            // extension → `None`). The prelude offset is content-independent;
            // rebuild it whole-line (rewrite segments get refined by the
            // server once the resolver is ready).
            Self::build_self_file_projection(&canonical_id, &source, &[], new_line_index.as_ref())
        };

        if let Some(mut entry) = self.documents.get_mut(&uri_str) {
            let document_revision = entry.document_revision.edited();
            entry.version = version;
            entry.document_revision = document_revision;
            entry.line_index = Arc::clone(&new_line_index);
            entry.source = source;
            entry.projection = projection;
            entry.feature_snapshot = registered.map(|(structure, projection_host_revision)| {
                Arc::new(DocumentFeatureSnapshot {
                    document_revision,
                    client_version: version,
                    line_index: new_line_index,
                    structure,
                    projection_host_revision,
                    analysis: None,
                })
            });
            // Invalidate the semantic snapshot INSIDE the same shard mutation:
            // the remove happens-before the write guard releases, so any
            // reader that observes the committed entry can never also read the
            // superseded snapshot. (`cached_semantic_analysis` carries no
            // revision predicate — this ordering is its correctness rail.
            // The semantic publication path holds the same document guard
            // while inserting, so the documents→semantic_snapshots lock order
            // is shared and deadlock-free.)
            self.semantic_snapshots.remove(&canonical_id);
        } else {
            self.semantic_snapshots.remove(&canonical_id);
        }
        #[cfg(test)]
        self.run_after_change_commit_hook(uri);

        result.unwrap_or_else(|e| {
            tracing::error!("upsert failed for {}: {:?}", uri_str, e);
            HostUpdateResult::no_change(canonical_id)
        })
    }

    /// Handle incremental document changes.
    ///
    /// Applies each content change event to the stored source text:
    /// - If a change has a `range`, it replaces that range in the source.
    /// - If a change has no `range`, it replaces the entire document (full sync fallback).
    ///
    /// After applying all changes, delegates to `did_change` with the final text.
    pub fn did_change_incremental(
        &self,
        uri: &Uri,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> HostUpdateResult {
        let uri_str = uri.as_str().to_string();

        // Get current source text
        let mut text = match self.documents.get(&uri_str) {
            Some(doc) => doc.source.to_string(),
            None => {
                // No document tracked — use last change's full text as fallback
                if let Some(change) = changes.last() {
                    return self.did_change(uri, version, &change.text);
                }
                return HostUpdateResult::no_change(uri_to_canonical_id(uri));
            }
        };

        // Apply each change in order
        for change in &changes {
            if let Some(range) = change.range {
                // Incremental change: apply to specific range
                let line_index = LineIndex::new(&text, self.encoding());
                let start_offset =
                    line_index.position_to_offset(&range.start).unwrap_or(0) as usize;
                let end_offset = line_index
                    .position_to_offset(&range.end)
                    .unwrap_or(text.len() as u32) as usize;

                let start_offset = start_offset.min(text.len());
                let end_offset = end_offset.min(text.len());

                text.replace_range(start_offset..end_offset, &change.text);
            } else {
                // Full replacement (no range = full document)
                text = change.text.clone();
            }
        }

        self.did_change(uri, version, &text)
    }

    /// Handle a document being closed.
    pub fn did_close(&self, uri: &Uri) {
        // Clear the VFS overlay so resolution falls back to snapshot/disk.
        let canonical_id = uri_to_canonical_id(uri);
        // route through `host.notify_close`
        // so the route-only shallow cache is evicted alongside the
        // workspace overlay clear.
        self.host.notify_close(&canonical_id);

        self.documents.remove(uri.as_str());
        self.semantic_snapshots.remove(&canonical_id);
        self.failed_carrier_ide_content_verdicts
            .remove(&canonical_id);
    }

    /// Get the document state for a URI.
    ///
    /// The returned value is a LIVE `DashMap` shard READ guard, not a snapshot.
    /// `did_open`, `did_change`, `did_change_incremental`, `did_close`,
    /// [`Self::get_ide`], [`Self::recompile_and_refresh_mapper`] and
    /// [`Self::refresh_self_file_rewrites`] take the WRITE side of that same
    /// shard, so calling any of them while this guard is alive parks the caller
    /// on a lock it is itself holding — a self-deadlock with no other live
    /// holder, and no timeout.
    ///
    /// Take what you need and drop the guard first (`DocumentState` is `Clone`)
    /// before calling back into the registry.
    pub fn get(&self, uri: &Uri) -> Option<dashmap::mapref::one::Ref<'_, String, DocumentState>> {
        self.documents.get(uri.as_str())
    }

    /// Capture the exact open-document revision that an asynchronous operation
    /// is about to observe.
    pub(crate) fn snapshot_identity(&self, uri: &Uri) -> Option<DocumentSnapshotIdentity> {
        let document = self.documents.get(uri.as_str())?;
        Some(DocumentSnapshotIdentity {
            version: document.version,
            revision: document.document_revision,
            source: Arc::clone(&document.source),
        })
    }

    /// Return whether `snapshot` is still the exact open-document revision.
    pub(crate) fn snapshot_identity_is_current(
        &self,
        uri: &Uri,
        snapshot: &DocumentSnapshotIdentity,
    ) -> bool {
        self.documents.get(uri.as_str()).is_some_and(|document| {
            document.version == snapshot.version && document.document_revision == snapshot.revision
        })
    }

    /// Run `install` while holding the document shard's read guard, but only
    /// when the live document is still `snapshot`.
    ///
    /// `didChange` needs the shard's write guard, so the comparison and the
    /// synchronous installation inside `install` form one linearized write
    /// point with no check→write gap.
    pub(crate) fn with_current_snapshot_identity<R>(
        &self,
        uri: &Uri,
        snapshot: &DocumentSnapshotIdentity,
        install: impl FnOnce(&DocumentState) -> R,
    ) -> Option<R> {
        let document = self.documents.get(uri.as_str())?;
        if document.version != snapshot.version || document.document_revision != snapshot.revision {
            return None;
        }
        Some(install(&document))
    }

    /// Return one immutable carrier feature snapshot for the current revision.
    pub fn feature_snapshot(&self, uri: &Uri) -> Option<Arc<DocumentFeatureSnapshot>> {
        self.documents
            .get(uri.as_str())?
            .feature_snapshot
            .as_ref()
            .map(Arc::clone)
    }

    /// Get the document's provider projection (the source↔provider mapper +
    /// the projection discriminant).
    pub fn get_projection(&self, uri: &Uri) -> Option<DocumentProviderProjection> {
        self.documents.get(uri.as_str())?.projection.clone()
    }

    /// Install a carrier's provider projection from the host's CACHED IDE
    /// surface, but only when the document has none.
    ///
    /// The document commit does not compile, so a carrier whose open-time
    /// compile failed carries no projection — and a document with no projection
    /// fails closed on every capture (`capture_provider_request_surface`
    /// returns `None`) until a repair path compiles one: the attempt-bounded
    /// interactive repair serves the request in hand, while the debounced
    /// coordinator recovers request-independently — it already compiles the
    /// IDE surface once per quiet window, and this turns that compile into the
    /// projection the document was missing, at no extra compile and on no
    /// foreground request path.
    ///
    /// Reads the host cache only — it never compiles — and leaves a document
    /// that already has a projection untouched, so it cannot disturb the
    /// steady-state carry.
    pub fn install_missing_carrier_projection(&self, canonical_id: &str) {
        let Some(uri) = self.canonical_id_to_uri(canonical_id) else {
            return;
        };
        let uri_str = uri.as_str().to_string();
        if self
            .documents
            .get(&uri_str)
            .is_none_or(|document| document.projection.is_some())
        {
            return;
        }
        // SOURCE-IDENTITY FENCE: the host-cache read below happens with no
        // lock held, so a `didChange` can commit a newer revision in the
        // read→install window. Capture the revision this cached surface is
        // being installed FOR, and compare under the shard write guard —
        // never install a mapper over bytes it does not describe.
        let Some(attempted) = self.snapshot_identity(&uri) else {
            return;
        };
        let Some(mapper) = self
            .host
            .get_ide(canonical_id, &self.tsx_profile.read())
            .and_then(|tsx| PositionMapper::from_json(&tsx.source_map?).ok())
        else {
            return;
        };
        #[cfg(test)]
        self.run_before_projection_install_hook(&uri);
        let mut installed = false;
        if let Some(mut entry) = self.documents.get_mut(&uri_str) {
            if entry.version == attempted.version
                && entry.document_revision == attempted.revision
                && entry.projection.is_none()
            {
                entry.projection = Some(DocumentProviderProjection::carrier_ide(mapper));
                installed = true;
            }
        }
        if installed {
            self.failed_carrier_ide_content_verdicts
                .remove(canonical_id);
        }
    }

    /// Get the unified source↔provider position mapper for a document, ready
    /// for the feature layer (projection-agnostic).
    pub fn get_position_mapper(&self, uri: &Uri) -> Option<ProviderPositionMapper> {
        Some(
            self.documents
                .get(uri.as_str())?
                .projection
                .as_ref()?
                .mapper(),
        )
    }

    /// Get the canonical ID for a document URI.
    pub fn get_canonical_id(&self, uri: &Uri) -> Option<String> {
        self.documents
            .get(uri.as_str())
            .map(|d| d.canonical_id.clone())
    }

    /// Reverse lookup: find the URI for a given canonical ID.
    ///
    /// Iterates all open documents to find a match. Used by completion resolve
    /// to map TSX paths back to Vue URIs.
    pub fn canonical_id_to_uri(&self, canonical_id: &str) -> Option<Uri> {
        for entry in self.documents.iter() {
            if entry.value().canonical_id == canonical_id {
                return entry.key().parse().ok();
            }
        }
        None
    }

    /// Get the IDE output (TSX or JSX) for a document.
    ///
    /// If compile_slots were cleared (e.g., by dependency invalidation via `did_open`
    /// on a `.ts` file), lazily recompiles the Vue file to restore IDE output.
    /// This prevents "no IDE context" failures after peek definition or go-to-definition
    /// opens a dependency file.
    pub fn get_ide(&self, uri: &Uri) -> Option<IdeResponse> {
        let canonical_id = self.get_canonical_id(uri)?;
        let profile = self.tsx_profile.read().clone();

        // SOURCE-IDENTITY FENCE: the host-cache read below happens with no
        // lock held, so a `didChange` can commit a newer revision in the
        // read→install/return window. Capture the revision this cached
        // surface is being served FOR, compare under the shard write guard
        // before installing, and re-check before returning — never install a
        // mapper (or hand back IDE output) over bytes the document no longer
        // holds.
        let fast_path_attempted = self.snapshot_identity(uri)?;

        // Fast path: cache hit
        if let Some(resp) = self.host.get_ide(&canonical_id, &profile) {
            #[cfg(test)]
            self.run_before_projection_install_hook(uri);
            // Lazily rebuild position mapper if it was None (startup race:
            // did_open runs before background_init completes, so the mapper
            // may not have been built, but the workspace scanner later compiles
            // the file and caches TSX in the host).
            let mut installed = false;
            if let Some(entry) = self.documents.get(uri.as_str()) {
                if entry.projection.is_none() {
                    drop(entry);
                    if let Some(mut entry) = self.documents.get_mut(uri.as_str()) {
                        if entry.version == fast_path_attempted.version
                            && entry.document_revision == fast_path_attempted.revision
                            && entry.projection.is_none()
                        {
                            if let Some(mapper) = resp
                                .source_map
                                .as_ref()
                                .and_then(|sm| PositionMapper::from_json(sm).ok())
                            {
                                entry.projection =
                                    Some(DocumentProviderProjection::carrier_ide(mapper));
                                installed = true;
                            }
                        }
                    }
                }
            }
            if installed {
                self.failed_carrier_ide_content_verdicts
                    .remove(&canonical_id);
            }
            // A `didChange` that committed after the host-cache read makes
            // this response describe a superseded revision: return nothing;
            // the newer revision owes its own repair.
            if !self.snapshot_identity_is_current(uri, &fast_path_attempted) {
                return None;
            }
            return Some(resp);
        }

        // Slow path: compile_slots were cleared (e.g., dependency invalidation).
        // Lazily recompile to restore IDE output. Carrier-general (Vue / Svelte).
        let is_carrier = self
            .documents
            .get(uri.as_str())
            .map(|d| {
                self.document_file_language(&d.language_id, &canonical_id)
                    .is_framework_carrier()
            })
            .unwrap_or(false);
        if !is_carrier {
            return None;
        }

        // SOURCE-IDENTITY FENCE: capture the exact revision this slow-path
        // compile serves. The compile below is a blocking call with no lock
        // held over it, so a `didChange` can commit a different revision while
        // it runs — installing the produced mapper (or returning the response)
        // would then describe bytes the document no longer holds. Same
        // pattern as `recompile_and_refresh_mapper`.
        let attempted = self.snapshot_identity(uri)?;

        // IDE-sync: drive the IDE/TSX surface, NOT the runtime `Main` node. A
        // Main-less carrier (Svelte) has a `CachedTsx` but no `Main`, so
        // `ensure_compiled` (which demands `Main`) would return
        // `MissingVirtualNode` and abort here even though the IDE TSX exists.
        // `ensure_ide_compiled` resolves through the `Ide` demand; `Ok(false)`
        // (a genuine no-IDE surface) skips, `Ok(true)` proceeds to `get_ide`.
        if !self
            .host
            .ensure_ide_compiled(&canonical_id, &profile)
            .ok()?
        {
            return None;
        }
        let resp = self.host.get_ide(&canonical_id, &profile)?;
        #[cfg(test)]
        self.run_after_compile_hook(uri);
        // A `didChange` that committed while the compile ran makes this
        // response describe a superseded revision: install nothing and return
        // nothing; the newer revision owes its own repair.
        if !self.snapshot_identity_is_current(uri, &attempted) {
            return None;
        }

        // Rebuild position mapper since TSX output was regenerated. Compare
        // under the document's shard write guard — the installation
        // linearization point (no check→write gap).
        let mut installed = false;
        if let Some(mut entry) = self.documents.get_mut(uri.as_str()) {
            if entry.version != attempted.version || entry.document_revision != attempted.revision {
                return None;
            }
            if let Some(mapper) = resp
                .source_map
                .as_ref()
                .and_then(|sm| PositionMapper::from_json(sm).ok())
            {
                entry.projection = Some(DocumentProviderProjection::carrier_ide(mapper));
                installed = true;
            }
        }
        if installed {
            self.failed_carrier_ide_content_verdicts
                .remove(&canonical_id);
        }

        Some(resp)
    }

    /// Force-recompile a file and rebuild its PositionMapper from the fresh source map.
    ///
    /// Used after blocker hydration or snapshot reconciliation triggers recompilation
    /// that may have changed the TSX output. Without this, hover would query correct
    /// TSX with stale position offsets.
    pub fn recompile_and_refresh_mapper(&self, uri: &Uri) -> Option<IdeResponse> {
        let canonical_id = self.get_canonical_id(uri)?;
        // Carrier-general (Vue / Svelte) — every carrier projects an IDE TSX.
        let is_carrier = self
            .documents
            .get(uri.as_str())
            .map(|d| {
                self.document_file_language(&d.language_id, &canonical_id)
                    .is_framework_carrier()
            })
            .unwrap_or(false);
        if !is_carrier {
            return None;
        }
        let profile = self.tsx_profile.read().clone();
        // Capture the exact revision the compile below consumes. The
        // SOURCE-IDENTITY FENCE on the produced response is load-bearing: the
        // compile is a blocking call with no lock held over it, so a `didChange` can
        //   commit a different revision while it runs. Installing the mapper it
        //   produced would then describe the document with a mapper built for
        //   bytes the document no longer holds, and returning the response
        //   would hand the caller the same mismatch to sync and record. Fail
        //   closed instead: the newer revision owes its own repair.
        let attempted = self.snapshot_identity(uri)?;
        let attempted_source = Arc::clone(&attempted.source);
        // Load the document (and, through the scheduler's dependency ingress,
        // what it imports) BEFORE compiling. Warm for an open document — the host's
        // fast path is a cached-source check — so the repair lane's own
        // `ensure_loaded` is not duplicated work of any consequence, and the
        // close/reopen caller gains the same guarantee.
        self.host.ensure_loaded(&canonical_id);
        // IDE-sync: drive the IDE/TSX surface (not the runtime `Main`) so a
        // Main-less carrier (Svelte) refreshes its mapper. `Ok(false)` (no IDE
        // surface) and `Err` yield no response; `Ok(true)` proceeds to `get_ide`.
        let ensured = self.host.ensure_ide_compiled(&canonical_id, &profile);
        let resp = match &ensured {
            Ok(true) => self.host.get_ide(&canonical_id, &profile),
            _ => None,
        };
        #[cfg(test)]
        self.run_after_compile_hook(uri);
        // SOURCE-IDENTITY FENCE (see `attempted` above): a `didChange` that
        // committed while the compile ran makes this response describe a
        // revision the document no longer holds. Install nothing and return
        // nothing; the newer revision owes its own repair.
        if !self.snapshot_identity_is_current(uri, &attempted) {
            return None;
        }
        #[cfg(test)]
        self.run_before_projection_install_hook(uri);
        // Compare under the document's shard write guard: this is the
        // installation linearization point. A check before `get_mut` leaves a
        // check→write window in which didChange can commit another revision.
        let mut projection_installed = false;
        if let Some(resp) = &resp {
            let projection = resp
                .source_map
                .as_ref()
                .and_then(|sm| PositionMapper::from_json(sm).ok())
                .map(DocumentProviderProjection::carrier_ide);
            projection_installed = projection.is_some();
            let mut entry = self.documents.get_mut(uri.as_str())?;
            if entry.version != attempted.version || entry.document_revision != attempted.revision {
                return None;
            }
            entry.projection = projection;
        }
        self.account_carrier_ide_content_verdict(
            &canonical_id,
            &attempted_source,
            &ensured,
            resp.is_some(),
            projection_installed,
        );
        resp
    }

    /// Refine an OPEN self-file document's projection with resolver-backed
    /// import-specifier rewrite segments.
    ///
    /// At did_open / did_change the projection carries the prelude offset with
    /// NO rewrite segments (the resolver may not be ready). Once the server has
    /// a published resolver it computes the rewrite replacements and calls this
    /// to refine the rewrite-aware column mapping. A no-op when the document is
    /// not a `SelfFile` projection (e.g. a carrier or an unknown extension).
    pub fn refresh_self_file_rewrites(&self, uri: &Uri, replacements: &[(usize, usize, String)]) {
        let Some(mut entry) = self.documents.get_mut(uri.as_str()) else {
            return;
        };
        if !matches!(
            entry.projection,
            Some(DocumentProviderProjection::SelfFile { .. })
        ) {
            return;
        }
        let canonical_id = entry.canonical_id.clone();
        let source = entry.source.clone();
        let line_index = entry.line_index.clone();
        if let Some(projection) =
            Self::build_self_file_projection(&canonical_id, &source, replacements, &line_index)
        {
            entry.projection = Some(projection);
        }
    }

    /// Check if a document's IDE output is JavaScript (JSX) rather than TypeScript (TSX).
    ///
    /// The IDE compile is the authority, but a cold caller (no compiled IDE
    /// output yet) must not GUESS `.tsx`: the parse-level script dialect is
    /// available after upsert and names the companion correctly from the
    /// start. Without the fallback a JS carrier first opens `{carrier}.tsx`,
    /// then flips to `{carrier}.jsx` once the compile lands — and tsserver's
    /// output-file membership check excludes the `.jsx` while the stale
    /// `.tsx` is still in the Program.
    pub fn is_jsx(&self, uri: &Uri) -> bool {
        if let Some(ide) = self.get_ide(uri) {
            return ide.is_jsx;
        }
        self.get_analysis(uri)
            .map(|analysis| !analysis.is_typescript)
            .unwrap_or(false)
    }

    /// Canonical-id variant of [`Self::is_jsx`], host-level (no open-document
    /// requirement): the IDE compile is authoritative and the parse-level
    /// script dialect is the cold fallback. Gateway/publish call sites that
    /// hold an `Option<IdeResponse>` must route through this rather than
    /// `map(is_jsx).unwrap_or(false)` — the `unwrap_or(false)` guesses `.tsx`
    /// for a JS carrier whose compile is momentarily unavailable (the
    /// `.tsx` → `.jsx` companion flip tsserver's output-file check rejects).
    pub fn is_jsx_for_canonical(&self, canonical_id: &str) -> bool {
        let profile = self.tsx_profile.read().clone();
        if let Some(ide) = self.host.get_ide(canonical_id, &profile) {
            return ide.is_jsx;
        }
        self.host
            .get_analysis(canonical_id)
            .map(|analysis| !analysis.is_typescript)
            .unwrap_or(false)
    }

    /// Get the diagnostics for a document.
    pub fn get_diagnostics(&self, uri: &Uri) -> Option<verter_session::DiagnosticsSnapshot> {
        let canonical_id = self.get_canonical_id(uri)?;
        self.host
            .get_diagnostics(&canonical_id, &self.tsx_profile.read())
    }

    /// Get all virtual files for a document, including TSX output.
    pub fn get_virtual_files(
        &self,
        uri: &Uri,
    ) -> Result<Option<VirtualFilesResponse>, verter_session::PublicApiProjectionError> {
        let Some(canonical_id) = self.get_canonical_id(uri) else {
            return Ok(None);
        };

        // Get IDE output (TSX/JSX for template type checking).
        //
        // `get_ide` is a pure cached read, and the document commit deliberately
        // does not compile — so this reader, whose whole purpose is to hand the
        // caller the current TSX, must drive the compile itself rather than
        // report the surface missing. An `Err` (a genuine compile failure) leaves
        // `ide` `None` exactly as before.
        let profile = self.tsx_profile.read().clone();
        let _ = self.host.ensure_ide_compiled(&canonical_id, &profile);
        let ide = self
            .host
            .get_ide(&canonical_id, &profile)
            .map(|t| CodeBlock {
                code: t.code.to_string(),
                source_map: t.source_map.map(|m| m.to_string()),
                is_js: t.is_jsx,
            });

        // Get API output (declaration for cross-file type resolution). This
        // pane presents the PROVIDER-facing API surface — the same bytes the
        // `.verter.ts` companion publishes — so it takes the TS-labeled
        // rendering and is never JavaScript, whatever the SFC's dialect.
        let api = self.host.get_public_api(&canonical_id)?.map(|t| CodeBlock {
            code: t.ts_labeled_code().to_string(),
            source_map: t.source_map.map(|m| m.to_string()),
            is_js: false,
        });

        // Get all virtual node kinds
        let node_kinds = self.host.list_virtual_nodes(&canonical_id);

        // Fetch each virtual file
        let mut virtual_files = Vec::with_capacity(node_kinds.len());
        for kind in &node_kinds {
            let kind_str = match kind {
                VirtualNodeKind::Main => "main".to_string(),
                VirtualNodeKind::Script => "script".to_string(),
                VirtualNodeKind::Template => "template".to_string(),
                VirtualNodeKind::Style { index } => format!("style:{index}"),
                VirtualNodeKind::Custom { index } => format!("custom:{index}"),
            };

            let vf = self.host.get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(canonical_id.clone()),
                node_kind: Some(kind.clone()),
                compile_profile: self.tsx_profile.read().clone(),
            });

            match vf {
                Ok(response) => {
                    virtual_files.push(VirtualFileEntry {
                        kind: kind_str,
                        code: response.code.to_string(),
                        lang: response.lang.unwrap_or_else(|| "js".to_string()),
                        source_map: response.source_map.map(|m| m.to_string()),
                        stale: response.stale,
                    });
                }
                Err(_) => {
                    // Skip virtual files that fail to compile
                    virtual_files.push(VirtualFileEntry {
                        kind: kind_str,
                        code: String::new(),
                        lang: "js".to_string(),
                        source_map: None,
                        stale: true,
                    });
                }
            }
        }

        Ok(Some(VirtualFilesResponse {
            ide,
            api,
            virtual_files,
        }))
    }

    /// Get the analysis snapshot as a JSON value.
    ///
    /// When the negotiated encoding is not UTF-8, all `spanStart`/`spanEnd` byte
    /// offsets in the analysis are converted to the negotiated encoding (UTF-16 or UTF-32).
    pub fn get_analysis_json(&self, uri: &Uri) -> Option<serde_json::Value> {
        let analysis = self.get_analysis(uri)?;
        let mut json = serde_json::to_value(&analysis).ok()?;
        let encoding = self.encoding();
        if encoding != PositionEncodingKind::UTF8 {
            // UTF-8 byte offsets are native — no conversion needed.
            // For UTF-16 or UTF-32, convert all span offsets.
            let source = self.documents.get(uri.as_str())?.source.clone();
            convert_analysis_spans_json(&mut json, &source, &encoding);
        }
        Some(json)
    }

    /// Get the virtual source URI for a document (if it's a virtual file).
    pub fn get_virtual_source_uri(&self, uri: &Uri) -> Option<String> {
        self.documents.get(uri.as_str())?.virtual_source_uri.clone()
    }

    /// Apply preprocessor-compiled style overrides for a document.
    ///
    /// The apply is REVISION-BOUND and both captured structure tokens — the
    /// `documentRevisionToken` and the `artifactToken` of the structure the
    /// override was computed against — are REQUIRED: a request missing
    /// either token is refused typed before any host mutation, and a token
    /// that no longer matches the live document is refused typed — an async
    /// transpile result bound to revision A must never overwrite revision
    /// B's state.
    ///
    /// ATOMICITY: the token validation and the host mutation form one
    /// freshness transaction only while the caller holds the document-commit
    /// fence (the server's `did_change_mutex`, which every did_change /
    /// did_open commit path holds across its host upsert + registry commit).
    /// The LSP handler is the sole production caller and takes that fence.
    pub fn apply_style_overrides(
        &self,
        uri: &Uri,
        overrides: Vec<StyleOverrideEntry>,
        expected_revision_token: Option<&str>,
        expected_artifact_token: Option<&str>,
    ) -> StyleOverrideApplyOutcome {
        let (Some(revision_token), Some(artifact_token)) =
            (expected_revision_token, expected_artifact_token)
        else {
            return StyleOverrideApplyOutcome::MissingTokens;
        };
        // Compare against the LIVE document in a scoped guard (dropped
        // before the host call). A closed document matches no token.
        let matched = {
            match self.documents.get(uri.as_str()) {
                Some(doc) => {
                    revision_token == doc.document_revision.public_token()
                        && doc.feature_snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot.structure().public_artifact_token().as_str() == artifact_token
                        })
                }
                None => false,
            }
        };
        if !matched {
            return StyleOverrideApplyOutcome::RevisionMismatch;
        }
        #[cfg(test)]
        if let Some(hook) = self.before_style_override_host_apply_hook.lock().take() {
            hook(self, uri);
        }
        let canonical_id = uri_to_canonical_id(uri);
        let req = StyleOverrideRequest {
            canonical_id,
            compile_profile: self.tsx_profile.read().clone(),
            overrides,
        };
        match self.host.apply_style_overrides(req) {
            Ok(_) => StyleOverrideApplyOutcome::Applied,
            Err(e) => {
                tracing::warn!("apply_style_overrides failed: {e:?}");
                StyleOverrideApplyOutcome::Failed
            }
        }
    }

    /// Return the URI strings of all currently open documents.
    pub fn open_uris(&self) -> Vec<String> {
        self.documents.iter().map(|e| e.key().clone()).collect()
    }

    /// Get the underlying verter_session reference.
    pub fn host(&self) -> &VerterHost {
        &self.host
    }

    /// Get a shared reference to the host (for MCP embedding).
    pub fn host_arc(&self) -> Arc<VerterHost> {
        Arc::clone(&self.host)
    }
}

/// Outcome of a revision-bound style-override apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleOverrideApplyOutcome {
    /// The host accepted and applied the overrides.
    Applied,
    /// The host refused or failed the apply (typed host error).
    Failed,
    /// The captured revision/artifact token no longer matches the live
    /// document: refused BEFORE any host mutation.
    RevisionMismatch,
    /// The request carried no (or only one) captured structure token: a
    /// revision-unvalidatable apply is refused, never applied unfenced.
    MissingTokens,
}

// ── Analysis span conversion ─────────────────────────────────────────

/// Recursively walk a JSON value and convert all `spanStart`/`spanEnd` fields
/// from UTF-8 byte offsets to the negotiated encoding.
use analysis::convert_analysis_spans_json;

/// Convert a UTF-8 byte offset to the target encoding's offset.
/// Convert an LSP document URI to a canonical file path ID.
///
/// Extracts the path component from `file://` URIs.
/// On Windows, strips the leading `/` from paths like `/C:/Users/...`.
pub fn uri_to_canonical_id(uri: &Uri) -> String {
    uri_to_canonical_id_from_str(uri.as_str())
}

/// Convert a `file://` URI string to a canonical filesystem path ID.
///
/// Handles percent-encoded characters (e.g., `%3A` → `:` on Windows) and
/// restores the leading `/` on Unix, then routes the result through the single
/// canonical-path owner (`verter_span::path`) so the produced ID is byte-equal
/// to every other producer (type providers, VFS ingestion, scheduler). On
/// Windows this lowercases the drive letter (`C:/…` → `c:/…`) — URI-derived IDs
/// previously stayed uppercase and split file identity against the owner-
/// normalized IDs the VFS keys on. On Unix it is a no-op (paths already
/// canonical).
pub fn uri_to_canonical_id_from_str(s: &str) -> String {
    verter_span::path::canonicalize_path(&file_uri_to_path(s))
}
/// Parse a `verter-virtual://` URI and extract the source .vue file URI.
///
/// URI format: `verter-virtual:///kind.lang?sourceUri=<encoded-source-uri>`
/// Returns the decoded source URI if the input is a virtual file URI.
fn parse_virtual_uri(uri: &str) -> Option<String> {
    if !uri.starts_with("verter-virtual://") {
        return None;
    }
    let query_start = uri.find("?sourceUri=")?;
    let encoded = &uri[query_start + "?sourceUri=".len()..];
    Some(percent_decode(encoded))
}

#[cfg(test)]
#[allow(unused_must_use)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_to_canonical_id_unix_file_uri() {
        let uri: Uri = "file:///home/user/project/App.vue".parse().unwrap();
        let id = uri_to_canonical_id(&uri);
        assert_eq!(id, "/home/user/project/App.vue");
    }

    #[test]
    fn test_uri_to_canonical_id_windows_file_uri() {
        // The canonical-ID owner lowercases the Windows drive letter so a
        // URI-derived ID is byte-equal to every other producer (type providers,
        // VFS ingestion) — otherwise `did_open` would key the VFS under
        // `C:/...` while lookups resolve `c:/...`, splitting file identity.
        // Pre-fix this returned the uppercase `C:/...`; this assertion is
        // discriminating.
        let uri: Uri = "file:///C:/Users/dev/project/App.vue".parse().unwrap();
        let id = uri_to_canonical_id(&uri);
        assert_eq!(id, "c:/Users/dev/project/App.vue");
        assert_ne!(id, "C:/Users/dev/project/App.vue");
    }

    #[test]
    fn test_uri_to_canonical_id_with_spaces() {
        let uri: Uri = "file:///home/user/my%20project/App.vue".parse().unwrap();
        let id = uri_to_canonical_id(&uri);
        assert_eq!(id, "/home/user/my project/App.vue");
    }

    #[test]
    fn test_uri_to_canonical_id_non_file_uri() {
        let uri: Uri = "untitled:Untitled-1".parse().unwrap();
        let id = uri_to_canonical_id(&uri);
        assert_eq!(id, "untitled:Untitled-1");
    }

    /// @ai-generated — Windows drive letter with percent-encoded colon (%3A)
    /// VS Code sometimes sends `file:///d%3A/path` instead of `file:///d:/path`.
    #[test]
    fn test_uri_to_canonical_id_windows_encoded_drive() {
        let id = uri_to_canonical_id_from_str("file:///d%3A/dev/example/examples");
        assert_eq!(id, "d:/dev/example/examples");
    }

    /// @ai-generated — Windows path with lowercase drive and normal colon
    #[test]
    fn test_uri_to_canonical_id_windows_lowercase_drive() {
        let id = uri_to_canonical_id_from_str("file:///d:/dev/example");
        assert_eq!(id, "d:/dev/example");
    }

    /// @ai-generated — Verify the from_str variant matches the Uri variant
    #[test]
    fn test_uri_to_canonical_id_from_str_matches_uri() {
        let uri: Uri = "file:///C:/Users/dev/project/App.vue".parse().unwrap();
        assert_eq!(
            uri_to_canonical_id(&uri),
            uri_to_canonical_id_from_str("file:///C:/Users/dev/project/App.vue")
        );
    }

    #[test]
    fn test_parse_virtual_uri_tsx() {
        let uri = "verter-virtual:///tsx.tsx?sourceUri=file%3A%2F%2F%2Fhome%2Fuser%2FApp.vue";
        let source = parse_virtual_uri(uri);
        assert_eq!(source, Some("file:///home/user/App.vue".to_string()));
    }

    #[test]
    fn test_parse_virtual_uri_non_virtual() {
        let uri = "file:///home/user/App.vue";
        assert_eq!(parse_virtual_uri(uri), None);
    }

    #[test]
    fn open_uris_returns_open_documents() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);

        // Open two documents
        registry.did_open(&TextDocumentItem {
            uri: "file:///home/user/App.vue".parse().unwrap(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><div>hello</div></template>".to_string(),
        });
        registry.did_open(&TextDocumentItem {
            uri: "file:///home/user/Main.vue".parse().unwrap(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><span>world</span></template>".to_string(),
        });

        let uris = registry.open_uris();
        assert_eq!(uris.len(), 2, "should have 2 open documents");
        assert!(
            uris.contains(&"file:///home/user/App.vue".to_string()),
            "should contain App.vue URI"
        );
        assert!(
            uris.contains(&"file:///home/user/Main.vue".to_string()),
            "should contain Main.vue URI"
        );

        // Close one
        registry.did_close(&"file:///home/user/App.vue".parse().unwrap());
        let uris = registry.open_uris();
        assert_eq!(uris.len(), 1, "should have 1 open document after close");
        assert!(
            !uris.contains(&"file:///home/user/App.vue".to_string()),
            "should not contain closed App.vue URI"
        );
        assert!(
            uris.contains(&"file:///home/user/Main.vue".to_string()),
            "should still contain Main.vue URI"
        );
    }

    /// A client may replace text without advancing its version. Diagnostics and
    /// other asynchronous results must still recognize that as a new revision.
    #[test]
    fn snapshot_identity_rejects_same_version_text_replacement() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/App.vue".parse().unwrap();
        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><div>old</div></template>".to_string(),
        });
        let captured = registry.snapshot_identity(&uri).unwrap();

        registry.did_change(&uri, 1, "<template><div>new</div></template>");

        assert!(!registry.snapshot_identity_is_current(&uri, &captured));
    }

    /// Close/reopen resets the client's version sequence. Even identical text
    /// at the same version is a distinct open-document lifetime (ABA fence).
    #[test]
    fn snapshot_identity_rejects_close_reopen_at_same_version() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/App.vue".parse().unwrap();
        let item = TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><div>same</div></template>".to_string(),
        };
        registry.did_open(&item);
        let captured = registry.snapshot_identity(&uri).unwrap();

        registry.did_close(&uri);
        registry.did_open(&item);

        assert!(!registry.snapshot_identity_is_current(&uri, &captured));
    }

    #[test]
    fn carrier_feature_snapshot_is_atomic_and_revision_stamped() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/App.vue".parse().unwrap();
        let source = "<template><div>same</div></template>";

        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 7,
            text: source.to_string(),
        });
        let first = registry.feature_snapshot(&uri).expect("carrier snapshot");
        assert_eq!(first.client_version(), 7);
        assert_eq!(first.source(), source);
        assert!(Arc::ptr_eq(
            first.source_arc(),
            first.structure().envelope().source().source_arc()
        ));
        assert!(Arc::ptr_eq(
            first.structure().envelope(),
            registry
                .host()
                .registered_file_structure("/home/user/App.vue")
                .unwrap()
                .envelope()
        ));

        registry.did_change(&uri, 7, source);
        let edited = registry.feature_snapshot(&uri).expect("edited snapshot");
        assert_ne!(first.document_revision(), edited.document_revision());

        registry.did_close(&uri);
        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 7,
            text: source.to_string(),
        });
        let reopened = registry.feature_snapshot(&uri).expect("reopened snapshot");
        assert_ne!(edited.document_revision(), reopened.document_revision());
        assert_ne!(first.document_revision(), reopened.document_revision());
    }

    #[test]
    fn test_parse_virtual_uri_windows() {
        let uri =
            "verter-virtual:///tsx.tsx?sourceUri=file%3A%2F%2F%2FC%3A%2FUsers%2Fdev%2FApp.vue";
        let source = parse_virtual_uri(uri);
        assert_eq!(source, Some("file:///C:/Users/dev/App.vue".to_string()));
    }

    /// Bug 1 regression: `get_ide()` fast path should lazily rebuild a missing
    /// position mapper when the host already has cached TSX output.
    #[test]
    fn position_mapper_lazily_rebuilt_on_fast_path() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/App.vue".parse().unwrap();

        // Open a Vue file — this compiles and builds the mapper
        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><div>hello</div></template><script setup lang=\"ts\">\nconst x = 1;\n</script>".to_string(),
        });

        // Verify mapper was built during did_open
        assert!(
            registry.get_position_mapper(&uri).is_some(),
            "mapper should be built during did_open"
        );

        // Simulate the startup race: clear the projection to None
        if let Some(mut entry) = registry.documents.get_mut(uri.as_str()) {
            entry.projection = None;
        }
        assert!(
            registry.get_position_mapper(&uri).is_none(),
            "mapper should be None after clearing"
        );

        // Call get_ide() — fast path should lazily rebuild the mapper
        let ide = registry.get_ide(&uri);
        assert!(ide.is_some(), "get_ide should return cached TSX");

        // The mapper should now be rebuilt
        assert!(
            registry.get_position_mapper(&uri).is_some(),
            "mapper should be lazily rebuilt on fast path cache hit"
        );
    }

    /// A `.svelte` framework carrier opened in the editor routes through the
    /// language classifier to the REGISTERED Svelte carrier: the upsert
    /// succeeds and the host holds a Svelte carrier source snapshot — it is NOT
    /// silently misparsed as a plain TypeScript script, and NOT rejected as a
    /// known-but-unsupported file.
    #[test]
    fn did_open_svelte_parses_through_the_registered_carrier() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/App.svelte".parse().unwrap();

        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "svelte".to_string(),
            version: 1,
            text:
                "<script lang=\"ts\">let { x }: { x: number } = $props();</script>\n<div>{x}</div>"
                    .to_string(),
        });

        // The editor document stays tracked…
        assert!(
            registry.open_uris().contains(&uri.as_str().to_string()),
            "the opened document must stay tracked in the registry"
        );
        // …AND the host holds a Svelte carrier source snapshot: the registered
        // carrier parses it (positive routing), never a misparsed script.
        assert!(
            registry
                .host()
                .get_source("/home/user/App.svelte")
                .is_some(),
            "the registered Svelte carrier must parse the .svelte document"
        );
    }

    /// End-to-end: a Main-less `.svelte` carrier's IDE TSX reaches the
    /// LSP IDE-sync path. `DocumentRegistry::get_ide`'s SLOW path (compile slots
    /// cleared) drove `host.ensure_compiled(canonical, profile).ok()?` before
    /// the migration — which DEMANDS `VirtualNodeKind::Main`. A Svelte carrier
    /// projects ONLY an IDE `CachedTsx` (no runtime `Main`), so the old
    /// `ensure_compiled` returned `MissingVirtualNode` → `.ok()?` short-circuited
    /// to `None` and the Svelte IDE surface NEVER reached the LSP, even though
    /// the TSX existed. After the migration to `ensure_ide_compiled` (which
    /// resolves through the `Ide` demand, never `Main`), the slow path restores
    /// the IDE TSX.
    ///
    /// DISCRIMINATING: this FAILS against the pre-migration `ensure_compiled`-
    /// gated slow path (it returns `None` for the Main-less carrier) and PASSES
    /// after the migration. A Vue file would not discriminate — Vue has a `Main`
    /// node, so its old gate succeeded; only a Main-less carrier exposes the bug.
    #[test]
    fn main_less_svelte_ide_tsx_reaches_lsp_get_ide_slow_path() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));
        let uri: Uri = "file:///home/user/Counter.svelte".parse().unwrap();
        let canonical = "/home/user/Counter.svelte";

        // Open a Main-less Svelte carrier (no runtime Main; IDE TSX only).
        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "svelte".to_string(),
            version: 1,
            text: "<script lang=\"ts\">let count = 0;</script>\n<button onclick={() => count++}>{count}</button>\n".to_string(),
        });

        // Force `get_ide`'s SLOW path: clear the compile slots so the host's
        // fast-path `get_ide` peek misses and the registry must recompile. This
        // is exactly the dependency-invalidation scenario the slow path exists
        // for — and the one that gated on `ensure_compiled` before the fix.
        registry.host().invalidate_compile_slots(canonical);
        assert!(
            registry
                .host()
                .get_ide(canonical, &registry.tsx_profile.read())
                .is_none(),
            "precondition: the fast-path peek must miss after invalidating the compile slots, \
             so `get_ide` takes the slow recompile path under test"
        );

        // The slow path now drives `ensure_ide_compiled` (Ide demand), so the
        // Main-less Svelte carrier's IDE TSX reaches the LSP.
        let ide = registry.get_ide(&uri);
        assert!(
            ide.is_some(),
            "the Main-less Svelte carrier's IDE TSX must reach the LSP `get_ide` slow path — \
             pre-migration this returned None because `ensure_compiled` demanded a runtime Main \
             the Svelte carrier never produces"
        );
        let ide = ide.unwrap();
        // Svelte-specific IDE output (the @verter/svelte-jsx pragma) — NOT Vue
        // TSX. This confirms the carrier-correct surface reached the LSP.
        assert!(
            ide.code.contains("@jsxImportSource @verter/svelte-jsx"),
            "the LSP must receive the Svelte-specific IDE TSX, got:\n{}",
            ide.code
        );
        assert!(
            !ide.code.contains("_sfc_main"),
            "the LSP must NOT receive Vue SFC TSX residue for a .svelte carrier"
        );
    }

    /// Plain-script documents keep parsing exactly as before: the
    /// classifier resolves the script row for the canonical path and
    /// the upsert succeeds.
    #[test]
    fn did_open_script_document_still_parses() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///home/user/util.ts".parse().unwrap();

        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "typescript".to_string(),
            version: 1,
            text: "export const x = 1;".to_string(),
        });

        assert!(
            registry.host().get_source("/home/user/util.ts").is_some(),
            "a plain script document must parse through the script path"
        );
    }

    /// The editor ingress and the workspace-scan ingress resolve the
    /// SAME `FileLanguage` row for the same path: both route through the
    /// host's language classifier. `language_id == "vue"` stays
    /// authoritative for the Vue carrier (an in-memory Vue document may
    /// not carry a `.vue` path).
    #[test]
    fn document_language_agrees_with_host_classifier() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));

        assert_eq!(
            registry.document_file_language("vue", "/x/App.vue"),
            FileLanguage::vue()
        );
        for path in ["/x/a.ts", "/x/a.js", "/x/a.svelte", "/x/notes.md"] {
            assert_eq!(
                registry.document_file_language("plaintext", path),
                host.language_classifier().classify(path),
                "editor ingress must agree with the scan ingress for {path}"
            );
        }
        // The classifier (not a hard-coded TypeScript fallback) decides:
        // a `.js` path resolves the JS script row.
        assert_eq!(
            registry.document_file_language("javascript", "/x/a.js"),
            FileLanguage::script(verter_session::ScriptSourceType::js())
        );
        // A `.svelte` path resolves its framework carrier row — never
        // the Vue row, never a plain script.
        let svelte = registry.document_file_language("svelte", "/x/a.svelte");
        assert!(svelte.is_framework_carrier());
        assert!(!svelte.is_vue());
        assert_ne!(svelte, FileLanguage::script_ts());
    }

    #[test]
    fn did_open_svelte_builds_an_ide_position_mapper() {
        // The carrier-general compile gate (is_framework_carrier, not is_vue)
        // must reach the Svelte IDE projection on did_open — the source map
        // yields a position mapper. A `.svelte` file with the Vue-only gate
        // would build NO mapper (the bug this pins).
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));

        let item = TextDocumentItem {
            uri: "file:///x/Comp.svelte".parse().expect("uri"),
            language_id: "svelte".to_string(),
            version: 1,
            text: "<script lang=\"ts\">let count = 0;</script>\n<div>{count}</div>".to_string(),
        };
        let _ = registry.did_open(&item);

        let state = registry
            .documents
            .get("file:///x/Comp.svelte")
            .expect("the .svelte document is registered");
        assert!(
            matches!(
                state.projection,
                Some(DocumentProviderProjection::CarrierIde { .. })
            ),
            "did_open on a .svelte carrier must build an IDE (carrier) projection \
             (the carrier-general compile gate reaches the Svelte projection)"
        );
    }

    /// The hot read path hands out a SHARED source-map mapper: two
    /// `get_position_mapper` results for the SAME carrier document must point at
    /// ONE `PositionMapper` allocation, not two deep copies of its
    /// `OwnedSourceMap` + precomputed lookup tables.
    ///
    /// Discriminating: with the mapper owned behind a `Box`, `.mapper()`
    /// deep-clones the whole `PositionMapper` pointee on every call, so the two
    /// results point at DISTINCT allocations and the address comparison FAILS.
    /// With the mapper owned behind an `Arc`, `.mapper()` clones the handle and
    /// both results share ONE allocation, so the comparison PASSES.
    #[test]
    fn get_position_mapper_shares_one_allocation() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///x/Comp.svelte".parse().expect("uri");

        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "svelte".to_string(),
            version: 1,
            text: "<script lang=\"ts\">let count = 0;</script>\n<div>{count}</div>".to_string(),
        });

        // Two independent reads of the unified mapper for the same document.
        let first = registry
            .get_position_mapper(&uri)
            .expect("a .svelte carrier must expose a source-map mapper");
        let second = registry
            .get_position_mapper(&uri)
            .expect("a .svelte carrier must expose a source-map mapper");

        // Both reads project the same carrier into its IDE TSX, so both are the
        // SourceMap arm. Compare the address of the underlying `PositionMapper`
        // pointee: a shared handle yields one allocation, a per-call deep clone
        // yields two.
        let (ProviderPositionMapper::SourceMap(a), ProviderPositionMapper::SourceMap(b)) =
            (&first, &second)
        else {
            panic!("a .svelte carrier mapper must be the SourceMap projection");
        };
        assert!(
            std::ptr::eq(a.as_ref(), b.as_ref()),
            "two get_position_mapper reads must share ONE PositionMapper allocation \
             (the read path hands out a cheap handle clone, not a deep copy of the \
             source map + lookup tables)"
        );
    }

    /// A mapper REBUILD replaces the position-mapper `Arc` rather than mutating
    /// the existing pointee: after `recompile_and_refresh_mapper` recompiles a
    /// carrier whose TSX changed, `get_position_mapper` hands out an `Arc` that
    /// is a FRESH allocation — NOT `Arc::ptr_eq` to the pre-rebuild handle.
    ///
    /// This is the companion to the read-path share test: within one projection
    /// the `Arc` is shared (cheap handle clones), but a rebuild REPLACES it whole
    /// (a fresh `PositionMapper` in a new `Arc::new`, overwriting
    /// `entry.projection`).
    ///
    /// The rebuild is driven explicitly because the document COMMIT no longer
    /// compiles — see `committing_an_edit_does_not_compile_the_ide_tsx`. The
    /// second half asserts that carry-forward directly, so the pair pins both
    /// halves of the split: the commit keeps the existing handle, and the
    /// demand-time rebuild replaces it.
    ///
    /// Discriminating: the handles are captured before and after and compared by
    /// allocation address. A refactor that mutated the mapper in place behind the
    /// SAME `Arc` (e.g. `Arc::get_mut` to overwrite the source map + lookup
    /// tables) would alias, `std::ptr::eq` would be TRUE, and this fails. The
    /// edit changes the `<script>` body so the recompiled TSX genuinely differs,
    /// forcing the rebuild branch rather than the keep-prior-projection fallback.
    #[test]
    fn a_mapper_rebuild_installs_a_fresh_position_mapper_arc() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(host);
        let uri: Uri = "file:///x/Comp.svelte".parse().expect("uri");

        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "svelte".to_string(),
            version: 1,
            text: "<script lang=\"ts\">let count = 0;</script>\n<div>{count}</div>".to_string(),
        });

        // Capture the pre-edit mapper handle's allocation.
        let before = registry
            .get_position_mapper(&uri)
            .expect("a .svelte carrier must expose a source-map mapper before the edit");
        let ProviderPositionMapper::SourceMap(before_arc) = &before else {
            panic!("a .svelte carrier mapper must be the SourceMap projection");
        };

        // Edit the document: change the script body so the recompiled IDE TSX
        // genuinely differs.
        let _ = registry.did_change(
            &uri,
            2,
            "<script lang=\"ts\">let total = 100; let extra = 1;</script>\n<div>{total}</div>",
        );

        // The COMMIT carries the existing handle forward untouched — it owes the
        // text, not the TSX.
        let committed = registry
            .get_position_mapper(&uri)
            .expect("the commit must CARRY the mapper, never drop it");
        let ProviderPositionMapper::SourceMap(committed_arc) = &committed else {
            panic!("a .svelte carrier mapper must remain the SourceMap projection");
        };
        assert!(
            std::ptr::eq(before_arc.as_ref(), committed_arc.as_ref()),
            "the document commit must carry the SAME mapper allocation forward: \
             rebuilding it there is the per-keystroke compile issue #96 is about, and \
             dropping it strands the document with no projection at all"
        );

        // The DEMAND-time rebuild is what replaces it, with a fresh allocation.
        registry
            .recompile_and_refresh_mapper(&uri)
            .expect("the edited carrier must recompile on demand");
        let after = registry
            .get_position_mapper(&uri)
            .expect("a .svelte carrier must expose a source-map mapper after the rebuild");
        let ProviderPositionMapper::SourceMap(after_arc) = &after else {
            panic!("a .svelte carrier mapper must remain the SourceMap projection");
        };

        assert!(
            !std::ptr::eq(before_arc.as_ref(), after_arc.as_ref()),
            "a mapper rebuild must REPLACE the position-mapper Arc with a fresh \
             allocation (it must not mutate the pre-rebuild pointee in place)"
        );
    }

    /// Cold compile RUNS this host has started — the feature-independent rail
    /// bumped once per cold run past the warm-hit consult.
    ///
    /// It must be this rail and not the post-success compile tick: a compile
    /// that FAILS returns before that tick, so a burst of malformed revisions
    /// could run a cold compile per keystroke while a tick-based counter
    /// reported zero.
    fn cold_compile_runs(host: &verter_session::VerterHost) -> u64 {
        host.provenance_snapshot().compile_cold_runs
    }

    fn carrier_revision(marker: &str) -> String {
        format!(
            "<script setup lang=\"ts\">\nconst msg = '{marker}'\n</script>\n\
             <template><div>{{{{ msg }}}}</div></template>\n"
        )
    }

    /// A commit owes the document's TEXT, not its IDE TSX.
    ///
    /// The compile runs inside the server's global document-commit mutex on the
    /// serve thread, where `tower-lsp-server` polls notification handlers inline
    /// and a handler runs from entry through commit without pending — so a
    /// per-keystroke compile there is a strictly serialized queue as long as the
    /// typing burst (https://github.com/pikax/verter/issues/96). Every consumer
    /// that needs the TSX drives `ensure_ide_compiled` itself.
    #[test]
    fn committing_an_edit_does_not_compile_the_ide_tsx() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));
        let uri: Uri = "file:///x/Committed.vue".parse().expect("uri");
        let canonical_id = uri_to_canonical_id(&uri);

        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: carrier_revision("v1"),
        });
        assert!(
            registry
                .get(&uri)
                .and_then(|document| document.projection.clone())
                .is_some(),
            "precondition: the OPEN must establish the carrier's projection, so the \
             edits below are steady-state commits"
        );

        let before = cold_compile_runs(&host);
        for (version, marker) in [(2, "v2"), (3, "v3"), (4, "v4")] {
            let result = registry.did_change(&uri, version, &carrier_revision(marker));
            assert!(result.changed, "revision {marker} must commit its text");
        }
        let compiles = cold_compile_runs(&host) - before;

        assert_eq!(
            compiles, 0,
            "three committed edits started {compiles} cold compile run(s); the commit \
             path must not compile at all"
        );
        assert_eq!(
            registry
                .get(&uri)
                .map(|document| document.source.to_string())
                .as_deref(),
            Some(carrier_revision("v4").as_str()),
            "the registry must hold the newest text — only the DERIVED TSX is deferred"
        );
        assert!(
            registry
                .get(&uri)
                .and_then(|document| document.projection.clone())
                .is_some(),
            "the prior position mapper must be CARRIED OVER, not dropped"
        );

        // Positive control: the rail is live and the deferred work is genuinely
        // still owed.
        let before_control = cold_compile_runs(&host);
        let profile = registry.tsx_profile.read().clone();
        assert!(
            host.ensure_ide_compiled(&canonical_id, &profile)
                .expect("the carrier must have an IDE surface"),
            "a .vue carrier projects an IDE surface"
        );
        assert!(
            cold_compile_runs(&host) > before_control,
            "compiling the committed revision must start a cold run — otherwise the \
             zero above is vacuous"
        );
    }

    /// The case issue #96 is actually about: a carrier with NO projection, edited
    /// repeatedly with revisions that do not compile.
    ///
    /// A commit that compiled "only until the first projection exists" is not
    /// bounded by document. A failed compile installs no projection, so the next
    /// edit compiles again — and a malformed intermediate revision is the
    /// ordinary state of a file being typed. That reinstates the serialized
    /// per-keystroke queue exactly.
    ///
    /// Measured on the COLD-RUN rail rather than the post-success compile tick,
    /// which a failing compile never reaches: on that tick this burst reads zero
    /// whether it compiled twelve times or none.
    #[test]
    fn repeated_invalid_edits_on_a_projectionless_carrier_never_compile() {
        let host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        ));
        let registry = DocumentRegistry::new(Arc::clone(&host));
        let uri: Uri = "file:///x/Invalid.vue".parse().expect("uri");

        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<script setup lang=\"ts\">\nconst broken = (((\n".to_string(),
        });
        assert!(
            registry
                .get(&uri)
                .and_then(|document| document.projection.clone())
                .is_none(),
            "precondition: this fixture must fail to project, or the projection-less \
             path under test is never entered"
        );

        // Keep typing, every revision still malformed — the normal mid-edit state.
        const BURST: usize = 12;
        let before = cold_compile_runs(&host);
        for index in 0..BURST {
            let _ = registry.did_change(
                &uri,
                2 + index as i32,
                &format!("<script setup lang=\"ts\">\nconst broken{index} = (((\n"),
            );
        }
        let compiles = cold_compile_runs(&host) - before;

        assert_eq!(
            compiles, 0,
            "{BURST} malformed edits on a projection-less carrier started {compiles} \
             cold compile run(s). A commit that compiles while no projection exists \
             never stops: the compile fails, no projection is installed, and the next \
             keystroke repeats it — the serialized queue of issue #96, reachable by \
             ordinary typing"
        );

        // The projection is still missing — which is exactly why recovery belongs
        // to the paths that already compile for open documents. See
        // `the_coordinator_installs_a_projection_the_failed_open_compile_never_built`
        // and `a_projectionless_carrier_recovers_without_a_provider_or_a_snapshot`.
        assert!(
            registry
                .get(&uri)
                .and_then(|document| document.projection.clone())
                .is_none(),
            "the fixture must still have no projection, or this asserts nothing about \
             the projection-less path"
        );

        // Positive control: the rail moves for this document when a compile is
        // actually demanded, so the zero above is not a dead counter.
        let before_control = cold_compile_runs(&host);
        let _ = registry.did_change(&uri, 99, &carrier_revision("repaired"));
        let _ = registry.recompile_and_refresh_mapper(&uri);
        assert!(
            cold_compile_runs(&host) > before_control,
            "the demand-side rebuild must start a cold run — otherwise the zero above \
             is vacuous"
        );
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
