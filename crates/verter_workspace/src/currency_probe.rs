//! Opt-in wall-clock/call-count probe rail for the path-precise resolution
//! currency chokepoints.
//!
//! COMPILED OUT unless the `currency_probe` feature is on. The feature is
//! absent from `default`, is not requested by any production edge (`verter_napi`,
//! `verter_lsp`, `verter_wasm`, `verter_tsc`), and is enabled only by the
//! `verter_bench` measurement harness. With the feature off,
//! [`probe_scope!`] expands to nothing and this module exposes no state, so a
//! production build carries neither the atomics nor the `Instant::now()` pairs.
//!
//! This exists to attribute the per-file currency tax across
//! `ensure_indexed_ready_serve`, `resolve_snapshot_imports`,
//! `resolve_import_outcome_in_published`, `record_parsed_edges`, and
//! `mutate_overlay_upsert` / `mutate_resolution_session`. Timers NEST: a site
//! that calls another instrumented site reports INCLUSIVE nanoseconds.

/// One instrumented site's totals.
#[cfg(feature = "currency_probe")]
#[derive(Clone, Copy, Debug)]
pub struct ProbeRow {
    pub name: &'static str,
    pub calls: u64,
    pub ns: u64,
}

#[cfg(feature = "currency_probe")]
mod imp {
    use std::sync::atomic::{AtomicU64, Ordering};

    pub struct Site {
        pub name: &'static str,
        pub calls: AtomicU64,
        pub ns: AtomicU64,
    }

    impl Site {
        pub const fn new(name: &'static str) -> Self {
            Self {
                name,
                calls: AtomicU64::new(0),
                ns: AtomicU64::new(0),
            }
        }
    }

    macro_rules! declare_sites {
        ($($ident:ident => $name:literal),* $(,)?) => {
            $(pub static $ident: Site = Site::new($name);)*
            pub static ALL: &[&Site] = &[$(&$ident),*];
        };
    }

    declare_sites! {
        UPSERT_MANY            => "session::upsert_many_with_priority",
        UPSERT_SUBMIT          => "session::upsert.submit_batch_atomic",
        UPSERT_WAIT            => "session::upsert.wait_batch",
        UPSERT_POST_COMMIT     => "session::finish_upsert_post_commit",
        NOTIFY_UPSERT          => "session::notify_upsert",
        REGISTER_FACTS         => "session::register_facts_for_new_content",
        GET_ANALYSIS           => "session::get_analysis_via_view",
        FINALIZE_SNAPSHOT      => "session::finalize_analysis_snapshot",
        RESOLVE_SNAPSHOT_IMPS  => "session::resolve_snapshot_imports",
        ENSURE_INDEXED_READY   => "session::ensure_indexed_ready_serve",
        ENSURE_INDEXED_COLD    => "session::ensure_indexed_ready (cold materialize)",
        MUTATE_OVERLAY_UPSERT  => "workspace::mutate_overlay_upsert",
        MUTATE_RESOLUTION_SESS => "workspace::mutate_resolution_session",
        RECORD_PARSED_EDGES    => "workspace::record_parsed_edges",
        RECORD_EDGES_FROZEN    => "workspace::record_parsed_edges_with_frozen_evidence",
        RESOLVE_IN_PUBLISHED   => "workspace::resolve_import_outcome_in_published",
        RESOLVE_ATTEMPT        => "  ..resolve: one world attempt",
        RESOLVE_CAPTURE_WORLD  => "  ..resolve: capture_stable_resolution_world",
        RESOLVE_REFRESH_EVID   => "  ..resolve: refresh_resolution_evidence",
        RESOLVE_TRACKED        => "  ..resolve: resolver.resolve_tracked",
        RESOLVE_TXN_FINISH     => "  ..resolve: transaction.finish (signature)",
        RESOLVE_FOLD_EVIDENCE  => "  ..resolve: fold_observed_base_evidence",
        RESOLVE_ADMIT          => "  ..resolve: admit_resolution_candidate",
        RESOLVE_PUBLISH_LOCK   => "  ..resolve: publication lock acquire",
        FINISH_COLLECT         => "    ..finish: clone observations into FactReadSet",
        FINISH_SORT            => "    ..finish: canonicalise (sort + dedup + run merge)",
        FINISH_ARC             => "    ..finish: Arc::from(observations)",
        OBS_PRE_DEDUP          => "    [tally] observations pre-dedup",
        OBS_POST_DEDUP         => "    [tally] observations post-dedup",
        ABSORBED_RUN_FACTS     => "    [tally] facts entering via absorbed canonical runs",
    }

    pub struct Guard {
        site: &'static Site,
        start: std::time::Instant,
    }

    impl Guard {
        #[inline]
        pub fn new(site: &'static Site) -> Self {
            Self {
                site,
                start: std::time::Instant::now(),
            }
        }
    }

    impl Drop for Guard {
        #[inline]
        fn drop(&mut self) {
            self.site.calls.fetch_add(1, Ordering::Relaxed);
            self.site
                .ns
                .fetch_add(self.start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }
    }

    pub fn snapshot() -> Vec<super::ProbeRow> {
        ALL.iter()
            .map(|s| super::ProbeRow {
                name: s.name,
                calls: s.calls.load(Ordering::Relaxed),
                ns: s.ns.load(Ordering::Relaxed),
            })
            .filter(|r| r.calls > 0)
            .collect()
    }

    pub fn reset() {
        for s in ALL {
            s.calls.store(0, Ordering::Relaxed);
            s.ns.store(0, Ordering::Relaxed);
        }
    }
}

#[cfg(feature = "currency_probe")]
pub use imp::{reset, snapshot, Guard, Site};

#[cfg(feature = "currency_probe")]
#[doc(hidden)]
pub mod sites {
    pub use super::imp::*;
}

/// Time the enclosing scope into the named probe site.
///
/// Expands to nothing when `currency_probe` is off.
#[cfg(feature = "currency_probe")]
#[macro_export]
macro_rules! probe_scope {
    ($site:ident) => {
        let _probe_guard =
            $crate::currency_probe::Guard::new(&$crate::currency_probe::sites::$site);
    };
}

#[cfg(not(feature = "currency_probe"))]
#[macro_export]
macro_rules! probe_scope {
    ($site:ident) => {};
}

/// Tally `$amount` into the named probe site's accumulator (one call each).
/// Used for non-time quantities (observation counts). Expands to nothing when
/// `currency_probe` is off — `$amount` is NOT evaluated.
#[cfg(feature = "currency_probe")]
#[macro_export]
macro_rules! probe_tally {
    ($site:ident, $amount:expr) => {{
        use std::sync::atomic::Ordering;
        let site = &$crate::currency_probe::sites::$site;
        site.calls.fetch_add(1, Ordering::Relaxed);
        site.ns.fetch_add($amount as u64, Ordering::Relaxed);
    }};
}

#[cfg(not(feature = "currency_probe"))]
#[macro_export]
macro_rules! probe_tally {
    ($site:ident, $amount:expr) => {};
}
