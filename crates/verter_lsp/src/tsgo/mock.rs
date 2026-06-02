//! Mock implementation of `TypeProvider` for testing.
//!
//! Allows tests to configure expected responses for each method.
//! Tracks all calls for assertion purposes.

#[cfg(test)]
pub use inner::*;

#[cfg(test)]
mod inner {
    use std::sync::{Arc, Mutex};

    use crate::tsgo::protocol::*;
    use crate::tsgo::traits::{ProviderFuture, TypeProvider};

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
            data: serde_json::Value,
        },
        ConfigurePaths {
            base_url: String,
            paths: serde_json::Value,
        },
        UpdateWorkspaceFolders {
            added: Vec<serde_json::Value>,
            removed: Vec<serde_json::Value>,
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
        resolve_completion_responses:
            Vec<(String, serde_json::Value, Option<CompletionResolveResult>)>,
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

        /// Configure completion resolution for a specific path and data payload.
        pub fn set_resolve_completion(
            &self,
            path: &str,
            data: serde_json::Value,
            result: Option<CompletionResolveResult>,
        ) {
            let mut state = self.state.lock().unwrap();
            state
                .resolve_completion_responses
                .push((path.to_string(), data, result));
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
            _data: serde_json::Value,
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
        fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::OpenFile {
                path: path.to_string(),
                content: content.to_string(),
            });
            let fail = state.fail_file_ops || state.fail_sync_paths.contains(path);
            Box::pin(async move { fail_or_ok(fail, "open_file") })
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
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::UpdateFile {
                path: path.to_string(),
                content: content.to_string(),
            });
            let fail = state.fail_file_ops || state.fail_sync_paths.contains(path);
            Box::pin(async move { fail_or_ok(fail, "update_file") })
        }

        fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::CloseFile {
                path: path.to_string(),
            });
            // `close_file` is intentionally NOT gated by `fail_sync_paths`:
            // failure-injection tests want to observe whether a stale path was
            // (wrongly) closed even while a sibling kind's sync fails.
            let fail = state.fail_file_ops;
            Box::pin(async move { fail_or_ok(fail, "close_file") })
        }

        fn get_completions(
            &self,
            path: &str,
            offset: u32,
            _trigger_character: Option<&str>,
        ) -> ProviderFuture<'_, CompletionResult> {
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
            Box::pin(async move {
                Ok(CompletionResult {
                    items,
                    is_incomplete: false,
                })
            })
        }

        fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetHover {
                path: path.to_string(),
                offset,
            });
            let result = state
                .hover_responses
                .iter()
                .find(|(p, o, _)| p == path && *o == offset)
                .and_then(|(_, _, info)| info.clone());
            Box::pin(async move { Ok(result) })
        }

        fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
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
            Box::pin(async move { Ok(result) })
        }

        fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
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
            Box::pin(async move { Ok(result) })
        }

        fn get_code_actions(
            &self,
            path: &str,
            start_offset: u32,
            end_offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(MockCall::GetCodeActions {
                path: path.to_string(),
                start_offset,
                end_offset,
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
            data: serde_json::Value,
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
