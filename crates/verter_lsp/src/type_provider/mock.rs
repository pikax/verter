//! Mock implementation of `TypeProvider` for testing.
//!
//! Allows tests to configure expected responses for each method.
//! Tracks all calls for assertion purposes.

#[cfg(test)]
pub use inner::*;

#[cfg(test)]
mod inner {
    use std::sync::{Arc, Mutex};

    use crate::server::test_support::{RequestBarrier, RequestBarriers};
    use crate::type_provider::protocol::*;
    use crate::type_provider::traits::{ProviderFuture, ProviderQuery, TypeProvider};
    use verter_type_runtime::provider_query::{ConflictKind, ProviderQueryConflict};

    /// Test-side mint for the branded provider display signature.
    ///
    /// Routes through [`MockTypeProvider`] — a genuine [`TypeProvider`] impl —
    /// because the witness that authorizes minting a [`DisplaySignature`] is
    /// obtainable only through a provider impl. Test fixtures use this instead
    /// of forging the brand (which does not compile: the inner field is
    /// private and the type is neither `Deserialize` nor coercible).
    pub fn test_display_signature(value: &str) -> DisplaySignature {
        DisplaySignature::from_provider_wire(MockTypeProvider::new().provider_wire_witness(), value)
    }

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
        /// An open issued on the BACKGROUND priority lane. Recorded distinctly from
        /// [`MockCall::OpenFile`] so a test can tell the interactive lane (which
        /// preempts user-facing traffic and, on the owned tsgo provider, takes a
        /// diagnostic barrier) from the background one.
        OpenFileBackground {
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
        WatchedFilesChanged {
            changes: Vec<verter_type_runtime::WatchedFileChange>,
        },
        NotifyCarrierChanged {
            companion_path: String,
        },
        NotifyCarriersChanged {
            companion_paths: Vec<String>,
        },
        RegisterCarrierMember {
            source_path: String,
            companion_path: String,
            content: String,
            project_file_name: String,
        },
        RegisterCarrierMetadata {
            source_path: String,
            companion_path: String,
            content: String,
            project_file_name: String,
        },
        ActivateCarrierMember {
            source_path: String,
            companion_path: String,
            project_file_name: String,
            script_kind: verter_type_runtime::CarrierScriptKind,
        },
        ActivateCarrierMembers {
            members: Vec<verter_type_runtime::CarrierActivation>,
        },
    }

    /// Shared state for the mock provider.
    #[derive(Default)]
    struct MockState {
        carrier_refresh_block: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
        carrier_batch_observer: Option<std::sync::Arc<tokio::sync::Notify>>,
        carrier_batch_block: Option<(
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        )>,
        /// One-shot pause for the next `activate_carrier_member`: `(arrived,
        /// release)`, as [`MockTypeProvider::block_next_carrier_activation`].
        carrier_activation_block: Option<(
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        )>,
        /// Bytes this engine accepted. Absent means not applied.
        applied: std::collections::HashMap<String, Arc<str>>,
        /// The serving engine incarnation: replacing the engine advances it.
        /// A query answer is bound to the incarnation that selected it, and a
        /// file write to the incarnation it was issued to.
        incarnation: Arc<std::sync::atomic::AtomicU64>,
        /// For each interactive query, in order: its path, the bytes the
        /// engine held there at the moment it selected the answer — the bytes
        /// the answer was evaluated against — and the incarnation that held
        /// them.
        evaluations: Vec<(String, Option<Arc<str>>, u64)>,
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
        fail_carrier_metadata_sources: std::collections::HashSet<String>,
        hover_responses: Vec<(String, u32, Option<HoverInfo>)>,
        /// Scripted transient hover failures: while > 0, each `get_hover`
        /// RECORDS its call and returns `Err` (simulating a provider/transport
        /// failure), decrementing the counter. Pins the no-silent-empty
        /// recovery contract: a failed provider hover must resync+retry,
        /// never vanish silently.
        fail_next_hovers: usize,
        /// As `fail_next_hovers`, for `get_definition`: while > 0, each call
        /// RECORDS itself and returns `Err` (a transient provider/transport
        /// failure), decrementing the counter. Pins the definition handler's
        /// resync+retry-once recovery — the same no-silent-empty contract
        /// hover holds, without which a member-position CTRL+CLICK vanishes
        /// while hover at the same cursor recovers.
        fail_next_definitions: usize,
        /// As `fail_next_definitions`, for `get_type_definition`.
        fail_next_type_definitions: usize,
        /// As `fail_next_hovers`, for `get_completions`: while > 0, each call
        /// RECORDS itself and returns `Err` (a transient provider/transport
        /// failure), decrementing the counter. The ONLY way a completion
        /// request reaches its bounded-recovery resync — and therefore the
        /// only way the production recovery arm's document-lane discipline is
        /// observable at all — so the lane-fence proofs drive it.
        fail_next_completions: usize,
        /// When `true`, `get_definition` RECORDS its call and then returns a
        /// future that NEVER resolves, simulating a wedged type provider (a
        /// managed tsgo stuck in a busy dispatch loop). Drives the handler
        /// cancellation and explicitly bounded diagnostic-host behavior.
        hang_definition: bool,
        /// As `hang_definition`, for `get_hover`.
        hang_hover: bool,
        /// As `hang_definition`, for `get_signature_help`.
        hang_signature_help: bool,
        /// As `hang_definition`, for `get_diagnostics`. This pins that an
        /// unbounded provider pull cannot starve independently available
        /// framework diagnostics.
        hang_diagnostics: bool,
        diagnostics_gate: Option<std::sync::Arc<tokio::sync::Semaphore>>,
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
        /// The ambient request deadline each `open_file` / `update_file` was
        /// issued under, in call order: the bound the engine would apply to
        /// that write.
        write_deadlines: Vec<(String, Option<tokio::time::Instant>)>,
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
        /// One-shot async gate for `get_rename_locations`, used to move project
        /// ownership after rename admission while the provider response is still
        /// in flight.
        #[allow(clippy::type_complexity)]
        rename_block: Option<(
            String,
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        )>,
        /// One-shot async gate for `configure_paths`, used to hold background
        /// initialization after it installs a rebuilt ownership authority but
        /// before it publishes the rebuilt workspace root.
        #[allow(clippy::type_complexity)]
        configure_paths_block: Option<(
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        )>,
        /// The configured-owner authority most recently installed by background
        /// initialization. Tests query it to prove the provider has crossed the
        /// ownership transition while the published root is deliberately held old.
        project_ownership:
            Option<std::sync::Arc<dyn verter_type_runtime::traits::ConfiguredOwnerAuthority>>,
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
        /// Test seam: the pid `child_pid` reports. Structural start
        /// announcements carry a real child pid; tests of the announcement
        /// wire policy distinguish engines by it.
        child_pid: Option<u32>,
        /// Test seam: the pulse `provider_restart_pulse` reports. `None` (the
        /// default) keeps the trait default — no engine-start pulse — so only
        /// tests that drive the pending-sync re-drive wiring opt in.
        restart_pulse: Option<std::sync::Arc<tokio::sync::Notify>>,
        /// Test seam: the bytes the engine holds for a foreign target file. A
        /// definition, type-definition, references, rename or code-action
        /// answer locating into one is refused when its requester will map it
        /// through other bytes, or when those bytes move before the answer
        /// settles, as an adapter's target decode and settlement refuse it.
        engine_targets: std::collections::HashMap<String, Arc<str>>,
    }

    impl MockState {
        /// Refuse an answer locating into `targets` when the query's requester
        /// will map a location there through bytes other than the ones the
        /// engine holds for it.
        fn check_targets<'a>(
            &self,
            query: &ProviderQuery,
            targets: impl IntoIterator<Item = &'a str>,
        ) -> Result<(), TypeProviderError> {
            for path in targets {
                if let Some(held) = self.engine_targets.get(path) {
                    query.check_intended_target(path, Some(held))?;
                }
            }
            Ok(())
        }

        /// The engine's bytes for each of `targets` as the query is
        /// dispatched, for [`settle_targets`] to recheck once the answer is in.
        fn held_targets<'a>(
            &self,
            targets: impl IntoIterator<Item = &'a str>,
        ) -> Vec<(String, Option<Arc<str>>)> {
            targets
                .into_iter()
                .map(|path| (path.to_string(), self.engine_targets.get(path).cloned()))
                .collect()
        }

        /// Record that a query at `path` is evaluated now, against the bytes
        /// the engine holds there at this instant.
        fn note_evaluation(&mut self, path: &str) {
            let bytes = self.applied.get(path).cloned();
            let incarnation = self.incarnation.load(std::sync::atomic::Ordering::SeqCst);
            self.evaluations
                .push((path.to_string(), bytes, incarnation));
        }
    }

    /// Refuse `result` when the engine's bytes for any target it locates into
    /// moved after the query was dispatched, as an adapter's settlement refuses
    /// an answer whose decoded targets changed under it.
    fn settle_targets<T>(
        state: &Mutex<MockState>,
        held: &[(String, Option<Arc<str>>)],
        result: Result<T, TypeProviderError>,
    ) -> Result<T, TypeProviderError> {
        let result = result?;
        let state = state.lock().unwrap();
        for (path, at_dispatch) in held {
            if state.engine_targets.get(path) != at_dispatch.as_ref() {
                return Err(ProviderQueryConflict::new(path, ConflictKind::Moved).into());
            }
        }
        Ok(result)
    }

    /// A mock `TypeProvider` for testing.
    ///
    /// All methods record their calls and return configured responses.
    /// Default response for methods without configured data is empty/None.
    #[derive(Clone)]
    pub struct MockTypeProvider {
        state: Arc<Mutex<MockState>>,
        call_recorded: Arc<tokio::sync::Notify>,
        /// Request barriers every interactive query reaches: dispatch before
        /// the answer is produced, decode after. Kept outside `state`, whose
        /// lock several query methods hold while building their future.
        request_barriers: Arc<Mutex<Option<Arc<RequestBarriers>>>>,
        /// Interactive queries still owed a scripted delivery failure, for
        /// every query kind alike.
        failed_deliveries: Arc<std::sync::atomic::AtomicUsize>,
        /// The serving engine incarnation, shared with `state` and readable
        /// without its lock.
        incarnation: Arc<std::sync::atomic::AtomicU64>,
    }

    impl Default for MockTypeProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockTypeProvider {
        pub fn new() -> Self {
            let state = MockState::default();
            let incarnation = Arc::clone(&state.incarnation);
            Self {
                state: Arc::new(Mutex::new(state)),
                call_recorded: Arc::new(tokio::sync::Notify::new()),
                request_barriers: Arc::new(Mutex::new(None)),
                failed_deliveries: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                incarnation,
            }
        }

        /// Make every interactive query reach `barriers` at provider dispatch
        /// and at provider-response decode.
        pub(crate) fn set_request_barriers(&self, barriers: Arc<RequestBarriers>) {
            *self.request_barriers.lock().unwrap() = Some(barriers);
        }

        /// Fail the next `count` interactive queries of any kind after their
        /// dispatch, as a provider whose response never decodes.
        pub fn fail_next_deliveries(&self, count: usize) {
            self.failed_deliveries
                .store(count, std::sync::atomic::Ordering::SeqCst);
        }

        fn barriered<'a, T: Send + 'a>(
            &self,
            answer: ProviderFuture<'a, T>,
        ) -> ProviderFuture<'a, T> {
            let barriers = self.request_barriers.lock().unwrap().clone();
            let failed_deliveries = Arc::clone(&self.failed_deliveries);
            // The answer was selected by the incarnation serving now. Like the
            // provider hub's retired-result fence, an answer whose engine was
            // replaced before it settled is refused.
            let incarnation = Arc::clone(&self.incarnation);
            let selected_by = incarnation.load(std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move {
                if let Some(barriers) = &barriers {
                    barriers.reach(RequestBarrier::ProviderDispatch).await;
                }
                let fail = failed_deliveries
                    .fetch_update(
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                        |owed| owed.checked_sub(1),
                    )
                    .is_ok();
                let result = if fail {
                    Err(TypeProviderError::new(
                        "scripted provider delivery failure".to_string(),
                    ))
                } else {
                    answer.await
                };
                let result = if incarnation.load(std::sync::atomic::Ordering::SeqCst) == selected_by
                {
                    result
                } else {
                    Err(TypeProviderError::new(
                        "the engine incarnation that evaluated this query was retired".to_string(),
                    ))
                };
                if let Some(barriers) = &barriers {
                    barriers.reach(RequestBarrier::ProviderDecode).await;
                }
                result
            })
        }

        /// Settle foreign targets after every response wait, including the
        /// decode barrier, before handing their locations back to the caller.
        fn barriered_targets<'a, T: Send + 'a>(
            &self,
            held: Vec<(String, Option<Arc<str>>)>,
            answer: ProviderFuture<'a, T>,
        ) -> ProviderFuture<'a, T> {
            let answer = self.barriered(answer);
            let state = Arc::clone(&self.state);
            Box::pin(async move { settle_targets(&state, &held, answer.await) })
        }

        fn note_recorded(&self) {
            self.call_recorded.notify_waiters();
        }

        /// Wait until `pred` holds over the recorded call log. Interest is
        /// enabled before the re-check so a call landing between the two
        /// cannot be missed.
        pub async fn wait_until_calls(&self, mut pred: impl FnMut(&[MockCall]) -> bool) {
            loop {
                let notified = self.call_recorded.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if pred(&self.calls()) {
                    return;
                }
                notified.await;
            }
        }

        /// Configure a hover response for a specific path and offset.
        pub fn set_hover(&self, path: &str, offset: u32, info: Option<HoverInfo>) {
            let mut state = self.state.lock().unwrap();
            state.hover_responses.push((path.to_string(), offset, info));
        }

        /// Set the pid `child_pid` reports — the identity structural start
        /// announcements carry, so wire-policy tests can tell engines apart.
        pub fn set_child_pid(&self, pid: Option<u32>) {
            self.state.lock().unwrap().child_pid = pid;
        }

        /// Script the next `count` `get_hover` calls to fail with `Err`
        /// (transient provider/transport failure) before normal responses
        /// resume.
        pub fn fail_next_hovers(&self, count: usize) {
            let mut state = self.state.lock().unwrap();
            state.fail_next_hovers = count;
        }

        /// Script the next `count` `get_definition` calls to fail with `Err`
        /// (transient provider/transport failure) before normal responses
        /// resume.
        pub fn fail_next_definitions(&self, count: usize) {
            let mut state = self.state.lock().unwrap();
            state.fail_next_definitions = count;
        }

        /// Script the next `count` `get_type_definition` calls to fail with
        /// `Err` (transient provider/transport failure) before normal
        /// responses resume.
        pub fn fail_next_type_definitions(&self, count: usize) {
            let mut state = self.state.lock().unwrap();
            state.fail_next_type_definitions = count;
        }

        /// Script the next `count` `get_completions` calls to fail with `Err`
        /// (transient provider/transport failure) before normal responses
        /// resume. This is the seam that makes completion's bounded recovery
        /// (the resync arm that republishes the open carrier) reachable.
        pub fn fail_next_completions(&self, count: usize) {
            let mut state = self.state.lock().unwrap();
            state.fail_next_completions = count;
        }

        /// Make every subsequent `get_definition` RECORD its call and then hang
        /// forever (a wedged type provider). The handler-deadline repro uses
        /// this to exercise cancellation or an explicit diagnostic-host bound.
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
        /// keystroke; production recovery comes from engine lifecycle health and
        /// client cancellation, not a feature latency deadline.
        pub fn hang_signature_help(&self) {
            let mut state = self.state.lock().unwrap();
            state.hang_signature_help = true;
        }

        /// Wedge `get_diagnostics` after recording the call.
        /// Test seam: every later `get_diagnostics` is RECORDED and then waits
        /// for one permit on the returned semaphore, so a test admits the
        /// pulls one at a time and can observe how many are in flight.
        pub fn gate_diagnostics(&self) -> std::sync::Arc<tokio::sync::Semaphore> {
            let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
            self.state.lock().unwrap().diagnostics_gate = Some(gate.clone());
            gate
        }

        pub fn hang_diagnostics(&self) {
            let mut state = self.state.lock().unwrap();
            state.hang_diagnostics = true;
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

        /// Hold `content` as the engine's bytes for the foreign target file
        /// `path`, whatever surface the LSP recorded for it.
        pub fn hold_engine_target(&self, path: &str, content: &str) {
            let mut state = self.state.lock().unwrap();
            state
                .engine_targets
                .insert(path.to_string(), Arc::from(content));
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

        /// Install the engine-start pulse `provider_restart_pulse` reports, so
        /// a test can fire (or withhold) the retry signal the pending-sync
        /// re-drive wiring listens for. `None` restores the trait default.
        pub fn set_restart_pulse(&self, pulse: Option<std::sync::Arc<tokio::sync::Notify>>) {
            self.state.lock().unwrap().restart_pulse = pulse;
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

        pub fn observe_carrier_batch(&self) -> std::sync::Arc<tokio::sync::Notify> {
            let observer = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().carrier_batch_observer = Some(observer.clone());
            observer
        }

        /// Pause the next `activate_carrier_members` until `release` is signalled.
        /// `arrived` fires once the engine has entered the batch.
        pub fn block_next_carrier_batch(
            &self,
        ) -> (
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        ) {
            let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().carrier_batch_block =
                Some((arrived.clone(), release.clone()));
            (arrived, release)
        }

        /// A dispatched refresh withholds applied bytes and FIFO diagnostics until released.
        pub fn block_next_carrier_refresh(
            &self,
        ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
            let arrived = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let gate = Arc::new(tokio::sync::Semaphore::new(0));
            let mut state = self.state.lock().unwrap();
            state.diagnostics_gate = Some(gate);
            state.carrier_refresh_block = Some((arrived.clone(), release.clone()));
            (arrived, release)
        }

        /// Pause the next `activate_carrier_member` until `release` is
        /// signalled. `arrived` fires once the engine has entered it.
        pub fn block_next_carrier_activation(
            &self,
        ) -> (
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        ) {
            let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().carrier_activation_block =
                Some((arrived.clone(), release.clone()));
            (arrived, release)
        }

        /// The incarnation a file write issued now is delivered to.
        fn serving_incarnation(&self) -> u64 {
            self.incarnation.load(std::sync::atomic::Ordering::SeqCst)
        }

        /// Accept `content` at `path` for a write issued to `issued_to`. A
        /// write issued to a retired incarnation reaches nothing the
        /// replacement holds.
        fn accept_applied(&self, path: &str, content: &str, issued_to: u64) {
            let mut state = self.state.lock().unwrap();
            if self.serving_incarnation() != issued_to {
                return;
            }
            state.applied.insert(path.to_string(), Arc::from(content));
        }

        fn drop_applied(&self, path: &str) {
            self.state.lock().unwrap().applied.remove(path);
        }

        /// Model an engine restart: the new incarnation holds none of the bytes
        /// the previous one accepted, so no earlier delivery is still applied.
        pub fn forget_applied_content(&self) {
            self.state.lock().unwrap().applied.clear();
        }

        /// Model the serving engine retiring and a replacement incarnation
        /// taking over: the replacement holds none of the bytes the retired
        /// engine accepted, an answer the retired engine selected never
        /// settles, and a write issued to the retired engine never reaches the
        /// replacement.
        pub fn replace_engine(&self) {
            let mut state = self.state.lock().unwrap();
            self.incarnation
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            state.applied.clear();
        }

        /// The serving engine incarnation.
        pub(crate) fn incarnation(&self) -> u64 {
            self.serving_incarnation()
        }

        /// Model a delivery of `path` the engine lost: it no longer holds the
        /// bytes it accepted there.
        pub fn lose_delivery(&self, path: &str) {
            self.drop_applied(path);
        }

        /// Model a delivery of `content` under `path` that reached the engine
        /// through another writer, before any surface describing it was
        /// recorded: the engine now holds those bytes.
        pub fn accept_unrecorded_delivery(&self, path: &str, content: &str) {
            self.accept_applied(path, content, self.serving_incarnation());
        }

        /// Get all recorded calls.
        pub fn calls(&self) -> Vec<MockCall> {
            self.state.lock().unwrap().calls.clone()
        }

        /// The ambient request deadline every `open_file` / `update_file` of
        /// `path` was issued under, in call order.
        pub fn write_deadlines(&self, path: &str) -> Vec<Option<tokio::time::Instant>> {
            self.state
                .lock()
                .unwrap()
                .write_deadlines
                .iter()
                .filter(|(written, _)| written == path)
                .map(|(_, deadline)| *deadline)
                .collect()
        }

        /// Get only file sync calls (open/load/update/close).
        pub fn file_sync_calls(&self) -> Vec<MockCall> {
            self.calls()
                .into_iter()
                .filter(|c| {
                    matches!(
                        c,
                        MockCall::OpenFile { .. }
                            | MockCall::OpenFileBackground { .. }
                            | MockCall::LoadFile { .. }
                            | MockCall::UpdateFile { .. }
                            | MockCall::CloseFile { .. }
                    )
                })
                .collect()
        }

        /// Clear all recorded calls.
        pub fn clear_calls(&self) {
            let mut state = self.state.lock().unwrap();
            state.calls.clear();
            state.evaluations.clear();
        }

        /// The bytes the engine held at `path` when it evaluated the most
        /// recent interactive query there: `None` when no query reached it,
        /// `Some(None)` when the engine held nothing at the path.
        pub(crate) fn last_evaluated_bytes(&self, path: &str) -> Option<Option<Arc<str>>> {
            self.state
                .lock()
                .unwrap()
                .evaluations
                .iter()
                .rev()
                .find(|(evaluated, _, _)| evaluated == path)
                .map(|(_, bytes, _)| bytes.clone())
        }

        /// The engine incarnation that evaluated the most recent interactive
        /// query at `path`, if any reached it.
        pub(crate) fn last_evaluating_incarnation(&self, path: &str) -> Option<u64> {
            self.state
                .lock()
                .unwrap()
                .evaluations
                .iter()
                .rev()
                .find(|(evaluated, _, _)| evaluated == path)
                .map(|(_, _, incarnation)| *incarnation)
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
        /// Test seam: every carrier-metadata registration for `source_path`
        /// is recorded and then rejected.
        pub fn set_fail_carrier_metadata_source(&self, source_path: &str) {
            self.state
                .lock()
                .unwrap()
                .fail_carrier_metadata_sources
                .insert(source_path.to_string());
        }

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

        /// [`Self::block_open_file`] for whichever path is opened NEXT.
        pub fn block_next_open_file(
            &self,
        ) -> (
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        ) {
            self.block_open_file("")
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

        /// Pause the next `get_rename_locations` for `path`, signalling
        /// `arrived` before awaiting `release`.
        pub fn block_get_rename_locations(
            &self,
            path: &str,
        ) -> (
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        ) {
            let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().rename_block =
                Some((path.to_string(), arrived.clone(), release.clone()));
            (arrived, release)
        }

        /// Pause the next `configure_paths`, signalling `arrived` before awaiting
        /// `release`. Background init installs provider ownership before this
        /// call, while publishing the rebuilt root after it.
        pub fn block_configure_paths(
            &self,
        ) -> (
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        ) {
            let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            self.state.lock().unwrap().configure_paths_block =
                Some((arrived.clone(), release.clone()));
            (arrived, release)
        }

        /// Query the provider's currently installed ownership authority.
        pub fn configured_owner(
            &self,
            canonical_id: &str,
        ) -> Option<verter_type_runtime::traits::ProjectOwnership> {
            self.state
                .lock()
                .unwrap()
                .project_ownership
                .as_ref()
                .map(|authority| authority.configured_owner(canonical_id))
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

        /// Every file op fails; a background load is no exception.
        fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            self.open_file(path, content)
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
            _query: &ProviderQuery,
            _offset: u32,
            _trigger_character: Option<&str>,
        ) -> ProviderFuture<'_, CompletionResult> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_hover(
            &self,
            _query: &ProviderQuery,
            _offset: u32,
        ) -> ProviderFuture<'_, Option<HoverInfo>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_definition(
            &self,
            _query: &ProviderQuery,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_type_definition(
            &self,
            _query: &ProviderQuery,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_references(
            &self,
            _query: &ProviderQuery,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_rename_locations(
            &self,
            _query: &ProviderQuery,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<RenameLocation>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_signature_help(
            &self,
            _query: &ProviderQuery,
            _offset: u32,
        ) -> ProviderFuture<'_, Option<SignatureHelp>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_code_actions(
            &self,
            _query: &ProviderQuery,
            _start_offset: u32,
            _end_offset: u32,
            _diagnostics: &[ProviderDiagnosticContext],
        ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_semantic_tokens(
            &self,
            _query: &ProviderQuery,
        ) -> ProviderFuture<'_, Vec<SemanticToken>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_document_highlights(
            &self,
            _query: &ProviderQuery,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn get_inlay_hints(
            &self,
            _query: &ProviderQuery,
            _start_offset: u32,
            _end_offset: u32,
        ) -> ProviderFuture<'_, Vec<InlayHint>> {
            let msg = self.error_message.clone();
            Box::pin(async move { Err(TypeProviderError::new(msg)) })
        }

        fn resolve_completion(
            &self,
            _query: &ProviderQuery,
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
        fn applied_content(&self, path: &str) -> verter_type_runtime::traits::AppliedContent {
            use verter_type_runtime::traits::AppliedContent;
            match self.state.lock().unwrap().applied.get(path) {
                Some(bytes) => AppliedContent::Applied(Arc::clone(bytes)),
                None => AppliedContent::NotApplied,
            }
        }

        fn provider_id(&self) -> &'static str {
            self.state.lock().unwrap().provider_id.unwrap_or("tsgo")
        }

        fn child_pid(&self) -> Option<u32> {
            self.state.lock().unwrap().child_pid
        }

        fn provider_restart_pulse(&self) -> Option<std::sync::Arc<tokio::sync::Notify>> {
            self.state.lock().unwrap().restart_pulse.clone()
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
                state
                    .write_deadlines
                    .push((path.to_string(), verter_type_runtime::deadline::current()));
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
                    Some((armed_path, _, _)) if armed_path.is_empty() || armed_path == path => {
                        state
                            .open_block
                            .take()
                            .map(|(_, arrived, release)| (arrived, release))
                    }
                    _ => None,
                };
                (fail, on_open, block)
            };
            self.note_recorded();
            if let Some(callback) = on_open {
                callback();
            }
            let this = self.clone();
            let path_owned = path.to_string();
            let content_owned = content.to_string();
            let issued_to = self.serving_incarnation();
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                fail_or_ok(fail, "open_file")?;
                this.accept_applied(&path_owned, &content_owned, issued_to);
                Ok(())
            })
        }

        fn notify_watched_files_changed<'a>(
            &'a self,
            changes: &'a [verter_type_runtime::WatchedFileChange],
        ) -> ProviderFuture<'a, ()> {
            self.state
                .lock()
                .unwrap()
                .calls
                .push(MockCall::WatchedFilesChanged {
                    changes: changes.to_vec(),
                });
            self.note_recorded();
            Box::pin(async { Ok(()) })
        }

        fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::LoadFile {
                path: path.to_string(),
                content: content.to_string(),
            });
            let fail = state.fail_file_ops || state.fail_sync_paths.contains(path);
            drop(state);
            self.note_recorded();
            let this = self.clone();
            let path_owned = path.to_string();
            let content_owned = content.to_string();
            let issued_to = self.serving_incarnation();
            Box::pin(async move {
                fail_or_ok(fail, "load_file")?;
                this.accept_applied(&path_owned, &content_owned, issued_to);
                Ok(())
            })
        }

        /// Recorded as its OWN call so a caller's priority lane is observable. The
        /// trait default would collapse it into `open_file` and make the two
        /// indistinguishable.
        fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::OpenFileBackground {
                path: path.to_string(),
                content: content.to_string(),
            });
            let fail = state.fail_file_ops || state.fail_sync_paths.contains(path);
            let block = match &state.open_block {
                Some((armed_path, _, _)) if armed_path.is_empty() || armed_path == path => state
                    .open_block
                    .take()
                    .map(|(_, arrived, release)| (arrived, release)),
                _ => None,
            };
            drop(state);
            self.note_recorded();
            let this = self.clone();
            let path_owned = path.to_string();
            let content_owned = content.to_string();
            let issued_to = self.serving_incarnation();
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                fail_or_ok(fail, "open_file_background")?;
                this.accept_applied(&path_owned, &content_owned, issued_to);
                Ok(())
            })
        }

        fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            let (fail, block) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::UpdateFile {
                    path: path.to_string(),
                    content: content.to_string(),
                });
                state
                    .write_deadlines
                    .push((path.to_string(), verter_type_runtime::deadline::current()));
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
            self.note_recorded();
            let this = self.clone();
            let path_owned = path.to_string();
            let content_owned = content.to_string();
            let issued_to = self.serving_incarnation();
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                fail_or_ok(fail, "update_file")?;
                this.accept_applied(&path_owned, &content_owned, issued_to);
                Ok(())
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
            let this = self.clone();
            let path_owned = path.to_string();
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    // Signal the test that the close has been reached (the closing
                    // task is paused HERE), then await the test's release.
                    arrived.notify_one();
                    release.notified().await;
                }
                fail_or_ok(fail, "close_file")?;
                this.drop_applied(&path_owned);
                Ok(())
            })
        }

        fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::NotifyCarrierChanged {
                companion_path: companion_path.to_string(),
            });
            Box::pin(async { Ok(()) })
        }

        fn notify_carriers_changed<'a>(
            &'a self,
            companion_paths: &'a [String],
        ) -> ProviderFuture<'a, ()> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::NotifyCarriersChanged {
                companion_paths: companion_paths.to_vec(),
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
            let this = self.clone();
            let companion = companion_path.to_string();
            let bytes = content.to_string();
            let issued_to = self.serving_incarnation();
            Box::pin(async move {
                if let Some(gate) = block {
                    gate.notified().await;
                }
                this.accept_applied(&companion, &bytes, issued_to);
                Ok(())
            })
        }

        fn register_carrier_metadata(
            &self,
            source_path: &str,
            companion_path: &str,
            content: &str,
            project_file_name: &str,
        ) -> ProviderFuture<'_, ()> {
            let fail = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::RegisterCarrierMetadata {
                    source_path: source_path.to_string(),
                    companion_path: companion_path.to_string(),
                    content: content.to_string(),
                    project_file_name: project_file_name.to_string(),
                });
                state.fail_carrier_metadata_sources.contains(source_path)
            };
            Box::pin(async move {
                if fail {
                    Err(TypeProviderError::new("mock carrier metadata failure"))
                } else {
                    Ok(())
                }
            })
        }

        fn activate_carrier_member(
            &self,
            source_path: &str,
            companion_path: &str,
            project_file_name: &str,
            script_kind: verter_type_runtime::CarrierScriptKind,
        ) -> ProviderFuture<'_, ()> {
            let block = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::ActivateCarrierMember {
                    source_path: source_path.to_string(),
                    companion_path: companion_path.to_string(),
                    project_file_name: project_file_name.to_string(),
                    script_kind,
                });
                state.carrier_activation_block.take()
            };
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                Ok(())
            })
        }

        fn activate_carrier_members<'a>(
            &'a self,
            members: &'a [verter_type_runtime::CarrierActivation],
        ) -> ProviderFuture<'a, ()> {
            if self.state.lock().unwrap().carrier_refresh_block.is_some() {
                return Box::pin(
                    async move { self.dispatch_carrier_members(members).await?.await },
                );
            }
            let block = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::ActivateCarrierMembers {
                    members: members.to_vec(),
                });
                let observer = state.carrier_batch_observer.clone();
                let block = state.carrier_batch_block.take();
                (observer, block)
            };
            if let Some(observer) = block.0 {
                observer.notify_one();
            }
            let block = block.1;
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                Ok(())
            })
        }

        fn dispatch_carrier_members<'a>(
            &'a self,
            members: &'a [verter_type_runtime::CarrierActivation],
        ) -> ProviderFuture<'a, verter_type_runtime::CarrierActivationSettlement> {
            use verter_type_runtime::CarrierActivationSettlement;
            let refresh = {
                let mut state = self.state.lock().unwrap();
                state.carrier_refresh_block.take().map(|block| {
                    let bytes: Vec<_> = members
                        .iter()
                        .filter_map(|member| {
                            state
                                .applied
                                .remove(&member.companion_path)
                                .map(|bytes| (member.companion_path.clone(), bytes))
                        })
                        .collect();
                    (
                        block,
                        bytes,
                        state.incarnation.load(std::sync::atomic::Ordering::SeqCst),
                        state
                            .diagnostics_gate
                            .clone()
                            .expect("refresh owns the FIFO gate"),
                    )
                })
            };
            let this = self.clone();
            Box::pin(async move {
                let Some(((arrived, release), bytes, incarnation, diagnostics_gate)) = refresh
                else {
                    this.activate_carrier_members(members).await?;
                    return Ok(CarrierActivationSettlement::settled());
                };
                arrived.notify_one();
                Ok(CarrierActivationSettlement::pending(async move {
                    release.notified().await;
                    for (path, bytes) in bytes {
                        this.accept_applied(&path, &bytes, incarnation);
                    }
                    this.state.lock().unwrap().diagnostics_gate = None;
                    diagnostics_gate.add_permits(tokio::sync::Semaphore::MAX_PERMITS);
                    Ok(())
                }))
            })
        }

        fn get_completions(
            &self,
            query: &ProviderQuery,
            offset: u32,
            _trigger_character: Option<&str>,
        ) -> ProviderFuture<'_, CompletionResult> {
            let path = query.path();
            let (items, on_query, block, fail) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetCompletions {
                    path: path.to_string(),
                    offset,
                });
                state.note_evaluation(path);
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
                let fail = if state.fail_next_completions > 0 {
                    state.fail_next_completions -= 1;
                    true
                } else {
                    false
                };
                (items, on_query, block, fail)
            };
            // Run the one-shot mid-request seam AFTER releasing the state lock
            // (a callback that re-enters the mock must not deadlock).
            if let Some(callback) = on_query {
                callback();
            }
            self.barriered(Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                if fail {
                    return Err(TypeProviderError::new(
                        "scripted transient completion failure".to_string(),
                    ));
                }
                Ok(CompletionResult {
                    items,
                    is_incomplete: false,
                })
            }))
        }

        fn get_hover(
            &self,
            query: &ProviderQuery,
            offset: u32,
        ) -> ProviderFuture<'_, Option<HoverInfo>> {
            let path = query.path();
            let (result, on_query, fail, hang) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetHover {
                    path: path.to_string(),
                    offset,
                });
                state.note_evaluation(path);
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
            self.barriered(Box::pin(async move {
                if fail {
                    return Err(TypeProviderError::new(
                        "scripted transient hover failure".to_string(),
                    ));
                }
                Ok(result)
            }))
        }

        fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
            let (result, on_query, hang, gate) = {
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
                (
                    result,
                    on_query,
                    state.hang_diagnostics,
                    state.diagnostics_gate.clone(),
                )
            };
            // Run the one-shot mid-request seam AFTER releasing the state lock
            // (a callback that re-enters the mock must not deadlock).
            if let Some(callback) = on_query {
                callback();
            }
            if hang {
                return Box::pin(std::future::pending());
            }
            self.note_recorded();
            Box::pin(async move {
                if let Some(gate) = gate {
                    gate.acquire()
                        .await
                        .expect("the diagnostics gate is never closed")
                        .forget();
                }
                Ok(result)
            })
        }

        fn get_definition(
            &self,
            query: &ProviderQuery,
            offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let path = query.path();
            let (result, held, on_query, fail, hang) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetDefinition {
                    path: path.to_string(),
                    offset,
                });
                state.note_evaluation(path);
                let fail = if state.fail_next_definitions > 0 {
                    state.fail_next_definitions -= 1;
                    true
                } else {
                    false
                };
                let result = state
                    .definition_responses
                    .iter()
                    .find(|(p, o, _)| p == path && *o == offset)
                    .map(|(_, _, locs)| locs.clone())
                    .unwrap_or_default();
                let held = state.held_targets(result.iter().map(|loc| loc.path.as_str()));
                let result = state
                    .check_targets(query, result.iter().map(|loc| loc.path.as_str()))
                    .map(|()| result);
                let on_query = match &state.on_query {
                    Some((armed_path, _)) if armed_path == path => {
                        state.on_query.take().map(|(_, cb)| cb)
                    }
                    _ => None,
                };
                (result, held, on_query, fail, state.hang_definition)
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
            self.barriered_targets(
                held,
                Box::pin(async move {
                    if fail {
                        return Err(TypeProviderError::new(
                            "scripted transient definition failure".to_string(),
                        ));
                    }
                    result
                }),
            )
        }

        fn get_type_definition(
            &self,
            query: &ProviderQuery,
            offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let path = query.path();
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetTypeDefinition {
                path: path.to_string(),
                offset,
            });
            state.note_evaluation(path);
            let fail = if state.fail_next_type_definitions > 0 {
                state.fail_next_type_definitions -= 1;
                true
            } else {
                false
            };
            let result = state
                .type_definition_responses
                .iter()
                .find(|(p, o, _)| p == path && *o == offset)
                .map(|(_, _, locs)| locs.clone())
                .unwrap_or_default();
            let held = state.held_targets(result.iter().map(|loc| loc.path.as_str()));
            let result = state
                .check_targets(query, result.iter().map(|loc| loc.path.as_str()))
                .map(|()| result);
            drop(state);
            self.barriered_targets(
                held,
                Box::pin(async move {
                    if fail {
                        return Err(TypeProviderError::new(
                            "scripted transient type-definition failure".to_string(),
                        ));
                    }
                    result
                }),
            )
        }

        fn get_references(
            &self,
            query: &ProviderQuery,
            offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            let path = query.path();
            let (result, held, on_query) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetReferences {
                    path: path.to_string(),
                    offset,
                });
                state.note_evaluation(path);
                let result = state
                    .reference_responses
                    .iter()
                    .find(|(p, o, _)| p == path && *o == offset)
                    .map(|(_, _, locs)| locs.clone())
                    .unwrap_or_default();
                let held = state.held_targets(result.iter().map(|loc| loc.path.as_str()));
                let result = state
                    .check_targets(query, result.iter().map(|loc| loc.path.as_str()))
                    .map(|()| result);
                let on_query = match &state.on_query {
                    Some((armed_path, _)) if armed_path == path => {
                        state.on_query.take().map(|(_, cb)| cb)
                    }
                    _ => None,
                };
                (result, held, on_query)
            };
            // Run the one-shot mid-request seam AFTER releasing the state lock
            // (a callback that re-enters the mock must not deadlock).
            if let Some(callback) = on_query {
                callback();
            }
            self.barriered_targets(held, Box::pin(async move { result }))
        }

        fn get_rename_locations(
            &self,
            query: &ProviderQuery,
            offset: u32,
        ) -> ProviderFuture<'_, Vec<RenameLocation>> {
            let path = query.path();
            let (result, held, block) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetRenameLocations {
                    path: path.to_string(),
                    offset,
                });
                state.note_evaluation(path);
                let result = state
                    .rename_responses
                    .iter()
                    .find(|(p, o, _)| p == path && *o == offset)
                    .map(|(_, _, locs)| locs.clone())
                    .unwrap_or_default();
                let held = state.held_targets(result.iter().map(|loc| loc.path.as_str()));
                let result = state
                    .check_targets(query, result.iter().map(|loc| loc.path.as_str()))
                    .map(|()| result);
                let block = match &state.rename_block {
                    Some((armed_path, _, _)) if armed_path == path => state
                        .rename_block
                        .take()
                        .map(|(_, arrived, release)| (arrived, release)),
                    _ => None,
                };
                (result, held, block)
            };
            self.barriered_targets(
                held,
                Box::pin(async move {
                    if let Some((arrived, release)) = block {
                        arrived.notify_one();
                        release.notified().await;
                    }
                    result
                }),
            )
        }

        fn get_signature_help(
            &self,
            query: &ProviderQuery,
            offset: u32,
        ) -> ProviderFuture<'_, Option<SignatureHelp>> {
            let path = query.path();
            let (result, on_query, hang) = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::GetSignatureHelp {
                    path: path.to_string(),
                    offset,
                });
                state.note_evaluation(path);
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
            self.barriered(Box::pin(async move { Ok(result) }))
        }

        fn get_code_actions(
            &self,
            query: &ProviderQuery,
            start_offset: u32,
            end_offset: u32,
            diagnostics: &[ProviderDiagnosticContext],
        ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
            let path = query.path();
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetCodeActions {
                path: path.to_string(),
                start_offset,
                end_offset,
                diagnostics: diagnostics.to_vec(),
            });
            state.note_evaluation(path);
            let result = state
                .code_action_responses
                .iter()
                .find(|(p, so, eo, _)| p == path && *so == start_offset && *eo == end_offset)
                .map(|(_, _, _, actions)| actions.clone())
                .unwrap_or_default();
            let edit_targets = || {
                result
                    .iter()
                    .flat_map(|action| action.edits.iter().map(|edit| edit.path.as_str()))
            };
            let held = state.held_targets(edit_targets());
            let checked = state.check_targets(query, edit_targets());
            let result = checked.map(|()| result);
            drop(state);
            self.barriered_targets(held, Box::pin(async move { result }))
        }

        fn get_semantic_tokens(
            &self,
            query: &ProviderQuery,
        ) -> ProviderFuture<'_, Vec<SemanticToken>> {
            let path = query.path();
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetSemanticTokens {
                path: path.to_string(),
            });
            state.note_evaluation(path);
            let result = state
                .semantic_token_responses
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, tokens)| tokens.clone())
                .unwrap_or_default();
            self.barriered(Box::pin(async move { Ok(result) }))
        }

        fn get_document_highlights(
            &self,
            query: &ProviderQuery,
            offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
            let path = query.path();
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetDocumentHighlights {
                path: path.to_string(),
                offset,
            });
            state.note_evaluation(path);
            let result = state
                .highlight_responses
                .iter()
                .find(|(p, o, _)| p == path && *o == offset)
                .map(|(_, _, hl)| hl.clone())
                .unwrap_or_default();
            self.barriered(Box::pin(async move { Ok(result) }))
        }

        fn get_inlay_hints(
            &self,
            query: &ProviderQuery,
            start_offset: u32,
            end_offset: u32,
        ) -> ProviderFuture<'_, Vec<InlayHint>> {
            let path = query.path();
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetInlayHints {
                path: path.to_string(),
                start_offset,
                end_offset,
            });
            state.note_evaluation(path);
            let result = state
                .inlay_hint_responses
                .iter()
                .find(|(p, so, eo, _)| p == path && *so == start_offset && *eo == end_offset)
                .map(|(_, _, _, hints)| hints.clone())
                .unwrap_or_default();
            self.barriered(Box::pin(async move { Ok(result) }))
        }

        fn resolve_completion(
            &self,
            query: &ProviderQuery,
            data: CompletionResolveData,
        ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
            let path = query.path();
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::ResolveCompletion {
                path: path.to_string(),
                data: data.clone(),
            });
            state.note_evaluation(path);
            let result = state
                .resolve_completion_responses
                .iter()
                .find(|(p, candidate, _)| p == path && *candidate == data)
                .and_then(|(_, _, resolved)| resolved.clone());
            self.barriered(Box::pin(async move { Ok(result) }))
        }

        fn configure_paths(
            &self,
            base_url: &str,
            paths: serde_json::Value,
        ) -> ProviderFuture<'_, ()> {
            let block = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(MockCall::ConfigurePaths {
                    base_url: base_url.to_string(),
                    paths,
                });
                state.configure_paths_block.take()
            };
            Box::pin(async move {
                if let Some((arrived, release)) = block {
                    arrived.notify_one();
                    release.notified().await;
                }
                Ok(())
            })
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

        fn set_project_ownership(
            &self,
            authority: std::sync::Arc<dyn verter_type_runtime::traits::ConfiguredOwnerAuthority>,
        ) {
            // Test-only divergence from production: this stores the assigned
            // authority only for `configured_owner` assertions. Mock opens,
            // updates, and queries never consult it, and the inherited
            // `resync_open_files` is a no-op, so this provider cannot model
            // per-file project rebinding after an ownership change.
            self.state.lock().unwrap().project_ownership = Some(authority);
        }
    }
}
