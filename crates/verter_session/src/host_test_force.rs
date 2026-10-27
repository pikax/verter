//! Host-specific test force-injection knobs grouped off the root `VerterHost`.
//!
//! These are the host-side counters, rendezvous and fence / refusal toggles the
//! in-process tests arm; grouping them into one sub-struct keeps the root
//! `VerterHost` struct thin. The engine's own forcing state is the
//! engine-owned [`TestKnobs`](verter_type_engine::engine_test_knobs::TestKnobs) record this
//! struct holds and the host hands to every engine binding.
//!
//! Each knob is per-host (no process-global concurrency hazard) and defaults to
//! inert. The struct and its `VerterHost` field are gated on
//! `any(test, feature = "test-support")` so the engine record exists wherever
//! the engine's injection sites do; the host-specific knobs, whose readers are
//! all `#[cfg(test)]`, are themselves `#[cfg(test)]`. A production build
//! carries none of it.

#[cfg(test)]
use verter_type_engine::engine_test_knobs::SeamHook;

/// Holds the scheduler's source read of one file until released, so a
/// test observes a load that is in flight. A held read waits at most
/// [`SourceLoadGate::HOLD_LIMIT`], so a test that never releases cannot hang a
/// scheduler worker.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct SourceLoadGate {
    held: parking_lot::Mutex<Option<String>>,
    released: parking_lot::Condvar,
}

#[cfg(test)]
impl SourceLoadGate {
    const HOLD_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

    /// Hold every later source read of `canonical` until [`Self::release`].
    pub(crate) fn hold(&self, canonical: &str) {
        *self.held.lock() = Some(canonical.to_owned());
    }

    /// Let held reads go on.
    pub(crate) fn release(&self) {
        *self.held.lock() = None;
        self.released.notify_all();
    }

    /// Wait while `canonical`'s reads are held.
    pub(crate) fn pass(&self, canonical: &str) {
        let deadline = std::time::Instant::now() + Self::HOLD_LIMIT;
        let mut held = self.held.lock();
        while held.as_deref() == Some(canonical) {
            if self.released.wait_until(&mut held, deadline).timed_out() {
                break;
            }
        }
    }
}

/// Per-host test force-injection knobs. See the module docs.
#[derive(Debug, Default)]
pub(crate) struct TestForceKnobs {
    /// The scheduler source-read hold this host's loader passes through.
    #[cfg(test)]
    pub(crate) source_load_gate: std::sync::Arc<SourceLoadGate>,
    /// Number of file loads this host began, owned or in place.
    #[cfg(test)]
    pub(crate) loads_begun: std::sync::atomic::AtomicUsize,
    /// The engine-owned forcing record, shared with every engine binding and
    /// macro-mirror selector this host attaches.
    pub(crate) engine: std::sync::Arc<verter_type_engine::engine_test_knobs::TestKnobs>,
    /// Cumulative host-level audit state exposed only to in-process tests.
    #[cfg(test)]
    pub(crate) audit: std::sync::Arc<crate::host_test_audit::HostTestAuditState>,
    /// Last scheduler priority observed by `upsert_with_priority`.
    #[cfg(test)]
    pub(crate) last_upsert_priority: parking_lot::Mutex<Option<verter_scheduler::stage::Priority>>,
    /// Number of `compile_one_in_batch` invocations.
    #[cfg(test)]
    pub(crate) compile_one_call_count: std::sync::atomic::AtomicUsize,
    /// Number of full public `HostUpdateResult` payloads materialized by
    /// scheduler-backed admission. `compile_many` needs only success/failure
    /// and must leave this at zero.
    #[cfg(test)]
    pub(crate) upsert_result_materialization_count: std::sync::atomic::AtomicUsize,
    /// Encoded `CallerKind` observed by the latest compile worker.
    #[cfg(test)]
    pub(crate) compile_one_caller_kind_tag: std::sync::atomic::AtomicU8,
    /// Seam fired inside the base `IndexedReady` materialise flight after the
    /// scheduler source snapshot is held and before remaining products are
    /// assembled from it. Fence tests install a content upsert here to assert
    /// every content-addressed product stays one snapshot object — never an
    /// independent later scheduler read.
    #[cfg(test)]
    pub(crate) indexed_source_capture_seam_hook: SeamHook,
    /// One-shot mutation after load preflight and before its visibility write.
    /// Invoked without a derived-state guard so the hook may evict or upsert.
    #[cfg(test)]
    pub(crate) host_load_publish_seam_hook: SeamHook,
    /// Observe admission ownership immediately before taking the contributor
    /// cursor, whose ingestion may synchronously integrate loaded content.
    #[cfg(test)]
    pub(crate) ambient_ingestion_cursor_hook: SeamHook,
    /// Deterministic entry/release rendezvous for the once-per-SFC Vue macro
    /// scheduled closure. The first barrier reports that the winner entered;
    /// the second holds it while a sibling joins and the winner is cancelled.
    #[cfg(test)]
    pub(crate) vue_macro_codegen_build_rendezvous:
        parking_lot::Mutex<Option<std::sync::Arc<(std::sync::Barrier, std::sync::Barrier)>>>,
    /// Observations of the session-wrapper operations that runtime-render
    /// compilation must bypass. Host-backed compilation is the firing control.
    #[cfg(test)]
    pub(crate) wrapper_source_clone_count: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) wrapper_cache_mode_classification_count: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) wrapper_sync_transitive_count: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) wrapper_store_view_read_count: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) wrapper_resolver_ctx_construction_count: std::sync::atomic::AtomicUsize,
    /// When armed, [`VerterHost::ensure_indexed_ready_serve`] treats a would-be
    /// PUBLISHED serve as FENCED (`store_published = false`) and fans a
    /// NON-CACHEABLE read onto every active tracer — the deterministic in-process
    /// equivalent of a same-generation singleflight-race fenced serve, WITHOUT a
    /// `project_generation` bump (so a `GenerationSuperseded` admission gate can
    /// NOT mask the fenced-serve refusal under test). Downstream route /
    /// prepared-decl / augmentation consumers that ride
    /// `ensure_indexed_ready_serve` (`resolve_imported_registry_symbol`, the
    /// module-augmentation stitch, the framework script-fact import resolution)
    /// therefore observe the fenced serve exactly as the production
    /// mid-flight-supersession path produces it, while the served `indexed` still
    /// resolves the value (ReturnOnly). Per-host (no process-global concurrency
    /// hazard). `#[cfg(test)]`-gated: the only reader is the `#[cfg(test)]`
    /// override at the top of `ensure_indexed_ready_serve`.
    #[cfg(test)]
    pub(crate) force_indexed_ready_serve_fence_for_tests: std::sync::atomic::AtomicBool,
    /// Force the owner import-route witness to take its typed refusal arm.
    /// The workspace Decision-DAG contract tests cover the upstream
    /// `ResolutionPublication::Refused` producers, while this seam isolates
    /// the session-side `UnrootableRoute` propagation and reuse carrier.
    #[cfg(test)]
    pub(crate) force_import_route_witness_refusal_for_tests: std::sync::atomic::AtomicBool,
    /// Refuse every type-route resolution of this exact specifier, as a
    /// publication whose final fence failed would. Isolates the session-side
    /// propagation of a refused edge inside a route walk.
    #[cfg(test)]
    pub(crate) force_type_route_refusal_for_specifier: parking_lot::Mutex<Option<String>>,
}
