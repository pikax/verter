//! Cache-runtime substrate.
//!
//! This module owns the deterministic identity types the cache runtime
//! shares across query-identity and content-addressed cache families.
//! It exposes [`WorldSnapshot`] and its scoped-dimension carriers
//! ([`WorldSnapshotDims`], [`ParseEnvDims`], [`ResolveEnvDims`],
//! [`TypeEnvDims`], [`CompileEnvDims`], [`OverlayIdentity`]).
//!
//! The [`singleflight`] submodule owns the cooperative get-or-compute
//! admission primitive ([`ComputeAdmission`](singleflight::ComputeAdmission)
//! plus the `cooperative_*` entry points): exactly-one-computer cold
//! builds over a `DashMap`-backed cache, cooperative joiner waits,
//! panic safety, and post-compute publish-fence revalidation.
//!
//! Architectural invariant: [`WorldSnapshot`] is a request-concurrency
//! identity, not a cache key. Cache layers project the snapshot down
//! to the dimensions they actually depend on via the `*_dims()`
//! accessors. Embedding the full snapshot as a single key field
//! violates R21 (the five env-hash dimensions must remain split) and
//! is statically rejected by
//! `tests/cases/g_misc3/world_snapshot_is_not_a_cache_key.rs`.

pub mod admission;
pub mod candidate_store;
pub mod flow_slice_node;
pub(crate) mod lookup_publish;
pub mod node;
pub mod singleflight;
pub(crate) mod singleflight_publish;
pub mod world_snapshot;

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
// `tests/cases/g_misc3/world_snapshot_is_not_a_cache_key.rs` verifies the types
// exist and have the right structural shape independent of any
// particular consumer, so the substrate must compile clean even
// before a given consumer wires the identity through.
#[allow(unused_imports)]
pub(crate) use world_snapshot::{
    CompileEnvDims, OverlayIdentity, ParseEnvDims, ResolveEnvDims, TypeEnvDims, WorldSnapshot,
    WorldSnapshotDims,
};

// `pub(crate)` re-exports for the cache-runtime substrate types. The
// admission vocabulary (`CacheAdmission` / `SignatureAdmission` /
// `CacheEntry` / `Candidate` / `FactCandidateDiscriminant` /
// `PublishCoreOutcome` / `PublishOutcome` / `NonAdmissionReason`) and the
// node-facing entry points (`ArtifactNode` / `QueryNode` / `ComputeCtx` /
// `QueryFlightKey` / `lookup` / `query::lookup`) are reached through one
// canonical `cache_runtime` path.
//
// `allow(unused_imports)`: the query-node surface (`QueryNode`,
// `query::lookup`, `Candidate`, `FactCandidateDiscriminant`,
// `ReverseIndexedCandidateStore`) and the `SignatureAdmission` finaliser
// are the substrate the query-identity cache families wire onto. The
// single-entry artifact families consume `ArtifactNode` + `lookup` now;
// the query-node consumers wire on top of this exported surface, which is
// exercised by the cache-runtime tests independent of any particular
// consumer.
#[allow(unused_imports)]
pub(crate) use admission::NonAdmissionReason;
#[allow(unused_imports)]
pub use admission::{
    refuse_result_cache_admission_if_partial, CacheAdmission, CacheEntry, Candidate,
    DeferredVictims, FactCandidateDiscriminant, PublishCoreOutcome, PublishOutcome,
};
#[allow(unused_imports)]
pub(crate) use node::{lookup, query, ArtifactNode, ComputeCtx, QueryFlightKey, QueryNode};
#[allow(unused_imports)]
pub(crate) use verter_session_query::facts::fact_cache::SignatureAdmission;

// The shared reverse-indexed multi-candidate store the query-identity
// caches with a per-canonical reverse index route through (imported
// registry, materialise-structure, ref-cycle). It owns candidate
// admission/replacement, the per-canonical reverse index keyed by
// `(key, admission_seq)`, an optional FIFO retention budget, and the
// retention gate. Exercised by the `cache_runtime` tests independent of
// any particular consumer.
#[allow(unused_imports)]
pub(crate) use candidate_store::ReverseIndexedCandidateStore;

// The typed compile-output cache nodes. The content-addressed
// `CompileOutputNodePureContent` and the query-identity
// `CompileOutputNodeFactValidatedSession` own the sole read / write /
// invalidation surface for compiled SFC output; `host_resolve`,
// `host_manage`, `host_lifecycle`, and `host_upsert` route every
// compile-slot access through the session node's typed methods rather
// than touching `ProfileState::compile_slots` directly.
