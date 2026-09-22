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

    /// A compile advances a document's diagnostics generation, and one document's
    /// pass compiles others: a parent compiles the children it imports. When that
    /// lands after the child's receipt is committed, no publication of the child
    /// is in flight to notice, and no editor signal follows. Every publication
    /// therefore settles the receipts its own computation may have outdated.
    fn refresh_outdated_receipts(&self) {
        let receipts: Vec<(Uri, DiagnosticPublication)> = self
            .diagnostics_state
            .lock()
            .receipts
            .iter()
            .filter_map(|(uri, (publication, _))| Some((uri.parse().ok()?, publication.clone())))
            .collect();
        for (uri, publication) in receipts {
            self.refresh_superseded_diagnostics(&uri, &publication);
        }
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

    /// Drop every trace of a CLOSED document from the diagnostics state.
    ///
    /// [`Self::invalidate_diagnostics`] cannot do this: it is also the RESERVE
    /// step of [`Self::admit_diagnostics_publication`], so the epoch it writes is
    /// the ownership token the publication about to run will present. Close is the
    /// one point at which no future publication for this URI can be legitimate, so
    /// it is the one point at which the entry can go.
    ///
    /// Still fail-closed for work already in flight: an outstanding publication
    /// compares its epoch against the map, and an ABSENT entry matches no epoch at
    /// all, so it is rejected exactly as a superseding epoch would reject it. A
    /// reopen starts from the monotonic `next_epoch`, which is never rewound, so a
    /// stale in-flight epoch can never be resurrected by the removal.
    ///
    /// Without this, every URI the session ever opened left a permanent entry
    /// behind — a slow leak proportional to how long the editor stays open, in a
    /// map whose live size should track only the OPEN documents.
    pub(crate) fn release_diagnostics_state(&self, uri: &str) {
        let mut state = self.diagnostics_state.lock();
        state.epochs.remove(uri);
        state.receipts.remove(uri);
    }

    /// Release a reservation whose publication will never run, but ONLY while
    /// this caller still owns it.
    ///
    /// [`Self::admit_diagnostics_publication`] reserves the epoch BEFORE
    /// capturing, because a suspended old capture must not be able to retire a
    /// newer publication when it resumes. When the capture then fails — the
    /// commonest cause being that the document is already CLOSED — the
    /// reservation owns nothing: no publication carries that epoch, so nothing
    /// will ever settle it. Left in place it is a permanent entry for a URI the
    /// session no longer has open, which is exactly the accumulation
    /// [`Self::release_diagnostics_state`] exists to prevent, re-created by the
    /// next publication attempt after the close.
    ///
    /// The `== Some(&epoch)` test is what makes this safe: a newer publication,
    /// invalidation or reopen has already replaced the entry, so this rollback
    /// finds a different epoch and leaves it alone. It can only ever remove the
    /// reservation it wrote itself.
    fn rollback_unowned_reservation(&self, uri: &str, epoch: u64) {
        let mut state = self.diagnostics_state.lock();
        if state.epochs.get(uri) == Some(&epoch) {
            state.epochs.remove(uri);
            state.receipts.remove(uri);
        }
    }

    /// The publication fence, held across a document CLOSE.
    ///
    /// A publication validates its epoch and then awaits the outbound
    /// notification. A close landing inside that await window would otherwise let
    /// the already-validated stale result reach the client, with only the receipt
    /// commit suppressed — a stale publication, which the snapshot rules forbid
    /// outright. Taking the same lock the publisher holds orders the two: either
    /// the close waits for a publication that was current when it was validated,
    /// or the publication finds the document closed at its (fenced) check and
    /// drops. There is no window in between.
    ///
    /// The lock protects only the bounded enqueue and the receipt commit —
    /// provider computation never holds it — so a close is never blocked behind
    /// real work.
    pub(crate) async fn diagnostics_publication_fence(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.diagnostics_publisher.lock().await
    }

    /// How many documents the diagnostics state is currently tracking. Tests only:
    /// the live size must track the OPEN documents, never the documents the
    /// session has ever seen.
    #[cfg(test)]
    pub(crate) fn tracked_diagnostics_documents(&self) -> usize {
        let state = self.diagnostics_state.lock();
        state.epochs.len().max(state.receipts.len())
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
        let Some((snapshot, generation)) = capture() else {
            // No publication will carry this epoch, so nothing will ever settle
            // it. Give it back — but only if it is still ours (see the method).
            self.rollback_unowned_reservation(uri.as_str(), epoch);
            return None;
        };
        Some(DiagnosticPublication {
            snapshot,
            generation,
            epoch,
        })
    }

    pub(crate) fn diagnostic_publication_is_current(
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
            self.refresh_outdated_receipts();
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
        self.refresh_outdated_receipts();
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

    fn registry() -> DocumentRegistry {
        DocumentRegistry::new(Arc::new(VerterHost::new_standalone(HostConfig::default())))
    }

    fn open(documents: &DocumentRegistry, uri: &Uri) {
        let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".into(),
            version: 1,
            text: "<template><p>x</p></template>".into(),
        });
    }

    /// A publication ATTEMPT for a closed document must leave no state behind.
    ///
    /// Admission reserves the epoch before capturing, because a suspended old
    /// capture must not be able to retire a newer publication. When the capture
    /// then fails — a closed document has no snapshot identity — that reservation
    /// owns nothing: no publication carries it, so nothing will ever settle it,
    /// and it is a permanent entry for a URI the session no longer has open.
    ///
    /// Discriminating: without the rollback, the count after the refused
    /// publication reads 1, and it stays 1 for the life of the session — the
    /// close-time release is undone by the very next publication attempt that
    /// races it.
    #[test]
    fn a_refused_publication_after_close_leaves_no_diagnostics_state() {
        let documents = registry();
        let uri: Uri = "file:///workspace/App.vue".parse().unwrap();
        open(&documents, &uri);
        documents.did_close(&uri);
        assert_eq!(documents.tracked_diagnostics_documents(), 0);

        assert!(
            documents.begin_diagnostics_publication(&uri).is_none(),
            "a closed document has no snapshot identity to publish against"
        );
        assert_eq!(
            documents.tracked_diagnostics_documents(),
            0,
            "a publication that could not be captured must not re-create the closed \
             document's bookkeeping"
        );
    }

    /// The rollback gives back ONLY the reservation its own caller wrote.
    ///
    /// A newer publication that took ownership while the older capture was
    /// suspended must survive the older one's failure — otherwise the rollback
    /// would un-fence a live publication, which is strictly worse than the leak
    /// it repairs.
    #[test]
    fn a_rollback_never_removes_a_newer_publications_reservation() {
        let documents = registry();
        let uri: Uri = "file:///workspace/App.vue".parse().unwrap();
        open(&documents, &uri);

        let mut newer = None;
        let older = documents.admit_diagnostics_publication(&uri, || {
            // A newer publication takes ownership while this capture is suspended,
            // and only then does this one fail.
            newer = documents.begin_diagnostics_publication(&uri);
            None
        });

        assert!(older.is_none(), "the failed capture yields no publication");
        let newer = newer.expect("the newer publication was admitted");
        assert!(
            documents.diagnostic_publication_is_current(&uri, &newer),
            "the older capture's rollback must leave the newer reservation intact"
        );
    }

    /// A close cannot slip between a publication's validity check and its send.
    ///
    /// `publish_diagnostics` validates the epoch/document and then AWAITS the
    /// outbound notification. Both halves run inside the publication fence, and
    /// the close lifecycle takes that same fence, so the two are mutually
    /// exclusive — the whole point of `did_close_fenced`.
    ///
    /// Discriminating: with an unfenced close, the close below completes while
    /// the simulated send is suspended, so the assertion that it is still pending
    /// fails, and the publication's already-passed validity check would carry a
    /// stale result to the client.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_close_cannot_land_inside_a_publications_send_window() {
        let documents = Arc::new(registry());
        let uri: Uri = "file:///workspace/App.vue".parse().unwrap();
        open(&documents, &uri);

        let publication = documents
            .begin_diagnostics_publication(&uri)
            .expect("an open document admits a publication");

        // Enter the fence exactly as `publish_diagnostics` does, validate, and
        // then suspend where its outbound `.await` would be.
        let fence = documents.diagnostics_publication_fence().await;
        assert!(
            documents.diagnostic_publication_is_current(&uri, &publication),
            "the pre-send check passes for a document that is still open"
        );

        let closing = tokio::spawn({
            let documents = Arc::clone(&documents);
            let uri = uri.clone();
            async move { documents.did_close_fenced(&uri).await }
        });

        // The close must NOT be able to complete while the send window is open.
        // An UNFENCED close completes as soon as its task is scheduled, so this
        // window only has to be long enough for that to have happened.
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !closing.is_finished(),
            "a close must not land between a publication's validity check and its send"
        );
        assert!(
            documents.diagnostic_publication_is_current(&uri, &publication),
            "the document the send was validated against is still the current one"
        );

        drop(fence);
        closing
            .await
            .expect("the close completes once the fence clears");

        assert!(
            !documents.diagnostic_publication_is_current(&uri, &publication),
            "after the close, the same publication is rejected at its fenced check"
        );
        assert_eq!(documents.tracked_diagnostics_documents(), 0);
    }

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
