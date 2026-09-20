use std::{collections::HashMap, sync::Arc};

use tower_lsp_server::{
    ls_types::{Diagnostic, Uri},
    Client,
};

use super::{uri_to_canonical_id, DocumentRegistry, DocumentSnapshotIdentity};
use crate::provider_surface_store::ProviderSurfaceSnapshot;

#[derive(Clone)]
pub(crate) struct DiagnosticPublication {
    pub snapshot: DocumentSnapshotIdentity,
    generation: Option<u64>,
    epoch: u64,
}

#[derive(Clone)]
pub(crate) struct DiagnosticsRefresh {
    pub uri: Uri,
    pub snapshot: DocumentSnapshotIdentity,
    epoch: u64,
}

#[derive(Default)]
pub(super) struct DiagnosticsState {
    next_epoch: u64,
    epochs: HashMap<String, u64>,
    receipts: HashMap<String, (DiagnosticPublication, Option<Arc<ProviderSurfaceSnapshot>>)>,
}

impl DocumentRegistry {
    pub(crate) fn subscribe_diagnostics_refresh(
        &self,
    ) -> tokio::sync::broadcast::Receiver<DiagnosticsRefresh> {
        self.diagnostics_refresh_tx.subscribe()
    }

    pub(crate) fn claim_diagnostics_refresh(&self, refresh: &DiagnosticsRefresh) -> bool {
        if !self.snapshot_identity_is_current(&refresh.uri, &refresh.snapshot) {
            return false;
        }
        let mut state = self.diagnostics_state.lock();
        if state.epochs.get(refresh.uri.as_str()) != Some(&refresh.epoch) {
            return false;
        }
        state.next_epoch += 1;
        let epoch = state.next_epoch;
        state.epochs.insert(refresh.uri.to_string(), epoch);
        state.receipts.remove(refresh.uri.as_str());
        true
    }

    /// A compiler generation can advance without an editor edit. Replace the
    /// discarded computation through the normal coordinator, once per epoch.
    /// A newer document or publication already owns its own work and must not
    /// be displaced by this older writer.
    fn refresh_superseded_diagnostics(&self, uri: &Uri, publication: &DiagnosticPublication) {
        if !self.snapshot_identity_is_current(uri, &publication.snapshot)
            || self
                .host
                .get_diagnostics_generation(&uri_to_canonical_id(uri))
                == publication.generation
        {
            return;
        }
        let mut state = self.diagnostics_state.lock();
        if state.epochs.get(uri.as_str()) != Some(&publication.epoch) {
            return;
        }
        state.next_epoch += 1;
        let epoch = state.next_epoch;
        state.epochs.insert(uri.to_string(), epoch);
        state.receipts.remove(uri.as_str());
        let _ = self.diagnostics_refresh_tx.send(DiagnosticsRefresh {
            uri: uri.clone(),
            snapshot: publication.snapshot.clone(),
            epoch,
        });
    }

    /// Pending work invalidates immediately, including same-version semantic
    /// enrichment. The epoch also fences publications already awaiting a provider.
    pub(crate) fn invalidate_diagnostics(&self, uri: &str) -> u64 {
        let mut state = self.diagnostics_state.lock();
        state.next_epoch += 1;
        let epoch = state.next_epoch;
        state.epochs.insert(uri.to_string(), epoch);
        state.receipts.remove(uri);
        tracing::debug!(uri, epoch, "diagnostics invalidated");
        epoch
    }

    pub(crate) fn begin_diagnostics_publication(&self, uri: &Uri) -> Option<DiagnosticPublication> {
        self.admit_diagnostics_publication(uri, || {
            Some((
                self.snapshot_identity(uri)?,
                self.host
                    .get_diagnostics_generation(&uri_to_canonical_id(uri)),
            ))
        })
    }

    fn admit_diagnostics_publication(
        &self,
        uri: &Uri,
        capture: impl FnOnce() -> Option<(DocumentSnapshotIdentity, Option<u64>)>,
    ) -> Option<DiagnosticPublication> {
        // Reserve ownership before capture: a suspended old read must never
        // retire a newer publication when it resumes.
        let epoch = self.invalidate_diagnostics(uri.as_str());
        let (snapshot, generation) = capture()?;
        Some(DiagnosticPublication {
            snapshot,
            generation,
            epoch,
        })
    }

    fn diagnostic_publication_is_current(
        &self,
        uri: &Uri,
        publication: &DiagnosticPublication,
    ) -> bool {
        self.snapshot_identity_is_current(uri, &publication.snapshot)
            && self
                .host
                .get_diagnostics_generation(&uri_to_canonical_id(uri))
                == publication.generation
            && self.diagnostics_state.lock().epochs.get(uri.as_str()) == Some(&publication.epoch)
    }

    /// All push writers share one send order. Provider computation never holds
    /// this lock; only the bounded notification enqueue and receipt commit do.
    pub(crate) async fn publish_diagnostics(
        &self,
        client: &Client,
        uri: &Uri,
        publication: &DiagnosticPublication,
        diagnostics: Vec<Diagnostic>,
        complete: bool,
        surface: Option<Arc<ProviderSurfaceSnapshot>>,
    ) {
        let _publisher = self.diagnostics_publisher.lock().await;
        if !self.diagnostic_publication_is_current(uri, publication) {
            tracing::debug!(uri = uri.as_str(), epoch = publication.epoch,
                current_epoch = ?self.diagnostics_state.lock().epochs.get(uri.as_str()),
                generation = ?publication.generation,
                current_generation = ?self.host.get_diagnostics_generation(&uri_to_canonical_id(uri)),
                "diagnostics publication superseded");
            self.refresh_superseded_diagnostics(uri, publication);
            return;
        }
        client
            .publish_diagnostics(uri.clone(), diagnostics, Some(publication.snapshot.version))
            .await;
        let mut state = self.diagnostics_state.lock();
        if state.epochs.get(uri.as_str()) == Some(&publication.epoch) {
            if complete {
                tracing::debug!(uri = uri.as_str(), epoch = publication.epoch, generation = ?publication.generation, "diagnostics complete");
                state
                    .receipts
                    .insert(uri.to_string(), (publication.clone(), surface));
            } else {
                state.receipts.remove(uri.as_str());
            }
        }
        drop(state);
        self.refresh_superseded_diagnostics(uri, publication);
    }

    pub(crate) fn diagnostics_ready(&self, uri: &Uri) -> bool {
        let receipt = self
            .diagnostics_state
            .lock()
            .receipts
            .get(uri.as_str())
            .cloned();
        receipt.is_some_and(|(publication, surface)| {
            self.diagnostic_publication_is_current(uri, &publication)
                && self.semantic_diagnostics_ready(uri, publication.snapshot.revision)
                && surface.is_none_or(|snapshot| {
                    self.provider_surfaces
                        .captured_snapshot_still_honored(&snapshot)
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::TextDocumentItem;
    use verter_session::{HostConfig, VerterHost};

    #[test]
    fn paused_diagnostics_admission_cannot_displace_a_newer_publication() {
        let documents =
            DocumentRegistry::new(Arc::new(VerterHost::new_standalone(HostConfig::default())));
        let uri: Uri = "file:///workspace/App.vue".parse().unwrap();
        let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".into(),
            version: 1,
            text: "<template><p>before</p></template>".into(),
        });
        let mut newer = None;
        let older = documents
            .admit_diagnostics_publication(&uri, || {
                let snapshot = documents.snapshot_identity(&uri).unwrap();
                let generation = documents
                    .host
                    .get_diagnostics_generation(&uri_to_canonical_id(&uri));
                // Suspend the old capture while a later edit and publication own the file.
                assert!(
                    documents
                        .did_change(&uri, 2, "<template><p>after</p></template>")
                        .changed
                );
                newer = documents.begin_diagnostics_publication(&uri);
                Some((snapshot, generation))
            })
            .unwrap();
        assert!(!documents.diagnostic_publication_is_current(&uri, &older));
        assert!(
            documents.diagnostic_publication_is_current(&uri, &newer.unwrap()),
            "resuming the old capture must preserve the newer publication's ownership"
        );
    }
}
