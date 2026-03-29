//! Adapter: wraps a `TypeProvider` as a `GeneratedQueryBackend`.
//!
//! This bridges the legacy hover-based `TypeProvider` interface to the
//! new `GeneratedQueryBackend` contract. Both tsserver and TSGO providers
//! can be used through this adapter for type expansion queries.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::backend::*;
use crate::protocol::Completion;
use crate::traits::TypeProvider;
use crate::tsgo::ipc::rewrite_vue_imports_for_tsgo;

/// Adapts a `TypeProvider` to implement `GeneratedQueryBackend`.
///
/// Internally tracks synced file revisions and converts `query_type_data`
/// into hover queries on the underlying provider.
pub struct TypeProviderAdapter {
    backend_label: String,
    provider: Arc<dyn TypeProvider>,
    synced_revisions: Mutex<HashMap<String, SyncedFileState>>,
    loaded_definition_files: Mutex<HashSet<String>>,
}
impl TypeProviderAdapter {
    pub fn new(provider: Arc<dyn TypeProvider>, backend_label: impl Into<String>) -> Self {
        Self {
            backend_label: backend_label.into(),
            provider,
            synced_revisions: Mutex::new(HashMap::new()),
            loaded_definition_files: Mutex::new(HashSet::new()),
        }
    }

    fn content_hash(content: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }

    fn virtual_path(file_id: &GeneratedFileId) -> String {
        let suffix = match file_id.profile {
            ArtifactProfile::ComponentMeta => ".meta.ts",
            ArtifactProfile::Lsp => ".tsx",
        };
        format!("{}{}", file_id.canonical_id, suffix)
    }

    async fn query_members_at_offset(
        &self,
        path: &str,
        generated_offset: u32,
    ) -> Result<BackendTypeData, BackendError> {
        let completions = self
            .provider
            .get_completions(path, generated_offset, Some("."))
            .await
            .map_err(|e| BackendError::BackendReported(e.message))?;
        let detailed = self
            .provider
            .get_completion_details(path, generated_offset, &completions.items)
            .await
            .map_err(|e| BackendError::BackendReported(e.message))?;
        let members = completion_items_to_backend_members(if detailed.is_empty() {
            &completions.items
        } else {
            &detailed
        });
        crate::type_runtime_trace_event!(
            "runtime_query_type_data_members",
            format!(
                "backend={} path={} has_members={} member_count={} preview={}",
                self.backend_label,
                path,
                !members.is_empty(),
                members.len(),
                trace_member_preview(&members, 6),
            ),
        );
        Ok(BackendTypeData {
            type_text: None,
            members,
            documentation: None,
            completeness: if completions.is_incomplete {
                BackendTypeCompleteness::Partial
            } else {
                BackendTypeCompleteness::Exact
            },
        })
    }

    async fn query_definition_type_at_offset(
        &self,
        path: &str,
        generated_offset: u32,
    ) -> Result<BackendTypeData, BackendError> {
        let definitions = self
            .provider
            .get_definition(path, generated_offset)
            .await
            .map_err(|e| BackendError::BackendReported(e.message))?;
        crate::type_runtime_trace_event!(
            "runtime_query_type_data_definition_locations",
            format!(
                "backend={} path={} offset={} definition_count={}",
                self.backend_label,
                path,
                generated_offset,
                definitions.len(),
            ),
        );

        for location in definitions {
            self.ensure_definition_file_loaded(path, &location.path)
                .await?;
            let hover = self
                .provider
                .get_hover(&location.path, location.start)
                .await
                .map_err(|e| BackendError::BackendReported(e.message))?;
            if let Some(hover) = hover.filter(|item| !item.contents.trim().is_empty()) {
                crate::type_runtime_trace_event!(
                    "runtime_query_type_data_definition_hover",
                    format!(
                        "backend={} source_path={} definition_path={} definition_offset={} text_len={} preview={}",
                        self.backend_label,
                        path,
                        location.path,
                        location.start,
                        hover.contents.len(),
                        trace_preview(&hover.contents, 120),
                    ),
                );
                return Ok(BackendTypeData {
                    type_text: Some(hover.contents),
                    members: vec![],
                    documentation: None,
                    completeness: BackendTypeCompleteness::Exact,
                });
            }
        }

        Ok(BackendTypeData {
            type_text: None,
            members: vec![],
            documentation: None,
            completeness: BackendTypeCompleteness::Failed,
        })
    }

    async fn ensure_definition_file_loaded(
        &self,
        query_path: &str,
        definition_path: &str,
    ) -> Result<(), BackendError> {
        if definition_path == query_path {
            return Ok(());
        }

        {
            let loaded = self.loaded_definition_files.lock().await;
            if loaded.contains(definition_path) {
                return Ok(());
            }
        }

        let content = match std::fs::read_to_string(definition_path) {
            Ok(content) => content,
            Err(error) => {
                crate::type_runtime_trace_event!(
                    "runtime_query_type_data_definition_load",
                    format!(
                        "backend={} path={} loaded=false error={error}",
                        self.backend_label, definition_path,
                    ),
                );
                return Err(BackendError::BackendReported(format!(
                    "failed to read definition file '{definition_path}': {error}"
                )));
            }
        };

        self.provider
            .open_file(definition_path, &content)
            .await
            .map_err(|e| BackendError::BackendReported(e.message))?;
        self.loaded_definition_files
            .lock()
            .await
            .insert(definition_path.to_string());
        crate::type_runtime_trace_event!(
            "runtime_query_type_data_definition_load",
            format!(
                "backend={} path={} loaded=true content_len={}",
                self.backend_label,
                definition_path,
                content.len(),
            ),
        );
        Ok(())
    }
}

impl GeneratedQueryBackend for TypeProviderAdapter {
    fn sync_file<'a>(
        &'a self,
        file_id: &'a GeneratedFileId,
        revision: u64,
        content: &'a str,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            let path = Self::virtual_path(file_id);
            let content_hash = Self::content_hash(content);
            let offset_translation =
                OffsetTranslation::for_backend_content(&self.backend_label, &path, content);
            let _trace = crate::type_runtime_trace_scope!(
                "runtime_sync_file",
                format!(
                    "backend={} path={} profile={:?} runtime_key={} revision={} content_len={} content_hash={:016x}",
                    self.backend_label,
                    path,
                    file_id.profile,
                    file_id.runtime_key,
                    revision,
                    content.len(),
                    content_hash,
                ),
            );
            let existing_state = {
                let revisions = self.synced_revisions.lock().await;
                if revisions.get(&path)
                    == Some(&SyncedFileState {
                        revision,
                        content_hash,
                        offset_translation: offset_translation.clone(),
                    })
                {
                    crate::type_runtime_trace_event!(
                        "runtime_sync_file_result",
                        format!(
                            "backend={} path={} cache_hit=true provider_op=skip revision={} content_hash={:016x}",
                            self.backend_label, path, revision, content_hash
                        ),
                    );
                    return Ok(());
                }
                revisions.get(&path).cloned()
            };

            let provider_op = if existing_state.is_some() {
                "update"
            } else {
                "open"
            };
            if existing_state.is_some() {
                self.provider
                    .update_file(&path, content)
                    .await
                    .map_err(|e| BackendError::BackendReported(e.message))?;
            } else {
                self.provider
                    .open_file(&path, content)
                    .await
                    .map_err(|e| BackendError::BackendReported(e.message))?;
            }

            self.synced_revisions.lock().await.insert(
                path.clone(),
                SyncedFileState {
                    revision,
                    content_hash,
                    offset_translation,
                },
            );
            crate::type_runtime_trace_event!(
                "runtime_sync_file_result",
                format!(
                    "backend={} path={} cache_hit=false provider_op={} previous_state={:?} next_revision={} next_content_hash={:016x}",
                    self.backend_label, path, provider_op, existing_state, revision, content_hash
                ),
            );
            Ok(())
        })
    }

    fn close_file<'a>(&'a self, file_id: &'a GeneratedFileId) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            let path = Self::virtual_path(file_id);
            let _trace = crate::type_runtime_trace_scope!(
                "runtime_close_file",
                format!(
                    "backend={} path={} profile={:?} runtime_key={}",
                    self.backend_label, path, file_id.profile, file_id.runtime_key
                ),
            );
            self.provider
                .close_file(&path)
                .await
                .map_err(|e| BackendError::BackendReported(e.message))?;
            self.synced_revisions.lock().await.remove(&path);
            crate::type_runtime_trace_event!(
                "runtime_close_file_result",
                format!("backend={} path={}", self.backend_label, path),
            );
            Ok(())
        })
    }

    fn evict_file<'a>(&'a self, file_id: &'a GeneratedFileId) -> BackendFuture<'a, ()> {
        self.close_file(file_id)
    }

    fn query_type_data<'a>(
        &'a self,
        file_id: &'a GeneratedFileId,
        expected_revision: u64,
        generated_offset: u32,
        query: BackendTypeQuery,
    ) -> BackendFuture<'a, BackendTypeData> {
        Box::pin(async move {
            let path = Self::virtual_path(file_id);
            let _trace = crate::type_runtime_trace_scope!(
                "runtime_query_type_data",
                format!(
                    "backend={} path={} profile={:?} runtime_key={} revision={} query={:?} offset={}",
                    self.backend_label,
                    path,
                    file_id.profile,
                    file_id.runtime_key,
                    expected_revision,
                    query,
                    generated_offset,
                ),
            );

            {
                let revisions = self.synced_revisions.lock().await;
                match revisions.get(&path) {
                    Some(state) if state.revision != expected_revision => {
                        crate::type_runtime_trace_event!(
                            "runtime_query_type_data_stale",
                            format!(
                                "backend={} path={} expected_revision={} synced_revision={} synced_content_hash={:016x}",
                                self.backend_label, path, expected_revision, state.revision, state.content_hash
                            ),
                        );
                        return Err(BackendError::ProtocolViolation(format!(
                            "stale query: expected revision {expected_revision}, synced {}",
                            state.revision
                        )));
                    }
                    None => {
                        crate::type_runtime_trace_event!(
                            "runtime_query_type_data_unsynced",
                            format!(
                                "backend={} path={} expected_revision={}",
                                self.backend_label, path, expected_revision
                            ),
                        );
                        return Err(BackendError::ProtocolViolation(
                            "file not synced".to_string(),
                        ));
                    }
                    _ => {}
                }
            }

            let translated_offset = {
                let revisions = self.synced_revisions.lock().await;
                revisions
                    .get(&path)
                    .map(|state| state.translate_offset(generated_offset))
                    .unwrap_or(generated_offset)
            };
            if translated_offset != generated_offset {
                crate::type_runtime_trace_event!(
                    "runtime_query_type_data_offset_translation",
                    format!(
                        "backend={} path={} original_offset={} translated_offset={}",
                        self.backend_label, path, generated_offset, translated_offset
                    ),
                );
            }

            match query {
                BackendTypeQuery::TypeAtOffset => {
                    let hover = self
                        .provider
                        .get_hover(&path, translated_offset)
                        .await
                        .map_err(|e| BackendError::BackendReported(e.message))?;

                    match hover {
                        Some(info) => {
                            crate::type_runtime_trace_event!(
                                "runtime_query_type_data_hover",
                                format!(
                                    "backend={} path={} has_hover=true text_len={} preview={}",
                                    self.backend_label,
                                    path,
                                    info.contents.len(),
                                    trace_preview(&info.contents, 120),
                                ),
                            );
                            Ok(BackendTypeData {
                                type_text: Some(info.contents),
                                members: vec![],
                                documentation: None,
                                completeness: BackendTypeCompleteness::Exact,
                            })
                        }
                        None => {
                            crate::type_runtime_trace_event!(
                                "runtime_query_type_data_hover",
                                format!(
                                    "backend={} path={} has_hover=false",
                                    self.backend_label, path
                                ),
                            );
                            Ok(BackendTypeData::default())
                        }
                    }
                }
                BackendTypeQuery::DefinitionTypeAtOffset => {
                    self.query_definition_type_at_offset(&path, translated_offset)
                        .await
                }
                BackendTypeQuery::MembersAtOffset => {
                    self.query_members_at_offset(&path, translated_offset).await
                }
                BackendTypeQuery::DocumentationAtOffset => {
                    let hover = self
                        .provider
                        .get_hover(&path, translated_offset)
                        .await
                        .map_err(|e| BackendError::BackendReported(e.message))?;

                    crate::type_runtime_trace_event!(
                        "runtime_query_type_data_hover",
                        format!(
                            "backend={} path={} has_hover={} documentation_len={}",
                            self.backend_label,
                            path,
                            hover.is_some(),
                            hover.as_ref().map(|item| item.contents.len()).unwrap_or(0),
                        ),
                    );

                    Ok(BackendTypeData {
                        type_text: None,
                        members: vec![],
                        documentation: hover.map(|h| h.contents),
                        completeness: BackendTypeCompleteness::Exact,
                    })
                }
            }
        })
    }

    fn shutdown(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let _trace = crate::type_runtime_trace_scope!(
                "runtime_backend_shutdown",
                format!("backend={}", self.backend_label),
            );
            self.provider
                .shutdown()
                .await
                .map_err(|e| BackendError::BackendReported(e.message))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncedFileState {
    revision: u64,
    content_hash: u64,
    offset_translation: Option<OffsetTranslation>,
}

impl SyncedFileState {
    fn translate_offset(&self, original_offset: u32) -> u32 {
        self.offset_translation
            .as_ref()
            .map(|translation| translation.to_translated_offset(original_offset))
            .unwrap_or(original_offset)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OffsetTranslation {
    insertions: Vec<(u32, u32)>,
}

impl OffsetTranslation {
    fn for_backend_content(backend_label: &str, path: &str, original: &str) -> Option<Self> {
        if backend_label != "tsgo" {
            return None;
        }
        let rewritten = rewrite_vue_imports_for_tsgo(original, path);
        if rewritten == original {
            return None;
        }
        Self::from_insertions_only(original, &rewritten)
    }

    fn from_insertions_only(original: &str, rewritten: &str) -> Option<Self> {
        let original_bytes = original.as_bytes();
        let rewritten_bytes = rewritten.as_bytes();
        let mut original_index = 0usize;
        let mut rewritten_index = 0usize;
        let mut insertions = Vec::new();
        let mut cumulative_delta = 0u32;

        while original_index < original_bytes.len() && rewritten_index < rewritten_bytes.len() {
            if original_bytes[original_index] == rewritten_bytes[rewritten_index] {
                original_index += 1;
                rewritten_index += 1;
                continue;
            }

            let insertion_start = rewritten_index;
            while rewritten_index < rewritten_bytes.len()
                && rewritten_bytes[rewritten_index] != original_bytes[original_index]
            {
                rewritten_index += 1;
            }
            let inserted = rewritten_index.saturating_sub(insertion_start);
            if inserted == 0 {
                return None;
            }
            cumulative_delta += inserted as u32;
            insertions.push((original_index as u32, cumulative_delta));
        }

        if original_index < original_bytes.len() {
            return None;
        }
        if rewritten_index < rewritten_bytes.len() {
            cumulative_delta += (rewritten_bytes.len() - rewritten_index) as u32;
            insertions.push((original_bytes.len() as u32, cumulative_delta));
        }

        if insertions.is_empty() {
            None
        } else {
            Some(Self { insertions })
        }
    }

    fn to_translated_offset(&self, original_offset: u32) -> u32 {
        let mut translated = original_offset;
        for (insertion_at, cumulative_delta) in &self.insertions {
            if *insertion_at > original_offset {
                break;
            }
            translated = original_offset + *cumulative_delta;
        }
        translated
    }
}

fn trace_preview(contents: &str, max_len: usize) -> String {
    let mut preview = String::new();
    for ch in contents.chars().take(max_len) {
        match ch {
            '\n' => preview.push_str("\\n"),
            '\r' => preview.push_str("\\r"),
            '\t' => preview.push_str("\\t"),
            _ => preview.push(ch),
        }
    }
    if contents.chars().count() > max_len {
        preview.push_str("...");
    }
    preview
}

fn trace_member_preview(members: &[BackendTypeMember], max_items: usize) -> String {
    members
        .iter()
        .take(max_items)
        .map(|member| member.name.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

fn completion_items_to_backend_members(items: &[Completion]) -> Vec<BackendTypeMember> {
    let mut seen = HashSet::new();
    let mut members = Vec::with_capacity(items.len());
    for item in items {
        let (normalized_name, optional_from_label) = normalize_completion_member_name(&item.label);
        if normalized_name.is_empty() || !seen.insert(normalized_name.clone()) {
            continue;
        }
        members.push(BackendTypeMember {
            name: normalized_name,
            type_text: item
                .detail
                .clone()
                .filter(|detail| !detail.trim().is_empty()),
            optional: optional_from_label || completion_detail_marks_optional(item),
            documentation: item.documentation.clone(),
        });
    }
    members
}

fn normalize_completion_member_name(label: &str) -> (String, bool) {
    let normalized = label.trim();
    if let Some(stripped) = normalized.strip_suffix('?') {
        return (stripped.trim().to_string(), true);
    }
    (normalized.to_string(), false)
}

fn completion_detail_marks_optional(item: &Completion) -> bool {
    let Some(detail) = item.detail.as_deref() else {
        return false;
    };
    let needle = format!("{}?", item.label);
    detail.contains(&needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::*;
    use crate::traits::{ProviderFuture, TypeProvider};
    use std::sync::Mutex as StdMutex;

    struct MockTypeProvider {
        hover: Option<HoverInfo>,
        hover_at: StdMutex<HashMap<(String, u32), Option<HoverInfo>>>,
        completions: CompletionResult,
        completion_details: Vec<Completion>,
        definitions: Vec<TypeLocation>,
        completion_calls: StdMutex<Vec<(String, u32)>>,
        detail_calls: StdMutex<Vec<(String, u32, usize)>>,
        definition_calls: StdMutex<Vec<(String, u32)>>,
        load_calls: StdMutex<Vec<(String, String)>>,
        hover_calls: StdMutex<Vec<(String, u32)>>,
        open_calls: StdMutex<Vec<(String, String)>>,
        update_calls: StdMutex<Vec<(String, String)>>,
    }

    impl Default for MockTypeProvider {
        fn default() -> Self {
            Self {
                hover: None,
                hover_at: StdMutex::new(HashMap::new()),
                completions: CompletionResult {
                    items: Vec::new(),
                    is_incomplete: false,
                },
                completion_details: Vec::new(),
                definitions: Vec::new(),
                completion_calls: StdMutex::new(Vec::new()),
                detail_calls: StdMutex::new(Vec::new()),
                definition_calls: StdMutex::new(Vec::new()),
                load_calls: StdMutex::new(Vec::new()),
                hover_calls: StdMutex::new(Vec::new()),
                open_calls: StdMutex::new(Vec::new()),
                update_calls: StdMutex::new(Vec::new()),
            }
        }
    }

    impl TypeProvider for MockTypeProvider {
        fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            self.open_calls
                .lock()
                .unwrap()
                .push((path.to_string(), content.to_string()));
            Box::pin(async { Ok(()) })
        }

        fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            self.update_calls
                .lock()
                .unwrap()
                .push((path.to_string(), content.to_string()));
            Box::pin(async { Ok(()) })
        }

        fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
            self.load_calls
                .lock()
                .unwrap()
                .push((path.to_string(), content.to_string()));
            Box::pin(async { Ok(()) })
        }

        fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }

        fn get_completions(
            &self,
            path: &str,
            offset: u32,
            _trigger_character: Option<&str>,
        ) -> ProviderFuture<'_, CompletionResult> {
            self.completion_calls
                .lock()
                .unwrap()
                .push((path.to_string(), offset));
            let result = self.completions.clone();
            Box::pin(async move { Ok(result) })
        }

        fn get_completion_details<'a>(
            &'a self,
            path: &'a str,
            offset: u32,
            items: &'a [Completion],
        ) -> ProviderFuture<'a, Vec<Completion>> {
            self.detail_calls
                .lock()
                .unwrap()
                .push((path.to_string(), offset, items.len()));
            let result = self.completion_details.clone();
            Box::pin(async move { Ok(result) })
        }

        fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
            self.hover_calls
                .lock()
                .unwrap()
                .push((path.to_string(), offset));
            let hover = self
                .hover_at
                .lock()
                .unwrap()
                .get(&(path.to_string(), offset))
                .cloned()
                .unwrap_or_else(|| self.hover.clone());
            Box::pin(async move { Ok(hover) })
        }

        fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
            self.definition_calls
                .lock()
                .unwrap()
                .push((path.to_string(), offset));
            let definitions = self.definitions.clone();
            Box::pin(async move { Ok(definitions) })
        }

        fn get_type_definition(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_references(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
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
    }

    fn test_file_id() -> GeneratedFileId {
        GeneratedFileId {
            canonical_id: "/src/Foo.vue".into(),
            profile: ArtifactProfile::ComponentMeta,
            runtime_key: "test".into(),
        }
    }

    #[test]
    fn virtual_path_component_meta() {
        assert_eq!(
            TypeProviderAdapter::virtual_path(&test_file_id()),
            "/src/Foo.vue.meta.ts"
        );
    }

    #[test]
    fn virtual_path_lsp() {
        let id = GeneratedFileId {
            canonical_id: "/src/Foo.vue".into(),
            profile: ArtifactProfile::Lsp,
            runtime_key: "test".into(),
        };
        assert_eq!(TypeProviderAdapter::virtual_path(&id), "/src/Foo.vue.tsx");
    }

    #[tokio::test]
    async fn members_query_uses_completion_details() {
        let provider = Arc::new(MockTypeProvider {
            completions: CompletionResult {
                items: vec![
                    Completion {
                        label: "collapsible".into(),
                        kind: Some(CompletionKind::Field),
                        detail: None,
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        insert_text: None,
                        sort_text: None,
                        data: None,
                    },
                    Completion {
                        label: "items".into(),
                        kind: Some(CompletionKind::Field),
                        detail: Some("AccordionProps<T>.items?: T[] | undefined".into()),
                        documentation: Some("List of items.".into()),
                        edit_range_start: None,
                        edit_range_end: None,
                        insert_text: None,
                        sort_text: None,
                        data: None,
                    },
                ],
                is_incomplete: false,
            },
            completion_details: vec![
                Completion {
                    label: "collapsible".into(),
                    kind: Some(CompletionKind::Field),
                    detail: Some("(property) collapsible?: boolean | undefined".into()),
                    documentation: Some("Allows closing an open item.".into()),
                    edit_range_start: None,
                    edit_range_end: None,
                    insert_text: None,
                    sort_text: None,
                    data: None,
                },
                Completion {
                    label: "items".into(),
                    kind: Some(CompletionKind::Field),
                    detail: Some("AccordionProps<T>.items?: T[] | undefined".into()),
                    documentation: Some("List of items.".into()),
                    edit_range_start: None,
                    edit_range_end: None,
                    insert_text: None,
                    sort_text: None,
                    data: None,
                },
            ],
            ..Default::default()
        });
        let adapter = TypeProviderAdapter::new(provider.clone(), "tsserver");
        let file_id = test_file_id();
        adapter
            .sync_file(&file_id, 1, "type Foo = {}")
            .await
            .expect("sync should succeed");

        let data = adapter
            .query_type_data(&file_id, 1, 42, BackendTypeQuery::MembersAtOffset)
            .await
            .expect("member query should succeed");

        assert!(data.type_text.is_none());
        assert_eq!(data.members.len(), 2);
        assert_eq!(data.members[0].name, "collapsible");
        assert_eq!(
            data.members[0].type_text.as_deref(),
            Some("(property) collapsible?: boolean | undefined")
        );
        assert!(data.members[0].optional);
        assert_eq!(
            data.members[0].documentation.as_deref(),
            Some("Allows closing an open item.")
        );
        assert_eq!(data.members[1].name, "items");
        assert!(data.members[1].optional);
        assert_eq!(
            provider.detail_calls.lock().unwrap().as_slice(),
            &[("/src/Foo.vue.meta.ts".to_string(), 42, 2)]
        );
    }

    #[tokio::test]
    async fn members_query_preserves_partial_completion_state() {
        let provider = Arc::new(MockTypeProvider {
            completions: CompletionResult {
                items: vec![Completion {
                    label: "labelKey".into(),
                    kind: Some(CompletionKind::Field),
                    detail: Some("AccordionProps<T>.labelKey?: any".into()),
                    documentation: Some("The key used to get the label from the item.".into()),
                    edit_range_start: None,
                    edit_range_end: None,
                    insert_text: None,
                    sort_text: None,
                    data: None,
                }],
                is_incomplete: true,
            },
            ..Default::default()
        });
        let adapter = TypeProviderAdapter::new(provider, "tsgo");
        let file_id = test_file_id();
        adapter
            .sync_file(&file_id, 7, "type Foo = {}")
            .await
            .expect("sync should succeed");

        let data = adapter
            .query_type_data(&file_id, 7, 11, BackendTypeQuery::MembersAtOffset)
            .await
            .expect("member query should succeed");

        assert_eq!(data.completeness, BackendTypeCompleteness::Partial);
        assert_eq!(data.members.len(), 1);
        assert_eq!(data.members[0].name, "labelKey");
        assert!(data.members[0].optional);
    }

    #[tokio::test]
    async fn sync_file_updates_when_same_revision_has_new_content() {
        let provider = Arc::new(MockTypeProvider::default());
        let adapter = TypeProviderAdapter::new(provider.clone(), "tsserver");
        let file_id = test_file_id();

        adapter
            .sync_file(&file_id, 1, "type Query = { a: string }\nquery.")
            .await
            .expect("initial sync should succeed");
        adapter
            .sync_file(&file_id, 1, "type Query = { b: number }\nquery.")
            .await
            .expect("content change at same revision should update");

        let open_calls = provider.open_calls.lock().unwrap();
        let update_calls = provider.update_calls.lock().unwrap();
        assert_eq!(open_calls.len(), 1);
        assert_eq!(update_calls.len(), 1);
        assert!(
            update_calls[0].1.contains("b: number"),
            "updated content should be pushed when revision is unchanged"
        );
    }

    #[tokio::test]
    async fn tsgo_member_queries_translate_offsets_after_vue_import_rewrite() {
        let provider = Arc::new(MockTypeProvider {
            completions: CompletionResult {
                items: vec![Completion {
                    label: "collapsible".into(),
                    kind: Some(CompletionKind::Property),
                    detail: Some("(property) collapsible?: boolean | undefined".into()),
                    documentation: None,
                    edit_range_start: None,
                    edit_range_end: None,
                    insert_text: None,
                    sort_text: None,
                    data: None,
                }],
                is_incomplete: false,
            },
            ..Default::default()
        });
        let adapter = TypeProviderAdapter::new(provider.clone(), "tsgo");
        let file_id = test_file_id();
        let content = "import Icon from './Icon.vue'\n\ntype Query = { collapsible?: boolean }\ndeclare const query: Query;\nquery.";
        let original_offset = content
            .find("query.")
            .map(|offset| offset as u32 + "query.".len() as u32)
            .expect("probe should exist");

        adapter
            .sync_file(&file_id, 1, content)
            .await
            .expect("sync should succeed");
        adapter
            .query_type_data(
                &file_id,
                1,
                original_offset,
                BackendTypeQuery::MembersAtOffset,
            )
            .await
            .expect("member query should succeed");

        assert_eq!(
            provider.completion_calls.lock().unwrap().as_slice(),
            &[("/src/Foo.vue.meta.ts".to_string(), original_offset + 3)]
        );
    }

    #[tokio::test]
    async fn definition_type_query_uses_definition_site_hover() {
        let temp_root = std::env::temp_dir().join(format!(
            "verter-provider-adapter-definition-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_root);
        std::fs::create_dir_all(&temp_root).expect("temp dir should be created");
        let definition_path = temp_root.join("types.ts");
        let definition_source =
            "export interface AccordionEmits<T> { 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined] }\n";
        std::fs::write(&definition_path, definition_source).expect("definition file should exist");
        let definition_path = definition_path.to_string_lossy().replace('\\', "/");
        let definition_offset = definition_source
            .find("'update:modelValue'")
            .expect("member name should exist") as u32
            + 2;

        let provider = Arc::new(MockTypeProvider {
            definitions: vec![TypeLocation {
                path: definition_path.clone(),
                start: definition_offset,
                end: definition_offset + "update:modelValue".len() as u32,
            }],
            hover_at: StdMutex::new(HashMap::from([(
                (definition_path.clone(), definition_offset),
                Some(HoverInfo {
                    contents:
                        "(property) 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined]"
                            .to_string(),
                    range_start: None,
                    range_end: None,
                }),
            )])),
            ..Default::default()
        });
        let adapter = TypeProviderAdapter::new(provider.clone(), "tsserver");
        let file_id = test_file_id();
        adapter
            .sync_file(&file_id, 1, "type Query = {}\n")
            .await
            .expect("sync should succeed");

        let data = adapter
            .query_type_data(&file_id, 1, 4, BackendTypeQuery::DefinitionTypeAtOffset)
            .await
            .expect("definition query should succeed");

        assert_eq!(
            data.type_text.as_deref(),
            Some("(property) 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined]")
        );
        assert_eq!(
            provider.definition_calls.lock().unwrap().as_slice(),
            &[("/src/Foo.vue.meta.ts".to_string(), 4)]
        );
        let open_calls = provider.open_calls.lock().unwrap();
        assert_eq!(open_calls.len(), 2);
        assert_eq!(open_calls[1].0, definition_path);
        assert_eq!(
            provider.hover_calls.lock().unwrap().as_slice(),
            &[(definition_path.clone(), definition_offset)]
        );

        let _ = std::fs::remove_dir_all(&temp_root);
    }
}
