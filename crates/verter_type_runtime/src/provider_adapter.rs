//! Adapter: wraps a `TypeProvider` as a `GeneratedQueryBackend`.
//!
//! This bridges the legacy hover-based `TypeProvider` interface to the
//! new `GeneratedQueryBackend` contract. Both tsserver and TSGO providers
//! can be used through this adapter for type expansion queries.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::backend::*;
use crate::traits::TypeProvider;

/// Adapts a `TypeProvider` to implement `GeneratedQueryBackend`.
///
/// Internally tracks synced file revisions and converts `query_type_data`
/// into hover queries on the underlying provider.
pub struct TypeProviderAdapter {
    provider: Arc<dyn TypeProvider>,
    /// Tracks synced file revisions: virtual_path → revision.
    synced_revisions: Mutex<HashMap<String, u64>>,
}

impl TypeProviderAdapter {
    pub fn new(provider: Arc<dyn TypeProvider>) -> Self {
        Self {
            provider,
            synced_revisions: Mutex::new(HashMap::new()),
        }
    }

    fn virtual_path(file_id: &GeneratedFileId) -> String {
        let suffix = match file_id.profile {
            ArtifactProfile::ComponentMeta => ".meta.ts",
            ArtifactProfile::Lsp => ".tsx",
        };
        format!("{}{}", file_id.canonical_id, suffix)
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
            let existing_revision = {
                let revisions = self.synced_revisions.lock().await;
                if revisions.get(&path) == Some(&revision) {
                    return Ok(());
                }
                revisions.get(&path).copied()
            };

            if existing_revision.is_some() {
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

            self.synced_revisions.lock().await.insert(path, revision);
            Ok(())
        })
    }

    fn close_file<'a>(&'a self, file_id: &'a GeneratedFileId) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            let path = Self::virtual_path(file_id);
            self.provider
                .close_file(&path)
                .await
                .map_err(|e| BackendError::BackendReported(e.message))?;
            self.synced_revisions.lock().await.remove(&path);
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

            // Validate revision
            {
                let revisions = self.synced_revisions.lock().await;
                match revisions.get(&path) {
                    Some(&rev) if rev != expected_revision => {
                        return Err(BackendError::ProtocolViolation(format!(
                            "stale query: expected revision {expected_revision}, synced {rev}"
                        )));
                    }
                    None => {
                        return Err(BackendError::ProtocolViolation(
                            "file not synced".to_string(),
                        ));
                    }
                    _ => {}
                }
            }

            match query {
                BackendTypeQuery::TypeAtOffset | BackendTypeQuery::MembersAtOffset => {
                    let hover = self
                        .provider
                        .get_hover(&path, generated_offset)
                        .await
                        .map_err(|e| BackendError::BackendReported(e.message))?;

                    match hover {
                        Some(info) => Ok(BackendTypeData {
                            type_text: Some(info.contents),
                            members: vec![],
                            documentation: None,
                            completeness: BackendTypeCompleteness::Exact,
                        }),
                        None => Ok(BackendTypeData::default()),
                    }
                }
                BackendTypeQuery::DocumentationAtOffset => {
                    let hover = self
                        .provider
                        .get_hover(&path, generated_offset)
                        .await
                        .map_err(|e| BackendError::BackendReported(e.message))?;

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
            self.provider
                .shutdown()
                .await
                .map_err(|e| BackendError::BackendReported(e.message))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_path_component_meta() {
        let id = GeneratedFileId {
            canonical_id: "/src/Foo.vue".into(),
            profile: ArtifactProfile::ComponentMeta,
            runtime_key: "test".into(),
        };
        assert_eq!(
            TypeProviderAdapter::virtual_path(&id),
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
}
