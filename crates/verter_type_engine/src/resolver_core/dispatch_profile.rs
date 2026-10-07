//! Measurement-only call counts for the resolver-context ports.
//!
//! The engine reaches its host through `&dyn ResolverContext<C>`. Whether a
//! port method is hot enough to deserve a concrete, inlinable read instead of
//! a virtual call is a question about call frequency per request, so each
//! port method's implementation names itself with
//! [`count_resolver_context_call!`](crate::count_resolver_context_call) and
//! this module tallies the calls.
//!
//! The counters exist only under the default-off `semantic-observe` feature
//! (classified OPTIONAL in `docs/arch/semantic-observe.md`). Without it the
//! macro expands to nothing and [`snapshot`] / [`reset`] do not resolve, so a
//! default build carries no counter, no atomic and no registration, and no
//! production code can read a count.
//!
//! Counts are process-wide. A request's profile is the difference between
//! two snapshots taken around it (or a [`reset`] before it and a
//! [`snapshot`] after) while no other request runs.

/// Record one call of the resolver-context port method `$method`
/// (`"Port::method"`).
///
/// Expands to nothing without the `semantic-observe` feature.
#[cfg(feature = "semantic-observe")]
#[macro_export]
macro_rules! count_resolver_context_call {
    ($method:literal) => {{
        static COUNTER: $crate::resolver_core::dispatch_profile::MethodCounter =
            $crate::resolver_core::dispatch_profile::MethodCounter::new($method);
        COUNTER.hit();
    }};
}

/// Record one call of the resolver-context port method `$method`
/// (`"Port::method"`).
///
/// Expands to nothing without the `semantic-observe` feature.
#[cfg(not(feature = "semantic-observe"))]
#[macro_export]
macro_rules! count_resolver_context_call {
    ($method:literal) => {{}};
}

#[cfg(feature = "semantic-observe")]
pub use enabled::{reset, snapshot, MethodCallCount, MethodCounter};

#[cfg(feature = "semantic-observe")]
mod enabled {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;

    /// Every counter that has been hit at least once since the process
    /// started. A counter registers itself on its first hit, so the registry
    /// holds exactly the methods a measurement exercised.
    static REGISTRY: Mutex<Vec<&'static MethodCounter>> = Mutex::new(Vec::new());

    /// One port method's call counter. Created only by
    /// [`count_resolver_context_call!`](crate::count_resolver_context_call).
    pub struct MethodCounter {
        method: &'static str,
        calls: AtomicU64,
        registered: AtomicBool,
    }

    impl MethodCounter {
        #[doc(hidden)]
        pub const fn new(method: &'static str) -> Self {
            Self {
                method,
                calls: AtomicU64::new(0),
                registered: AtomicBool::new(false),
            }
        }

        #[doc(hidden)]
        #[inline]
        pub fn hit(&'static self) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if !self.registered.load(Ordering::Relaxed)
                && !self.registered.swap(true, Ordering::AcqRel)
            {
                REGISTRY
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(self);
            }
        }
    }

    /// One method's call count in a [`snapshot`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MethodCallCount {
        /// `"Port::method"`.
        pub method: &'static str,
        /// Calls since the last [`reset`].
        pub calls: u64,
    }

    /// Every method called since the last [`reset`], with its count, sorted
    /// by method name. Methods not called since the reset are omitted.
    ///
    /// One counter exists per macro expansion site, and a port implemented
    /// through a shared macro can expand the same `"Port::method"` label in
    /// several impls, so several live counters can carry one label. Rows are
    /// summed by label: the snapshot reports one count per method, not per
    /// expansion site.
    pub fn snapshot() -> Vec<MethodCallCount> {
        let registry = REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut by_method: std::collections::BTreeMap<&'static str, u64> =
            std::collections::BTreeMap::new();
        for counter in registry.iter() {
            let calls = counter.calls.load(Ordering::Relaxed);
            if calls > 0 {
                *by_method.entry(counter.method).or_insert(0) += calls;
            }
        }
        by_method
            .into_iter()
            .map(|(method, calls)| MethodCallCount { method, calls })
            .collect()
    }

    /// Zero every counter.
    pub fn reset() {
        for counter in REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
        {
            counter.calls.store(0, Ordering::Relaxed);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn tally(method: &str) -> u64 {
            snapshot()
                .into_iter()
                .find(|count| count.method == method)
                .map_or(0, |count| count.calls)
        }

        #[test]
        fn each_call_site_counts_its_own_calls_and_reset_zeroes_them() {
            fn probe_a() {
                crate::count_resolver_context_call!("ProfileTest::a");
            }
            fn probe_b() {
                crate::count_resolver_context_call!("ProfileTest::b");
            }
            for _ in 0..3 {
                probe_a();
            }
            probe_b();
            assert_eq!(tally("ProfileTest::a"), 3);
            assert_eq!(tally("ProfileTest::b"), 1);
            reset();
            assert_eq!(tally("ProfileTest::a"), 0);
            probe_a();
            assert_eq!(tally("ProfileTest::a"), 1);
        }

        /// The macro declares its counter inside the method body, so two
        /// impls sharing one label own two private counters. A snapshot
        /// reports one row per label with the SUM, not one row per counter
        /// (and a consumer folding rows into a map would silently drop all
        /// but the last).
        #[test]
        fn counters_sharing_one_label_are_summed_into_one_row() {
            fn first_impl() {
                crate::count_resolver_context_call!("ProfileTest::shared");
            }
            fn second_impl() {
                crate::count_resolver_context_call!("ProfileTest::shared");
            }
            reset();
            for _ in 0..3 {
                first_impl();
            }
            second_impl();
            let rows = snapshot();
            let shared: Vec<_> = rows
                .iter()
                .filter(|count| count.method == "ProfileTest::shared")
                .collect();
            assert_eq!(
                shared.len(),
                1,
                "one row per method label regardless of how many impls carry it: {rows:?}"
            );
            assert_eq!(shared[0].calls, 4);
        }
    }
}
