use super::*;

mod delivered_receipts {
    use super::*;

    /// The change signal receipt mutations wake freshness waiters on.
    ///
    /// A change bumps a version; a waiter subscribes ONCE, and the
    /// subscription remembers the version it was taken at. A change that lands
    /// after a waiter subscribed is therefore delivered even if the waiter has
    /// not reached its await yet — which is the whole point, because a waiter
    /// reads receipt state between the two. The channel is private to this
    /// module and no accessor hands out a subscription, so
    /// [`ChangeSignal::wait_until`] is the only place that ordering exists and
    /// no caller can subscribe after its own state read. Inside `wait_until`
    /// the ordering is held by a test that reads state and then mutates it
    /// before the await, which a subscription taken after the read it then
    /// awaits on misses.
    #[cfg(test)]
    mod change_signal {
        pub(super) struct ChangeSignal(tokio::sync::watch::Sender<u64>);

        impl Default for ChangeSignal {
            fn default() -> Self {
                Self(tokio::sync::watch::channel(0).0)
            }
        }

        impl ChangeSignal {
            /// Record one change. `send_modify` does not require a live
            /// receiver, so a signal nobody is waiting on is a plain bump.
            pub(super) fn notify_changed(&self) {
                self.0
                    .send_modify(|version| *version = version.wrapping_add(1));
            }

            /// Await until `is_ready` holds.
            ///
            /// The subscription is taken BEFORE any `is_ready` read, so a
            /// change racing a read — landing after it observed stale state
            /// and before the await — advances a version the subscription has
            /// not seen, and resolves the await instead of being lost.
            pub(super) async fn wait_until(&self, mut is_ready: impl FnMut() -> bool) {
                let mut changes = self.0.subscribe();
                loop {
                    if is_ready() {
                        return;
                    }
                    changes
                        .changed()
                        .await
                        .expect("the signal owns the sender for as long as a waiter borrows it");
                }
            }
        }
    }

    #[derive(Clone, Copy)]
    struct DeliveredImportReceipt {
        key: (u64, u64),
        /// Exact proof that this document has no import/module-reference roots,
        /// so another document's edit cannot change its publication closure.
        rootless: bool,
    }

    /// Closed mutation vocabulary for delivered import receipts. The storage
    /// is private to this child module, so every state-changing path must pass
    /// through [`DeliveredReceipts::apply`].
    pub(super) enum Mutation<'a> {
        Record {
            canonical_id: String,
            key: (u64, u64),
            rootless: bool,
        },
        PromoteAfterIsolatedEdit {
            edited_canonical_id: &'a str,
            from: (u64, u64),
            to: (u64, u64),
            edited_frontier_unchanged: bool,
        },
        EvictAll,
    }

    #[derive(Default)]
    pub(super) struct DeliveredReceipts {
        entries: DashMap<String, DeliveredImportReceipt>,
        /// Test receipt for every mutation that can satisfy a freshness waiter.
        #[cfg(test)]
        changed: change_signal::ChangeSignal,
    }

    impl DeliveredReceipts {
        pub(super) fn is_fresh_at(&self, canonical_id: &str, key: (u64, u64)) -> bool {
            self.entries.get(canonical_id).map(|entry| entry.key) == Some(key)
        }

        /// The only mutation gateway for delivered receipts. A promotion that
        /// changes at least one exact key wakes freshness waiters in the same
        /// operation, so a caller cannot perform the mutation and omit the
        /// corresponding wake.
        pub(super) fn apply(&self, mutation: Mutation<'_>) {
            let changed = match mutation {
                Mutation::Record {
                    canonical_id,
                    key,
                    rootless,
                } => {
                    self.entries
                        .insert(canonical_id, DeliveredImportReceipt { key, rootless });
                    true
                }
                Mutation::PromoteAfterIsolatedEdit {
                    edited_canonical_id,
                    from,
                    to,
                    edited_frontier_unchanged,
                } => {
                    let mut changed = false;
                    for mut entry in self.entries.iter_mut() {
                        let receipt = *entry.value();
                        if receipt.key != from {
                            continue;
                        }
                        let unaffected = if entry.key() == edited_canonical_id {
                            edited_frontier_unchanged
                        } else {
                            receipt.rootless
                        };
                        if unaffected && receipt.key != to {
                            entry.value_mut().key = to;
                            changed = true;
                        }
                    }
                    changed
                }
                Mutation::EvictAll => {
                    self.entries.clear();
                    true
                }
            };

            #[cfg(test)]
            if changed {
                self.changed.notify_changed();
            }
            #[cfg(not(test))]
            let _ = changed;
        }

        #[cfg(test)]
        pub(super) fn len(&self) -> usize {
            self.entries.len()
        }

        #[cfg(test)]
        pub(super) async fn wait_fresh_at(&self, canonical_id: &str, key: (u64, u64)) {
            self.changed
                .wait_until(|| self.is_fresh_at(canonical_id, key))
                .await;
        }

        /// Runs `after_first_read` immediately after the waiter's first
        /// exact-key read, while that read's stale answer is still what the
        /// waiter is about to act on. A mutation performed there is the
        /// lost-wake case in its exact shape: the waiter has already read, so
        /// only a subscription taken before the read can carry the change.
        #[cfg(test)]
        pub(super) async fn wait_fresh_at_with_read_probe(
            &self,
            canonical_id: &str,
            key: (u64, u64),
            after_first_read: impl FnOnce(),
        ) {
            let mut after_first_read = Some(after_first_read);
            self.changed
                .wait_until(|| {
                    let fresh = self.is_fresh_at(canonical_id, key);
                    if let Some(probe) = after_first_read.take() {
                        probe();
                    }
                    fresh
                })
                .await;
        }
    }
}

use delivered_receipts::{DeliveredReceipts, Mutation as ReceiptMutation};

#[derive(Default)]
pub(crate) struct ImportSyncMemo {
    delivered: DeliveredReceipts,
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
        self.delivered.is_fresh_at(canonical_id, key)
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
        self.delivered.apply(ReceiptMutation::Record {
            canonical_id,
            key,
            rootless,
        });
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
        self.delivered
            .apply(ReceiptMutation::PromoteAfterIsolatedEdit {
                edited_canonical_id,
                from,
                to,
                edited_frontier_unchanged,
            });
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
        self.delivered.apply(ReceiptMutation::EvictAll);
        self.locks.clear();
        self.in_flight.clear();
        self.enqueue_epochs.clear();
    }

    /// Number of documents currently recorded as delivered (test observation).
    #[cfg(test)]
    pub(crate) fn recorded_len(&self) -> usize {
        self.delivered.len()
    }

    /// Wait until `canonical_id` has a receipt at the exact requested key.
    /// A latch at a different key does not resolve this wait.
    #[cfg(test)]
    pub(crate) async fn wait_fresh_at(&self, canonical_id: &str, key: (u64, u64)) {
        self.delivered.wait_fresh_at(canonical_id, key).await;
    }

    /// Test seam that runs `after_first_read` immediately after the waiter's
    /// first exact-key read, while the stale answer is still what the waiter is
    /// about to act on. A mutation performed there is delivered only because
    /// the waiter subscribed before that read.
    #[cfg(test)]
    async fn wait_fresh_at_with_read_probe(
        &self,
        canonical_id: &str,
        key: (u64, u64),
        after_first_read: impl FnOnce(),
    ) {
        self.delivered
            .wait_fresh_at_with_read_probe(canonical_id, key, after_first_read)
            .await;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The promotion lands in the waiter's lost-wake window: the waiter has
    /// read the stale key, and the promotion runs before that read's answer is
    /// acted on. Only a subscription taken before the read carries the change,
    /// so a waiter that subscribes after the read it then awaits on — the
    /// classic check-then-subscribe race — never completes here.
    ///
    /// The deadline is VIRTUAL time. A correct waiter resolves within the very
    /// first poll of the timeout and never parks the runtime, so the clock
    /// cannot advance under it; a waiter that missed the wake parks the runtime
    /// with the deadline as the only pending timer, which auto-advances and
    /// fails the test at once instead of hanging until an outer killer.
    #[tokio::test(start_paused = true)]
    async fn isolated_edit_promotion_after_the_key_read_wakes_a_subscribed_waiter() {
        let memo = ImportSyncMemo::default();
        let canonical = "/workspace/Comp.vue".to_string();
        let from = (7, 11);
        let to = (8, 12);
        memo.record_delivered_with_rootless(canonical.clone(), from, true);

        let promote_after_read = || memo.promote_after_isolated_edit(&canonical, from, to, true);
        tokio::time::timeout(
            std::time::Duration::from_secs(60),
            memo.wait_fresh_at_with_read_probe(&canonical, to, promote_after_read),
        )
        .await
        .expect("a promotion landing right after the key read must wake the subscribed waiter");

        assert!(memo.is_fresh_at(&canonical, to));
        assert!(!memo.is_fresh_at(&canonical, from));
    }
}
