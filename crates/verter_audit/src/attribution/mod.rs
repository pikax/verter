//! Measurement-only work attribution.
//!
//! A typed, closed schema of work sites ([`WorkSite`]) plus a counter
//! table, so that the cost of a strategic operation can be explained by
//! WHAT ran and HOW OFTEN rather than by a profiler's symbol names.
//!
//! ## What this is not
//!
//! It is not a semantic authority, and it is structurally incapable of
//! becoming one. The schema types ([`WorkSite`], [`WorkDomain`],
//! [`WorkUnit`]) compile unconditionally, because the recording macros
//! name a site even when disabled — but they carry no storage and
//! expose no value. EVERYTHING that can produce a number —
//! [`snapshot`], [`snapshot_all`], [`read`], [`reset`], [`SiteSample`],
//! `record_*`, the report renderers — is behind the non-default
//! `attribution` feature.
//!
//! So a default build cannot write `if attribution::read(site).calls > n`:
//! the path does not resolve. There is no runtime flag to audit, no
//! "disabled" stub returning zero that a caller could branch on by
//! accident, and no way for a counter to reach a decision without a
//! Cargo feature change that shows up in review. `verter_audit` is a
//! leaf crate (`verter_span` only) and this module adds no dependency,
//! so the confinement holds for every consumer.
//!
//! ## Cost when disabled
//!
//! With the feature off, [`attribute!`], [`attribute_n!`],
//! [`attribute_scope!`], [`attribute_max!`] and [`attribute_digest!`]
//! expand to a single `const` item that names the site and nothing else.
//! A `const` with no reads produces no code, so a disabled build carries
//! no atomics, no clock reads, and no branch — and the amount/digest
//! ARGUMENT IS NEVER EVALUATED, so instrumenting a site with an
//! expensive quantity (`v.iter().map(len).sum()`) costs nothing when the
//! feature is off.
//!
//! Naming the site in the disabled arm is deliberate: a typo'd or
//! deleted variant fails a DEFAULT build, so instrumentation cannot rot
//! silently behind a feature nobody enables. The amount expression is
//! NOT type-checked in the disabled arm — that would require mentioning
//! it, and mentioning it in any form (`if false`, an uncalled closure)
//! either emits code or takes a borrow that can conflict with the
//! surrounding function. `cargo check -p <crate> --features
//! verter_audit/attribution` is what type-checks the amounts; the
//! disabled arm's job is only to pin the site names.
//!
//! ## Usage
//!
//! ```ignore
//! use verter_audit::{attribute_n, attribute_scope};
//! use verter_audit::attribution::WorkSite;
//!
//! fn hash_16(input: &[u8]) -> [u8; 16] {
//!     attribute_n!(ContentHash, input.len());
//!     // ...
//! }
//!
//! fn build_index(source: &str) -> Index {
//!     attribute_scope!(IndexedReadyBuild);
//!     // times this region AND attributes its heap traffic
//! }
//! ```

mod schema;

pub use schema::{WorkDomain, WorkSite, WorkUnit};

#[cfg(feature = "attribution")]
mod alloc;
#[cfg(feature = "attribution")]
pub mod report;
#[cfg(feature = "attribution")]
mod scope;
#[cfg(feature = "attribution")]
mod table;

#[cfg(feature = "attribution")]
pub use alloc::AttributingAllocator;
#[cfg(feature = "attribution")]
pub use scope::ScopeGuard;
#[cfg(feature = "attribution")]
pub use table::{
    read, record_amount, record_call, record_digest, record_scope, reset, snapshot, snapshot_all,
    SiteSample,
};

#[cfg(all(test, not(feature = "attribution")))]
mod disabled_tests;
#[cfg(all(test, feature = "attribution"))]
mod table_tests;

/// Record one hit on a work site.
///
/// Expands to a site-name check and nothing else when the `attribution`
/// feature is off.
#[cfg(feature = "attribution")]
#[macro_export]
macro_rules! attribute {
    ($site:ident) => {{
        $crate::attribution::record_call($crate::attribution::WorkSite::$site);
    }};
}

/// Record one hit on a work site.
///
/// Expands to a site-name check and nothing else when the `attribution`
/// feature is off.
#[cfg(not(feature = "attribution"))]
#[macro_export]
macro_rules! attribute {
    ($site:ident) => {{
        const _: $crate::attribution::WorkSite = $crate::attribution::WorkSite::$site;
    }};
}

/// Record one hit carrying `$amount` in the site's declared unit.
///
/// `$amount` is cast with `as u64`, so any integer expression works.
/// When the `attribution` feature is off `$amount` IS NOT EVALUATED.
#[cfg(feature = "attribution")]
#[macro_export]
macro_rules! attribute_n {
    ($site:ident, $amount:expr) => {{
        #[allow(clippy::unnecessary_cast)]
        let amount = $amount as u64;
        $crate::attribution::record_amount($crate::attribution::WorkSite::$site, amount);
    }};
}

/// Record one hit carrying `$amount` in the site's declared unit.
///
/// `$amount` is cast with `as u64`, so any integer expression works.
/// When the `attribution` feature is off `$amount` IS NOT EVALUATED.
#[cfg(not(feature = "attribution"))]
#[macro_export]
macro_rules! attribute_n {
    ($site:ident, $amount:expr) => {{
        const _: $crate::attribution::WorkSite = $crate::attribution::WorkSite::$site;
    }};
}

/// Raise a gauge site's high-water mark to `$amount`.
///
/// Only meaningful on a [`WorkUnit::Gauge`](crate::attribution::WorkUnit::Gauge)
/// site; on a summing site this is identical to [`attribute_n!`].
/// When the `attribution` feature is off `$amount` IS NOT EVALUATED.
#[cfg(feature = "attribution")]
#[macro_export]
macro_rules! attribute_max {
    ($site:ident, $amount:expr) => {{
        #[allow(clippy::unnecessary_cast)]
        let amount = $amount as u64;
        $crate::attribution::record_amount($crate::attribution::WorkSite::$site, amount);
    }};
}

/// Raise a gauge site's high-water mark to `$amount`.
///
/// Only meaningful on a [`WorkUnit::Gauge`](crate::attribution::WorkUnit::Gauge)
/// site; on a summing site this is identical to [`attribute_n!`].
/// When the `attribution` feature is off `$amount` IS NOT EVALUATED.
#[cfg(not(feature = "attribution"))]
#[macro_export]
macro_rules! attribute_max {
    ($site:ident, $amount:expr) => {{
        const _: $crate::attribution::WorkSite = $crate::attribution::WorkSite::$site;
    }};
}

/// Time the enclosing scope into a work site, and own that scope for
/// heap attribution.
///
/// Binds a guard in the current block; timing is INCLUSIVE of nested
/// guards. Expands to a site-name check and nothing else when the
/// `attribution` feature is off.
#[cfg(feature = "attribution")]
#[macro_export]
macro_rules! attribute_scope {
    ($site:ident) => {
        let _attribution_scope_guard =
            $crate::attribution::ScopeGuard::enter($crate::attribution::WorkSite::$site);
    };
}

/// Time the enclosing scope into a work site, and own that scope for
/// heap attribution.
///
/// Binds a guard in the current block; timing is INCLUSIVE of nested
/// guards. Expands to a site-name check and nothing else when the
/// `attribution` feature is off.
#[cfg(not(feature = "attribution"))]
#[macro_export]
macro_rules! attribute_scope {
    ($site:ident) => {
        let _attribution_scope_guard: () = {
            const _: $crate::attribution::WorkSite = $crate::attribution::WorkSite::$site;
        };
    };
}

/// Fold `$value` into a site's order-independent determinism digest.
///
/// Two runs that produced the same multiset of values agree, whatever
/// order the threads reported them in. When the `attribution` feature is
/// off `$value` IS NOT EVALUATED.
#[cfg(feature = "attribution")]
#[macro_export]
macro_rules! attribute_digest {
    ($site:ident, $value:expr) => {{
        #[allow(clippy::unnecessary_cast)]
        let value = $value as u64;
        $crate::attribution::record_digest($crate::attribution::WorkSite::$site, value);
    }};
}

/// Fold `$value` into a site's order-independent determinism digest.
///
/// Two runs that produced the same multiset of values agree, whatever
/// order the threads reported them in. When the `attribution` feature is
/// off `$value` IS NOT EVALUATED.
#[cfg(not(feature = "attribution"))]
#[macro_export]
macro_rules! attribute_digest {
    ($site:ident, $value:expr) => {{
        const _: $crate::attribution::WorkSite = $crate::attribution::WorkSite::$site;
    }};
}
