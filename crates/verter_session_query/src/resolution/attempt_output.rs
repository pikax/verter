//! Shared outbound observation records produced by a completed attempt.

use crate::resolution::CanonicalId;

/// One ambient-dependency edge discovered while answering an attempt —
/// `record_ambient_dependency`'s output shape:
/// a genuine workspace dependency-graph mutation the session driver
/// applies after `Complete`, never something the kernel calls directly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AmbientDependency {
    pub consumer_canonical: CanonicalId,
    pub virtual_id: CanonicalId,
}

/// One of `ResolverObservation`'s module-resolution observations
/// (`path_probe`/`real_path`/`package_manifest`) the kernel ACTUALLY
/// CONSUMED before short-circuiting an attempt. A
/// correctness rule: a `NeedInputs` round may prefetch several sibling
/// observations speculatively (staged priority-frontier batching), but
/// the eventual `Complete` attempt's recorded witness must report ONLY
/// what it actually used, never every speculatively-prefetched fact.
///
/// The resolution-witness contract defines "consumed" as every observation
/// along the ACTUAL WINNING FALLTHROUGH CHAIN,
/// including higher-priority candidates checked and rejected (`Absent`)
/// before the eventual winner — e.g. resolving `./mod.js` to a
/// lower-priority `.tsx` sibling still consumes the `Absent` probe of the
/// higher-priority `.ts` sibling checked first, because recording only
/// the winner would serve a stale positive once the `.ts` sibling
/// appears — and, on a miss, the COMPLETE exhausted candidate set, not a
/// sampled subset. "Prefetched-but-not-consumed" is scoped to a
/// DIFFERENT, never-reached branch (e.g. batched `node_modules`
/// ancestor-directory probes fetched speculatively but never examined
/// because an earlier, unrelated branch already resolved first).
///
/// Deliberately a NARROW, dedicated key — NOT a bare `Vec<InputKey>`:
/// `InputKey` means "independently loadable missing input" and carries
/// unrelated variants (`FileContent`, `DeclBody`, `ModuleAugmentationIndex`,
/// `FlowFunctionSkeleton`) that have no place in a consumed-observation
/// witness. A consumed key is only a SELECTOR into the same immutable
/// observation view the attempt already held — it is NOT itself an
/// authoritative version witness (that's `FactVersionRef`'s job, tracked
/// separately via `verter_resolution::AttemptOutput::record_fact`); the session/workspace
/// side translates or enriches a consumed key using that attempt's own
/// observation snapshot before replaying it into the fact tracer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConsumedResolutionObservationKey {
    PathProbe {
        path: CanonicalId,
    },
    RealPath {
        path: CanonicalId,
    },
    PackageManifest {
        directory: CanonicalId,
    },
    /// Ancestor-directory recovery scope — mirrors
    /// `verter_workspace::resolution_currency::ResolutionFactKey::
    /// RecoveryScope { canonical_prefix, .. }` (a distinct production fact,
    /// not subsumed by `DirectoryMembers`). Detects a new file appearing in a previously-
    /// empty/absent ancestor directory chain: `resolution_witness_
    /// contract_tests.rs` retains one of these for every ancestor of
    /// every requested AND resolved path on both the positive and
    /// exhausted-miss paths.
    RecoveryScope {
        canonical_prefix: CanonicalId,
    },
}
