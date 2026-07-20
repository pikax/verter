//! Mock implementation of `TypeProvider` for testing.
//!
//! Allows tests to configure expected responses for each method.
//! Tracks all calls for assertion purposes.

#[cfg(test)]
pub use inner::*;

#[cfg(test)]
mod inner {
    use std::sync::{Arc, Mutex};

    use crate::type_provider::protocol::*;
    use crate::type_provider::traits::{ProviderFuture, TypeProvider};

    /// Return `Err` when failure injection is enabled, otherwise `Ok(())`.
    fn fail_or_ok(fail: bool, op: &str) -> Result<(), TypeProviderError> {
        if fail {
            Err(TypeProviderError::new(format!(
                "MockTypeProvider: injected {op} failure"
            )))
        } else {
            Ok(())
        }
    }

    /// A recorded call to the mock type provider.
    #[derive(Debug, Clone)]
    pub enum MockCall {
        OpenFile {
            path: String,
            content: String,
        },
        LoadFile {
            path: String,
            content: String,
        },
        UpdateFile {
            path: String,
            content: String,
        },
        CloseFile {
            path: String,
        },
        GetCompletions {
            path: String,
            offset: u32,
        },
        GetHover {
            path: String,
            offset: u32,
        },
        GetDiagnostics {
            path: String,
        },
        GetDefinition {
            path: String,
            offset: u32,
        },
        GetTypeDefinition {
            path: String,
            offset: u32,
        },
        GetReferences {
            path: String,
            offset: u32,
        },
        GetRenameLocations {
            path: String,
            offset: u32,
        },
        GetSignatureHelp {
            path: String,
            offset: u32,
        },
        GetCodeActions {
            path: String,
            start_offset: u32,
            end_offset: u32,
            /// The diagnostic contexts threaded from the handler — lets a test
            /// assert the parsed error codes (e.g. `[6133]`) reached the provider.
            diagnostics: Vec<ProviderDiagnosticContext>,
        },
        GetSemanticTokens {
            path: String,
        },
        GetDocumentHighlights {
            path: String,
            offset: u32,
        },
        GetInlayHints {
            path: String,
            start_offset: u32,
            end_offset: u32,
        },
        ResolveCompletion {
            path: String,
            data: CompletionResolveData,
        },
        ConfigurePaths {
            base_url: String,
            paths: serde_json::Value,
        },
        UpdateWorkspaceFolders {
            added: Vec<serde_json::Value>,
            removed: Vec<serde_json::Value>,
        },
        NotifyCarrierChanged {
            companion_path: String,
        },
        RegisterCarrierMember {
            source_path: String,
            companion_path: String,
            content: String,
            project_file_name: String,
        },
    }

    /// Shared state for the mock provider.
    #[derive(Default)]
    struct MockState {
        calls: Vec<MockCall>,
        /// When `true`, the file-op methods (`open_file`/`load_file`/
        /// `update_file`/`close_file`) RECORD their call and then return
        /// `Err`, simulating a provider whose file I/O fails (e.g. a crashed
        /// child). Used to exercise failure-retain behaviour in the drain.
        fail_file_ops: bool,
        /// Per-path failure injection: any `open_file`/`load_file`/
        /// `update_file` against a path in this set RECORDS its call and then
        /// returns `Err`. This is the only way to fail a specific KIND of sync
        /// (IDE vs API), because `*_tsx` and `*_dts` both map to the same
        /// underlying `open_file`/`update_file` primitive — they differ only by
        /// path. `close_file` is intentionally NOT gated by this set: tests
        /// that fail a kind's sync still want to observe whether a stale path of
        /// that kind was (wrongly) closed.
        fail_sync_paths: std::collections::HashSet<String>,
        hover_responses: Vec<(String, u32, Option<HoverInfo>)>,
        /// Scripted transient hover failures: while > 0, each `get_hover`
        /// RECORDS its call and returns `Err` (simulating a provider/transport
        /// failure), decrementing the counter. Pins the no-silent-empty
        /// recovery contract: a failed provider hover must resync+retry,
        /// never vanish silently.
        fail_next_hovers: usize,
        /// When `true`, `get_definition` RECORDS its call and then returns a
        /// future that NEVER resolves, simulating a wedged type provider (a
        /// managed tsgo stuck in a busy dispatch loop). Drives the handler
        /// deadline repro: without an always-on production request deadline the
        /// definition handler parks on this forever.
        hang_definition: bool,
        /// As `hang_definition`, for `get_hover`.
        hang_hover: bool,
        /// As `hang_definition`, for `get_signature_help`.
        hang_signature_help: bool,
        completion_responses: Vec<(String, u32, Vec<Completion>)>,
        diagnostic_responses: Vec<(String, Vec<TypeDiagnostic>)>,
        definition_responses: Vec<(String, u32, Vec<TypeLocation>)>,
        type_definition_responses: Vec<(String, u32, Vec<TypeLocation>)>,
        reference_responses: Vec<(String, u32, Vec<TypeLocation>)>,
        rename_responses: Vec<(String, u32, Vec<RenameLocation>)>,
        highlight_responses: Vec<(String, u32, Vec<TypeDocumentHighlight>)>,
        signature_help_responses: Vec<(String, u32, Option<SignatureHelp>)>,
        code_action_responses: Vec<(String, u32, u32, Vec<TypeCodeAction>)>,
        semantic_token_responses: Vec<(String, Vec<SemanticToken>)>,
        inlay_hint_responses: Vec<(String, u32, u32, Vec<InlayHint>)>,
        resolve_completion_responses: Vec<(
            String,
            CompletionResolveData,
            Option<CompletionResolveResult>,
        )>,
        /// Provider identity reported by [`MockTypeProvider::provider_id`].
        /// Defaults to `"tsgo"`; tests that exercise provider-id validation set
        /// it explicitly via [`MockTypeProvider::set_provider_id`].
        provider_id: Option<&'static str>,
        /// Test seam: when set to `Some((path, gate))`, a
        /// `register_carrier_member` against `path` RECORDS its call and then
        /// AWAITS `gate` before returning — pausing the caller (e.g. a respawn's
        /// carrier replay) on that exact registration so a concurrency test can
        /// deterministically open the snapshot→swap window. Other paths are
        /// unaffected.
        register_block: Option<(String, std::sync::Arc<tokio::sync::Notify>)>,
        /// Test seam: when set to `Some((path, arrived, release))`, a `close_file`
        /// against `path` RECORDS its call, SIGNALS `arrived` (so the test observes
        /// the close has been reached), and then AWAITS `release` before returning.
        /// Pauses the closing task INSIDE the provider close so a concurrency test
        /// can deterministically run other work (e.g. a closure-pass re-record)
        /// while a `did_close` is mid-flight in its overlay-release half. Other
        /// paths close without blocking.
        #[allow(clippy::type_complexity)]
        close_block: Option<(
            String,
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        )>,
        /// Test seam matching `close_block`, but for a one-shot `update_file`.
        /// It lets concurrency tests pause an edit after the document registry
        /// has accepted new source while the provider refresh is still in flight.
        #[allow(clippy::type_complexity)]
        update_block: Option<(
            String,
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        )>,
        /// One-shot async gate for `open_file`, used to keep the winning
        /// singleflight repair pending while every waiter is polled and queues.
        #[allow(clippy::type_complexity)]
        open_block: Option<(
            String,
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        )>,
        /// One-shot async gate for `get_completions`, used to advance a carrier
        /// document version while a completion request is genuinely suspended.
        #[allow(clippy::type_complexity)]
        completion_block: Option<(
            String,
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        )>,
        /// Test seam: when set to `Some((path, callback))`, the FIRST `open_file`
        /// whose path equals `path` RECORDS its call, takes the callback (one-shot)
        /// and RUNS it synchronously — after releasing the state lock and before
        /// returning the future. Lets a test deterministically interleave a side
        /// effect (e.g. closing a document in the `DocumentRegistry`) at the exact
        /// moment a specific overlay open fires, so a mid-pass close can be exercised
        /// against the real async pass without a non-deterministic thread race. Other
        /// paths, and all subsequent opens of the same path, are unaffected.
        #[allow(clippy::type_complexity)]
        on_open_file: Option<(String, Box<dyn FnOnce() + Send>)>,
        /// Test seam: when set to `Some((path, callback))`, the FIRST interactive
        /// query (`get_hover` / `get_completions`) whose path equals `path` RECORDS
        /// its call, takes the callback (one-shot) and RUNS it synchronously —
        /// after releasing the state lock and before the query's future is
        /// returned. Lets a test deterministically interleave a mid-request event
        /// (a re-sync recording a fresh provider-surface generation, a surface
        /// retirement racing a `did_close`) between a feature handler's context
        /// capture and its provider-response merge, so fail-closed torn-request
        /// behaviour is exercised without a timing race. Other paths, and all
        /// subsequent queries of the same path, are unaffected.
        #[allow(clippy::type_complexity)]
        on_query: Option<(String, Box<dyn FnOnce() + Send>)>,
    }

    /// A mock `TypeProvider` for testing.
    ///
    /// All methods record their calls and return configured responses.
    /// Default response for methods without configured data is empty/None.
    #[derive(Clone)]
    pub struct MockTypeProvider {
        state: Arc<Mutex<MockState>>,
    }

    impl Default for MockTypeProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockTypeProvider {
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState::default())),
            }
        }

        /// Configure a hover response for a specific path and offset.
        pub fn set_hover(&self, path: &str, offset: u32, info: Option<HoverInfo>) {
            let mut state = self.state.lock().unwrap();
            state.hover_responses.push((path.to_string(), offset, info));
        }

        /// Script the next `count` `get_hover` calls to fail with `Err`
        /// (transient provider/transport failure) before normal responses
        /// resume.
        pub fn fail_next_hovers(&self, count: usize) {
            let mut state = self.state.lock().unwrap();
            state.fail_next_hovers = count;
        }

        /// Make every subsequent `get_definition` RECORD its call and then hang
        /// forever (a wedged type provider). The handler-deadline repro uses
        /// this to prove the definition handler now fails closed on a deadline
        /// instead of parking.
        pub fn hang_definition(&self) {
            let mut state = self.state.lock().unwrap();
            state.hang_definition = true;
        }

        /// Wedge `get_hover` the same way [`Self::hang_definition`] wedges
        /// definition: record the call, then never resolve.
        pub fn hang_hover(&self) {
            let mut state = self.state.lock().unwrap();
            state.hang_hover = true;
        }

        /// Wedge `get_signature_help`. Signature help reaches the provider on a
        /// keystroke, so a wedge here parks the handler on every `(` the user
        /// types until the request deadline fires.
        pub fn hang_signature_help(&self) {
            let mut state = self.state.lock().unwrap();
            state.hang_signature_help = true;
        }

        /// Configure completions for a specific path and offset.
        pub fn set_completions(&self, path: &str, offset: u32, items: Vec<Completion>) {
            let mut state = self.state.lock().unwrap();
            state
                .completion_responses
                .push((path.to_string(), offset, items));
        }

        /// Configure diagnostics for a specific path.
        pub fn set_diagnostics(&self, path: &str, diags: Vec<TypeDiagnostic>) {
            let mut state = self.state.lock().unwrap();
            state.diagnostic_responses.push((path.to_string(), diags));
        }

        /// Configure definition locations for a specific path and offset.
        pub fn set_definitions(&self, path: &str, offset: u32, locs: Vec<TypeLocation>) {
            let mut state = self.state.lock().unwrap();
            state
                .definition_responses
                .push((path.to_string(), offset, locs));
        }

        /// Configure type definition locations for a specific path and offset.
        pub fn set_type_definitions(&self, path: &str, offset: u32, locs: Vec<TypeLocation>) {
            let mut state = self.state.lock().unwrap();
            state
                .type_definition_responses
                .push((path.to_string(), offset, locs));
        }

        /// Configure reference locations for a specific path and offset.
        pub fn set_references(&self, path: &str, offset: u32, locs: Vec<TypeLocation>) {
            let mut state = self.state.lock().unwrap();
            state
                .reference_responses
                .push((path.to_string(), offset, locs));
        }

        /// Configure rename locations for a specific path and offset.
        pub fn set_rename_locations(&self, path: &str, offset: u32, locs: Vec<RenameLocation>) {
            let mut state = self.state.lock().unwrap();
            state
                .rename_responses
                .push((path.to_string(), offset, locs));
        }

        /// Configure document highlights for a specific path and offset.
        pub fn set_highlights(
            &self,
            path: &str,
            offset: u32,
            highlights: Vec<TypeDocumentHighlight>,
        ) {
            let mut state = self.state.lock().unwrap();
            state
                .highlight_responses
                .push((path.to_string(), offset, highlights));
        }

        /// Configure signature help for a specific path and offset.
        pub fn set_signature_help(&self, path: &str, offset: u32, help: Option<SignatureHelp>) {
            let mut state = self.state.lock().unwrap();
            state
                .signature_help_responses
                .push((path.to_string(), offset, help));
        }

        /// Configure code actions for a specific path and offset range.
        pub fn set_code_actions(
            &self,
            path: &str,
            start_offset: u32,
            end_offset: u32,
            actions: Vec<TypeCodeAction>,
        ) {
            let mut state = self.state.lock().unwrap();
            state
                .code_action_responses
                .push((path.to_string(), start_offset, end_offset, actions));
        }

        /// Configure semantic tokens for a specific path.
        pub fn set_semantic_tokens(&self, path: &str, tokens: Vec<SemanticToken>) {
            let mut state = self.state.lock().unwrap();
            state
                .semantic_token_responses
                .push((path.to_string(), tokens));
        }

        /// Configure inlay hints for a specific path and offset range.
        pub fn set_inlay_hints(
            &self,
            path: &str,
            start_offset: u32,
            end_offset: u32,
            hints: Vec<InlayHint>,
        ) {
            let mut state = self.state.lock().unwrap();
            state
                .inlay_hint_responses
                .push((path.to_string(), start_offset, end_offset, hints));
        }

        /// Configure completion resolution for a specific path and resolve key.
        pub fn set_resolve_completion(
            &self,
            path: &str,
            data: CompletionResolveData,
            result: Option<CompletionResolveResult>,
        ) {
            let mut state = self.state.lock().unwrap();
            state
                .resolve_completion_responses
                .push((path.to_string(), data, result));
        }

        /// Override the provider identity reported by `provider_id()`.
        ///
        /// Used by dispatch tests that need the mock to impersonate a specific
        /// backend (`"tsgo"` / `"tsserver"` / `"extension"`) so provider-id
        /// validation can be exercised in both the matching and mismatching
        /// directions.
        pub fn set_provider_id(&self, provider_id: &'static str) {
            self.state.lock().unwrap().provider_id = Some(provider_id);
        }

        /// Install a one-shot side effect that fires the FIRST time `open_file` is
        /// called for `path`. The callback runs synchronously, after the state lock
        /// is released and before the open's future is returned. Used to
        /// deterministically interleave a mid-pass event (e.g. a `did_close`) at the
        /// exact moment a specific overlay open fires. See [`MockState::on_open_file`].
        pub fn set_on_open_file(&self, path: &str, callback: Box<dyn FnOnce() + Send>) {
            self.state.lock().unwrap().on_open_file = Some((path.to_string(), callback));
        }

        /// Install a ONE-SHOT callback fired the first time an interactive query
        /// (`get_hover` / `get_completions`) is issued for `path`. The callback
        /// runs synchronously, after the state lock is released and before the
        /// query's future is returned — i.e. between the feature handler's
        /// context capture and its merge of the provider response. Used to
        /// deterministically interleave a mid-request surface mutation (re-sync
        /// generation advance, `did_close`-side surface retirement). See
        /// [`MockState::on_query`].
        pub fn set_on_query(&self, path: &str, callback: Box<dyn FnOnce() + Send>) {
            self.state.lock().unwrap().on_query = Some((path.to_string(), callback));
        }

        /// Get all recorded calls.
        pub fn calls(&self) -> Vec<MockCall> {
            self.state.lock().unwrap().calls.clone()
        }

        /// Get only file sync calls (open/load/update/close).
        pub fn file_sync_calls(&self) -> Vec<MockCall> {
            self.calls()
                .into_iter()
                .filter(|c| {
                    matches!(
                        c,
                        MockCall::OpenFile { .. }
                            | MockCall::LoadFile { .. }
                            | MockCall::UpdateFile { .. }
                            | MockCall::CloseFile { .. }
                    )
                })
                .collect()
        }

        /// Clear all recorded calls.
        pub fn clear_calls(&self) {
            self.state.lock().unwrap().calls.clear();
        }

        /// Make every subsequent file-op (`open_file`/`load_file`/
        /// `update_file`/`close_file`) RECORD its call and then return `Err`.
        ///
        /// The call is still recorded so a test can assert which provider
        /// operations were attempted while verifying that none of them
        /// succeeded.
        pub fn set_fail_file_ops(&self, fail: bool) {
            self.state.lock().unwrap().fail_file_ops = fail;
        }

        /// Make any `open_file`/`load_file`/`update_file` against `path` RECORD
        /// its call and then return `Err`, while every other path succeeds.
        ///
        /// Used to inject a per-KIND sync failure (e.g. fail the IDE `.tsx`
        /// while the API `.ts` succeeds) so tests can prove a kind's stale path
        /// is retained when only that kind's replacement sync fails. `close_file`
        /// is NOT gated, so a wrongful close of the stale path is still observed.
        pub fn set_fail_sync_path(&self, path: &str) {
            self.state
                .lock()
                .unwrap()
                .fail_sync_paths
                .insert(path.to_string());
        }

        /// Test seam: make `register_carrier_member` against `path` RECORD its call
        /// and then BLOCK until the returned [`Notify`](tokio::sync::Notify) is
        /// signalled. Returns the gate the test signals (`notify_one`, which stores
        /// a permit so there is no signal-before-await race) to release the blocked
        /// registration. Used to pause a respawn's carrier replay mid-flight so the
        /// registration TOCTOU window is deterministically observable. Other paths
        /// register without blocking.
        pub fn block_register_carrier_member(
            &self,
            path: &str,
        ) -> std::sync::Arc<tokio::sync::Notify> {
            let gate = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().register_block = Some((path.to_string(), gate.clone()));
            gate
        }

        /// Test seam: make `close_file` against `path` RECORD its call, SIGNAL the
        /// returned `arrived` gate, and then BLOCK until the returned `release` gate
        /// is signalled. Returns `(arrived, release)`: the test awaits `arrived` to
        /// learn the close has been reached (the closing task is now paused INSIDE
        /// the provider close, e.g. mid-`did_close` overlay release), does whatever
        /// concurrent work it needs to interleave, then signals `release`
        /// (`notify_one`, which stores a permit so there is no signal-before-await
        /// race) to let the close return. Other paths close without blocking.
        pub fn block_close_file(
            &self,
            path: &str,
        ) -> (
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        ) {
            let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().close_block =
                Some((path.to_string(), arrived.clone(), release.clone()));
            (arrived, release)
        }

        /// Test seam: pause the next `update_file` for `path`, signalling
        /// `arrived` before awaiting `release`.
        pub fn block_update_file(
            &self,
            path: &str,
        ) -> (
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        ) {
            let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().update_block =
                Some((path.to_string(), arrived.clone(), release.clone()));
            (arrived, release)
        }

        pub fn block_open_file(
            &self,
            path: &str,
        ) -> (
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        ) {
            let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().open_block =
                Some((path.to_string(), arrived.clone(), release.clone()));
            (arrived, release)
        }

        pub fn block_get_completions(
            &self,
            path: &str,
        ) -> (
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        ) {
            let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().completion_block =
                Some((path.to_string(), arrived.clone(), release.clone()));
            (arrived, release)
        }
    }

    /// A `TypeProvider` that always returns errors.
    ///
    /// Simulates a crashed/dead child process (e.g., tsgo pipe closed with OS error 232).
    /// Used to test that callers handle provider errors gracefully.
    pub struct FailingTypeProvider {
        pub error_message: String,
    }

    impl FailingTypeProvider {
        pub fn new(message: &str) -> Self {
            Self {
                error_message: message.to_string(),
            }
        }
    }

    impl TypeProvider for FailingTypeProvider {
        fn provider_id(&self) -> &'static str {
            "tsgo"
        }

        fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn update_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_completions(
            &self,
            _path: &str,
            _offset: u32,
            _trigger_character: Option<&str>,
        ) -> ProviderFuture<'_, CompletionResult> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_definition(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_type_definition(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_references(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_rename_locations(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<RenameLocation>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_signature_help(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Option<SignatureHelp>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_code_actions(
            &self,
            _path: &str,
            _start_offset: u32,
            _end_offset: u32,
            _diagnostics: &[ProviderDiagnosticContext],
        ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_document_highlights(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_inlay_hints(
            &self,
            _path: &str,
            _start_offset: u32,
            _end_offset: u32,
        ) -> ProviderFuture<'_, Vec<InlayHint>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn resolve_completion(
            &self,
            _path: &str,
            _data: CompletionResolveData,
        ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn configure_paths(
            &self,
            _base_url: &str,
            _paths: serde_json::Value,
        ) -> ProviderFuture<'_, ()> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn update_workspace_folders(
            &self,
            _added: Vec<serde_json::Value>,
            _removed: Vec<serde_json::Value>,
        ) -> ProviderFuture<'_, ()> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }
    }

    impl TypeProvider for MockTypeProvider {
        fn provider_id(&self) -> &'static str {
            self.state.lock().unwrap().provider_id.unwrap_or("tsgo")
        }

        fn supports_completion_resolve(&self) -> bool {
            true
        }

        fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            // Record the call + take the one-shot interleave callback (if armed for
            // this exact path) WHILE holding the lock, then RELEASE the lock before
            // running the callback — running it under the std mutex would deadlock if
            // it re-entered the mock. The callback runs synchronously here so its
            // effect (e.g. a `did_close`) is observable before the open's future is
            // even returned, which is the realistic mid-pass ordering.
            let (fail, on_open, block) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::OpenFile {
                    path: path.to_string(),
                    content: content.to_string(),
                });
                let fail = state.fail_file_ops || state.fail_sync_paths.contains(path);
                let on_open = match &state.on_open_file {
                    Some((armed_path, _)) if armed_path == path => {
                        state.on_open_file.take().map(|(_, cb)| cb)
                    }
                    _ => None,
                };
                let block = match &state.open_block {
                    Some((armed_path, _, _)) if armed_path == path => state
                        .open_block
                        .take()
                        .map(|(_, arrived, release)| (arrived, release)),
                    _ => None,
                };
                (fail, on_open, block)
            };
            if let Some(callback) = on_open {
                callback();
            }
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                fail_or_ok(fail, "open_file")
            })
        }

        fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::LoadFile {
                path: path.to_string(),
                content: content.to_string(),
            });
            let fail = state.fail_file_ops || state.fail_sync_paths.contains(path);
            Box::pin(async move { fail_or_ok(fail, "load_file") })
        }

        fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            let (fail, block) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::UpdateFile {
                    path: path.to_string(),
                    content: content.to_string(),
                });
                let fail = state.fail_file_ops || state.fail_sync_paths.contains(path);
                let block = match &state.update_block {
                    Some((armed_path, _, _)) if armed_path == path => state
                        .update_block
                        .take()
                        .map(|(_, arrived, release)| (arrived, release)),
                    _ => None,
                };
                (fail, block)
            };
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                fail_or_ok(fail, "update_file")
            })
        }

        fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
            // Record the call + capture the one-shot block gate (if armed for this
            // exact path) WHILE holding the sync lock, then RELEASE the lock before
            // awaiting — awaiting under the std mutex would deadlock every other
            // mock op. The gate is taken (one-shot) so subsequent closes of the
            // same path do not block.
            let (fail, block) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::CloseFile {
                    path: path.to_string(),
                });
                // `close_file` is intentionally NOT gated by `fail_sync_paths`:
                // failure-injection tests want to observe whether a stale path was
                // (wrongly) closed even while a sibling kind's sync fails.
                let fail = state.fail_file_ops;
                let block = match &state.close_block {
                    Some((armed_path, _, _)) if armed_path == path => state
                        .close_block
                        .take()
                        .map(|(_, arrived, release)| (arrived, release)),
                    _ => None,
                };
                (fail, block)
            };
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    // Signal the test that the close has been reached (the closing
                    // task is paused HERE), then await the test's release.
                    arrived.notify_one();
                    release.notified().await;
                }
                fail_or_ok(fail, "close_file")
            })
        }

        fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::NotifyCarrierChanged {
                companion_path: companion_path.to_string(),
            });
            Box::pin(async { Ok(()) })
        }

        fn register_carrier_member(
            &self,
            source_path: &str,
            companion_path: &str,
            content: &str,
            project_file_name: &str,
        ) -> ProviderFuture<'_, ()> {
            // Record the call and capture the block gate (if this path is gated)
            // while holding the sync lock, then RELEASE the lock before awaiting —
            // awaiting under the std mutex would deadlock every other mock op.
            let block = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::RegisterCarrierMember {
                    source_path: source_path.to_string(),
                    companion_path: companion_path.to_string(),
                    content: content.to_string(),
                    project_file_name: project_file_name.to_string(),
                });
                state
                    .register_block
                    .as_ref()
                    .filter(|(blocked_path, _)| blocked_path == companion_path)
                    .map(|(_, gate)| gate.clone())
            };
            Box::pin(async move {
                if let Some(gate) = block {
                    gate.notified().await;
                }
                Ok(())
            })
        }

        fn get_completions(
            &self,
            path: &str,
            offset: u32,
            _trigger_character: Option<&str>,
        ) -> ProviderFuture<'_, CompletionResult> {
            let (items, on_query, block) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetCompletions {
                    path: path.to_string(),
                    offset,
                });
                let items = state
                    .completion_responses
                    .iter()
                    .find(|(p, o, _)| p == path && *o == offset)
                    .map(|(_, _, items)| items.clone())
                    .unwrap_or_default();
                let on_query = match &state.on_query {
                    Some((armed_path, _)) if armed_path == path => {
                        state.on_query.take().map(|(_, cb)| cb)
                    }
                    _ => None,
                };
                let block = match &state.completion_block {
                    Some((armed_path, _, _)) if armed_path == path => state
                        .completion_block
                        .take()
                        .map(|(_, arrived, release)| (arrived, release)),
                    _ => None,
                };
                (items, on_query, block)
            };
            // Run the one-shot mid-request seam AFTER releasing the state lock
            // (a callback that re-enters the mock must not deadlock).
            if let Some(callback) = on_query {
                callback();
            }
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                Ok(CompletionResult {
                    items,
                    is_incomplete: false,
                })
            })
        }

        fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
            let (result, on_query, fail, hang) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetHover {
                    path: path.to_string(),
                    offset,
                });
                let fail = if state.fail_next_hovers > 0 {
                    state.fail_next_hovers -= 1;
                    true
                } else {
                    false
                };
                let result = state
                    .hover_responses
                    .iter()
                    .find(|(p, o, _)| p == path && *o == offset)
                    .and_then(|(_, _, info)| info.clone());
                let on_query = match &state.on_query {
                    Some((armed_path, _)) if armed_path == path => {
                        state.on_query.take().map(|(_, cb)| cb)
                    }
                    _ => None,
                };
                (result, on_query, fail, state.hang_hover)
            };
            if hang {
                // A wedged provider: never resolves. The handler must fail
                // closed on its request deadline rather than park here.
                return Box::pin(std::future::pending());
            }
            // Run the one-shot mid-request seam AFTER releasing the state lock
            // (a callback that re-enters the mock must not deadlock).
            if let Some(callback) = on_query {
                callback();
            }
            Box::pin(async move {
                if fail {
                    return Err(TypeProviderError::new(
                        "scripted transient hover failure".to_string(),
                    ));
                }
                Ok(result)
            })
        }

        fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
            let (result, on_query) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetDiagnostics {
                    path: path.to_string(),
                });
                let result = state
                    .diagnostic_responses
                    .iter()
                    .find(|(p, _)| p == path)
                    .map(|(_, diags)| diags.clone())
                    .unwrap_or_default();
                let on_query = match &state.on_query {
                    Some((armed_path, _)) if armed_path == path => {
                        state.on_query.take().map(|(_, cb)| cb)
                    }
                    _ => None,
                };
                (result, on_query)
            };
            // Run the one-shot mid-request seam AFTER releasing the state lock
            // (a callback that re-enters the mock must not deadlock).
            if let Some(callback) = on_query {
                callback();
            }
            Box::pin(async move { Ok(result) })
        }

        fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let (result, on_query, hang) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetDefinition {
                    path: path.to_string(),
                    offset,
                });
                let result = state
                    .definition_responses
                    .iter()
                    .find(|(p, o, _)| p == path && *o == offset)
                    .map(|(_, _, locs)| locs.clone())
                    .unwrap_or_default();
                let on_query = match &state.on_query {
                    Some((armed_path, _)) if armed_path == path => {
                        state.on_query.take().map(|(_, cb)| cb)
                    }
                    _ => None,
                };
                (result, on_query, state.hang_definition)
            };
            if hang {
                // A wedged provider: never resolves. The handler must fail closed
                // on its production deadline rather than park here forever.
                return Box::pin(std::future::pending());
            }
            // Run the one-shot mid-request seam AFTER releasing the state lock
            // (a callback that re-enters the mock must not deadlock).
            if let Some(callback) = on_query {
                callback();
            }
            Box::pin(async move { Ok(result) })
        }

        fn get_type_definition(
            &self,
            path: &str,
            offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetTypeDefinition {
                path: path.to_string(),
                offset,
            });
            let result = state
                .type_definition_responses
                .iter()
                .find(|(p, o, _)| p == path && *o == offset)
                .map(|(_, _, locs)| locs.clone())
                .unwrap_or_default();
            Box::pin(async move { Ok(result) })
        }

        fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let (result, on_query) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetReferences {
                    path: path.to_string(),
                    offset,
                });
                let result = state
                    .reference_responses
                    .iter()
                    .find(|(p, o, _)| p == path && *o == offset)
                    .map(|(_, _, locs)| locs.clone())
                    .unwrap_or_default();
                let on_query = match &state.on_query {
                    Some((armed_path, _)) if armed_path == path => {
                        state.on_query.take().map(|(_, cb)| cb)
                    }
                    _ => None,
                };
                (result, on_query)
            };
            // Run the one-shot mid-request seam AFTER releasing the state lock
            // (a callback that re-enters the mock must not deadlock).
            if let Some(callback) = on_query {
                callback();
            }
            Box::pin(async move { Ok(result) })
        }

        fn get_rename_locations(
            &self,
            path: &str,
            offset: u32,
        ) -> ProviderFuture<'_, Vec<RenameLocation>> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetRenameLocations {
                path: path.to_string(),
                offset,
            });
            let result = state
                .rename_responses
                .iter()
                .find(|(p, o, _)| p == path && *o == offset)
                .map(|(_, _, locs)| locs.clone())
                .unwrap_or_default();
            Box::pin(async move { Ok(result) })
        }

        fn get_signature_help(
            &self,
            path: &str,
            offset: u32,
        ) -> ProviderFuture<'_, Option<SignatureHelp>> {
            let (result, on_query, hang) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetSignatureHelp {
                    path: path.to_string(),
                    offset,
                });
                let result = state
                    .signature_help_responses
                    .iter()
                    .find(|(p, o, _)| p == path && *o == offset)
                    .and_then(|(_, _, help)| help.clone());
                let on_query = match &state.on_query {
                    Some((armed_path, _)) if armed_path == path => {
                        state.on_query.take().map(|(_, cb)| cb)
                    }
                    _ => None,
                };
                (result, on_query, state.hang_signature_help)
            };
            if hang {
                // A wedged provider: never resolves. The handler must fail
                // closed on its request deadline rather than park here.
                return Box::pin(std::future::pending());
            }
            // Run the one-shot mid-request seam AFTER releasing the state lock
            // (a callback that re-enters the mock must not deadlock).
            if let Some(callback) = on_query {
                callback();
            }
            Box::pin(async move { Ok(result) })
        }

        fn get_code_actions(
            &self,
            path: &str,
            start_offset: u32,
            end_offset: u32,
            diagnostics: &[ProviderDiagnosticContext],
        ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetCodeActions {
                path: path.to_string(),
                start_offset,
                end_offset,
                diagnostics: diagnostics.to_vec(),
            });
            let result = state
                .code_action_responses
                .iter()
                .find(|(p, so, eo, _)| p == path && *so == start_offset && *eo == end_offset)
                .map(|(_, _, _, actions)| actions.clone())
                .unwrap_or_default();
            Box::pin(async move { Ok(result) })
        }

        fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetSemanticTokens {
                path: path.to_string(),
            });
            let result = state
                .semantic_token_responses
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, tokens)| tokens.clone())
                .unwrap_or_default();
            Box::pin(async move { Ok(result) })
        }

        fn get_document_highlights(
            &self,
            path: &str,
            offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetDocumentHighlights {
                path: path.to_string(),
                offset,
            });
            let result = state
                .highlight_responses
                .iter()
                .find(|(p, o, _)| p == path && *o == offset)
                .map(|(_, _, hl)| hl.clone())
                .unwrap_or_default();
            Box::pin(async move { Ok(result) })
        }

        fn get_inlay_hints(
            &self,
            path: &str,
            start_offset: u32,
            end_offset: u32,
        ) -> ProviderFuture<'_, Vec<InlayHint>> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetInlayHints {
                path: path.to_string(),
                start_offset,
                end_offset,
            });
            let result = state
                .inlay_hint_responses
                .iter()
                .find(|(p, so, eo, _)| p == path && *so == start_offset && *eo == end_offset)
                .map(|(_, _, _, hints)| hints.clone())
                .unwrap_or_default();
            Box::pin(async move { Ok(result) })
        }

        fn resolve_completion(
            &self,
            path: &str,
            data: CompletionResolveData,
        ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::ResolveCompletion {
                path: path.to_string(),
                data: data.clone(),
            });
            let result = state
                .resolve_completion_responses
                .iter()
                .find(|(p, candidate, _)| p == path && *candidate == data)
                .and_then(|(_, _, resolved)| resolved.clone());
            Box::pin(async move { Ok(result) })
        }

        fn configure_paths(
            &self,
            base_url: &str,
            paths: serde_json::Value,
        ) -> ProviderFuture<'_, ()> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::ConfigurePaths {
                base_url: base_url.to_string(),
                paths,
            });
            Box::pin(async { Ok(()) })
        }

        fn update_workspace_folders(
            &self,
            added: Vec<serde_json::Value>,
            removed: Vec<serde_json::Value>,
        ) -> ProviderFuture<'_, ()> {
            let mut state = self.state.lock().unwrap();
            state
                .calls
                .push(MockCall::UpdateWorkspaceFolders { added, removed });
            Box::pin(async { Ok(()) })
        }
    }
}
