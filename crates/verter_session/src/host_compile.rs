//! Host-backed parallel SFC compilation.
//!
//! Bundler/runtime output only. Returns the assembled Main virtual
//! file (script + template render fn). IDE TSX and TSC type-extract
//! batch surfaces are out of scope here; they would land as separate
//! `ide_many` / `public_api_many` entry points.
//!
//! ## Four-stage batch
//!
//! 1. **Stage A — short-circuit empty input.** Empty input returns
//!    immediately; no thread pool is constructed.
//! 2. **Stage B — group + selective upsert.** Group by `canonical_id`.
//!    Reject groups with conflicting source (every entry for that id
//!    receives a duplicate-conflict error). For non-conflicting unique
//!    groups, skip the upsert when the scheduler already holds
//!    byte-identical source (preserves warm `compile_slot` cache).
//!    Submit upserts via [`VerterHost::upsert_with_priority`] (which
//!    performs the same `semantic_db` pre-invalidation that the
//!    existing public `upsert` does) at the caller-configured priority
//!    and wait for all to commit.
//! 3. **Stage C — compile each unique canonical group exactly once.**
//!    Call [`VerterHost::get_virtual_file`] for `Main`. Per-input panic
//!    isolation is owned by the host batch coordinator's generic catch
//!    boundary (a codegen panic in one input becomes an error
//!    `CompileBatchEntry` for that slot via `compile_panic_entry`,
//!    leaving siblings intact). The cache-hit determination and mode
//!    metadata come back on the response, decided at the single
//!    classification site. Read/process-once invariant: same
//!    canonical+profile is never compiled twice within one batch even
//!    if the input list contains duplicates.
//! 4. **Stage D — fan out.** For each original input position, look up
//!    the result for that canonical and clone its `Arc<str>` payloads
//!    (refcount-only, no string copy).
//!
//! Both parallel stages fan out through the host batch coordinator
//! ([`VerterHost::batch_coordinator`] →
//! [`crate::host_batch_coordinator::HostBatchCoordinator::run_batch`]),
//! the single host-side coordination rule shared with the
//! component-meta batch path. The coordinator installs on the
//! host-owned [`verter_scheduler::HostCpuPool`], built once at host
//! construction with an 8 MiB worker stack so the stack guard applies
//! to every code path (no fall-through to Rayon's global pool with its
//! 1 MiB Windows default). `run_batch` is synchronous: Stage B fully
//! completes before Stage C begins.
//!
//! The coordinator pool's workers register as
//! [`verter_scheduler::caller_kind::CallerKind::External`], so when the
//! compile coordinator blocks on a scheduler completion handle the host
//! worker parks on the condvar rather than inline-executing scheduler
//! CPU tasks. Running the outer wait on the coordinator pool (never the
//! scheduler's stage pool) eliminates the deadlock class where a
//! saturated scheduler CPU pool could starve `compile_many`'s outer
//! collect/order/finalise phase.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use verter_scheduler::stage::Priority;

use crate::hash::hash_16;
use crate::types::{
    CompileCacheMode, CompileProfile, DowngradeReason, FileKind, HostError, HostSeverity,
    UpsertRequest, VirtualNodeKind, VirtualQuery,
};
use crate::VerterHost;

/// Test-only sentinel: any input with this canonical id panics inside
/// [`VerterHost::compile_one_in_batch`]'s worker body, so the panic
/// unwinds through the host batch coordinator's generic catch boundary
/// exactly like a real codegen panic. Used by the
/// `compile_many_isolates_panics` test to verify the production catch
/// path (the coordinator boundary + `compile_panic_entry` conversion),
/// not just the test scaffolding.
#[cfg(test)]
pub(crate) const PANIC_INJECT_SENTINEL: &str = "/__compile_panic_inject__.vue";

/// One file in a batch compile call.
#[derive(Debug, Clone)]
pub struct CompileBatchInput {
    pub canonical_id: String,
    pub source: Arc<str>,
    /// Caller-requested compile cache mode for this input. `None`
    /// inherits the batch default ([`CompileBatchOptions::default_mode`]),
    /// which in turn defaults to [`CompileCacheMode::Session`].
    pub requested_mode: Option<CompileCacheMode>,
}

/// Result for a single original input position. `cache_hit` is `true`
/// iff this input was served from a warm cache slot (the fact-validated
/// session slot OR the content-addressed store), as decided by the
/// single mode classifier and surfaced on the compile response.
#[derive(Clone)]
pub struct CompileBatchEntry {
    pub canonical_id: String,
    pub code: Arc<str>,
    pub source_map: Option<Arc<str>>,
    pub errors: Vec<String>,
    pub duration_ms: f64,
    pub cache_hit: bool,
    /// The compile cache mode the caller requested for this input.
    pub requested_mode: CompileCacheMode,
    /// The compile cache mode the runtime actually ran under (equals
    /// `requested_mode` unless an explicit `Content` request downgraded
    /// to `Stateless`).
    pub actual_mode: CompileCacheMode,
    /// The highest-priority reason the requested mode was constrained,
    /// or `None` when no reason fired.
    pub downgrade_reason: Option<DowngradeReason>,
}

/// Caller-configurable batch options.
///
/// `priority = None` defaults to [`Priority::Background`] (yields to
/// concurrent interactive work). Callers with no concurrent interactive
/// work (benchmarks, CI cold-start measurement) should pass
/// [`Priority::Interactive`]. Worker count is fixed at host
/// construction time via [`crate::HostConfig::host_cpu_threads`] —
/// the host-owned CPU pool is not resized per call.
#[derive(Default, Clone, Debug)]
pub struct CompileBatchOptions {
    pub priority: Option<Priority>,
    /// Default compile cache mode applied to inputs whose
    /// [`CompileBatchInput::requested_mode`] is `None`. `None` resolves
    /// to [`CompileCacheMode::Session`] (the host default).
    pub default_mode: Option<CompileCacheMode>,
}

/// Bundler-default compile profile: production codegen, no SSR, no
/// HMR. `compile_many` always uses this profile internally — no
/// JS-side profile parameter, no IDE preset helper.
pub fn compile_profile_for_bundler() -> CompileProfile {
    CompileProfile {
        is_production: true,
        ssr: false,
        ..CompileProfile::default()
    }
}

impl VerterHost {
    /// Host-backed parallel SFC batch compile.
    ///
    /// See module-level docs for the four-stage algorithm. Returns
    /// one [`CompileBatchEntry`] per input, in the original input
    /// order. Output ordering is fixed by Stage D, not by Stage B/C's
    /// (non-deterministic) HashMap iteration.
    ///
    /// Per-input panic isolation: if `get_virtual_file` panics for one
    /// input, only that input's entry receives a `compiler panic: ...`
    /// error; the rest of the batch completes normally.
    pub fn compile_many(
        &self,
        inputs: Vec<CompileBatchInput>,
        options: CompileBatchOptions,
    ) -> Vec<CompileBatchEntry> {
        // ── short-circuit empty input ──
        // No pool is constructed. Tested by
        // `compile_many_with_zero_inputs`.
        if inputs.is_empty() {
            return Vec::new();
        }

        let profile = compile_profile_for_bundler();
        let priority = options.priority.unwrap_or(Priority::Background);
        // Batch default cache mode; a per-input `requested_mode` overrides
        // it. `None` on both resolves to the host default `Session`.
        let default_mode = options.default_mode.unwrap_or(CompileCacheMode::Session);

        // Fan the batch out through the host batch coordinator — the
        // single host-side coordination rule. The coordinator installs
        // on the host-owned coordinator pool (built once at host
        // construction with an 8 MiB worker stack; workers register as
        // `CallerKind::External`, so the coordinator never inline-
        // executes scheduler CPU tasks while blocked on a completion
        // handle). The outer wait therefore runs on the coordinator
        // pool, never on the scheduler's stage pool. Worker count is
        // fixed at construction time (`HostConfig::host_cpu_threads`)
        // and is not resized per call.
        let coordinator = self.batch_coordinator();

        // ── group + selective upsert ──
        // HashMap iteration is non-deterministic, but we only iterate
        // it for parallel-independent upserts and probe-keys — never
        // for a position-sensitive output. Output order is fixed in
        // Stage D by iterating `inputs` (the caller's order).
        let mut groups: HashMap<String, Vec<&CompileBatchInput>> =
            HashMap::with_capacity(inputs.len());
        for input in &inputs {
            groups
                .entry(input.canonical_id.clone())
                .or_default()
                .push(input);
        }

        // Per-canonical errors discovered in Stage B (duplicate-source
        // conflicts and upsert failures). Surfaced to every original
        // input position belonging to that canonical in Stage D.
        // Source conflicts and upsert failures are properties of the
        // canonical's source, not of the requested mode, so this map is
        // keyed per-canonical and applies to every mode of that canonical.
        let mut group_errors: HashMap<String, String> = HashMap::new();
        let mut canonical_to_upsert: Vec<&CompileBatchInput> = Vec::new();
        // Compile dedup is keyed by `(canonical, effective requested_mode)`:
        // the requested mode is part of the compile identity (a different
        // mode is a genuinely distinct compile with distinct routing and
        // cache side-effects), so two inputs that share a canonical but
        // request different modes each compile exactly once. The effective
        // mode is `input.requested_mode.unwrap_or(default_mode)`, matching
        // the per-input profile built in `compile_one_in_batch`.
        let mut seen_compile_keys: HashSet<(String, CompileCacheMode)> = HashSet::new();
        let mut canonical_to_compile: Vec<&CompileBatchInput> = Vec::new();
        for (canonical_id, group) in &groups {
            let first = group[0];
            let conflict = group
                .iter()
                .skip(1)
                .any(|other| other.source.as_bytes() != first.source.as_bytes());
            if conflict {
                group_errors.insert(
                    canonical_id.clone(),
                    "duplicate canonical_id with conflicting source in batch".to_string(),
                );
                continue;
            }
            // One upsert per canonical (source is mode-independent).
            if self.scheduler_source_differs_from(canonical_id, &first.source) {
                canonical_to_upsert.push(first);
            }
            // One compile per distinct `(canonical, effective mode)`.
            for input in group {
                let effective_mode = input.requested_mode.unwrap_or(default_mode);
                if seen_compile_keys.insert((canonical_id.clone(), effective_mode)) {
                    canonical_to_compile.push(input);
                }
            }
        }

        // The host batch coordinator owns the shared coordination
        // concerns (host-coordinator-pool fan-out, deterministic
        // ordering, the non-reentrant guard, and the generic per-item
        // panic boundary). `compile_many` performs NO per-batch scheduler
        // submission accounting, so its policy carries `scheduler: None`;
        // it supplies only the item work and the domain conversion of a
        // caught panic into an error result for that slot.
        let upsert_policy = crate::host_batch_coordinator::BatchPolicy {
            scheduler: None,
            label: "compile_many_upsert",
            on_item_panic: &|panic: crate::host_batch_coordinator::BatchItemPanic<
                '_,
                &CompileBatchInput,
            >| {
                (
                    panic.item.canonical_id.clone(),
                    Err(format!("upsert panicked: {}", panic.message())),
                )
            },
        };
        let upsert_results = coordinator.run_batch(&canonical_to_upsert, &upsert_policy, |input| {
            // Render the upsert error to a string inside the worker so a
            // caught panic (converted by `on_item_panic`) can produce the
            // same `Result<(), String>` slot shape — the consumer below
            // only needs the message.
            let res = self
                .upsert_with_priority_for_batch(input, priority)
                .map_err(|e| format!("upsert failed: {e}"));
            (input.canonical_id.clone(), res)
        });
        for (id, res) in upsert_results {
            if let Err(message) = res {
                group_errors.entry(id).or_insert(message);
            }
        }

        // ── compile each UNIQUE canonical group exactly once ──
        // `run_batch` is synchronous: this block doesn't begin until
        // Stage B's `run_batch` above has returned. The
        // `compile_one_call_count` test-only counter on `VerterHost` is
        // incremented at the top of `compile_one_in_batch` to make the
        // read-once invariant directly observable.
        //
        // Per-input panic isolation is owned by the coordinator's generic
        // catch boundary: a codegen panic in one input is caught there
        // and handed to this policy's `on_item_panic`, which renders it
        // into an error `CompileBatchEntry` for that slot (the domain
        // conversion). Sibling inputs are unaffected and `compile_many`
        // still returns one entry per input. `compile_many` performs no
        // scheduler submission accounting (`scheduler: None`).
        let compile_policy = crate::host_batch_coordinator::BatchPolicy {
            scheduler: None,
            label: "compile_many",
            on_item_panic: &|panic: crate::host_batch_coordinator::BatchItemPanic<
                '_,
                &CompileBatchInput,
            >| {
                let input = panic.item;
                let effective_mode = input.requested_mode.unwrap_or(default_mode);
                let entry = compile_panic_entry(input, effective_mode, &panic.message());
                ((input.canonical_id.clone(), effective_mode), entry)
            },
        };
        let compiled: HashMap<(String, CompileCacheMode), CompileBatchEntry> = coordinator
            .run_batch(&canonical_to_compile, &compile_policy, |input| {
                let pre_err = group_errors.get(&input.canonical_id).cloned();
                let entry = self.compile_one_in_batch(input, &profile, default_mode, pre_err);
                let effective_mode = input.requested_mode.unwrap_or(default_mode);
                ((input.canonical_id.clone(), effective_mode), entry)
            })
            .into_iter()
            .collect();

        // ── fan out to original input order ──
        // For canonicals that errored in Stage B (duplicate-source
        // conflict) or Stage C (compile/host error / panic), every
        // original input position receives the same error entry.
        // Otherwise each position receives the entry compiled for ITS OWN
        // `(canonical, effective requested_mode)` group, so two positions
        // that share a canonical but requested different modes each carry
        // their own requested / actual mode and downgrade reason. Cloning a
        // `CompileBatchEntry` is refcount-only on the `Arc<str>` payloads —
        // no string allocation.
        inputs
            .iter()
            .map(|input| {
                if let Some(err) = group_errors.get(&input.canonical_id) {
                    // Stage B failed before compile, so the request never
                    // ran: report the requested mode unchanged, no reason.
                    let requested = input.requested_mode.unwrap_or(default_mode);
                    return CompileBatchEntry {
                        canonical_id: input.canonical_id.clone(),
                        code: Arc::from(""),
                        source_map: None,
                        errors: vec![err.clone()],
                        duration_ms: 0.0,
                        cache_hit: false,
                        requested_mode: requested,
                        actual_mode: requested,
                        downgrade_reason: None,
                    };
                }
                let effective_mode = input.requested_mode.unwrap_or(default_mode);
                compiled
                    .get(&(input.canonical_id.clone(), effective_mode))
                    .cloned()
                    .expect("stage C compiled every non-error (canonical, mode) group")
            })
            .collect()
    }

    /// True iff the scheduler holds source for `canonical_id` whose
    /// `whole_hash` matches `hash_16(source.as_bytes())`. Inverted by
    /// the caller to decide whether an upsert is needed.
    fn scheduler_source_differs_from(&self, canonical_id: &str, source: &Arc<str>) -> bool {
        use crate::host_executor::HostSourceData;
        let snap = match self.scheduler.try_get_source(canonical_id) {
            Some(s) => s,
            None => return true,
        };
        let hd = match snap.downcast_data::<HostSourceData>() {
            Some(h) => h,
            None => return true,
        };
        hash_16(source.as_bytes()) != hd.parse.whole_hash
    }

    /// Batch-side upsert wrapper. Goes through `upsert_with_priority`
    /// (NOT `upsert_via_scheduler_with_priority` directly) to preserve
    /// the `semantic_db.invalidate(id)` pre-invalidation that the
    /// existing public `upsert` performs. See
    /// `host_upsert.rs::VerterHost::upsert_with_priority`.
    fn upsert_with_priority_for_batch(
        &self,
        input: &CompileBatchInput,
        priority: Priority,
    ) -> Result<(), HostError> {
        self.upsert_with_priority(
            UpsertRequest {
                canonical_id: Some(input.canonical_id.clone()),
                input_id: input.canonical_id.clone(),
                source: Arc::clone(&input.source),
                file_kind: FileKind::VueSfc,
                aliases: Vec::new(),
            },
            priority,
        )
        .map(|_| ())
    }

    /// Per-input compile worker. The `precomputed_error` slot is
    /// `Some(...)` when Stage B already failed for this canonical
    /// (duplicate-source conflict or upsert error) — the compile is
    /// short-circuited but the test-only call counter is still
    /// incremented at the top.
    fn compile_one_in_batch(
        &self,
        input: &CompileBatchInput,
        profile: &CompileProfile,
        default_mode: CompileCacheMode,
        precomputed_error: Option<String>,
    ) -> CompileBatchEntry {
        // Test-only: increment the call counter at the VERY TOP of the
        // function so every call site is observed, including the
        // precomputed-error short-circuit. Production builds compile
        // this branch out completely; see field doc on
        // `VerterHost::compile_one_call_count`.
        #[cfg(test)]
        self.compile_one_call_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Test-only: record the caller-kind tag of the worker
        // running this `compile_one_in_batch`. Workers running on
        // `HostCpuPool` MUST report `External` (the dual-pool
        // isolation invariant); a regression that ran `compile_many`
        // on the scheduler's CPU pool would record `CpuWorker`
        // instead. Read by
        // `compile_many_workers_carry_host_cpu_pool_id` (secondary
        // caller-kind canary alongside the primary pool-id token
        // assertion).
        #[cfg(test)]
        {
            let tag: u8 = match verter_scheduler::caller_kind::CallerKind::current() {
                verter_scheduler::caller_kind::CallerKind::External => 1,
                verter_scheduler::caller_kind::CallerKind::Driver => 2,
                verter_scheduler::caller_kind::CallerKind::CpuWorker => 3,
                verter_scheduler::caller_kind::CallerKind::IoWorker => 4,
                verter_scheduler::caller_kind::CallerKind::Inline => 5,
            };
            self.compile_one_caller_kind_tag
                .store(tag, std::sync::atomic::Ordering::Relaxed);
            // Record the host-CPU-pool identity token of this worker.
            // The discriminator: a worker running on *this host's*
            // host pool reports `Some(host.host_cpu_pool().pool_id())`;
            // a regression that re-routes `compile_many` onto a
            // per-call Rayon pool or any other `External`-defaulting
            // thread reports `None` (no `start_handler` installed the
            // token). Stored as `usize` with `usize::MAX` reserved as
            // the "unobserved / None" sentinel so the field stays
            // lock-free.
            let token_repr = verter_scheduler::host_cpu_pool_token().unwrap_or(usize::MAX);
            self.compile_one_host_cpu_pool_token
                .store(token_repr, std::sync::atomic::Ordering::Relaxed);
        }

        let start = Instant::now();

        // Effective requested mode for this input, and the per-input
        // profile that carries it into `get_virtual_file`.
        let requested_mode = input.requested_mode.unwrap_or(default_mode);
        let per_input_profile = CompileProfile {
            requested_mode,
            ..profile.clone()
        };

        if let Some(err) = precomputed_error {
            return CompileBatchEntry {
                canonical_id: input.canonical_id.clone(),
                code: Arc::from(""),
                source_map: None,
                errors: vec![err],
                duration_ms: start.elapsed().as_secs_f64() * 1000.0,
                cache_hit: false,
                requested_mode,
                actual_mode: requested_mode,
                downgrade_reason: None,
            };
        }

        // Per-input panic isolation is owned by the host batch
        // coordinator's generic catch boundary (see `compile_many`'s
        // `compile_policy.on_item_panic`). This worker does NOT wrap its
        // own `catch_unwind`: a codegen panic propagates to the
        // coordinator, which catches it and renders the error
        // `CompileBatchEntry` via `compile_panic_entry`. Centralizing the
        // catch keeps one coordination rule for every batch client.
        //
        // Test-only panic injection — fired in the worker so it unwinds
        // through the coordinator's catch exactly like a real codegen
        // panic. Production builds compile this branch out completely.
        #[cfg(test)]
        if input.canonical_id == PANIC_INJECT_SENTINEL {
            panic!("synthetic panic for compile_many_isolates_panics test");
        }
        let result = self.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(input.canonical_id.clone()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: per_input_profile.clone(),
        });

        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        let id_prefix = format!("[{}] ", input.canonical_id);

        match result {
            Ok(response) => {
                let errors: Vec<String> = response
                    .diagnostics
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == HostSeverity::Error)
                    .map(|d| d.message.clone())
                    .collect();
                // The cache-hit determination, actual mode, and downgrade
                // reason are all authoritative on the response (decided at
                // the single classification site inside `get_virtual_file`).
                CompileBatchEntry {
                    canonical_id: input.canonical_id.clone(),
                    code: response.code,
                    source_map: response.source_map,
                    errors,
                    duration_ms,
                    cache_hit: response.cache_hit,
                    requested_mode: response.requested_mode,
                    actual_mode: response.actual_mode,
                    downgrade_reason: response.downgrade_reason,
                }
            }
            // CRITICAL: HostError::CompileError carries a
            // DiagnosticsSnapshot. Its `Display` impl collapses to the
            // static "compile error" string, so a `format!("host
            // error: {host_err}")` would lose every diagnostic. Unpack
            // the variant explicitly so all error-severity diagnostics
            // reach `errors: Vec<String>`. Tested by
            // `compile_many_compile_error_preserves_all_diagnostics`.
            //
            // The compile-failure payload also carries the mode metadata
            // decided at classification time. A compile that errored after
            // a downgrade (e.g. a `Content` request floored to `Stateless`)
            // must report the mode it actually ran under, not the requested
            // mode — so the error entry mirrors the success entry's mode
            // surface instead of resetting to the request.
            Err(HostError::CompileError(failure)) => {
                let mut errors: Vec<String> = failure
                    .diagnostics
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == HostSeverity::Error)
                    .map(|d| format!("{id_prefix}{}", d.message))
                    .collect();
                if errors.is_empty() {
                    errors.push(format!("{id_prefix}compile error (no diagnostic messages)"));
                }
                CompileBatchEntry {
                    canonical_id: input.canonical_id.clone(),
                    code: Arc::from(""),
                    source_map: None,
                    errors,
                    duration_ms,
                    cache_hit: false,
                    requested_mode: failure.requested_mode,
                    actual_mode: failure.actual_mode,
                    downgrade_reason: failure.downgrade_reason,
                }
            }
            Err(host_err) => CompileBatchEntry {
                canonical_id: input.canonical_id.clone(),
                code: Arc::from(""),
                source_map: None,
                errors: vec![format!("{id_prefix}host error: {host_err}")],
                duration_ms,
                cache_hit: false,
                requested_mode,
                actual_mode: requested_mode,
                downgrade_reason: None,
            },
        }
    }
}

/// Render a caught per-input compile panic into the error
/// `CompileBatchEntry` for that slot. The host batch coordinator owns
/// the generic `catch_unwind`; this is the domain conversion
/// `compile_many` supplies through its `BatchPolicy::on_item_panic`, so
/// a panicking input produces a one-error entry (prefixed with the
/// canonical id and `"compiler panic:"`) without aborting the batch or
/// poisoning sibling inputs.
fn compile_panic_entry(
    input: &CompileBatchInput,
    effective_mode: CompileCacheMode,
    message: &str,
) -> CompileBatchEntry {
    CompileBatchEntry {
        canonical_id: input.canonical_id.clone(),
        code: Arc::from(""),
        source_map: None,
        errors: vec![format!("[{}] compiler panic: {}", input.canonical_id, message)],
        duration_ms: 0.0,
        cache_hit: false,
        requested_mode: effective_mode,
        actual_mode: effective_mode,
        downgrade_reason: None,
    }
}
