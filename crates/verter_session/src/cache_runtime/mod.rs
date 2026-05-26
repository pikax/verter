//! Cache-runtime substrate.
//!
//! This module owns the deterministic identity types the cache runtime
//! shares across query-identity and content-addressed cache families.
//! Today it exposes [`WorldSnapshot`] and its scoped-dimension carriers
//! ([`WorldSnapshotDims`], [`ParseEnvDims`], [`ResolveEnvDims`],
//! [`TypeEnvDims`], [`CompileEnvDims`], [`OverlayIdentity`]).
//!
//! Architectural invariant: [`WorldSnapshot`] is a request-concurrency
//! identity, not a cache key. Cache layers project the snapshot down
//! to the dimensions they actually depend on via the `*_dims()`
//! accessors. Embedding the full snapshot as a single key field
//! violates R21 (the five env-hash dimensions must remain split) and
//! is statically rejected by
//! `tests/world_snapshot_is_not_a_cache_key.rs`.

mod world_snapshot;

// `pub(crate)` re-exports so other modules inside `verter_session`
// can reach `WorldSnapshot` and its `*Dims` companions through one
// canonical path. The `cache_runtime` module itself is `pub(crate)`
// in `lib.rs`, so these re-exports do NOT extend the public crate
// surface. There is no `for_tests` re-export for these types — the
// construction contract is exercised by `#[cfg(test)] mod tests`
// inline in `world_snapshot.rs`.
//
// `allow(unused_imports)` is intentional: the substrate exposes
// `WorldSnapshot` and its `*Dims` companions as canonical identity
// types that downstream cache-runtime consumers thread through every
// query- and content-addressed cache family. The architecture guard
// `tests/world_snapshot_is_not_a_cache_key.rs` verifies the types
// exist and have the right structural shape independent of any
// particular consumer, so the substrate must compile clean even
// before a given consumer wires the identity through.
#[allow(unused_imports)]
pub(crate) use world_snapshot::{
    CompileEnvDims, OverlayIdentity, ParseEnvDims, ResolveEnvDims, TypeEnvDims, WorldSnapshot,
    WorldSnapshotDims,
};
