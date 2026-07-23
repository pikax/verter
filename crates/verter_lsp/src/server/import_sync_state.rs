use super::*;

#[derive(Clone, Copy)]
struct DeliveredImportReceipt {
    key: (u64, u64),
    /// Exact proof that this document has no import/module-reference roots, so
    /// another document's edit cannot change its publication closure.
    rootless: bool,
}

#[derive(Default)]
pub(crate) struct ImportSyncMemo {
    fresh_at: DashMap<String, DeliveredImportReceipt>,
    locks: DashMap<String, Arc<tokio::sync::Mutex<()>>>,
    /// In-flight background publication per document: a watch receiver that
    /// resolves (value change or sender drop) when the publication settles.
    /// Registered by the publisher AFTER it holds the singleflight lock, so at
    /// most one live entry exists per document.
    in_flight: DashMap<String, tokio::sync::watch::Receiver<bool>>,
    /// Edit-debounce epochs: a debounced enqueue bumps its document's epoch and
    /// only the LATEST enqueue survives its debounce sleep, so a typing burst
    /// coalesces onto one publication after the silence window.
    enqueue_epochs: DashMap<String, u64>,
    epoch_counter: std::sync::atomic::AtomicU64,
}

impl ImportSyncMemo {
    /// The per-document singleflight lock. A tokio `Mutex` is fair (FIFO) and
    /// cancel-safe, so a request storm cannot starve or wedge the pass.
    pub(crate) fn lock_for(&self, canonical_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .entry(canonical_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Whether this document's import set was already delivered at `key`.
    pub(crate) fn is_fresh_at(&self, canonical_id: &str, key: (u64, u64)) -> bool {
        self.fresh_at.get(canonical_id).map(|entry| entry.key) == Some(key)
    }

    #[cfg(test)]
    pub(crate) fn record_delivered(&self, canonical_id: String, key: (u64, u64)) {
        self.record_delivered_with_rootless(canonical_id, key, false);
    }

    pub(crate) fn record_delivered_with_rootless(
        &self,
        canonical_id: String,
        key: (u64, u64),
        rootless: bool,
    ) {
        self.fresh_at
            .insert(canonical_id, DeliveredImportReceipt { key, rootless });
    }

    /// Advance every receipt provably unaffected by one isolated document edit.
    /// The edited document needs an unchanged frontier proof; another document
    /// advances only when its delivered closure is exactly rootless. Imported
    /// documents remain cold until publication revalidates their graph.
    pub(crate) fn promote_after_isolated_edit(
        &self,
        edited_canonical_id: &str,
        from: (u64, u64),
        to: (u64, u64),
        edited_frontier_unchanged: bool,
    ) {
        for mut entry in self.fresh_at.iter_mut() {
            if entry.key != from {
                continue;
            }
            let unaffected = if entry.key() == edited_canonical_id {
                edited_frontier_unchanged
            } else {
                entry.rootless
            };
            if unaffected {
                entry.key = to;
            }
        }
    }

    /// Register this document's publication as in flight and return the guard
    /// that (a) resolves every joiner and (b) removes the registration when the
    /// publication settles — including on panic, so a dead entry can never
    /// strand joiners. Call ONLY while holding [`Self::lock_for`]'s lock.
    pub(crate) fn begin_in_flight(self: &Arc<Self>, canonical_id: &str) -> InFlightPublication {
        let (sender, receiver) = tokio::sync::watch::channel(false);
        self.in_flight.insert(canonical_id.to_string(), receiver);
        InFlightPublication {
            memo: Arc::clone(self),
            canonical_id: canonical_id.to_string(),
            sender,
        }
    }

    /// The in-flight publication watch for `canonical_id`, if one is running.
    /// A joiner awaits `changed()` on the clone (value change or sender drop
    /// both resolve it) and then re-reads the receipt — it never starts work.
    pub(crate) fn in_flight_watch(
        &self,
        canonical_id: &str,
    ) -> Option<tokio::sync::watch::Receiver<bool>> {
        self.in_flight.get(canonical_id).map(|entry| entry.clone())
    }

    /// Bump and return the edit-debounce epoch for `canonical_id`. A debounced
    /// enqueue captures the returned value and abandons itself after its sleep
    /// when a newer enqueue has bumped past it.
    pub(crate) fn bump_enqueue_epoch(&self, canonical_id: &str) -> u64 {
        let epoch = self
            .epoch_counter
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1;
        self.enqueue_epochs.insert(canonical_id.to_string(), epoch);
        epoch
    }

    /// Whether `epoch` is still the newest enqueue for `canonical_id`.
    pub(crate) fn enqueue_epoch_is_current(&self, canonical_id: &str, epoch: u64) -> bool {
        self.enqueue_epochs.get(canonical_id).map(|entry| *entry) == Some(epoch)
    }

    /// Drop every entry. Called whenever the workspace is replaced, because the
    /// generations the keys are built from belong to the OLD workspace.
    pub(crate) fn evict_all(&self) {
        self.fresh_at.clear();
        self.locks.clear();
        self.in_flight.clear();
        self.enqueue_epochs.clear();
    }

    /// Number of documents currently recorded as delivered (test observation).
    #[cfg(test)]
    pub(crate) fn recorded_len(&self) -> usize {
        self.fresh_at.len()
    }
}

/// RAII registration of an in-flight background publication. Dropping it —
/// normal completion or panic — removes the in-flight entry and resolves every
/// joiner's watch. The receipt (if any) must be recorded BEFORE this drops so a
/// woken joiner re-reads a settled memo.
pub(crate) struct InFlightPublication {
    memo: Arc<ImportSyncMemo>,
    canonical_id: String,
    sender: tokio::sync::watch::Sender<bool>,
}

impl Drop for InFlightPublication {
    fn drop(&mut self) {
        self.memo
            .in_flight
            .remove_if(&self.canonical_id, |_, current| {
                current.same_channel(&self.sender.subscribe())
            });
        // Wake joiners AFTER the registry removal so a woken joiner that
        // re-checks sees either the fresh receipt or a clean Missing state.
        let _ = self.sender.send(true);
    }
}
