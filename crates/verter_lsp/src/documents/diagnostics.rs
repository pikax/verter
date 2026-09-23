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

/// One registered outbound diagnostics send.
///
/// The publication that owns a URI's epoch registers this BEFORE it awaits the
/// client enqueue, and drops it when the send settles. Dropping the sender — which
/// is what removing the entry does — resolves the publisher's receiver, and the
/// publisher abandons its enqueue instead of completing it.
struct InFlightSend {
    /// The epoch of the publication that owns this send, so a late publisher only
    /// ever releases the slot it registered itself.
    epoch: u64,
    /// Held by the state map; dropped on cancellation. The publisher's receiver
    /// resolving (by value or by sender drop) means CANCELLED.
    _cancel: tokio::sync::oneshot::Sender<()>,
}

#[derive(Default)]
pub(super) struct DiagnosticsState {
    next_epoch: u64,
    epochs: HashMap<String, u64>,
    receipts: HashMap<String, (DiagnosticPublication, Option<Arc<ProviderSurfaceSnapshot>>)>,
    /// The outbound sends currently suspended on the client channel, one per URI.
    in_flight: HashMap<String, InFlightSend>,
}

impl DiagnosticsState {
    /// Take ownership of a URI's epoch slot: write `epoch` (or clear the slot when
    /// `None`), drop its receipt, and CANCEL whatever outbound send the previous
    /// owner still had suspended on the client channel.
    ///
    /// This is the publication fence. It is ONE synchronous critical section, so a
    /// close or a supersession is ordered against the send without ever waiting for
    /// it: the publisher registers its send under this same lock only after
    /// confirming it still owns the epoch, so a taker either wins the race (the
    /// publisher's later epoch check fails and nothing is sent) or finds the
    /// registered send and cancels it before it can reach the client. A publication
    /// that was already committed to the wire is simply not in the map any more.
    ///
    /// The previous design made the close AWAIT a global publisher mutex that the
    /// send held across the client enqueue, so one stalled client consumer could
    /// strand every close, open and change behind it.
    fn take_uri(&mut self, uri: &str, epoch: Option<u64>) {
        match epoch {
            Some(epoch) => {
                self.epochs.insert(uri.to_string(), epoch);
            }
            None => {
                self.epochs.remove(uri);
            }
        }
        self.receipts.remove(uri);
        self.in_flight.remove(uri);
    }
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
        state.take_uri(refresh.uri.as_str(), Some(epoch));
        true
    }

    /// A compiler generation can advance without an editor edit. Replace the
    /// discarded computation through the normal coordinator, once per epoch.
    /// A newer document or publication already owns its own work and must not
    /// be displaced by this older writer.
    fn refresh_superseded_diagnostics(&self, uri: &Uri, publication: &DiagnosticPublication) {
        if !self.snapshot_identity_is_current(uri, &publication.snapshot)
            || self
                .host()
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
        state.take_uri(uri.as_str(), Some(epoch));
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
        state.take_uri(uri, Some(epoch));
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
        self.diagnostics_state.lock().take_uri(uri, None);
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
            state.take_uri(uri, None);
        }
    }

    /// Claim the outbound send slot for `publication`, or refuse it.
    ///
    /// Returns the cancellation receiver the publisher must race its client
    /// enqueue against. The epoch check and the registration happen in ONE
    /// synchronous critical section against [`DiagnosticsState::take_uri`], which
    /// is what makes the fence atomic: a close or supersession either lands first
    /// (this claim finds a different epoch and refuses) or lands second (it finds
    /// the registered send and cancels it before a byte reaches the client).
    fn claim_outbound_send(
        &self,
        uri: &Uri,
        publication: &DiagnosticPublication,
    ) -> Option<tokio::sync::oneshot::Receiver<()>> {
        let mut state = self.diagnostics_state.lock();
        if state.epochs.get(uri.as_str()) != Some(&publication.epoch) {
            return None;
        }
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        state.in_flight.insert(
            uri.to_string(),
            InFlightSend {
                epoch: publication.epoch,
                _cancel: cancel,
            },
        );
        Some(cancelled)
    }

    /// Release the send slot this publication registered, iff it is still ours,
    /// and report whether it was.
    fn release_outbound_send(&self, uri: &Uri, epoch: u64) -> bool {
        let mut state = self.diagnostics_state.lock();
        if state
            .in_flight
            .get(uri.as_str())
            .is_some_and(|send| send.epoch == epoch)
        {
            state.in_flight.remove(uri.as_str());
            return true;
        }
        false
    }

    /// Whether any outbound diagnostics send is registered for `uri`. Tests only:
    /// it is how a test observes that a publisher has reached its enqueue without
    /// timing the observation.
    #[cfg(test)]
    pub(crate) fn outbound_send_in_flight(&self, uri: &Uri) -> bool {
        self.diagnostics_state
            .lock()
            .in_flight
            .contains_key(uri.as_str())
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
                self.host()
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
                .host()
                .get_diagnostics_generation(&uri_to_canonical_id(uri))
                == publication.generation
            && self.diagnostics_state.lock().epochs.get(uri.as_str()) == Some(&publication.epoch)
    }

    /// Publish one result, racing the client enqueue against this URI's
    /// cancellation.
    ///
    /// The enqueue is bounded by the client channel, and a client that stops
    /// draining it suspends the send for as long as it likes. NOTHING the document
    /// lifecycle needs is held across that await: the fence is the synchronous
    /// claim/cancel slot, not a mutex the close has to wait on, so a close, open or
    /// change never queues behind a stalled consumer. A close instead CANCELS the
    /// suspended enqueue, and the abandoned payload never reaches the client — the
    /// same stale-publication guarantee, without the stranding.
    pub(crate) async fn publish_diagnostics(
        &self,
        client: &Client,
        uri: &Uri,
        publication: &DiagnosticPublication,
        diagnostics: Vec<Diagnostic>,
        complete: bool,
        surface: Option<Arc<ProviderSurfaceSnapshot>>,
    ) {
        // Validity first (snapshot identity, compiler generation, epoch), then the
        // atomic epoch-recheck-and-claim that fences the send itself.
        let claimed = self
            .diagnostic_publication_is_current(uri, publication)
            .then(|| self.claim_outbound_send(uri, publication))
            .flatten();
        let Some(cancelled) = claimed else {
            tracing::debug!(uri = uri.as_str(), epoch = publication.epoch,
                current_epoch = ?self.diagnostics_state.lock().epochs.get(uri.as_str()),
                generation = ?publication.generation,
                current_generation = ?self.host().get_diagnostics_generation(&uri_to_canonical_id(uri)),
                "diagnostics publication superseded");
            self.refresh_superseded_diagnostics(uri, publication);
            self.refresh_outdated_receipts();
            return;
        };

        let sent = tokio::select! {
            // Cancellation wins a tie: a close that has already taken the URI must
            // never have its result committed as a receipt.
            biased;
            _ = cancelled => false,
            () = client.publish_diagnostics(
                uri.clone(),
                diagnostics,
                Some(publication.snapshot.version),
            ) => true,
        };
        // Releasing our own slot is also how a cancelled publisher reports that the
        // URI moved on: the slot it registered is gone.
        let still_ours = self.release_outbound_send(uri, publication.epoch);
        if !sent || !still_ours {
            tracing::debug!(
                uri = uri.as_str(),
                epoch = publication.epoch,
                "diagnostics send cancelled before it reached the client"
            );
            self.refresh_outdated_receipts();
            return;
        }

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

    /// A close CANCELS a suspended outbound send; it never waits for one.
    ///
    /// `publish_diagnostics` awaits a BOUNDED client channel, and a consumer that
    /// stops draining it suspends that enqueue for as long as it likes. Nothing the
    /// document lifecycle needs may be held across that await. `did_close` is
    /// therefore a plain synchronous call — the signature itself forbids it from
    /// waiting on the client — and what it does instead is take the URI's epoch slot
    /// and drop the registered send, which resolves the publisher's cancellation and
    /// makes it abandon the enqueue rather than complete it.
    ///
    /// The earlier design was the opposite: the close awaited a global publisher
    /// mutex the send held across the client enqueue, so one stalled consumer could
    /// strand every close, open and change in the session.
    ///
    /// Fully observed, never timed: the send slot is claimed exactly as
    /// `publish_diagnostics` claims it, and cancellation is read off the channel
    /// state rather than inferred from a sleep.
    ///
    /// Discriminating: with the `in_flight` drop removed from
    /// [`DiagnosticsState::take_uri`], the registered send survives the close and
    /// the receiver still reads `Empty` — the suspended payload would resume and
    /// reach the client for a document that is no longer open.
    #[test]
    fn a_close_cancels_a_suspended_send_instead_of_waiting_for_it() {
        use tokio::sync::oneshot::error::TryRecvError;

        let documents = registry();
        let uri: Uri = "file:///workspace/App.vue".parse().unwrap();
        open(&documents, &uri);

        let publication = documents
            .begin_diagnostics_publication(&uri)
            .expect("an open document admits a publication");
        let mut cancelled = documents
            .claim_outbound_send(&uri, &publication)
            .expect("the current publication claims the send slot");
        assert!(
            documents.outbound_send_in_flight(&uri),
            "the claim registers the send before the client enqueue is awaited"
        );
        assert!(
            matches!(cancelled.try_recv(), Err(TryRecvError::Empty)),
            "nothing has cancelled a send for a document that is still open"
        );

        documents.did_close(&uri);

        assert!(
            matches!(cancelled.try_recv(), Err(TryRecvError::Closed)),
            "the close must cancel the suspended send rather than leave it armed"
        );
        assert!(
            !documents.outbound_send_in_flight(&uri),
            "the closed document keeps no send registration"
        );
        assert!(
            !documents.release_outbound_send(&uri, publication.epoch),
            "a cancelled publisher must observe that the slot is no longer its own"
        );
        assert!(
            !documents.diagnostic_publication_is_current(&uri, &publication),
            "the closed document's publication is rejected"
        );
        assert_eq!(
            documents.tracked_diagnostics_documents(),
            0,
            "a cancelled send must not re-create the closed document's bookkeeping"
        );
    }

    /// A publication superseded while its send is suspended is CANCELLED, not
    /// merely denied its receipt.
    ///
    /// The epoch check and the send registration are one critical section against
    /// the take, so a newer publication either loses the race — its epoch write is
    /// not yet visible, and the owner that sends is the current one — or wins and
    /// cancels the older send in place. Without the cancellation the older payload
    /// resumes and reaches the client after the newer publication already owns the
    /// document.
    #[test]
    fn a_superseding_publication_cancels_the_older_suspended_send() {
        use tokio::sync::oneshot::error::TryRecvError;

        let documents = registry();
        let uri: Uri = "file:///workspace/App.vue".parse().unwrap();
        open(&documents, &uri);

        let older = documents
            .begin_diagnostics_publication(&uri)
            .expect("an open document admits a publication");
        let mut cancelled = documents
            .claim_outbound_send(&uri, &older)
            .expect("the current publication claims the send slot");

        let newer = documents
            .begin_diagnostics_publication(&uri)
            .expect("a newer publication takes the URI");

        assert!(
            matches!(cancelled.try_recv(), Err(TryRecvError::Closed)),
            "taking the URI must cancel the older send that was still suspended"
        );
        assert!(
            documents.claim_outbound_send(&uri, &older).is_none(),
            "the superseded publication can no longer claim the send slot"
        );
        assert!(
            documents.diagnostic_publication_is_current(&uri, &newer),
            "the newer publication still owns the document"
        );
        assert!(
            documents.claim_outbound_send(&uri, &newer).is_some(),
            "the current publication may claim the slot it owns"
        );
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
                    .host()
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
