//! Proactive transitive declaration-overlay graph for the tgo resolution path.
//!
//! tgo resolves a bare framework-carrier import (`import B from "./B.vue"`) to
//! the virtual `B.d.<ext>.ts` declaration via its native basename-append probe,
//! but it has NO module-resolution hook — so every declaration an importing
//! carrier (transitively) needs must already be OPEN as an overlay when that
//! carrier is type-checked, or the import fails with TS2307.
//!
//! [`DeclOverlayOwner`] is the SOLE lifecycle authority for those `.d.<ext>.ts`
//! overlays. It owns, behind a private surface:
//!
//!   * the reachability graph — each declaration companion path (`B.d.vue.ts`) →
//!     the set of OPEN carrier roots whose transitive declaration closure reaches
//!     it, FOLDED together with that overlay's close-lifecycle generation into one
//!     [`DeclOverlaySlot`] so `(generation, roots)` share ONE critical section; and
//!   * a per-declaration-path async serialization lock so the OPEN and CLOSE of a
//!     single overlay never interleave across threads.
//!
//! An overlay stays open while ANY open root reaches it; it is closed only when
//! the last reaching root closes (its set drains to empty). The owner is the only
//! place that issues a provider `close_dts` for a declaration overlay (the
//! `did_close` lifecycle and the background closure pass both route through it), so
//! there is no second, unguarded Decl-close path.
//!
//! Split out of `background_drain` (a sibling `#[path]` child module of `server`);
//! both share `use super::*;` so the same `super::` / `crate::` paths resolve, and
//! the closure reuses `background_drain`'s carrier helpers
//! (`configure_provider_paths_for_source`, `commit_sync_transition`, etc.).
//!
//! ## Lifecycle serialization & lock discipline
//!
//! The provider OPEN/CLOSE of an overlay lands its EFFECT at the tokio
//! await-completion point, not at call entry — so a close decided from stale state
//! could otherwise clobber a concurrent open of the same overlay (stranding an
//! open root's bare carrier import on TS2307), or a re-open could resurrect an
//! overlay no live root reaches (a leak with no future closer). Both are closed by
//! serializing every open and close of ONE declaration path behind that path's
//! [`tokio::sync::Mutex`]:
//!
//!   * an OPEN acquires the path lock, REVALIDATES the reaching root is still open,
//!     records `{decl -> root}` + bumps the generation in one slot critical
//!     section, then issues the provider open WHILE HOLDING the path lock;
//!   * a CLOSE acquires the SAME path lock, re-checks under the slot lock that the
//!     reaching set is still empty AND the generation has not advanced past the
//!     value observed when the close was decided, issues the provider close, and on
//!     success GCs the slot only if it is still empty-and-unchanged.
//!
//! A close in flight therefore blocks a concurrent open of the same path until it
//! completes; the open then revalidates against the now-current state instead of
//! racing it. The final provider state follows serialized lifecycle order, not a
//! compensate-after-the-fact repair.
//!
//! Strict lock discipline (deadlock avoidance): at most ONE declaration-path mutex
//! is held at a time; a DashMap entry guard is NEVER held across an `.await` (the
//! `Arc<Mutex<_>>` is cloned out and the DashMap guard dropped BEFORE awaiting the
//! mutex, and every slot read/write is a synchronous critical section that ends
//! before any provider await); a path mutex is never acquired while a slot guard is
//! held. A slow provider open/close blocks ONLY that one declaration path.
//!
//! ## Cancellation invariant
//!
//! Each lifecycle mutation is paired with its provider await: an open RECORDS
//! `{decl -> root}` + bumps the generation and then MUST run its `open_dts`/
//! `sync_dts` to completion; a close drains the slot to a tombstone and then MUST
//! run its `close_dts` to completion before the slot is GC'd. The owner relies on
//! its enclosing task running each mutation+await to completion: the closure pass
//! is spawned (`tokio::spawn`) with no stored `JoinHandle`, no `.abort()`, no
//! `select!`, and no `CancellationToken`, so its future is never dropped between a
//! state mutation and the matching provider await. A best-effort safety net backs
//! this up: a failed/skipped open is re-attempted by the next pass (the recorded
//! root keeps the overlay scheduled for re-open), and a close that FAILS (provider
//! returned an error) leaves a surviving empty tombstone with its in-flight mark
//! cleared, which [`DeclOverlayOwner::reconcile_open_roots`] re-returns so a later
//! sweep re-issues the close. A close that is merely IN-FLIGHT carries a
//! `close_pending` mark so the sweep does NOT re-issue against the path lock it
//! holds.
//!
//! If a future change EVER makes this task externally cancellable (a `select!`, an
//! `.abort()`, or a `CancellationToken` over the closure pass), it MUST add an
//! explicit rollback/sweep so a future dropped between a mutation and its provider
//! await cannot leave the owner and the provider disagreeing — an open-side orphan
//! (recorded root, provider never opened) reconciled away or re-opened, and a
//! close-side orphan (slot drained, provider never closed) re-closed.

use super::*;
use std::sync::Arc;
use tokio::sync::Mutex;

/// One declaration overlay's lifecycle state: its reaching-root set FOLDED with its
/// close-lifecycle generation, so both are read and written inside ONE
/// `slots.entry(path)` critical section.
///
/// The generation is the ABA defense the bare reaching-root set cannot provide on
/// its own: a slot's reaching-root set can transiently drain to empty and refill (a
/// different root closes and another opens) while a stale close decision is
/// in-flight; the generation strictly advances on every open, so a close decision
/// captured at generation `G` is recognised as STALE the moment any open bumps it
/// past `G` — even if the set looks empty again at the close-time gate. Because both
/// live in one value behind one entry lock, a reader can never observe a
/// half-applied `(generation, roots)` pair.
///
/// An emptied slot is kept as a generation TOMBSTONE until the provider close
/// confirms; the slot is GC'd only when it is still empty AND its generation is
/// unchanged from the close decision, so a re-open that lands during a close is
/// never lost to a premature slot removal.
#[derive(Debug, Default, Clone)]
struct DeclOverlaySlot {
    /// Strictly-advancing close-lifecycle generation; bumped on every open.
    generation: u64,
    /// The OPEN carrier root canonicals whose transitive closure reaches this
    /// overlay. The overlay is closed once this drains to empty.
    roots: HashSet<String>,
    /// `true` while a guarded close for THIS overlay is committed and awaiting its
    /// provider `close_dts`. Set under the slot lock the instant the close passes
    /// the supersession gate (before the provider await), cleared when that close
    /// completes (a success GCs the slot; a failure clears the flag and keeps the
    /// tombstone). It distinguishes an empty tombstone whose close is IN-FLIGHT
    /// (recovery must NOT re-issue — a redundant close would contend on the
    /// in-flight close's held path lock) from one whose close already finished
    /// UNCONFIRMED (recovery MUST re-issue), so `reconcile_open_roots` re-returns
    /// only the latter.
    close_pending: bool,
}

/// A declaration overlay to close, paired with the close GENERATION observed at the
/// moment the close was DECIDED (the reconcile/release that drained the slot, read
/// under that slot's held entry lock). The guarded close re-checks this against the
/// live generation before issuing the destructive provider close: a generation that
/// has advanced means an open landed since the decision, so the close is superseded.
///
/// Owner-internal: produced by the slot-surgery methods and consumed by
/// [`DeclOverlayOwner::guarded_close`]; never a public mutation surface.
pub(crate) type DeclCloseTarget = (String, u64);

/// THE declaration-overlay lifecycle owner — the SOLE authority for the proactive
/// `.d.<ext>.ts` overlay graph and the only code that issues a provider `close_dts`
/// for a declaration overlay.
///
/// State is PRIVATE: external code (the `did_close` lifecycle, the background
/// closure pass, the provider-state close dispatch) drives the graph exclusively
/// through the owner's methods (open / release / reconcile / guarded close), never
/// by mutating a raw map. The server holds one `Arc<DeclOverlayOwner>` shared across
/// the closure pass and the `did_close` lifecycle.
#[derive(Debug, Default)]
pub(crate) struct DeclOverlayOwner {
    /// Per-overlay lifecycle state (reaching roots folded with the close
    /// generation). The entry lock for a path serializes all `(generation, roots)`
    /// reads/writes for that path.
    slots: DashMap<String, DeclOverlaySlot>,
    /// Per-declaration-path async serialization lock. Cloned out (and the DashMap
    /// guard dropped) before awaiting, so a provider open/close of one path blocks
    /// only that path. Kept for any path ever seen (the value is cheap and GCing it
    /// is not waiter-safe) — only the slot VALUE is GC'd.
    locks: DashMap<String, Arc<Mutex<()>>>,
    /// Test-only contention probe: a one-shot per-path [`tokio::sync::Notify`] fired
    /// the instant an [`Self::open_overlay`] for that exact path finds the path's
    /// serialization lock already HELD (a concurrent close/open holds the strand) and
    /// is about to await it. Armed by [`Self::signal_open_lock_contended_for_test`].
    /// `#[cfg(test)]`-gated, so production carries no field and no probe overhead.
    #[cfg(test)]
    open_lock_contention_signals: DashMap<String, Arc<tokio::sync::Notify>>,
}

impl DeclOverlayOwner {
    /// Clone out (creating if absent) the per-path serialization lock for
    /// `decl_path`, dropping the `locks` DashMap guard before returning so the
    /// caller awaits the mutex without holding a shard guard (lock discipline).
    fn path_lock(&self, decl_path: &str) -> Arc<Mutex<()>> {
        Arc::clone(
            self.locks
                .entry(decl_path.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .value(),
        )
    }

    /// Drop an open root from every declaration-overlay reaching set, returning the
    /// overlays that are NO LONGER reachable from any open root (their set drained
    /// to empty) — the overlays the caller must now close through [`Self::guarded_close`].
    ///
    /// Pure slot surgery, no provider I/O. An overlay still reachable from a
    /// DIFFERENT open root is retained (NOT returned) — closing it would strand that
    /// other root's bare carrier imports on TS2307. Each returned target carries the
    /// generation read under the same held entry lock that removed the root (so the
    /// close decision's generation baseline is consistent with the roots it saw).
    /// An emptied slot is kept as a TOMBSTONE (its generation preserved) rather than
    /// removed, so a re-open racing the pending close cannot be lost to a premature
    /// slot removal; the slot is GC'd by the guarded close once the provider close
    /// confirms it is still empty-and-unchanged.
    pub(crate) fn release_root(&self, root_canonical: &str) -> Vec<DeclCloseTarget> {
        let mut now_unreferenced = Vec::new();
        // Snapshot the keys so we don't hold shard guards across the loop; the
        // per-key mutate+check below is a SINGLE synchronous critical section.
        let decl_paths: Vec<String> = self.slots.iter().map(|entry| entry.key().clone()).collect();
        for decl_path in decl_paths {
            if let dashmap::mapref::entry::Entry::Occupied(mut occupied) =
                self.slots.entry(decl_path.clone())
            {
                let slot = occupied.get_mut();
                let removed = slot.roots.remove(root_canonical);
                // Return the overlay only when THIS release actually drained it —
                // `removed && empty`. An already-empty tombstone (a prior drain whose
                // close has not yet confirmed) is NOT re-returned, so a no-op release
                // of an unrelated root never re-issues a close for it. Keep the
                // emptied slot as a generation TOMBSTONE; capture the generation under
                // this same held entry lock as the close baseline. The guarded close
                // GCs the slot on a confirmed close.
                if removed && slot.roots.is_empty() {
                    now_unreferenced.push((decl_path, slot.generation));
                }
            }
        }
        now_unreferenced
    }

    /// Make a single root's reachability edges EQUAL its CURRENT closure: add an
    /// edge for every `current_decl_paths` entry, and DROP the root from every other
    /// overlay it previously reached but no longer does. Returns the overlays whose
    /// reaching set drained to empty (the caller must close them).
    ///
    /// This keeps the graph from being append-only: when a root stops importing a
    /// carrier, the dropped overlay loses this root and closes if no other open root
    /// reaches it. An overlay still reached by a DIFFERENT root is retained (NOT
    /// returned). Pure slot surgery — no provider I/O.
    pub(crate) fn reconcile_root_reachability(
        &self,
        root_canonical: &str,
        current_decl_paths: &[String],
    ) -> Vec<DeclCloseTarget> {
        let current: HashSet<&str> = current_decl_paths.iter().map(String::as_str).collect();

        // 1) Add this root to every overlay in its current closure.
        for decl_path in current_decl_paths {
            self.slots
                .entry(decl_path.clone())
                .or_default()
                .roots
                .insert(root_canonical.to_string());
        }

        // 2) Drop this root from every overlay NOT in its current closure.
        let mut now_unreferenced = Vec::new();
        let all_paths: Vec<String> = self.slots.iter().map(|entry| entry.key().clone()).collect();
        for decl_path in all_paths {
            if current.contains(decl_path.as_str()) {
                continue; // still reached by this root — keep the edge
            }
            if let dashmap::mapref::entry::Entry::Occupied(mut occupied) =
                self.slots.entry(decl_path.clone())
            {
                let slot = occupied.get_mut();
                let removed = slot.roots.remove(root_canonical);
                if removed && slot.roots.is_empty() {
                    now_unreferenced.push((decl_path, slot.generation));
                }
            }
        }
        now_unreferenced
    }

    /// Drop EVERY root no longer in the live open-root set from every overlay,
    /// returning EVERY overlay whose reaching set is now empty — both the overlays
    /// THIS reconcile just drained AND any surviving empty tombstone whose provider
    /// close never confirmed.
    ///
    /// Two jobs, one sweep:
    ///   * the race-closing reconcile — if a root closed between the closure snapshot
    ///     and a per-overlay record, its late record may have re-added the now-closed
    ///     root; reconciling against the LIVE open-root set drops it again, so a
    ///     closed root leaves NOTHING behind even when its close raced the pass; and
    ///   * close-side orphan RECOVERY — a confirmed close GCs its slot
    ///     ([`Self::guarded_close`] removes the entry on a confirmed close), so a
    ///     SURVIVING empty tombstone whose close is NOT in-flight means the provider
    ///     close already finished UNCONFIRMED (the close failed) and the provider
    ///     overlay is still open with no live root reaching it. Re-returning such a
    ///     tombstone gives that orphan a future closer; the re-issued
    ///     [`Self::guarded_close`] finally closes the provider overlay and GCs the
    ///     slot.
    ///
    /// An empty tombstone whose close is IN-FLIGHT (`close_pending`) is NOT
    /// re-returned: that close is committed and holds the overlay's path lock across
    /// its provider await, so a re-issued close would only block on that held lock
    /// (and, under a paused/gated close, deadlock the very pass that would release
    /// it). The in-flight close is the future closer; once it finishes it either GCs
    /// the slot (confirmed) or clears `close_pending` and keeps the tombstone
    /// (unconfirmed), and only then does a later sweep recover it.
    ///
    /// Pure slot surgery — no provider I/O.
    pub(crate) fn reconcile_open_roots(
        &self,
        live_roots: &HashSet<String>,
    ) -> Vec<DeclCloseTarget> {
        let mut now_unreferenced = Vec::new();
        let all_paths: Vec<String> = self.slots.iter().map(|entry| entry.key().clone()).collect();
        for decl_path in all_paths {
            if let dashmap::mapref::entry::Entry::Occupied(mut occupied) =
                self.slots.entry(decl_path.clone())
            {
                let slot = occupied.get_mut();
                slot.roots.retain(|root| live_roots.contains(root));
                // Return a currently-empty slot UNLESS a close for it is already
                // in-flight. Empty + not-pending is either an overlay this reconcile
                // just drained or a surviving tombstone whose earlier close finished
                // unconfirmed — both need a (re-)close. Empty + pending has a
                // committed close holding the path lock; re-issuing would only
                // contend on that lock, so leave it to the in-flight close.
                if slot.roots.is_empty() && !slot.close_pending {
                    now_unreferenced.push((decl_path, slot.generation));
                }
            }
        }
        now_unreferenced
    }

    /// Issue the guarded provider close for each drained declaration overlay — the
    /// SOLE path that issues a provider `close_dts` for a `.d.<ext>.ts` overlay.
    /// BOTH the `did_close`-side release and the closure-pass-side reconcile route
    /// their Decl closes here, so the supersession gate is applied uniformly.
    ///
    /// For each `(decl_path, gen_at_decision)`, under that path's serialization lock:
    ///   1. RE-CHECK under the slot entry lock: if the overlay's reaching set is
    ///      NON-EMPTY (a reference reappeared) OR its generation has ADVANCED past
    ///      `gen_at_decision` (an open landed since the close was decided), SKIP — the
    ///      close is superseded.
    ///   2. Strip the `Decl` kind from the owner carrier's committed provider state
    ///      and issue the provider `close_dts` WHILE HOLDING the path lock (so a
    ///      concurrent open of the same path waits behind it, then revalidates).
    ///   3. On success, GC the slot only if it is STILL empty AND its generation is
    ///      unchanged from `gen_at_decision` (otherwise an open landed — keep the
    ///      live slot). On failure, keep the tombstone (fail closed).
    ///
    /// The provider close needs only the carrier `sync` handle and the committed
    /// provider states — no resolver snapshot — because serialization replaces the
    /// old compensate-after-close re-open: a re-open simply waits behind this close
    /// and then runs against current state.
    pub(crate) async fn guarded_close(
        &self,
        sync: &ProjectSync,
        provider_sync_states: &DashMap<String, ProviderSyncState>,
        targets: &[DeclCloseTarget],
    ) {
        for (decl_path, gen_at_decision) in targets {
            let lock = self.path_lock(decl_path);
            let _path_guard = lock.lock().await;

            // 1. Supersession gate under the slot entry lock (synchronous; the guard
            //    is dropped before the provider await below). When NOT superseded,
            //    mark the slot's close IN-FLIGHT in the SAME critical section, so a
            //    concurrent `reconcile_open_roots` does not re-issue a redundant close
            //    that would only contend on the path lock this close is about to hold.
            let superseded = {
                match self.slots.entry(decl_path.clone()) {
                    dashmap::mapref::entry::Entry::Occupied(mut occupied) => {
                        let slot = occupied.get_mut();
                        let superseded =
                            !slot.roots.is_empty() || slot.generation != *gen_at_decision;
                        if !superseded {
                            slot.close_pending = true;
                        }
                        superseded
                    }
                    // No slot ⇒ never opened (or already GC'd at gen 0). Superseded
                    // only if the decision baseline was a real (non-zero) generation
                    // that has since been GC'd; a 0-baseline close of an absent slot
                    // is a no-op either way.
                    dashmap::mapref::entry::Entry::Vacant(_) => *gen_at_decision != 0,
                }
            };
            if superseded {
                continue;
            }

            // 2. Strip the Decl kind from owner state + issue the destructive close
            //    while holding the path lock.
            for mut entry in provider_sync_states.iter_mut() {
                if entry.decl_path.as_deref() == Some(decl_path.as_str()) {
                    entry.decl_path = None;
                    entry.set_background_loaded(ProviderPathKind::Decl, false);
                }
            }
            if let Err(error) = sync.close_dts(decl_path).await {
                tracing::warn!(
                    "declaration_closure: failed to close unreferenced declaration overlay \
                     {decl_path}: {error}"
                );
                // Fail closed: keep the tombstone slot, but clear the in-flight mark
                // so a later sweep can RECOVER this unconfirmed close (re-issue it).
                if let dashmap::mapref::entry::Entry::Occupied(mut occupied) =
                    self.slots.entry(decl_path.clone())
                {
                    occupied.get_mut().close_pending = false;
                }
                continue;
            }

            // 3. GC the slot only if still empty AND unchanged (a re-open that landed
            //    after the gate — only possible if it did NOT hold this path lock,
            //    which the serialization forbids — would leave the slot live). The
            //    re-check + removal is one synchronous critical section. If a re-open
            //    DID land (set non-empty or generation advanced), keep the live slot
            //    but clear the in-flight mark — the close is done, the slot is live.
            if let dashmap::mapref::entry::Entry::Occupied(mut occupied) =
                self.slots.entry(decl_path.clone())
            {
                if occupied.get().roots.is_empty() && occupied.get().generation == *gen_at_decision
                {
                    occupied.remove();
                } else {
                    occupied.get_mut().close_pending = false;
                }
            }
        }
    }

    /// Proactively open the transitive DECLARATION-overlay closure reachable from
    /// every OPEN carrier root, recording per-root reachability into the owner.
    ///
    /// For each open carrier root, a breadth-first walk follows the root's analyzed
    /// carrier IMPORT SET (the carrier subset of the script imports, resolved through
    /// the engine's workspace resolver — see [`carrier_dependency_ids`]; never a scan
    /// of the generated declaration text, per the typed-IR-only rule). The walk SEEDS
    /// each root as its own first node, so a root's OWN `.d.<ext>.ts` is opened too:
    /// every open carrier emits its own declaration, independent of whether any OTHER
    /// open file imports it. A per-root VISITED set bounds the walk so an import cycle
    /// (`A → B → A`) terminates.
    ///
    /// Each pass RECONCILES the graph rather than only appending:
    ///   * per root, the reachability edges are made to EQUAL the root's CURRENT
    ///     closure — a carrier the root no longer imports loses this root, and the
    ///     overlay is CLOSED if no other open root still reaches it;
    ///   * after every root, the graph is reconciled against the open-root set
    ///     RE-READ after the async closure work (NOT the start-of-pass snapshot), so a
    ///     root that closed mid-pass (racing its `did_close` release) leaves NOTHING
    ///     behind.
    ///
    /// Returns `true` if any declaration overlay was opened or updated.
    pub(crate) async fn open_declaration_closure_for_open_files(
        &self,
        sync: &ProjectSync,
        documents: &DocumentRegistry,
        provider_sync_states: &DashMap<String, ProviderSyncState>,
        snapshot: &super::PublishedResolverSnapshot,
    ) -> bool {
        let host = documents.host();
        let mut synced_any = false;

        // Snapshot the CURRENTLY-open carrier roots: every open URI whose canonical
        // id is a framework carrier. The start-of-pass value seeds the closure walk;
        // the SAME computation is re-run just before the final reconcile (and as the
        // per-open revalidation), so the graph validates against the open set as it
        // is AFTER the async closure work, not a stale snapshot.
        let collect_open_carrier_roots = || {
            let mut roots: HashSet<String> = HashSet::new();
            for uri_str in documents.open_uris() {
                let Ok(uri) = uri_str.parse::<Uri>() else {
                    continue;
                };
                let Some(canonical_id) = documents.get_canonical_id(&uri) else {
                    continue;
                };
                if carrier_language_for(&canonical_id).is_some() {
                    roots.insert(canonical_id);
                }
            }
            roots
        };

        let live_roots = collect_open_carrier_roots();

        for root in &live_roots {
            // Per-root BFS over the transitive carrier-dependency graph. The visited
            // set bounds cycles; the worklist is the frontier. SEED with the root
            // itself so the root's OWN declaration overlay is opened.
            let mut visited: HashSet<String> = HashSet::new();
            let mut reached_decl_paths: Vec<String> = Vec::new();
            let mut frontier: Vec<String> = vec![root.clone()];
            while let Some(node) = frontier.pop() {
                if !visited.insert(node.clone()) {
                    continue; // already walked this carrier under this root
                }

                if let Some(decl_path) = self
                    .open_overlay(sync, documents, provider_sync_states, snapshot, root, &node)
                    .await
                {
                    synced_any = true;
                    reached_decl_paths.push(decl_path);
                }

                for next in carrier_dependency_ids(host, &node) {
                    if !visited.contains(&next) {
                        frontier.push(next);
                    }
                }
            }

            // Reconcile this root's edges to EQUAL its current closure, then close
            // overlays drained to empty. Race-safe: if `root` closed mid-pass, its
            // `did_close` already released its edges; this re-records them, and the
            // post-loop reconcile against the FRESHLY re-read open set drops `root`
            // again iff it is no longer open.
            let now_unreferenced = self.reconcile_root_reachability(root, &reached_decl_paths);
            self.guarded_close(sync, provider_sync_states, &now_unreferenced)
                .await;
        }

        // Final reconcile: drop every root no longer in the live open-root set,
        // closing any overlay that drains empty. The reconcile MUST validate against
        // the CURRENT open-root set, re-read after the async closure work above — a
        // root that was open at the start but closed during this pass (its
        // `did_close` released its edges, which the per-root reconcile re-recorded)
        // is dropped here, so a closed root leaves NOTHING behind.
        let current_live_roots = collect_open_carrier_roots();
        let now_unreferenced = self.reconcile_open_roots(&current_live_roots);
        self.guarded_close(sync, provider_sync_states, &now_unreferenced)
            .await;

        synced_any
    }

    /// Open (or update) one carrier dependency's declaration overlay (`.d.<ext>.ts`)
    /// in the provider and record the reaching root — serialized behind that
    /// overlay's path lock so it cannot interleave with a concurrent close of the
    /// same overlay.
    ///
    /// Ensures the dependency is loaded, derives its declaration-carrier path,
    /// fetches the declaration-mode public API, then under the path lock: REVALIDATES
    /// the reaching root is still open (a root that closed since the closure snapshot
    /// must not be recorded — that would resurrect an overlay no live root reaches),
    /// records `{decl_path -> root}` + bumps the generation in one slot critical
    /// section, and issues the provider open WHILE HOLDING the path lock. The
    /// declaration content is RE-FETCHED each pass, so a changed carrier yields a
    /// fresh overlay automatically.
    ///
    /// Returns the opened `.d.<ext>.ts` path on success (so the caller folds it into
    /// the root's per-pass reached set for reconciliation), or `None` when the
    /// carrier was not loadable / projects no declaration / the root closed / the
    /// open failed.
    async fn open_overlay(
        &self,
        sync: &ProjectSync,
        documents: &DocumentRegistry,
        provider_sync_states: &DashMap<String, ProviderSyncState>,
        snapshot: &super::PublishedResolverSnapshot,
        root_canonical: &str,
        dep_canonical_id: &str,
    ) -> Option<String> {
        let host = documents.host();
        if !host.ensure_loaded(dep_canonical_id) {
            return None;
        }
        let decl_path = host.declaration_carrier_path(dep_canonical_id)?; // adapter projects no decl

        // tgo resolves the declaration through path config; mirror the API/IDE
        // carrier path configuration so the overlay is resolvable in the owner project.
        configure_provider_paths_for_source(sync, snapshot, dep_canonical_id, true).await;

        let profile = documents.tsx_profile.read().clone();
        let api = block_in_place_if_available(|| {
            host.get_public_api_with_mode(
                dep_canonical_id,
                verter_session::PublicApiMode::Declaration,
                Some(&profile),
            )
        })?; // no declaration surface this pass

        // Serialize this open against any concurrent close of the SAME overlay path:
        // a close in flight holds this lock across its provider `close_dts`, so this
        // open waits behind it and then records/opens against current state.
        let lock = self.path_lock(&decl_path);
        // Acquire the path lock. Under `#[cfg(test)]`, FIRST try to take it without
        // blocking: a FAILURE means the strand is already held by a concurrent
        // close/open of THIS exact path, so fire the one-shot contention probe BEFORE
        // awaiting — "an `open_overlay` for this path hit the held strand and is about
        // to block on it." The probe is the sound signal present in BOTH worlds at the
        // right moment (it fires only on real contention, never on a mere attempt), so
        // a lifecycle test can assert the serialized open genuinely blocked behind an
        // in-flight close instead of racing it. A SUCCESSFUL `try_lock` is the
        // uncontended path: we already hold the guard, no await and no signal.
        #[cfg(test)]
        let _path_guard = match lock.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                self.fire_open_lock_contended_for_test(&decl_path);
                lock.lock().await
            }
        };
        #[cfg(not(test))]
        let _path_guard = lock.lock().await;

        // REVALIDATE the reaching root is still open under the path lock — BEFORE
        // recording. A root that closed since the start-of-pass snapshot must not be
        // recorded: recording it would resurrect an overlay no live root reaches,
        // leaking it in the provider with no future closer. Re-derive "is this root
        // still open" from the live open-document set (the same authority the closure
        // walk seeds from).
        let root_still_open = documents.open_uris().iter().any(|uri_str| {
            uri_str
                .parse::<Uri>()
                .ok()
                .and_then(|uri| documents.get_canonical_id(&uri))
                .is_some_and(|id| id == root_canonical)
        });
        if !root_still_open {
            return None;
        }

        // Record `{decl_path -> root}` + bump the generation in ONE slot critical
        // section, BEFORE the provider open — so the record/bump is visible the
        // instant a concurrent guarded close re-checks (and that close is, in any
        // case, serialized behind this path lock).
        {
            let mut slot = self.slots.entry(decl_path.clone()).or_default();
            slot.roots.insert(root_canonical.to_string());
            slot.generation += 1;
        }

        // Open the overlay (or update it in place when already background-loaded),
        // still holding the path lock.
        let already_live = provider_sync_states
            .get(dep_canonical_id)
            .map(|state| {
                state.decl_background_loaded
                    && state.decl_path.as_deref() == Some(decl_path.as_str())
            })
            .unwrap_or(false);
        let result = if already_live {
            sync.sync_dts(&decl_path, &api.code).await
        } else {
            sync.open_dts(&decl_path, &api.code).await
        };
        if let Err(error) = result {
            tracing::warn!(
                "declaration_closure: failed to open declaration overlay {decl_path} \
                 for {dep_canonical_id}: {error}"
            );
            return None;
        }

        // Record the live decl path + background-loaded flag on the carrier's
        // committed provider state (so close/lifecycle reaches it), preserving the
        // other kinds. A carrier already has a committed state from the carrier
        // passes; update only the Decl kind in place.
        if let Some(mut state) = provider_sync_states.get_mut(dep_canonical_id) {
            state.decl_path = Some(decl_path.clone());
            state.set_background_loaded(ProviderPathKind::Decl, true);
        } else {
            // No prior carrier state (the carrier reached only via the closure):
            // commit a minimal owner-resolved state carrying the Decl kind.
            let mut state = ProviderSyncState::default();
            if let Some(owner) = snapshot.resolver.owner_for_file(dep_canonical_id) {
                let owner_key = owner
                    .tsconfig_path
                    .clone()
                    .unwrap_or_else(|| owner.root.clone());
                state.owner_binding = crate::provider_sync::ProviderOwnerBinding::Owned(owner_key);
            }
            state.decl_path = Some(decl_path.clone());
            state.set_background_loaded(ProviderPathKind::Decl, true);
            commit_sync_transition(provider_sync_states, dep_canonical_id, state);
        }

        Some(decl_path)
    }

    /// The CURRENT close generation for `decl_path` (0 when no slot is tracked) — the
    /// decision baseline a direct close (e.g. a carrier-state close that surfaced a
    /// `Decl` path) pairs with the path before handing it to [`Self::guarded_close`].
    /// A concurrent open that races the close bumps the generation past this baseline,
    /// so the guarded close recognises it as superseded.
    pub(crate) fn current_generation(&self, decl_path: &str) -> u64 {
        self.slots.get(decl_path).map(|s| s.generation).unwrap_or(0)
    }

    /// Build the [`DeclCloseTarget`] for a direct close of `decl_path`, pairing it
    /// with the current generation as the supersession baseline.
    pub(crate) fn close_target_for(&self, decl_path: &str) -> DeclCloseTarget {
        (decl_path.to_string(), self.current_generation(decl_path))
    }

    /// Test-only: the reaching-root set recorded for `decl_path`, or `None` when no
    /// slot is tracked. Used by the lifecycle regression tests to assert the
    /// reachability graph directly without exposing the private slot map.
    #[cfg(test)]
    pub(crate) fn test_slot_roots(&self, decl_path: &str) -> Option<HashSet<String>> {
        self.slots.get(decl_path).map(|s| s.roots.clone())
    }

    /// Test-only: the close generation recorded for `decl_path` (0 when no slot is
    /// tracked). Used by the ABA / supersession regression tests.
    #[cfg(test)]
    pub(crate) fn test_slot_generation(&self, decl_path: &str) -> u64 {
        self.slots.get(decl_path).map(|s| s.generation).unwrap_or(0)
    }

    /// Test-only: every tracked `(decl_path, reaching_roots)` pair — the snapshot the
    /// lifecycle tests scan to assert which roots reach which overlays.
    #[cfg(test)]
    pub(crate) fn test_slots_snapshot(&self) -> Vec<(String, HashSet<String>)> {
        self.slots
            .iter()
            .map(|e| (e.key().clone(), e.value().roots.clone()))
            .collect()
    }

    /// Test-only: directly seed a `{decl_path -> root}` edge with a chosen
    /// generation, mirroring what a real open records — so a unit test can set up a
    /// slot state without driving a full provider open.
    #[cfg(test)]
    pub(crate) fn test_seed_slot(&self, decl_path: &str, roots: &[&str], generation: u64) {
        let mut slot = self.slots.entry(decl_path.to_string()).or_default();
        slot.generation = generation;
        for root in roots {
            slot.roots.insert((*root).to_string());
        }
    }

    /// Test-only: REPLACE a slot's reaching-root set (and generation) atomically — so
    /// a concurrency stress test can reset a slot to an exact state between
    /// iterations without leaking the prior iteration's roots.
    #[cfg(test)]
    pub(crate) fn test_replace_slot(&self, decl_path: &str, roots: &[&str], generation: u64) {
        let mut slot = self.slots.entry(decl_path.to_string()).or_default();
        slot.generation = generation;
        slot.roots = roots.iter().map(|r| (*r).to_string()).collect();
    }

    /// Test-only: ARM a one-shot CONTENTION probe for `decl_path` and return its
    /// [`tokio::sync::Notify`]. The probe fires the instant an [`Self::open_overlay`]
    /// for this exact path finds the path's serialization lock already HELD (a
    /// concurrent close/open holds the strand) and is about to await it — i.e. the
    /// serialized open genuinely BLOCKED behind an in-flight close rather than racing
    /// it. A waiter on the returned `Notify` is released only on that real contention,
    /// so a lifecycle test can FORCE-discriminate the per-path serialization without a
    /// timing fallback: post-fix the open hits the held lock and this fires; pre-fix
    /// (no serialization) the open is never lock-blocked and this never fires.
    #[cfg(test)]
    pub(crate) fn signal_open_lock_contended_for_test(
        &self,
        decl_path: &str,
    ) -> Arc<tokio::sync::Notify> {
        Arc::clone(
            self.open_lock_contention_signals
                .entry(decl_path.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Notify::new()))
                .value(),
        )
    }

    /// Test-only: fire the armed contention probe for `decl_path` (if any). Called by
    /// [`Self::open_overlay`] when its `try_lock` on the path strand fails — the exact
    /// moment the serialized open is about to await a held lock. `notify_one` stores a
    /// permit, so a waiter that has not yet parked is still released (no
    /// signal-before-await race).
    #[cfg(test)]
    fn fire_open_lock_contended_for_test(&self, decl_path: &str) {
        if let Some(signal) = self.open_lock_contention_signals.get(decl_path) {
            signal.notify_one();
        }
    }
}

/// The carrier (`.vue` / `.svelte`) dependency canonical ids of `canonical_id`,
/// resolved from its analyzed SCRIPT import set (no re-parse, no scan of generated
/// text). A specifier that resolves to a non-carrier file is dropped (the closure
/// follows only carrier→carrier edges — a barrel or plain `.ts` dependency is
/// handled by the other passes).
///
/// Each carrier specifier is resolved through the SAME workspace resolver the engine
/// uses for codegen: the analysis-time `resolved_canonical_id`, falling back to
/// [`resolve_import_specifier_standalone`] (which routes through
/// `host.resolve_import_via_workspace` — the alias / tsconfig-`paths` /
/// `node_modules` resolver under `ResolvePhase::CodegenBlocker`). This covers the
/// carrier dependencies the engine itself can resolve; it is the carrier subset of
/// the script imports (template component imports are a subset of the script
/// imports), NOT a guaranteed superset of the declaration's *type* references.
///
/// TODO(follow-up): a carrier specifier that resolves through neither rail — an
/// import whose `resolved_canonical_id` is absent AND whose workspace resolution
/// misses at closure time (e.g. an alias whose target only materialises once the
/// overlay itself exists) — is dropped here and its `.d.<ext>.ts` is not opened, so a
/// bare import of that carrier can still surface TS2307 until a later pass resolves
/// it. The proper fix is to drive this narrow residual through the engine's
/// resolution rather than dropping it; tracked for the closure pass.
pub(crate) fn carrier_dependency_ids(
    host: &verter_session::VerterHost,
    canonical_id: &str,
) -> Vec<String> {
    let Some(analysis) = host.get_analysis(canonical_id) else {
        return Vec::new();
    };
    let mut deps: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for import in &analysis.imports {
        let resolved = import
            .resolved_canonical_id
            .clone()
            .or_else(|| resolve_import_specifier_standalone(host, canonical_id, &import.source));
        let Some(resolved) = resolved else {
            continue;
        };
        if verter_workspace::path_is_carrier(&resolved) && seen.insert(resolved.clone()) {
            deps.push(resolved);
        }
    }
    deps
}
