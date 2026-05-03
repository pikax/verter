//! Host-backed parallel SFC compilation.
//!
//! Bundler/runtime output only. Returns the assembled Main virtual
//! file (script + template render fn). For IDE TSX or TSC type-extract
//! batch surfaces, see future `ide_many` / `public_api_many` (sub-plan
//! §8.3 — out of scope, not deferred).
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
//!    Probe [`VerterHost::compile_slot_is_warm`], then call
//!    [`VerterHost::get_virtual_file`] for `Main` inside
//!    `std::panic::catch_unwind`. Read/process-once invariant: same
//!    canonical+profile is never compiled twice within one batch even
//!    if the input list contains duplicates.
//! 4. **Stage D — fan out.** For each original input position, look up
//!    the result for that canonical and clone its `Arc<str>` payloads
//!    (refcount-only, no string copy).
//!
//! All Rayon work uses ONE locally-built thread pool with an 8 MiB
//! worker stack. The local pool is always built (never falls through
//! to Rayon's global pool) to ensure the stack guard applies to the
//! default path, not just explicit `threads: Some(N)` callers.
//! `pool.install` is synchronous: Stage B fully completes before
//! Stage C begins.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;
use verter_scheduler::stage::Priority;

use crate::hash::hash_16;
use crate::types::{
    CompileProfile, FileKind, HostError, HostSeverity, UpsertRequest, VirtualNodeKind, VirtualQuery,
};
use crate::VerterHost;

/// Test-only sentinel: any input with this canonical id panics inside
/// the production `catch_unwind` boundary inside
/// [`VerterHost::compile_one_in_batch`]. Used by the
/// `compile_many_isolates_panics` test to verify the production catch
/// boundary, not just the test scaffolding.
#[cfg(test)]
pub(crate) const PANIC_INJECT_SENTINEL: &str = "/__phase09b_panic_inject__.vue";

/// One file in a batch compile call.
#[derive(Debug, Clone)]
pub struct CompileBatchInput {
    pub canonical_id: String,
    pub source: Arc<str>,
}

/// Result for a single original input position. `cache_hit` is `true`
/// iff the slot was already warm in `compile_cache` before this call
/// (probed via [`VerterHost::compile_slot_is_warm`]).
#[derive(Clone)]
pub struct CompileBatchEntry {
    pub canonical_id: String,
    pub code: Arc<str>,
    pub source_map: Option<Arc<str>>,
    pub errors: Vec<String>,
    pub duration_ms: f64,
    pub cache_hit: bool,
}

/// Caller-configurable batch options.
///
/// `threads = None` / `Some(0)` resolves to
/// [`std::thread::available_parallelism`]. `priority = None` defaults
/// to [`Priority::Background`] (yields to concurrent interactive
/// work). Callers with no concurrent interactive work (benchmarks,
/// CI cold-start measurement) should pass `Priority::Interactive`.
#[derive(Default, Clone, Debug)]
pub struct CompileBatchOptions {
    pub threads: Option<usize>,
    pub priority: Option<Priority>,
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

        // Build a local Rayon pool with an 8 MiB worker stack. The
        // local pool is ALWAYS built — None / Some(0) resolves to
        // available_parallelism; Rayon's global pool (with the default
        // 1 MiB Windows stack) is never reached, so the stack guard
        // applies to every code path (tested by
        // `compile_many_default_pool_has_8mib_stack`).
        let thread_count = options.threads.filter(|&n| n > 0).unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        });
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count)
            .stack_size(8 * 1024 * 1024)
            .build()
            .expect("build rayon thread pool");

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
        let mut group_errors: HashMap<String, String> = HashMap::new();
        let mut canonical_to_upsert: Vec<&CompileBatchInput> = Vec::new();
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
            canonical_to_compile.push(first);
            if self.scheduler_source_differs_from(canonical_id, &first.source) {
                canonical_to_upsert.push(first);
            }
        }

        let upsert_results = pool.install(|| {
            canonical_to_upsert
                .par_iter()
                .map(|input| {
                    let res = self.upsert_with_priority_for_batch(input, priority);
                    (input.canonical_id.clone(), res)
                })
                .collect::<Vec<_>>()
        });
        for (id, res) in upsert_results {
            if let Err(e) = res {
                group_errors
                    .entry(id)
                    .or_insert_with(|| format!("upsert failed: {e}"));
            }
        }

        // ── compile each UNIQUE canonical group exactly once ──
        // pool.install is synchronous: this block doesn't begin until
        // Stage B's pool.install above has returned. The
        // `compile_one_call_count` test-only counter on `VerterHost` is
        // incremented at the top of `compile_one_in_batch` to make the
        // read-once invariant directly observable.
        let compiled: HashMap<String, CompileBatchEntry> = pool.install(|| {
            canonical_to_compile
                .par_iter()
                .map(|input| {
                    let pre_err = group_errors.get(&input.canonical_id).cloned();
                    let entry = self.compile_one_in_batch(input, &profile, pre_err);
                    (input.canonical_id.clone(), entry)
                })
                .collect()
        });

        // ── fan out to original input order ──
        // For canonicals that errored in Stage B (duplicate-source
        // conflict) or Stage C (compile/host error / panic), every
        // original input position receives the same error entry.
        // Cloning a `CompileBatchEntry` is refcount-only on the
        // `Arc<str>` payloads — no string allocation.
        inputs
            .iter()
            .map(|input| {
                if let Some(err) = group_errors.get(&input.canonical_id) {
                    return CompileBatchEntry {
                        canonical_id: input.canonical_id.clone(),
                        code: Arc::from(""),
                        source_map: None,
                        errors: vec![err.clone()],
                        duration_ms: 0.0,
                        cache_hit: false,
                    };
                }
                compiled
                    .get(&input.canonical_id)
                    .cloned()
                    .expect("stage C compiled every non-error canonical group")
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

        let start = Instant::now();

        if let Some(err) = precomputed_error {
            return CompileBatchEntry {
                canonical_id: input.canonical_id.clone(),
                code: Arc::from(""),
                source_map: None,
                errors: vec![err],
                duration_ms: start.elapsed().as_secs_f64() * 1000.0,
                cache_hit: false,
            };
        }

        // Probe warm state BEFORE the compile call so the result
        // reflects pre-call state (the get_virtual_file call below
        // populates the slot on a cold miss).
        let was_warm = self.compile_slot_is_warm(&input.canonical_id, profile);

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            // Test-only panic injection inside the production
            // `catch_unwind` boundary — same code path as a real
            // codegen panic. Production builds compile this branch
            // out completely.
            #[cfg(test)]
            if input.canonical_id == PANIC_INJECT_SENTINEL {
                panic!("synthetic panic for compile_many_isolates_panics test");
            }
            self.get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some(input.canonical_id.clone()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
        }));

        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        let id_prefix = format!("[{}] ", input.canonical_id);

        match result {
            Ok(Ok(response)) => {
                let errors: Vec<String> = response
                    .diagnostics
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == HostSeverity::Error)
                    .map(|d| d.message.clone())
                    .collect();
                CompileBatchEntry {
                    canonical_id: input.canonical_id.clone(),
                    code: response.code,
                    source_map: response.source_map,
                    errors,
                    duration_ms,
                    cache_hit: was_warm,
                }
            }
            // CRITICAL: HostError::CompileError carries a
            // DiagnosticsSnapshot. Its `Display` impl collapses to the
            // static "compile error" string, so a `format!("host
            // error: {host_err}")` would lose every diagnostic. Unpack
            // the variant explicitly so all error-severity diagnostics
            // reach `errors: Vec<String>`. Tested by
            // `compile_many_compile_error_preserves_all_diagnostics`.
            Ok(Err(HostError::CompileError { diagnostics })) => {
                let mut errors: Vec<String> = diagnostics
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
                }
            }
            Ok(Err(host_err)) => CompileBatchEntry {
                canonical_id: input.canonical_id.clone(),
                code: Arc::from(""),
                source_map: None,
                errors: vec![format!("{id_prefix}host error: {host_err}")],
                duration_ms,
                cache_hit: false,
            },
            Err(panic_payload) => CompileBatchEntry {
                canonical_id: input.canonical_id.clone(),
                code: Arc::from(""),
                source_map: None,
                errors: vec![format!(
                    "{id_prefix}compiler panic: {}",
                    panic_message(&panic_payload)
                )],
                duration_ms,
                cache_hit: false,
            },
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}
