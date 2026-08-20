//! Measurement-only work attribution.
//!
//! Closed [`WorkSite`] schema plus a counter table. Not a semantic
//! authority and structurally unable to become one: schema types compile
//! unconditionally (macros name a site even when disabled) but carry no
//! storage. Everything that can produce a number — [`snapshot`],
//! [`read`], `record_*`, the report renderers — is behind the
//! non-default `attribution` feature. A default build cannot write
//! `if attribution::read(site).calls > n`: the path does not resolve.
//! There is no disabled stub returning zero.
//!
//! With the feature off, [`attribute!`] / [`attribute_n!`] /
//! [`attribute_scope!`] / [`attribute_max!`] / [`attribute_digest!`]
//! expand to a `const` that names the site. The amount/digest argument
//! is **not evaluated**. Naming the site in the disabled arm is
//! deliberate: a typo fails a default build. Amounts are type-checked
//! only under `--features verter_audit/attribution`.
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
