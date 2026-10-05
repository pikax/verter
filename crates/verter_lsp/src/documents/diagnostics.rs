use std::{collections::HashMap, sync::Arc};

use tower_lsp_server::ls_types::{Diagnostic, Uri};

use super::{uri_to_canonical_id, DocumentRegistry, DocumentSnapshotIdentity};
use crate::provider_surface_store::ProviderSurfaceSnapshot;

#[derive(Clone)]
pub(crate) struct ReadinessBasis {
    pub snapshot: DocumentSnapshotIdentity,
    generation: Option<u64>,
    // Weak identity prevents address reuse without retaining a retired root's payload.
    workspace: Option<std::sync::Weak<verter_workspace::PublishedRoot>>,
}

impl ReadinessBasis {
    pub(crate) fn capture(documents: &DocumentRegistry, uri: &Uri) -> Option<Self> {
        let snapshot = documents.snapshot_identity(uri)?;
        let host = documents.host();
        Some(Self {
            snapshot,
            generation: host.get_diagnostics_generation(&uri_to_canonical_id(uri)),
            workspace: host
                .workspace_read()
                .published_root()
                .as_ref()
                .map(Arc::downgrade),
        })
    }

    fn workspace_matches(
        &self,
        current: Option<&std::sync::Weak<verter_workspace::PublishedRoot>>,
    ) -> bool {
        match (self.workspace.as_ref(), current) {
            (Some(before), Some(after)) => std::sync::Weak::ptr_eq(before, after),
            (None, None) => true,
            _ => false,
        }
    }

    fn environment_is_current(&self, documents: &DocumentRegistry, uri: &Uri) -> bool {
        let host = documents.host();
        let workspace = host.workspace_read().published_root();
        host.get_diagnostics_generation(&uri_to_canonical_id(uri)) == self.generation
            && self.workspace_matches(workspace.as_ref().map(Arc::downgrade).as_ref())
    }

    fn is_current(&self, documents: &DocumentRegistry, uri: &Uri) -> bool {
        documents.snapshot_identity_is_current(uri, &self.snapshot)
            && self.environment_is_current(documents, uri)
    }
}

pub(crate) struct ForegroundSettlement {
    basis: Option<ReadinessBasis>,
}

impl ForegroundSettlement {
    pub(crate) fn capture(documents: &DocumentRegistry, uri: &Uri) -> Self {
        Self {
            basis: ReadinessBasis::capture(documents, uri),
        }
    }

    pub(crate) fn is_current(&self, documents: &DocumentRegistry, uri: &Uri) -> bool {
        match &self.basis {
            Some(basis) => basis.is_current(documents, uri),
            None => documents.snapshot_identity(uri).is_none(),
        }
    }

    pub(crate) fn version(&self) -> Option<i32> {
        self.basis.as_ref().map(|basis| basis.snapshot.version)
    }

    /// Admission for recomputing a native result, never a publication check.
    pub(crate) fn document_and_workspace_are_current(
        &self,
        documents: &DocumentRegistry,
        uri: &Uri,
    ) -> bool {
        let Some(basis) = &self.basis else {
            return documents.snapshot_identity(uri).is_none();
        };
        let workspace = documents.host().workspace_read().published_root();
        documents.snapshot_identity_is_current(uri, &basis.snapshot)
            && basis.workspace_matches(workspace.as_ref().map(Arc::downgrade).as_ref())
    }

    pub(crate) fn settle<T>(
        &self,
        documents: &DocumentRegistry,
        uri: &Uri,
        response: Option<T>,
    ) -> tower_lsp_server::jsonrpc::Result<Option<T>> {
        if response.is_none() || self.is_current(documents, uri) {
            Ok(response)
        } else {
            Err(tower_lsp_server::jsonrpc::Error::new(
                tower_lsp_server::jsonrpc::ErrorCode::ContentModified,
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PublicationEpoch(u64);

#[derive(Clone)]
pub(crate) struct BackgroundPublication {
    basis: ReadinessBasis,
    epoch: PublicationEpoch,
}

#[derive(Clone)]
pub(crate) struct DiagnosticsRefresh {
    pub uri: Uri,
    pub snapshot: DocumentSnapshotIdentity,
    epoch: PublicationEpoch,
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
    epoch: PublicationEpoch,
    /// Held by the state map; dropped on cancellation. The publisher's receiver
    /// resolving (by value or by sender drop) means CANCELLED.
    _cancel: tokio::sync::oneshot::Sender<()>,
}

/// What a document's last publication left behind. A COMPLETE receipt
/// certifies the document (`diagnostics_ready`). An INCOMPLETE one — the
/// provider batch could not be merged: no committed IDE surface at pull time,
/// a refused commit, a surface superseded mid-flight — certifies nothing but
/// records that the document is OWED a publication, so the generation advance
/// that eventually settles its carrier re-arms it like any outdated receipt.
/// Without that record an incomplete publication left nothing to outdate, and
/// the open document stayed uncertified until the next editor signal, which an
/// editor waiting on the receipt never sends.
#[derive(Clone)]
struct DiagnosticsReceipt {
    publication: BackgroundPublication,
    surface: Option<Arc<ProviderSurfaceSnapshot>>,
    complete: bool,
}

#[derive(Default)]
pub(super) struct DiagnosticsState {
    next_epoch: u64,
    epochs: HashMap<String, PublicationEpoch>,
    receipts: HashMap<String, DiagnosticsReceipt>,
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
    fn take_uri(&mut self, uri: &str, epoch: Option<PublicationEpoch>) {
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
        let epoch = PublicationEpoch(state.next_epoch);
        state.take_uri(refresh.uri.as_str(), Some(epoch));
        true
    }

    /// A compiler generation can advance without an editor edit. Replace the
    /// discarded computation through the normal coordinator, once per epoch.
    /// A newer document or publication already owns its own work and must not
    /// be displaced by this older writer.
    fn refresh_superseded_diagnostics(&self, uri: &Uri, publication: &BackgroundPublication) {
        if !self.snapshot_identity_is_current(uri, &publication.basis.snapshot)
            || publication.basis.environment_is_current(self, uri)
        {
            return;
        }
        let mut state = self.diagnostics_state.lock();
        if state.epochs.get(uri.as_str()) != Some(&publication.epoch) {
            return;
        }
        state.next_epoch += 1;
        let epoch = PublicationEpoch(state.next_epoch);
        state.take_uri(uri.as_str(), Some(epoch));
        let _ = self.diagnostics_refresh_tx.send(DiagnosticsRefresh {
            uri: uri.clone(),
            snapshot: publication.basis.snapshot.clone(),
            epoch,
        });
    }

    /// A compile advances a document's diagnostics generation, and one document's
    /// pass compiles others: a parent compiles the children it imports. When that
    /// lands after the child's receipt is committed, no publication of the child
    /// is in flight to notice, and no editor signal follows. Every publication
    /// therefore settles the receipts its own computation may have outdated.
    fn refresh_outdated_receipts(&self) {
        let receipts: Vec<(Uri, BackgroundPublication)> = self
            .diagnostics_state
            .lock()
            .receipts
            .iter()
            .filter_map(|(uri, receipt)| Some((uri.parse().ok()?, receipt.publication.clone())))
            .collect();
        for (uri, publication) in receipts {
            self.refresh_superseded_diagnostics(&uri, &publication);
        }
    }

    /// A background pass settled `canonical_id`'s carrier (the drain re-synced
    /// it and advanced its diagnostics generation) with no publication in
    /// flight to notice. If the open document holds a receipt that pass
    /// outdated — complete or owed — replace it through the coordinator.
    pub(crate) fn refresh_owed_diagnostics(&self, canonical_id: &str) {
        let Some(uri) = self.canonical_id_to_uri(canonical_id) else {
            return;
        };
        let receipt = self
            .diagnostics_state
            .lock()
            .receipts
            .get(uri.as_str())
            .map(|receipt| receipt.publication.clone());
        if let Some(publication) = receipt {
            self.refresh_superseded_diagnostics(&uri, &publication);
        }
    }

    /// Pending work invalidates immediately, including same-version semantic
    /// enrichment. The epoch also fences publications already awaiting a provider.
    pub(crate) fn invalidate_diagnostics(&self, uri: &str) -> PublicationEpoch {
        let mut state = self.diagnostics_state.lock();
        state.next_epoch += 1;
        let epoch = PublicationEpoch(state.next_epoch);
        state.take_uri(uri, Some(epoch));
        tracing::debug!(uri, epoch = epoch.0, "diagnostics invalidated");
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
    fn rollback_unowned_reservation(&self, uri: &str, epoch: PublicationEpoch) {
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
        publication: &BackgroundPublication,
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
    fn release_outbound_send(&self, uri: &Uri, epoch: PublicationEpoch) -> bool {
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

    pub(crate) fn begin_diagnostics_publication(&self, uri: &Uri) -> Option<BackgroundPublication> {
        self.admit_diagnostics_publication(uri, || ReadinessBasis::capture(self, uri))
    }

    fn admit_diagnostics_publication(
        &self,
        uri: &Uri,
        capture: impl FnOnce() -> Option<ReadinessBasis>,
    ) -> Option<BackgroundPublication> {
        // Reserve ownership before capture: a suspended old read must never
        // retire a newer publication when it resumes.
        let epoch = self.invalidate_diagnostics(uri.as_str());
        let Some(basis) = capture() else {
            // No publication will carry this epoch, so nothing will ever settle
            // it. Give it back — but only if it is still ours (see the method).
            self.rollback_unowned_reservation(uri.as_str(), epoch);
            return None;
        };
        Some(BackgroundPublication { basis, epoch })
    }

    pub(crate) fn diagnostic_publication_is_current(
        &self,
        uri: &Uri,
        publication: &BackgroundPublication,
    ) -> bool {
        publication.basis.is_current(self, uri)
            && self.diagnostics_state.lock().epochs.get(uri.as_str()) == Some(&publication.epoch)
    }

    /// Publish one result, racing its delivery against this URI's cancellation.
    ///
    /// The payload goes into the bounded [`crate::outbound::ReplaceableLane`],
    /// and a client that stops reading suspends its delivery for as long as it
    /// likes. NOTHING the document lifecycle needs is held across that await: the
    /// fence is the synchronous claim/cancel slot, not a mutex the close has to
    /// wait on, so a close, open or change never queues behind a stalled consumer.
    /// A close or supersession instead CANCELS the suspended delivery, which
    /// withdraws the payload from the lane, so it never reaches the client — the
    /// same stale-publication guarantee, without the stranding. The transport
    /// writer re-checks this epoch, under the same lock as the fence, at the
    /// moment it takes the payload, so a publication cancelled before the take
    /// is never written even if this task has not yet observed the cancellation.
    /// Only the one payload already being written may finish.
    pub(crate) async fn publish_diagnostics(
        &self,
        uri: &Uri,
        publication: &BackgroundPublication,
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
            tracing::debug!(uri = uri.as_str(), epoch = publication.epoch.0,
                current_epoch = ?self.diagnostics_state.lock().epochs.get(uri.as_str()),
                generation = ?publication.basis.generation,
                current_generation = ?self.host().get_diagnostics_generation(&uri_to_canonical_id(uri)),
                "diagnostics publication superseded");
            self.refresh_superseded_diagnostics(uri, publication);
            self.refresh_outdated_receipts();
            return;
        };

        let params = tower_lsp_server::ls_types::PublishDiagnosticsParams::new(
            uri.clone(),
            diagnostics,
            Some(publication.basis.snapshot.version),
        );
        let still_current = {
            let state = Arc::clone(&self.diagnostics_state);
            let uri = uri.to_string();
            let epoch = publication.epoch;
            move || state.lock().epochs.get(&uri) == Some(&epoch)
        };
        let sent = tokio::select! {
            // Cancellation wins a tie: a close that has already taken the URI must
            // never have its result committed as a receipt. Losing the race drops
            // the lane offer, which withdraws a payload the writer has not taken.
            biased;
            _ = cancelled => false,
            delivery = self.diagnostics_outbound.publish_diagnostics(
                publication.epoch.0,
                params,
                still_current,
            ) => delivery == crate::outbound::Delivery::Delivered,
        };
        // Releasing our own slot is also how a cancelled publisher reports that the
        // URI moved on: the slot it registered is gone.
        let still_ours = self.release_outbound_send(uri, publication.epoch);
        if !sent || !still_ours {
            tracing::debug!(
                uri = uri.as_str(),
                epoch = publication.epoch.0,
                "diagnostics send cancelled before it reached the client"
            );
            self.refresh_outdated_receipts();
            return;
        }

        let mut state = self.diagnostics_state.lock();
        if state.epochs.get(uri.as_str()) == Some(&publication.epoch) {
            if complete {
                tracing::debug!(uri = uri.as_str(), epoch = publication.epoch.0, generation = ?publication.basis.generation, "diagnostics complete");
            } else {
                tracing::debug!(uri = uri.as_str(), epoch = publication.epoch.0, generation = ?publication.basis.generation, "diagnostics incomplete; publication owed");
            }
            // An incomplete publication is recorded as OWED rather than dropped:
            // the generation advance that settles its carrier must find it.
            state.receipts.insert(
                uri.to_string(),
                DiagnosticsReceipt {
                    publication: publication.clone(),
                    surface,
                    complete,
                },
            );
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
        receipt.is_some_and(|receipt| {
            receipt.complete
                && self.diagnostic_publication_is_current(uri, &receipt.publication)
                && self.semantic_diagnostics_ready(uri, receipt.publication.basis.snapshot.revision)
                && receipt.surface.is_none_or(|snapshot| {
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

    fn diagnostic(message: &str) -> Vec<Diagnostic> {
        vec![Diagnostic::new_simple(
            tower_lsp_server::ls_types::Range::default(),
            message.into(),
        )]
    }

    /// A registry publishing through `outbound`'s diagnostics lane.
    fn registry_on(outbound: &crate::outbound::Outbound) -> DocumentRegistry {
        DocumentRegistry::with_diagnostics_lane(
            Arc::new(VerterHost::new_standalone(HostConfig::default())),
            outbound.diagnostics_lane(),
        )
    }

    fn message_text(message: &tower_lsp_server::jsonrpc::Request) -> (String, String) {
        let params: tower_lsp_server::ls_types::PublishDiagnosticsParams =
            serde_json::from_value(message.params().cloned().expect("params"))
                .expect("publishDiagnostics params decode");
        let text = params
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join(",");
        (params.uri.as_str().to_owned(), text)
    }

    /// Everything the writer would write right now, without waiting for more.
    fn written_now(wire: &mut crate::outbound::Wire) -> Vec<(String, String)> {
        use futures_util::{FutureExt as _, StreamExt as _};
        let mut written = Vec::new();
        while let Some(Some(message)) = wire.next().now_or_never() {
            if message.method() == "textDocument/publishDiagnostics" {
                written.push(message_text(&message));
            }
        }
        written
    }

    /// A publication cancelled while the client is not reading never reaches
    /// it, even when its own publisher task is still suspended.
    ///
    /// The editor stops reading, so the writer takes nothing and the edited
    /// document's publication waits in the lane, its publisher parked on the
    /// delivery. A newer edit (or a close) then cancels it — and the publisher
    /// is deliberately NOT polled again, as when the runtime has not yet
    /// scheduled it. When the editor resumes reading, the writer reaches the
    /// stale payload first; it must see that the publication is no longer
    /// current and retire it rather than write it.
    ///
    /// Discriminating: a writer that takes whatever is queued, relying on the
    /// publisher to withdraw its own payload once it observes the cancellation,
    /// writes `stale` (and, for the close, a diagnostics set for a document the
    /// editor no longer has open).
    #[tokio::test(flavor = "current_thread")]
    async fn a_cancelled_publication_is_retired_when_the_writer_reaches_it() {
        for close in [false, true] {
            let outbound = crate::outbound::Outbound::default();
            let documents = registry_on(&outbound);
            let edited: Uri = "file:///workspace/Edited.vue".parse().unwrap();
            open(&documents, &edited);

            let stale = documents.begin_diagnostics_publication(&edited).unwrap();
            let stale_send =
                documents.publish_diagnostics(&edited, &stale, diagnostic("stale"), true, None);
            tokio::pin!(stale_send);
            assert!(
                futures_util::poll!(&mut stale_send).is_pending(),
                "nobody is writing, so the publication waits for the client"
            );
            assert_eq!(outbound.load().replaceable.admitted.messages, 1);

            if close {
                documents.did_close(&edited);
            } else {
                let _newer = documents.begin_diagnostics_publication(&edited).unwrap();
            }

            // The editor resumes reading before the stale publisher runs again.
            let mut wire = outbound.wire();
            assert_eq!(
                written_now(&mut wire),
                Vec::<(String, String)>::new(),
                "the cancelled payload must never be written (close: {close})"
            );
            assert_eq!(
                outbound.load().replaceable.retained(),
                Default::default(),
                "the retired payload is no longer retained"
            );
            stale_send.await;
            assert!(!documents.diagnostics_ready(&edited));
        }
    }

    /// A publication superseded while it waits for a slow client is withdrawn,
    /// and the client receives only the newer diagnostics once it reads again.
    #[tokio::test(flavor = "current_thread")]
    async fn a_send_superseded_behind_a_slow_client_never_reaches_it() {
        let outbound = crate::outbound::Outbound::default();
        let documents = registry_on(&outbound);
        let busy: Uri = "file:///workspace/Busy.vue".parse().unwrap();
        let edited: Uri = "file:///workspace/Edited.vue".parse().unwrap();
        open(&documents, &busy);
        open(&documents, &edited);

        let first = documents.begin_diagnostics_publication(&busy).unwrap();
        let first_send =
            documents.publish_diagnostics(&busy, &first, diagnostic("busy-1"), true, None);
        tokio::pin!(first_send);
        assert!(futures_util::poll!(&mut first_send).is_pending());

        let stale = documents.begin_diagnostics_publication(&edited).unwrap();
        let stale_send =
            documents.publish_diagnostics(&edited, &stale, diagnostic("stale"), true, None);
        tokio::pin!(stale_send);
        assert!(
            futures_util::poll!(&mut stale_send).is_pending(),
            "the slow client keeps the publication waiting"
        );
        // An edit supersedes the waiting publication; its publisher observes the
        // cancellation and returns without a receipt.
        let fresh = documents.begin_diagnostics_publication(&edited).unwrap();
        stale_send.await;

        let fresh_send =
            documents.publish_diagnostics(&edited, &fresh, diagnostic("fresh"), true, None);
        tokio::pin!(fresh_send);
        assert!(futures_util::poll!(&mut fresh_send).is_pending());

        let mut wire = outbound.wire();
        assert_eq!(
            written_now(&mut wire),
            vec![
                (busy.as_str().to_owned(), "busy-1".to_owned()),
                (edited.as_str().to_owned(), "fresh".to_owned()),
            ],
            "the superseded payload must never reach the client"
        );
        first_send.await;
        fresh_send.await;
        assert!(documents.diagnostics_ready(&busy));
        assert!(documents.diagnostics_ready(&edited));
    }

    struct HandshakeOnly;

    impl tower_lsp_server::LanguageServer for HandshakeOnly {
        async fn initialize(
            &self,
            _: tower_lsp_server::ls_types::InitializeParams,
        ) -> tower_lsp_server::jsonrpc::Result<tower_lsp_server::ls_types::InitializeResult>
        {
            Ok(Default::default())
        }

        async fn shutdown(&self) -> tower_lsp_server::jsonrpc::Result<()> {
            Ok(())
        }
    }

    async fn send_frame(
        writer: &mut (impl tokio::io::AsyncWrite + Unpin),
        message: serde_json::Value,
    ) {
        use tokio::io::AsyncWriteExt as _;
        let body = serde_json::to_vec(&message).unwrap();
        writer
            .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await
            .unwrap();
        writer.write_all(&body).await.unwrap();
        writer.flush().await.unwrap();
    }

    async fn recv_frame(reader: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> serde_json::Value {
        use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _};
        let mut length = None;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = Some(value.parse::<usize>().unwrap());
            }
        }
        let mut body = vec![0; length.expect("every frame carries a length")];
        reader.read_exact(&mut body).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    /// Read diagnostics until `uri` receives `last`, returning every set read
    /// as `(uri, joined messages)`; the large `busy` set reads as `busy`.
    async fn read_diagnostics_until(
        reader: &mut (impl tokio::io::AsyncBufRead + Unpin),
        busy_set: &[Diagnostic],
        uri: &Uri,
        last: &str,
    ) -> Vec<(String, String)> {
        let mut received = Vec::new();
        loop {
            let message = recv_frame(reader).await;
            assert_eq!(message["method"], "textDocument/publishDiagnostics");
            let params: tower_lsp_server::ls_types::PublishDiagnosticsParams =
                serde_json::from_value(message["params"].clone()).unwrap();
            let text = if params.diagnostics.len() == busy_set.len() {
                assert_eq!(
                    params.diagnostics, busy_set,
                    "the taken payload lands whole"
                );
                "busy".to_owned()
            } else {
                params
                    .diagnostics
                    .iter()
                    .map(|d| d.message.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let done = params.uri == *uri && text == last;
            received.push((params.uri.as_str().to_owned(), text));
            if done {
                return received;
            }
        }
    }

    /// Cancellation, supersession and close/reopen against the real transport
    /// writer while the editor is not reading.
    ///
    /// A large publication is in the middle of being written into a full pipe —
    /// the one payload the writer has taken. Behind it, one document's
    /// publication is superseded by an edit and another document is closed,
    /// with nothing newer offered for either, and neither cancelled publisher is
    /// polled again before the editor resumes. The editor then receives the
    /// taken payload whole and the publication queued after the cancelled ones —
    /// never the superseded set, never a set for the closed document — and,
    /// once the document reopens, only the current publications.
    ///
    /// Discriminating: a writer that takes whatever is queued and relies on each
    /// publisher to withdraw its own payload writes `stale` and `closed-stale`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stalled_transport_never_writes_a_cancelled_publication() {
        use futures_util::FutureExt as _;
        use serde_json::json;

        let outbound = crate::outbound::Outbound::default();
        let documents = registry_on(&outbound);
        let (service, _socket) = tower_lsp_server::LspService::new(|_| HandshakeOnly);
        let (editor_end, server_end) = tokio::io::duplex(2 * 1024);
        let (server_in, server_out) = tokio::io::split(server_end);
        let server = tokio::spawn(crate::outbound::serve(
            server_in,
            server_out,
            service,
            outbound.clone(),
        ));
        let (editor_in, mut editor_out) = tokio::io::split(editor_end);
        let mut editor_in = tokio::io::BufReader::new(editor_in);
        send_frame(
            &mut editor_out,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}),
        )
        .await;
        assert_eq!(recv_frame(&mut editor_in).await["id"], 1);

        let busy: Uri = "file:///workspace/Busy.vue".parse().unwrap();
        let edited: Uri = "file:///workspace/Edited.vue".parse().unwrap();
        let closed: Uri = "file:///workspace/Closed.vue".parse().unwrap();
        let sentinel: Uri = "file:///workspace/Sentinel.vue".parse().unwrap();
        for uri in [&busy, &edited, &closed, &sentinel] {
            open(&documents, uri);
        }

        // Far larger than the pipe: the writer takes it first and stays parked
        // in the middle of writing it.
        let busy_set: Vec<Diagnostic> = (0..200)
            .flat_map(|i| diagnostic(&format!("busy-{i}-{}", "x".repeat(64))))
            .collect();
        let busy_publication = documents.begin_diagnostics_publication(&busy).unwrap();
        let busy_send =
            documents.publish_diagnostics(&busy, &busy_publication, busy_set.clone(), true, None);
        tokio::pin!(busy_send);
        assert!(futures_util::poll!(&mut busy_send).is_pending());

        let stale = documents.begin_diagnostics_publication(&edited).unwrap();
        let stale_send =
            documents.publish_diagnostics(&edited, &stale, diagnostic("stale"), true, None);
        tokio::pin!(stale_send);
        assert!(futures_util::poll!(&mut stale_send).is_pending());
        let closed_stale = documents.begin_diagnostics_publication(&closed).unwrap();
        let closed_stale_send = documents.publish_diagnostics(
            &closed,
            &closed_stale,
            diagnostic("closed-stale"),
            true,
            None,
        );
        tokio::pin!(closed_stale_send);
        assert!(futures_util::poll!(&mut closed_stale_send).is_pending());

        // An edit supersedes one publication and the other document closes,
        // with nothing newer offered for either yet. A later publication queues
        // behind both and marks where the writer has reached.
        let fresh = documents.begin_diagnostics_publication(&edited).unwrap();
        documents.did_close(&closed);
        let marker = documents.begin_diagnostics_publication(&sentinel).unwrap();
        let marker_send =
            documents.publish_diagnostics(&sentinel, &marker, diagnostic("sentinel"), true, None);
        tokio::pin!(marker_send);
        assert!(futures_util::poll!(&mut marker_send).is_pending());

        let received = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            read_diagnostics_until(&mut editor_in, &busy_set, &sentinel, "sentinel"),
        )
        .await
        .expect("the resumed editor reaches the later publication");
        assert_eq!(
            received,
            vec![
                (busy.as_str().to_owned(), "busy".to_owned()),
                (sentinel.as_str().to_owned(), "sentinel".to_owned()),
            ],
            "the payload being written finishes; a cancelled publication is never written"
        );

        // The document reopens, and both documents publish their current sets.
        open(&documents, &closed);
        let reopened = documents.begin_diagnostics_publication(&closed).unwrap();
        let fresh_send =
            documents.publish_diagnostics(&edited, &fresh, diagnostic("fresh"), true, None);
        let reopened_send =
            documents.publish_diagnostics(&closed, &reopened, diagnostic("reopened"), true, None);
        let (received, (), ()) = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            tokio::join!(
                async {
                    let mut received =
                        read_diagnostics_until(&mut editor_in, &busy_set, &edited, "fresh").await;
                    if !received.contains(&(closed.as_str().to_owned(), "reopened".to_owned())) {
                        received.extend(
                            read_diagnostics_until(&mut editor_in, &busy_set, &closed, "reopened")
                                .await,
                        );
                    }
                    received
                },
                fresh_send,
                reopened_send
            )
        })
        .await
        .expect("the current publications reach the editor");
        received.iter().for_each(|(_, text)| {
            assert!(
                text == "fresh" || text == "reopened",
                "only current publications follow: {received:?}"
            )
        });

        // The cancelled publishers resume only now, and own nothing.
        stale_send.await;
        closed_stale_send.await;
        assert!(busy_send.as_mut().now_or_never().is_some());
        assert!(marker_send.as_mut().now_or_never().is_some());
        for uri in [&busy, &edited, &closed, &sentinel] {
            assert!(documents.diagnostics_ready(uri), "{}", uri.as_str());
        }
        assert_eq!(
            outbound.load().replaceable.retained(),
            Default::default(),
            "nothing is retained once the editor has read everything"
        );

        send_frame(&mut editor_out, json!({"jsonrpc": "2.0", "method": "exit"})).await;
        tokio::time::timeout(std::time::Duration::from_secs(30), server)
            .await
            .expect("the server exits")
            .unwrap();
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
                let basis = ReadinessBasis::capture(&documents, &uri).unwrap();
                // Suspend the old capture while a later edit and publication own the file.
                assert!(
                    documents
                        .did_change(&uri, 2, "<template><p>after</p></template>")
                        .changed
                );
                newer = documents.begin_diagnostics_publication(&uri);
                Some(basis)
            })
            .unwrap();
        assert!(!documents.diagnostic_publication_is_current(&uri, &older));
        assert!(
            documents.diagnostic_publication_is_current(&uri, &newer.unwrap()),
            "resuming the old capture must preserve the newer publication's ownership"
        );
    }
}
