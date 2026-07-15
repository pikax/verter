//! Opaque session-owned cache identity for the scheduler.
//!
//! The cache layer above the scheduler issues these ids; the scheduler
//! only uses them as discriminators on a work-node identity
//! ([`crate::dag::WorkNodeIdentity::CacheNode`]). It NEVER interprets the
//! bytes — cache-family semantics (which store, which key axes) live in
//! the session crate, never here ([`crate::dedupe_hook`] documents the
//! same H20 leaf boundary).
//!
//! The type is a plain transparent newtype over `u64`. It is deliberately
//! NOT an enum: an enum would leak session cache-family meaning into the
//! scheduler and create a second source of truth for cache identity. The
//! scheduler stays domain-agnostic — the opaque id is the discriminator,
//! and the session owns its interpretation.

/// Opaque session-owned cache identity for
/// [`crate::dag::WorkNodeIdentity::CacheNode`].
///
/// The cache layer above the scheduler issues these ids; the scheduler
/// only uses them as discriminators on the work node identity. The type
/// is `Copy` so it composes cheaply into hash keys, and `Ord` so it can
/// sit inside ordered keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SchedulerCacheId(pub u64);
