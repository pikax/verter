//! `impl VerterHost` — upsert and style-override methods.
//!
//! Contains [`VerterHost::upsert`] and [`VerterHost::apply_style_overrides`],
//! which handle file ingestion, change detection, cache invalidation, and
//! style override application.

use std::collections::BTreeSet;
use std::sync::Arc;

use rustc_hash::FxHashMap;

// `Instant` is only referenced by `upsert_legacy` (WASM-only). Native paths
// measure parse durations via the scheduler executor.

use crate::cache::sorted_nodes;
use crate::hash::{compile_profile_hash, content_override_hash, style_override_hash};
use crate::id::{canonicalize_id, render_ids};
use crate::parse::parse_vue_snapshot;
use crate::types::*;
use crate::upsert::compute_upsert_changes_from_parse;
use crate::upsert::{build_upsert_result, UpsertResultData};
use crate::VerterHost;
use verter_scheduler::stage::Priority;

/// One per-request outcome from the shared upsert engine
/// [`VerterHost::upsert_many_with_priority`]. `result` is the same
/// `Result<HostUpdateResult, HostError>` the single-file
/// [`VerterHost::upsert`] returns; `canonical_id` is the request's
/// resolved canonical, carried so callers (`compile_many`'s Stage B)
/// can fold a failure into a per-canonical error map without re-deriving
/// it.
pub(crate) struct UpsertBatchOutcome {
    pub canonical_id: String,
    pub result: Result<HostUpdateResult, HostError>,
}

/// The per-canonical state captured BEFORE the atomic batch is
/// submitted, paired with its request, so the post-commit step can run
/// against the correct prior snapshot after the single `wait_batch`.
///
/// `old_source_snap` is a cloned `Arc<SourceSnapshot>` (or `None` for a
/// cold file) read from the scheduler before submission — never a live
/// DashMap shard `Ref`, so no guard crosses `submit_batch_atomic`.
pub(crate) struct PreparedUpsertCommit {
    canonical_id: String,
    req: UpsertRequest,
    old_source_snap: Option<Arc<verter_scheduler::node::SourceSnapshot>>,
}

/// An in-flight upsert batch transaction: the prepared per-canonical
/// state index-aligned with the submitted [`BatchHandle`].
///
/// The transaction is the unit that owns the index alignment invariant:
/// `prepared[i]` is the request whose completion is `batch.handles()[i]`.
/// `submit` builds it (resolve + assert canonical uniqueness FIRST,
/// capture context once, prepare each request, ONE `submit_batch_atomic`);
/// `finish` consumes it (ONE `wait_batch`, then zip `state[i]` ↔
/// `prepared[i]` and run each post-commit). There is no path that
/// produces a `prepared`/`batch` pair out of alignment.
pub(crate) struct UpsertBatchTxn {
    prepared: Vec<PreparedUpsertCommit>,
    batch: verter_scheduler::scheduler::BatchHandle,
}

impl UpsertBatchTxn {
    /// Drain the batch with EXACTLY ONE input-order `wait_batch`, then
    /// zip each completion `state[i]` with its `prepared[i]` and map to
    /// one [`UpsertBatchOutcome`]. Every index is mapped — a partial
    /// failure never early-returns, so the result `Vec` always has one
    /// outcome per submitted request, in input order.
    fn finish(self, host: &VerterHost) -> Vec<UpsertBatchOutcome> {
        let UpsertBatchTxn { prepared, batch } = self;
        // ONE input-order wait. `wait_batch` returns `state[i]` for the
        // i-th submitted request regardless of completion order.
        let states = host.scheduler.wait_batch(&batch);
        debug_assert_eq!(
            states.len(),
            prepared.len(),
            "wait_batch must return one completion state per submitted request"
        );
        Self::map_states(host, prepared, states)
    }

    /// The completion-state → [`UpsertBatchOutcome`] mapper: zip each
    /// `state[i]` with its `prepared[i]` and route by completion arm.
    /// This is the SOLE partial-failure mapping logic — `finish` calls it
    /// after the production `wait_batch`, and the test-only
    /// `finish_from_states` seam calls it with controlled states so the
    /// mapping under test is the production one (never duplicated).
    ///
    /// Every index is mapped (no early-return on a non-Ready arm), in
    /// input order:
    ///   - `Ready(_)`     → `finish_upsert_post_commit` (carries the
    ///                       request's canonical on success)
    ///   - `Failed(e)`    → `HostError::Scheduler(e)`
    ///   - `Superseded`   → `HostError::Superseded`
    ///   - `Shutdown`     → `HostError::Shutdown`
    fn map_states(
        host: &VerterHost,
        prepared: Vec<PreparedUpsertCommit>,
        states: Vec<verter_scheduler::job::CompletionState<verter_scheduler::job::RequestResult>>,
    ) -> Vec<UpsertBatchOutcome> {
        use verter_scheduler::job::CompletionState;

        prepared
            .into_iter()
            .zip(states)
            .map(|(prepared, state)| {
                let canonical_id = prepared.canonical_id.clone();
                let result = match state {
                    CompletionState::Ready(ready) => {
                        host.finish_upsert_post_commit(prepared, ready)
                    }
                    CompletionState::Failed(e) => Err(HostError::Scheduler(e)),
                    CompletionState::Superseded => Err(HostError::Superseded),
                    CompletionState::Shutdown => Err(HostError::Shutdown),
                };
                UpsertBatchOutcome {
                    canonical_id,
                    result,
                }
            })
            .collect()
    }

    /// Test-only seam: run the REAL completion-state mapper
    /// ([`Self::map_states`]) against a caller-supplied
    /// `(prepared, states)` pair, bypassing only the `wait_batch` source
    /// of the states. Lets a test drive MIXED `Ready`/`Failed`/
    /// `Superseded`/`Shutdown` arms through the production mapping logic
    /// without standing up a scheduler that produces those terminal
    /// states on demand — the mapping itself is not duplicated in the
    /// test. `prepared` and `states` MUST be the same length (the §6c
    /// index-alignment invariant); a length mismatch silently truncates
    /// via `zip`, so the seam asserts equality up front.
    #[cfg(test)]
    pub(crate) fn finish_from_states(
        host: &VerterHost,
        prepared: Vec<PreparedUpsertCommit>,
        states: Vec<verter_scheduler::job::CompletionState<verter_scheduler::job::RequestResult>>,
    ) -> Vec<UpsertBatchOutcome> {
        assert_eq!(
            prepared.len(),
            states.len(),
            "finish_from_states requires one completion state per prepared entry"
        );
        Self::map_states(host, prepared, states)
    }
}

impl VerterHost {
    /// Insert or update a file in the host.
    ///
    /// Parses the source, computes content hashes, detects granular slice-level
    /// changes, invalidates affected compile slots, and returns a
    /// [`HostUpdateResult`] describing which virtual nodes changed or were removed.
    ///
    /// On native (scheduler-backed): the scheduler is the sole parser. `upsert()`
    /// submits to the scheduler, waits for Source+Analysis to commit, then reads
    /// back the result and populates the compile cache. The `files` map is also
    /// populated for the WASM path (non-scheduler).
    ///
    /// `upsert` is a thin forwarder onto `upsert_with_priority` at
    /// `Priority::Interactive`. Batch callers (`compile_many`) call
    /// `upsert_with_priority` directly with a caller-configured priority.
    /// The semantic-db pre-invalidation invariant is owned by
    /// `upsert_with_priority`.
    pub fn upsert(&self, req: UpsertRequest) -> Result<HostUpdateResult, HostError> {
        // Provenance counter (R17 — host-mutation entry point).
        // Increments on every `VerterHost::upsert(...)` call. The
        // counter is consumed by `tests/session_view_isolation.rs`
        // to assert that the session query paths never re-enter
        // host-side mutation.
        self.provenance
            .host_upsert_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.upsert_with_priority(req, Priority::Interactive)
    }

    /// Insert or update a file with caller-configured scheduler priority.
    ///
    /// Collapses onto the single shared upsert engine
    /// [`Self::upsert_many_with_priority`] as a 1-element atomic batch —
    /// there is no second single-file submit path. The semantic-db
    /// pre-invalidation (the parse-domain producer contract) and the
    /// test-only priority observable live in `upsert_many_with_priority`,
    /// applied per request, so a 1-element call observes them exactly as a
    /// multi-request batch does.
    pub(crate) fn upsert_with_priority(
        &self,
        req: UpsertRequest,
        priority: Priority,
    ) -> Result<HostUpdateResult, HostError> {
        let canonical_id = req
            .canonical_id
            .clone()
            .unwrap_or_else(|| canonicalize_id(&req.input_id).into_owned());
        // Drain the single outcome of the 1-element batch back into the
        // single-file return shape. `upsert_many_with_priority` always
        // returns exactly one outcome per input request.
        self.upsert_many_with_priority(vec![req], priority)
            .into_iter()
            .next()
            .map(|outcome| outcome.result)
            .unwrap_or_else(|| {
                // Unreachable: a non-empty input yields one outcome. Keep
                // a typed error rather than panicking on an impossible
                // empty drain.
                Err(HostError::MissingSource { canonical_id })
            })
    }

    /// The single shared upsert engine: submit every request as ONE
    /// atomic batch, wait once, and run per-canonical post-commit in
    /// input order.
    ///
    /// This is the SOLE scheduler-backed upsert path. The public
    /// single-file [`Self::upsert_with_priority`] collapses onto it as a
    /// 1-element batch, and `compile_many`'s Stage B drives it directly
    /// with the deduped per-canonical request list. There is no
    /// hand-rolled per-file submit/wait anywhere else.
    ///
    /// Flow:
    /// 1. Empty input short-circuits to an empty `Vec`.
    /// 2. Every request's canonical is resolved up front and checked for
    ///    uniqueness FIRST, before any per-request side effect or the
    ///    atomic submission. A release-active assertion
    ///    (`assert_canonicals_unique`) rejects a duplicated canonical: two
    ///    source-updating requests for one file would each bump the node
    ///    generation and supersede the prior in the same critical section,
    ///    a correctness bug `submit_batch_atomic` does not guard against
    ///    (it does not dedup). This is a caller-contract invariant —
    ///    every caller dedups by canonical before reaching the engine.
    /// 3. The calling thread's `OpaqueRequestContext` is captured ONCE
    ///    here (before any submission) and cloned into every batch
    ///    `Request`. This is the correct audit owner: the scheduler
    ///    installs the context into the source / analysis worker TLS, so
    ///    fan-out events stay attributable to the outer audited request.
    /// 4. Per request: the parse-domain producer contract
    ///    (`register_facts_for_new_content`) fires, the test-only
    ///    priority observable is recorded, the pre-submit source snapshot
    ///    is captured (a cloned `Arc`, never a live shard `Ref` — no
    ///    DashMap guard crosses `submit_batch_atomic`), and one
    ///    `Request { target: Analysis, .. }` is built.
    /// 5. Exactly ONE `submit_batch_atomic` + exactly ONE `wait_batch`.
    /// 6. `wait_batch` returns completion states in INPUT order; `state[i]`
    ///    is zipped with `prepared[i]` and mapped to one outcome each. A
    ///    partial failure does NOT early-return — every index is mapped.
    ///
    /// Invalidation is lazy on both axes (R1/R3): the upsert performs no
    /// eager own-canonical query-identity drain and fires no
    /// reverse-dependent cascade. The only cache work is the parse-domain
    /// producer contract; see `finish_upsert_post_commit`.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn upsert_many_with_priority(
        &self,
        requests: Vec<UpsertRequest>,
        priority: Priority,
    ) -> Vec<UpsertBatchOutcome> {
        // 1. Empty input: no submission, no wake, no accounting.
        if requests.is_empty() {
            return Vec::new();
        }
        // 2–5: build the transaction (resolve + uniqueness-check canonicals
        //      first, capture context once, prepare each request, ONE
        //      `submit_batch_atomic`).
        // 6:   drive it (ONE `wait_batch`, then per-index post-commit).
        self.submit_upsert_batch(requests, priority).finish(self)
    }

    /// Build the in-flight upsert transaction: resolve and uniqueness-
    /// check every canonical FIRST, capture the calling thread's request
    /// context once, run each request's producer contract + priority
    /// observable + pre-submit snapshot, and issue the SINGLE
    /// `submit_batch_atomic`.
    fn submit_upsert_batch(
        &self,
        requests: Vec<UpsertRequest>,
        priority: Priority,
    ) -> UpsertBatchTxn {
        // ── Canonical uniqueness: resolved + enforced FIRST ──
        //
        // Resolve every request's canonical id up front, then reject a
        // duplicated canonical BEFORE any per-request side effect runs
        // (`register_facts_for_new_content`, the pre-submit snapshot
        // read) and BEFORE the atomic submission. A duplicated canonical
        // in one source-updating batch would bump that node's generation
        // twice under the single `dag.lock()` acquisition, self-superseding
        // the earlier admit and corrupting the batch — `submit_batch_atomic`
        // does NOT dedup. Resolving first also means the side-effect loop
        // below reuses these canonicals instead of re-deriving them.
        //
        // This is a `pub(crate)` caller-contract invariant, not a runtime
        // input class: every caller already guarantees uniqueness
        // (`compile_many`'s Stage B keys `canonical_to_upsert` by canonical
        // so it carries one entry per file; the single-file
        // `upsert_with_priority` submits a 1-element batch that is trivially
        // unique). A breach is a programming bug in the caller, so it is a
        // `assert!` (active in release, unlike the inputs it guards) that
        // fails loudly before admission rather than a recoverable error
        // arm. `assert_canonicals_unique` panics on a duplicate.
        let canonicals: Vec<String> = requests
            .iter()
            .map(|req| {
                req.canonical_id
                    .clone()
                    .unwrap_or_else(|| canonicalize_id(&req.input_id).into_owned())
            })
            .collect();
        Self::assert_canonicals_unique(&canonicals);

        // Capture the CALLING thread's request context ONCE, before any
        // submission, and clone it into every batch Request. The
        // scheduler installs it into the source / analysis worker TLS, so
        // fan-out events stay attributable to the outer audited request.
        let request_context = verter_scheduler::request_context::current_context();

        #[cfg(feature = "session_metrics")]
        self.metrics
            .upserts
            .fetch_add(requests.len() as u64, std::sync::atomic::Ordering::Relaxed);

        let mut prepared: Vec<PreparedUpsertCommit> = Vec::with_capacity(requests.len());
        let mut scheduler_requests: Vec<verter_scheduler::scheduler::Request> =
            Vec::with_capacity(requests.len());
        for (req, canonical_id) in requests.into_iter().zip(canonicals) {
            // R4 producer: drop the prior parse-domain `FileSemantic` for
            // this canonical so the next resolver pass rebuilds the fact
            // registry from the new content. This is the producer
            // contract — NOT downstream cache invalidation. Downstream
            // caches revalidate lazily through their own
            // `fact_dep_signature` checks (R3); the upsert itself does not
            // eagerly drain them. The engine owns this per request so the
            // single-file and batch entry points behave identically.
            if let Some(ref id) = req.canonical_id {
                self.register_facts_for_new_content(id);
            }

            // Test-only observable: lets `compile_many_propagates_*_priority`
            // tests confirm the priority that flowed to the scheduler.
            // Production builds compile this branch out completely.
            #[cfg(test)]
            {
                *self.last_upsert_priority.lock() = Some(priority);
            }

            // Pre-submit: read old state from the scheduler. `try_get_source`
            // returns a cloned `Arc<SourceSnapshot>`; the shard `Ref` is
            // dropped before this returns, so NO DashMap guard crosses
            // `submit_batch_atomic` (AB-BA: holding an `Arc` is not a lock).
            let old_source_snap = self.scheduler.try_get_source(&canonical_id);

            scheduler_requests.push(verter_scheduler::scheduler::Request {
                file_id: canonical_id.clone(),
                target: verter_scheduler::stage::TargetStage::Analysis,
                priority,
                source: Some(req.source.clone()),
                file_kind: Some(match req.file_kind {
                    FileKind::VueSfc => verter_scheduler::source_loader::FileKind::VueSfc,
                    FileKind::NonSfc => verter_scheduler::source_loader::FileKind::NonSfc,
                }),
                request_context: request_context.clone(),
            });
            prepared.push(PreparedUpsertCommit {
                canonical_id,
                req,
                old_source_snap,
            });
        }

        // Exactly ONE atomic submission. `submit_batch_atomic` lands a
        // single `NewRequestBatch` admitted under one `dag.lock()` and
        // accounts once itself — do NOT call `account_batch_submission` on
        // top of it. The returned `BatchHandle`'s handles are in input
        // order, index-aligned with `prepared`.
        let batch = self.scheduler.submit_batch_atomic(scheduler_requests);
        UpsertBatchTxn { prepared, batch }
    }

    /// Enforce the engine's canonical-uniqueness caller contract.
    ///
    /// Panics (active in release) if `canonicals` contains a duplicated
    /// entry. A source-updating atomic batch with a repeated canonical
    /// would bump that node's generation twice under the single
    /// `dag.lock()` acquisition, self-superseding the earlier admit and
    /// corrupting the batch; `submit_batch_atomic` does not dedup, so the
    /// guard is the only thing standing between a buggy caller and a torn
    /// batch. Callers MUST dedup by canonical before reaching the engine.
    ///
    /// Lifted out of `submit_upsert_batch` so it is a single named
    /// invariant the §6c regression test can drive directly and so the
    /// check is unambiguously computed before any per-request side effect.
    fn assert_canonicals_unique(canonicals: &[String]) {
        let mut seen = std::collections::HashSet::with_capacity(canonicals.len());
        for canonical in canonicals {
            assert!(
                seen.insert(canonical.as_str()),
                "upsert_many_with_priority received duplicate canonical_id \
                 `{canonical}` — a source-updating batch with a repeated \
                 file_id would bump the node generation twice under one \
                 DAG-lock and supersede the prior admit, corrupting the \
                 batch. Dedup by canonical before calling the engine."
            );
        }
    }

    /// Test-only seam: run the REAL `submit_upsert_batch` and hand back
    /// its `(prepared, batch)` parts (index-aligned) so a test can call
    /// `wait_batch` for the genuine `Ready` states, then splice in
    /// synthetic non-Ready terminal states at chosen indices and drive
    /// the whole vector through the production mapper
    /// ([`UpsertBatchTxn::finish_from_states`]). The Ready arm therefore
    /// exercises the real `finish_upsert_post_commit` against genuinely
    /// committed source snapshots, while the failure arms exercise the
    /// real error mapping — no mapping logic is reconstructed in the test.
    #[cfg(test)]
    pub(crate) fn test_submit_upsert_batch_parts(
        &self,
        requests: Vec<UpsertRequest>,
        priority: Priority,
    ) -> (
        Vec<PreparedUpsertCommit>,
        verter_scheduler::scheduler::BatchHandle,
    ) {
        let UpsertBatchTxn { prepared, batch } = self.submit_upsert_batch(requests, priority);
        (prepared, batch)
    }

    /// Per-canonical post-commit: read back the committed parse, compute
    /// granular changes, run the per-domain own-canonical cache drain, and
    /// build the [`HostUpdateResult`].
    ///
    /// Runs once per canonical on the CALLING thread after the single
    /// `wait_batch` returns. Drives a generation commit-fence: the
    /// `RequestResult::Analysis` snapshot's `generation` is the fence; the
    /// source snapshot read back here must carry the SAME generation. A
    /// mismatch means a newer source landed mid-flight and this result is
    /// stale → `HostError::Superseded` (never warm a torn provisional
    /// result into a shared cache).
    ///
    /// Invalidation is lazy on both axes. Same-canonical: a warm
    /// query-identity entry for the upserted canonical is rejected by its
    /// current-content self-version root on the cold-recompute read path.
    /// Cross-file: a downstream consumer's warm entry is revalidated on
    /// read through its own `fact_dep_signature` check (R3), not by an
    /// eager cascade fired here. The only cache work here is the
    /// parse-domain producer contract (`register_facts_for_new_content`)
    /// plus the upserted file's own-cache drain.
    fn finish_upsert_post_commit(
        &self,
        prepared: PreparedUpsertCommit,
        ready: verter_scheduler::job::RequestResult,
    ) -> Result<HostUpdateResult, HostError> {
        use crate::host_executor::HostSourceData;
        use verter_scheduler::job::RequestResult;

        let PreparedUpsertCommit {
            canonical_id,
            req,
            old_source_snap,
        } = prepared;

        // The batch target was `Analysis`, so the Ready payload is the
        // Analysis snapshot. Its generation is the commit fence.
        let RequestResult::Analysis(analysis_snap) = ready else {
            return Err(HostError::MissingSource {
                canonical_id: canonical_id.clone(),
            });
        };
        let committed_generation = analysis_snap.generation;

        let old_host_data = old_source_snap
            .as_ref()
            .and_then(|s| s.downcast_data::<HostSourceData>());

        // ── Post-commit: read new state from scheduler ──
        let new_source_snap = self
            .scheduler
            .try_get_source(&canonical_id)
            .ok_or_else(|| HostError::MissingSource {
                canonical_id: canonical_id.clone(),
            })?;

        // Commit fence: the source snapshot must match the generation the
        // Analysis stage committed against. A higher source generation
        // means a newer upsert raced in after our batch admitted; the
        // read-back parse would be torn relative to the analysis we waited
        // on — reject as superseded rather than publishing a stale result.
        if new_source_snap.generation != committed_generation {
            return Err(HostError::Superseded);
        }

        let new_host_data = new_source_snap
            .downcast_data::<HostSourceData>()
            .ok_or_else(|| HostError::MissingSource {
                canonical_id: canonical_id.clone(),
            })?;
        let parse = &new_host_data.parse;
        let parse_duration_ms = new_host_data.parse_duration_ms;

        #[cfg(feature = "session_metrics")]
        self.metrics.slice_hash_time_us_total.fetch_add(
            (parse_duration_ms * 1000.0) as u64,
            std::sync::atomic::Ordering::Relaxed,
        );

        // ── Compute changes ──
        let changes = compute_upsert_changes_from_parse(old_host_data.map(|h| &h.parse), parse);

        let mut alias_set: BTreeSet<String> = req
            .aliases
            .iter()
            .map(|a| canonicalize_id(a).into_owned())
            .collect();
        alias_set.insert(canonicalize_id(&req.input_id).into_owned());
        alias_set.insert(canonical_id.clone());

        let new_deps: BTreeSet<String> = parse
            .external_requests
            .iter()
            .map(|r| r.resolved_canonical_id.clone())
            .chain(
                parse
                    .script_analysis
                    .imports
                    .iter()
                    .filter(|imp| imp.source.starts_with('.'))
                    .map(|imp| {
                        let resolved = crate::id::resolve_external(&canonical_id, &imp.source);
                        self.resolve_eval_dependency_canonical(&resolved)
                            .unwrap_or(resolved)
                    }),
            )
            .collect();

        // ── Fast path: quintuple-unchanged source ──
        //
        // Per R1: `host.upsert(canonical, source)` is a cache-state no-op
        // iff the quintuple `(canonical, content_hash, parse_env_hash,
        // resolve_env_hash, lib_env_hash)` is unchanged. Env-hash
        // dimensions are construction-time on the host; the only paths
        // that mutate them (`configure_projects`, `set_workspace`) reset
        // the entire host before any subsequent `upsert` can hit this
        // gate. The whole-hash equality below is therefore sufficient to
        // prove quintuple-unchanged in the live `upsert` call.
        //
        // Per R2: cache eviction is an explicit method with a stated
        // scope — it is NEVER a side effect of `upsert`. This path
        // performs zero cache mutations, zero semantic invalidations,
        // zero store-view epoch bumps, and zero workspace-edge writes.
        //
        // The single exception is alias-map sync, which fires only when
        // `req.aliases` introduces a new alias that did not previously
        // point to this canonical. Alias identity is a caller-supplied
        // dimension distinct from the quintuple; it must update on
        // disagreement to honour the contract that aliases caller
        // supplied are reachable from the canonical.
        let old_whole_hash = old_host_data.map(|h| h.parse.whole_hash);
        if !changes.changed && old_whole_hash == Some(parse.whole_hash) {
            // Alias-map sync runs only when `req.aliases` differs from
            // the previously recorded set for this canonical. This is
            // the only mutation permitted on the quintuple-unchanged
            // fast path; everything else is a no-op per R1.
            //
            // The compare uses the EXISTING DependencyState entry as the
            // sole source of truth. When no entry exists (e.g. a
            // scheduler-only cold load preceded the upsert) we MUST NOT
            // materialise one as a side effect of the fast path — that
            // would violate R1. Callers that need DependencyState created
            // run the full upsert path (a structural change) or
            // `ensure_loaded`.
            if let Some(existing_dep) = self.dependency_cache().get(&canonical_id) {
                if existing_dep.aliases != alias_set {
                    let old_aliases = existing_dep.aliases.clone();
                    // Drop the read-guard before taking the entry write.
                    drop(existing_dep);
                    if let Some(mut dep_ref) = self.dependency_cache().get_mut(&canonical_id) {
                        dep_ref.value_mut().aliases = alias_set.clone();
                    }
                    self.update_alias_map(&canonical_id, &old_aliases, &alias_set);
                }
            }
            return Ok(HostUpdateResult {
                canonical_id,
                changed: false,
                slice_changes: SliceChanges::default(),
                changed_virtual_nodes: Vec::new(),
                removed_virtual_nodes: Vec::new(),
                changed_virtual_ids: Vec::new(),
                removed_virtual_ids: Vec::new(),
                changed_lsp_ids: Vec::new(),
                removed_lsp_ids: Vec::new(),
                diagnostics: DiagnosticsSnapshot::default(),
                external_source_requests: Vec::new(),
                import_specifiers: Vec::new(),
                module_references: Vec::new(),
                preprocessor_requests: Vec::new(),
                export_signatures: Vec::new(),
                parse_duration_ms,
            });
        }
        // ── Per-domain invalidation per D48 invalidation matrix ──
        // A source-content-change (whole_hash_changed or semantic_changed)
        // is the per-canonical "Source content change for owner" trigger:
        // DerivedRawState + DependencyState are dropped/refreshed; ProfileState
        // is preserved for byte-identical compile reuse, but per-profile
        // outputs that depended on the changed slices are evicted from the
        // ProfileState entry below.
        let whole_hash_changed = old_whole_hash != Some(parse.whole_hash);
        let prev_nodes = old_host_data
            .map(|h| h.parse.meta.virtual_nodes())
            .unwrap_or_default();
        let old_aliases;

        // ── ProfileState (compile_cache_db) — per-profile compile outputs ──
        // The matrix preserves ProfileState on a source-content trigger; the
        // per-slice eviction below mutates only the profile fields whose
        // freshness depends on the changed source slice (e.g. compile_slots
        // when semantic_changed). Override maps are cleared on whole_hash
        // change because synthetic parses and remapped CSS spans become
        // stale on byte-offset shifts.
        //
        // Audit hook (R1): every observable drain emits one
        // `CacheDrainedAtUpsert` event so tests can prove the
        // byte-identical fast path skipped this branch entirely.
        {
            let mut profile_ref = self
                .compile_cache()
                .entry(canonical_id.clone())
                .or_default();
            let profile = profile_ref.value_mut();
            if whole_hash_changed {
                profile.content_overrides.clear();
                profile.style_overrides.clear();
                crate::host_manage::push_cache_drained_at_upsert(
                    "compile_cache_overrides",
                    &canonical_id,
                );
            }
            if changes.changed && changes.semantic_changed {
                let session_node =
                    crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                session_node.clear_compile_outputs_for_file(profile);
                profile.latest_diagnostics.clear();
                profile.diagnostics_generation += 1;
                crate::host_manage::push_cache_drained_at_upsert("compile_slots", &canonical_id);
            }
        }

        // ── Content-addressed compile-output node ──
        // `CompileOutputNode_PureContent` keys on
        // `(canonical, content_hash, env_*, profile, source_map_policy)`.
        // A `whole_hash` change moves the canonical to a NEW content_hash,
        // so any prior-version Content entries for this canonical become
        // unreachable but stay in the store — concurrent edits would let
        // them accumulate without bound. The targeted invalidation path
        // (`invalidate_compile_slots`) already calls `remove_canonical`
        // here; mirror that on source-content edits so the live entry
        // count stays bounded by the live-content-reachable set. The
        // per-canonical reverse index makes this O(prev_entries) on the
        // SHARD owning `canonical`, no global scan.
        //
        // Same-content edits (`whole_hash_changed == false`) hit the
        // quintuple-unchanged fast path above and never reach this
        // branch, so the live content key is preserved.
        if whole_hash_changed {
            self.compile_output_pure_content()
                .remove_canonical(&canonical_id);
            crate::host_manage::push_cache_drained_at_upsert(
                "compile_output_pure_content",
                &canonical_id,
            );
        }

        // ── DerivedRawState (derived_raw_cache_db) — source-derived caches ──
        // The matrix invalidates DerivedRawState on a source-content trigger.
        // Resolved-meta + tsc-extract + raw-template-analysis are
        // source-derived; clearing them on whole-hash / semantic / slice
        // changes flushes stale derived projections.
        {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(canonical_id.clone())
                .or_default();
            let derived = derived_ref.value_mut();
            let mut drained_derived = false;
            if whole_hash_changed {
                derived.cached_tsc_extract = None;
                derived.cached_resolved_meta.clear();
                derived.cached_meta_payload = None;
                derived.cached_fallthrough = None;
                drained_derived = true;
            }
            if changes.changed {
                derived.cached_resolved_meta.clear();
                derived.cached_meta_payload = None;
                derived.cached_fallthrough = None;
                drained_derived = true;
            }
            if changes.changed && changes.semantic_changed {
                derived.cached_resolved_meta.clear();
                derived.cached_meta_payload = None;
                derived.cached_fallthrough = None;
                drained_derived = true;
            }
            if changes.changed
                && (changes.slice_changes.script_changed
                    || changes.slice_changes.structure_changed
                    || changes.slice_changes.template_changed
                    || changes.slice_changes.descriptor_changed)
            {
                derived.cached_tsc_extract = None;
                derived.cached_resolved_meta.clear();
                derived.cached_meta_payload = None;
                derived.cached_fallthrough = None;
                drained_derived = true;
            }
            if whole_hash_changed || changes.semantic_changed {
                derived.raw_template_analysis = None;
                drained_derived = true;
            }
            // import_routes is the sub-mirror of IndexedReady.import_routes.
            // It is recomputed by downstream resolver passes after this
            // upsert; clear here so stale entries do not leak into the next
            // resolver run. R3/R26/R28 Gap 2: drop the parallel
            // per-specifier known-miss generation table so subsequent
            // bundler resolutions admit fresh tags.
            derived.import_routes.clear();
            derived
                .import_routes_known_miss_recorded_at_generation
                .clear();
            derived.evicted = false;
            if drained_derived {
                crate::host_manage::push_cache_drained_at_upsert(
                    "derived_raw_cache",
                    &canonical_id,
                );
            }
        }

        // ── DependencyState (dependency_cache_db) — dep-closure metadata ──
        // The matrix invalidates DependencyState on a source-content trigger
        // (the new deps set replaces the old). Read old aliases before
        // overwriting so the alias-map diff sees the prior state.
        {
            let mut dep_ref = self
                .dependency_cache()
                .entry(canonical_id.clone())
                .or_default();
            let dep = dep_ref.value_mut();
            old_aliases = dep.aliases.clone();
            dep.dependencies = new_deps.clone();
            dep.aliases = alias_set.clone();
            dep.generation = dep.generation.saturating_add(1);
        }
        crate::host_manage::push_cache_drained_at_upsert("dependency_cache", &canonical_id);

        // ── Build result data from parse ──
        let result_data = UpsertResultData {
            new_meta: parse.meta.clone(),
            parse_diagnostics: parse.parse_diagnostics.clone(),
            imports: parse.script_analysis.imports.clone(),
            module_references: parse.script_analysis.module_references.clone(),
            external_requests: parse.external_requests.clone(),
            preprocessor_requests: parse.preprocessor_requests.clone(),
            export_signatures: parse.export_signatures.clone(),
        };

        // ── Post-commit: parse-domain producer contract ──
        //
        // Re-emit the parse-domain fact registry for the new content so
        // later resolver passes (and the fact-signature helpers) observe
        // current-content facts. This is the ONLY cache work the upsert
        // performs: it does not drain the upserted canonical's own
        // query-identity caches, and it fires no reverse-dependent
        // cascade. A warm entry for the upserted canonical is rejected
        // by its current-content self-version root on the cold-recompute
        // read path; a cross-file consumer's warm entry is revalidated
        // lazily on read through its own `fact_dep_signature` check (R3).
        self.register_facts_for_new_content(&canonical_id);
        crate::host_manage::push_cache_drained_at_upsert("semantic_invalidate", &canonical_id);

        self.update_alias_map(&canonical_id, &old_aliases, &alias_set);

        // Sync parsed edges to VFS so the workspace's reverse-dep graph
        // reflects the new edges (R22 memory-bound GC + LSP
        // affected-files reporting). The reverse graph is content-
        // addressed bookkeeping only — it is NOT wired to cache
        // invalidation; cross-file consumers revalidate lazily on read.
        self.record_parsed_edges_to_vfs(&canonical_id, &result_data);
        crate::host_manage::push_cache_drained_at_upsert("workspace_parsed_edges", &canonical_id);

        self.ws().notify_upsert(&canonical_id, req.source.clone());

        let result = build_upsert_result(
            canonical_id.clone(),
            result_data,
            &changes,
            &prev_nodes,
            &old_host_data
                .map(|h| h.parse.meta.clone())
                .unwrap_or_default(),
            parse_duration_ms,
        );
        self.bump_store_view_epoch();
        crate::host_manage::push_cache_drained_at_upsert("store_view_epoch", &canonical_id);
        result
    }

    /// Sync parsed edges to VFS (thin wrapper around the shared edge builder).
    fn record_parsed_edges_to_vfs(&self, canonical_id: &str, result_data: &UpsertResultData) {
        let parsed_edges = Self::build_parsed_edges_from_analysis(
            canonical_id,
            &result_data.external_requests,
            &result_data.imports,
            &result_data.module_references,
        );
        self.ws().record_parsed_edges(canonical_id, &parsed_edges);
    }

    /// Build the set of `ParsedEdge` records from a file's parse analysis.
    ///
    /// Sub- (R5 dedupe contract): dedupe by
    /// `(specifier, ResolveRequestKind)`, NOT by specifier alone. This
    /// closes Codex P2 / F14: a file with `import { foo } from './x'` AND
    /// `import type { Bar } from './x'` must produce TWO `ParsedEdge::Relative`
    /// entries — one per kind — because the workspace's
    /// `parsed_unresolved_relatives` is keyed by `(specifier, kind)` and
    /// silently dropping the second kind would leak a stale stem.
    ///
    /// Called from BOTH [`Self::record_parsed_edges_to_vfs`] (the upsert
    /// full-path / fast-path flow) AND `integrate_scheduler_snapshot` (the
    /// ensure_loaded scheduler-snapshot integration path) — single edge-
    /// extraction implementation, no drift.
    pub(crate) fn build_parsed_edges_from_analysis(
        _canonical_id: &str,
        external_requests: &[crate::ExternalSourceRequest],
        imports: &[verter_semantic::analysis::AnalyzedImport],
        module_references: &[verter_semantic::analysis::AnalyzedModuleReference],
    ) -> Vec<verter_workspace::ParsedEdge> {
        let mut parsed_edges = Vec::new();

        for req in external_requests {
            parsed_edges.push(verter_workspace::ParsedEdge::ExternalSrc {
                specifier: req.specifier.clone(),
                resolved_path: Some(req.resolved_canonical_id.clone()),
            });
        }

        // R5 dedupe by (specifier, kind) — NOT by specifier alone.
        let mut seen: rustc_hash::FxHashSet<(String, verter_workspace::ResolveRequestKind)> =
            rustc_hash::FxHashSet::default();

        for imp in imports {
            let kind = if imp.is_type_only {
                verter_workspace::ResolveRequestKind::TypeImport
            } else {
                verter_workspace::ResolveRequestKind::EsmImport
            };
            if !seen.insert((imp.source.clone(), kind)) {
                continue;
            }
            if imp.source.starts_with('.') {
                parsed_edges.push(verter_workspace::ParsedEdge::Relative {
                    specifier: imp.source.clone(),
                    kind,
                });
            } else {
                parsed_edges.push(verter_workspace::ParsedEdge::Bare {
                    specifier: imp.source.clone(),
                    kind,
                });
            }
        }

        for modref in module_references {
            let kind = if modref.is_type_only {
                verter_workspace::ResolveRequestKind::TypeImport
            } else {
                verter_workspace::ResolveRequestKind::EsmImport
            };

            if let Some(specifier) = modref.literal_specifier.as_ref() {
                let s: &str = specifier;
                if !s.is_empty() && seen.insert((specifier.clone(), kind)) {
                    if s.starts_with('.') {
                        parsed_edges.push(verter_workspace::ParsedEdge::Relative {
                            specifier: specifier.clone(),
                            kind,
                        });
                    } else {
                        parsed_edges.push(verter_workspace::ParsedEdge::Bare {
                            specifier: specifier.clone(),
                            kind,
                        });
                    }
                }
            }

            for specifier in &modref.finite_specifiers {
                let s: &str = specifier;
                if !s.is_empty() && seen.insert((specifier.clone(), kind)) {
                    if s.starts_with('.') {
                        parsed_edges.push(verter_workspace::ParsedEdge::Relative {
                            specifier: specifier.clone(),
                            kind,
                        });
                    } else {
                        parsed_edges.push(verter_workspace::ParsedEdge::Bare {
                            specifier: specifier.clone(),
                            kind,
                        });
                    }
                }
            }
        }

        parsed_edges
    }

    /// Apply preprocessor-compiled style overrides for a file+profile.
    ///
    /// Called by the bundler after an external CSS preprocessor (Sass, Less, etc.)
    /// has compiled each `<style>` block. The overrides replace the raw style
    /// content in the compile slot so that `get_virtual_file` serves the
    /// preprocessed CSS. Returns a [`HostUpdateResult`] listing affected style nodes.
    pub fn apply_style_overrides(
        &self,
        req: StyleOverrideRequest,
    ) -> Result<HostUpdateResult, HostError> {
        #[cfg(feature = "session_metrics")]
        self.metrics
            .style_override_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let canonical = self.resolve_alias_or_canonical(&req.canonical_id);
        let profile_hash = compile_profile_hash(&req.compile_profile);

        let mut by_index = FxHashMap::default();
        for ov in req.overrides {
            by_index.insert(ov.index, ov);
        }
        let override_hash = style_override_hash(&by_index);

        // Read raw data needed for CSS analysis + span remapping.
        // On scheduler path: read from scheduler snapshots (raw, unmodified).
        // The override results go into compile_cache per-profile.
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};

            let source_snap = self.scheduler.try_get_source(&canonical).ok_or_else(|| {
                HostError::MissingSource {
                    canonical_id: canonical.clone(),
                }
            })?;
            let hd = source_snap
                .downcast_data::<HostSourceData>()
                .ok_or_else(|| HostError::MissingSource {
                    canonical_id: canonical.clone(),
                })?;
            let analysis_snap = self.scheduler.try_get_analysis(&canonical);
            let raw_style_analyses: Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>> =
                analysis_snap
                    .as_ref()
                    .and_then(|a| a.downcast_data::<HostAnalysisData>())
                    .map(|ad| Arc::clone(&ad.style_analyses))
                    .unwrap_or_default();

            // Check previous hash
            let previous_hash = self
                .compile_cache()
                .get(&canonical)
                .and_then(|cc| cc.style_overrides.get(&profile_hash).map(|o| o.hash))
                .unwrap_or(0);
            if override_hash == previous_hash {
                let mut result = HostUpdateResult::no_change(canonical);
                result.external_source_requests = hd.parse.external_requests.clone();
                return Ok(result);
            }

            let source = &source_snap.source;
            let meta = &hd.parse.meta;

            // Re-analyze compiled CSS and remap spans
            let mut analyses_vec: Vec<Option<verter_semantic::analysis::StyleBlockAnalysis>> =
                vec![None; raw_style_analyses.len()];
            let mut lang_overrides_vec: Vec<Option<String>> = vec![None; meta.style_langs.len()];

            for (&idx, ov) in &by_index {
                if idx < raw_style_analyses.len() {
                    let existing = &raw_style_analyses[idx];
                    let content_offset = existing.content_offset;

                    let mut new_analysis = verter_semantic::analysis::build_css_style_analysis(
                        &ov.code,
                        verter_semantic::analysis::VueStyleInput::default(),
                        existing.scoped,
                        existing.is_module,
                        existing.module_name.as_deref(),
                        content_offset,
                    );

                    if let (Some(sm_json), Some(ref mut css)) =
                        (&ov.source_map, &mut new_analysis.css)
                    {
                        let content_start = content_offset as usize;
                        let original_content = if content_start < source.len() {
                            let rest = &source[content_start..];
                            if let Some(end) = rest.find("</style") {
                                &rest[..end]
                            } else {
                                rest
                            }
                        } else {
                            ""
                        };
                        crate::source_map_remap::remap_css_analysis_spans(
                            css,
                            &ov.code,
                            sm_json,
                            original_content,
                            content_offset,
                        );
                    }

                    if let Some(ref css) = new_analysis.css {
                        css.debug_assert_valid_spans(source.len() as u32);
                    }
                    new_analysis.v_binds = existing.v_binds.clone();
                    new_analysis.special_pseudos = existing.special_pseudos.clone();

                    analyses_vec[idx] = Some(new_analysis);
                }
                if idx < lang_overrides_vec.len() {
                    lang_overrides_vec[idx] = Some("css".to_string());
                }
            }

            // Store in compile_cache
            let layer = StyleOverrideLayer {
                hash: override_hash,
                by_index: by_index.clone(),
            };
            if let Some(mut cc) = self.compile_cache().get_mut(&canonical) {
                cc.style_overrides.insert(
                    profile_hash,
                    StyleOverrideWithAnalysis {
                        layer: layer.clone(),
                        analyses: analyses_vec,
                        lang_overrides: lang_overrides_vec,
                        hash: override_hash,
                    },
                );
                let session_node =
                    crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                session_node.remove(&mut cc, profile_hash);
            }

            let mut changed_nodes: Vec<VirtualNodeKind> = by_index
                .keys()
                .map(|idx| VirtualNodeKind::Style { index: *idx })
                .collect();
            changed_nodes = sorted_nodes(changed_nodes);

            let mut changed_virtual_ids = Vec::new();
            let mut changed_lsp_ids = Vec::new();
            for node in &changed_nodes {
                let (b, l) = render_ids(&canonical, node, meta);
                changed_virtual_ids.push(b);
                changed_lsp_ids.push(l);
            }

            let result = HostUpdateResult {
                canonical_id: canonical,
                changed: true,
                slice_changes: SliceChanges::default(),
                changed_virtual_nodes: changed_nodes,
                removed_virtual_nodes: Vec::new(),
                changed_virtual_ids,
                removed_virtual_ids: Vec::new(),
                changed_lsp_ids,
                removed_lsp_ids: Vec::new(),
                diagnostics: DiagnosticsSnapshot::default(),
                external_source_requests: hd.parse.external_requests.clone(),
                import_specifiers: Vec::new(),
                module_references: Vec::new(),
                preprocessor_requests: Vec::new(),
                export_signatures: Vec::new(),
                parse_duration_ms: 0.0,
            };
            self.bump_store_view_epoch();
            Ok(result)
        }

        // Legacy path (WASM)
    }

    /// Apply preprocessed block overrides for template, script, style, and custom blocks.
    ///
    /// Unified API that replaces the single-purpose `apply_style_overrides`.
    /// Template/script overrides build a synthetic SFC source with the `lang`
    /// attribute stripped and block content replaced, then invalidate the compile
    /// slot so the next `get_virtual_file` recompiles from the synthetic source.
    /// Style overrides delegate to the existing style override logic.
    pub fn apply_block_overrides(
        &self,
        req: BlockOverrideRequest,
    ) -> Result<HostUpdateResult, HostError> {
        let canonical = self.resolve_alias_or_canonical(&req.canonical_id);
        let profile_hash = compile_profile_hash(&req.compile_profile);

        // Separate overrides into template/script vs style buckets
        let mut template_override: Option<ContentOverride> = None;
        let mut script_override: Option<ContentOverride> = None;
        let mut style_overrides_vec: Vec<StyleOverrideEntry> = Vec::new();

        for ov in req.overrides {
            match ov.block_type {
                PreprocessorBlockType::Template => {
                    template_override = Some(ContentOverride {
                        code: ov.code,
                        source_map: ov.source_map,
                    });
                }
                PreprocessorBlockType::Script => {
                    script_override = Some(ContentOverride {
                        code: ov.code,
                        source_map: ov.source_map,
                    });
                }
                PreprocessorBlockType::Style | PreprocessorBlockType::Custom => {
                    style_overrides_vec.push(StyleOverrideEntry {
                        index: ov.index,
                        code: ov.code,
                        source_map: ov.source_map,
                    });
                }
            }
        }

        // Handle style overrides via the existing mechanism
        if !style_overrides_vec.is_empty() {
            let style_req = StyleOverrideRequest {
                canonical_id: req.canonical_id.clone(),
                compile_profile: req.compile_profile.clone(),
                overrides: style_overrides_vec,
            };
            // Apply style overrides (this also invalidates the compile slot)
            let _ = self.apply_style_overrides(style_req)?;
        }

        // Handle template/script content overrides
        let has_content_overrides = template_override.is_some() || script_override.is_some();
        if !has_content_overrides {
            // Only style overrides were provided; style overrides already handled above.
            // Read external_requests from scheduler (or files on WASM).
            {
                use crate::host_executor::HostSourceData;
                let snap = self.scheduler.try_get_source(&canonical).ok_or_else(|| {
                    HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    }
                })?;
                let hd = snap.downcast_data::<HostSourceData>().ok_or_else(|| {
                    HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    }
                })?;
                let mut result = HostUpdateResult::no_change(canonical);
                result.external_source_requests = hd.parse.external_requests.clone();
                result.changed = true;
                return Ok(result);
            }
        }

        let override_hash =
            content_override_hash(template_override.as_ref(), script_override.as_ref());

        // Scheduler path: read raw source+meta from scheduler, store override in compile_cache
        {
            use crate::host_executor::HostSourceData;
            let source_snap = self.scheduler.try_get_source(&canonical).ok_or_else(|| {
                HostError::MissingSource {
                    canonical_id: canonical.clone(),
                }
            })?;
            let hd = source_snap
                .downcast_data::<HostSourceData>()
                .ok_or_else(|| HostError::MissingSource {
                    canonical_id: canonical.clone(),
                })?;

            let previous_hash = self
                .compile_cache()
                .get(&canonical)
                .and_then(|cc| {
                    cc.content_overrides
                        .get(&profile_hash)
                        .map(|o| o.layer.hash)
                })
                .unwrap_or(0);

            if override_hash == previous_hash {
                let mut result = HostUpdateResult::no_change(canonical);
                result.external_source_requests = hd.parse.external_requests.clone();
                return Ok(result);
            }

            // Build synthetic source from raw scheduler source
            let synthetic_source = build_synthetic_source(
                &source_snap.source,
                &hd.parse.meta,
                template_override.as_ref(),
                script_override.as_ref(),
            );
            let synthetic_arc: Arc<str> = Arc::from(synthetic_source.as_str());

            let (new_snapshot, new_parsed) =
                parse_vue_snapshot(&canonical, &synthetic_source, self.config.effective_scope());

            let layer = ContentOverrideLayer {
                hash: override_hash,
                template: template_override.clone(),
                script: script_override.clone(),
            };

            // Store ContentOverrideWithParse in compile_cache
            if let Some(mut cc) = self.compile_cache().get_mut(&canonical) {
                cc.content_overrides.insert(
                    profile_hash,
                    ContentOverrideWithParse {
                        layer: layer.clone(),
                        parse: new_snapshot.clone(),
                        cached_parse: Some(Arc::new(new_parsed)),
                        source: synthetic_arc,
                    },
                );
                let session_node =
                    crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                session_node.remove(&mut cc, profile_hash);
            }

            let meta = &new_snapshot.meta;
            let mut changed_nodes = Vec::new();
            if meta.has_template {
                changed_nodes.push(VirtualNodeKind::Main);
                changed_nodes.push(VirtualNodeKind::Template);
            }
            if meta.has_script {
                changed_nodes.push(VirtualNodeKind::Script);
            }
            changed_nodes = sorted_nodes(changed_nodes);

            let mut changed_virtual_ids = Vec::new();
            let mut changed_lsp_ids = Vec::new();
            for node in &changed_nodes {
                let (b, l) = render_ids(&canonical, node, meta);
                changed_virtual_ids.push(b);
                changed_lsp_ids.push(l);
            }

            let result = HostUpdateResult {
                canonical_id: canonical,
                changed: true,
                slice_changes: SliceChanges::default(),
                changed_virtual_nodes: changed_nodes,
                removed_virtual_nodes: Vec::new(),
                changed_virtual_ids,
                removed_virtual_ids: Vec::new(),
                changed_lsp_ids,
                removed_lsp_ids: Vec::new(),
                diagnostics: DiagnosticsSnapshot::default(),
                external_source_requests: hd.parse.external_requests.clone(),
                import_specifiers: Vec::new(),
                module_references: Vec::new(),
                preprocessor_requests: Vec::new(),
                export_signatures: Vec::new(),
                parse_duration_ms: 0.0,
            };
            self.bump_store_view_epoch();
            Ok(result)
        }

        // Legacy path (WASM)
    }
}

/// Build a synthetic SFC source with preprocessed content replacing original
/// block content and `lang` attributes stripped.
///
/// The synthetic source preserves the same byte structure (tags, offsets) where
/// possible, but replaces block content and removes `lang="xxx"` from template
/// and script tags so the compiler treats them as native HTML/JS.
fn build_synthetic_source(
    original: &str,
    meta: &FileMeta,
    template_override: Option<&ContentOverride>,
    script_override: Option<&ContentOverride>,
) -> String {
    // Simple approach: scan and replace content using string markers.
    // We look for the block tags, strip lang attributes, and replace content.
    let mut result = original.to_string();

    // Replace template content (if override provided)
    if let Some(tpl) = template_override {
        result = replace_block_content(&result, "template", &tpl.code, true);
    }

    // Replace script content (if override provided)
    if let Some(scr) = script_override {
        // Determine which script tag to target
        let tag = if meta.script_lang.is_some() {
            "script"
        } else {
            // No non-native script lang; should not happen, but handle gracefully
            "script"
        };
        result = replace_block_content(&result, tag, &scr.code, true);
    }

    result
}

/// Replace the content of an SFC block tag and optionally strip its `lang` attribute.
///
/// Finds `<{tag}...>...content...</{tag}>` and replaces the content between
/// the opening and closing tags. If `strip_lang` is true, removes `lang="xxx"`
/// from the opening tag.
fn replace_block_content(source: &str, tag: &str, new_content: &str, strip_lang: bool) -> String {
    let bytes = source.as_bytes();

    // Find the opening tag
    let open_pattern = format!("<{}", tag);
    let Some(tag_start) = find_tag_start(bytes, &open_pattern) else {
        return source.to_string();
    };

    // Find the end of the opening tag (the `>`)
    let Some(tag_end) = find_char_after(bytes, tag_start, b'>') else {
        return source.to_string();
    };
    let content_start = tag_end + 1;

    // Find the closing tag
    let close_pattern = format!("</{}", tag);
    let Some(close_start) = find_pattern_after(bytes, content_start, close_pattern.as_bytes())
    else {
        return source.to_string();
    };

    // Build the result
    let mut result = String::with_capacity(source.len() + new_content.len());

    // Opening tag (with optional lang stripping)
    let opening_tag = &source[tag_start..content_start];
    if strip_lang {
        result.push_str(&source[..tag_start]);
        result.push_str(&strip_lang_attr(opening_tag));
    } else {
        result.push_str(&source[..content_start]);
    }

    // New content
    result.push_str(new_content);

    // From closing tag to end
    result.push_str(&source[close_start..]);

    result
}

/// Strip `lang="..."` or `lang='...'` from an opening tag string.
fn strip_lang_attr(tag: &str) -> String {
    // Match lang="..." or lang='...' with optional whitespace around =
    let bytes = tag.as_bytes();
    let mut result = String::with_capacity(tag.len());
    let mut i = 0;
    while i < bytes.len() {
        // Check if we're at "lang"
        if i + 4 <= bytes.len()
            && bytes[i..i + 4].eq_ignore_ascii_case(b"lang")
            && (i == 0 || bytes[i - 1].is_ascii_whitespace())
        {
            // Skip past lang="..."
            let mut j = i + 4;
            // Skip whitespace around =
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'=' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                    let quote = bytes[j];
                    j += 1;
                    while j < bytes.len() && bytes[j] != quote {
                        j += 1;
                    }
                    if j < bytes.len() {
                        j += 1; // skip closing quote
                    }
                }
                // Also consume any trailing whitespace after the value
                // but keep at least one space if we're between attributes
                i = j;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn find_tag_start(bytes: &[u8], pattern: &str) -> Option<usize> {
    let pat = pattern.as_bytes();
    bytes
        .windows(pat.len())
        .position(|w| w.eq_ignore_ascii_case(pat))
}

fn find_char_after(bytes: &[u8], start: usize, ch: u8) -> Option<usize> {
    bytes[start..]
        .iter()
        .position(|&b| b == ch)
        .map(|p| start + p)
}

fn find_pattern_after(bytes: &[u8], start: usize, pattern: &[u8]) -> Option<usize> {
    if start >= bytes.len() {
        return None;
    }
    bytes[start..]
        .windows(pattern.len())
        .position(|w| w.eq_ignore_ascii_case(pattern))
        .map(|p| start + p)
}
