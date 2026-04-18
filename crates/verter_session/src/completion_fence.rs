//! Mandatory completion fence for top-level semantic queries (Phase 1)
//!
//! A [`CompletionFence`] is created at each top-level public entry point that
//! publishes to shared caches — today that is `get_component_meta` and any
//! exported-type query that returns a final payload. Subqueries and artifact
//! lookups append touched dependency facts to the fence as they execute.
//! Before the fence publishes, it revalidates those facts against the live
//! host state. If anything shifted mid-flight, the provisional result is
//! discarded and the solve restarts against the new state.
//!
//! ## Contract
//!
//! - Retries are bounded to [`CompletionFence::MAX_ATTEMPTS`] (= `3`). After
//!   the third failed revalidation, the fence returns
//!   [`FenceOutcome::Unstable`] and **publishes nothing** — torn provisional
//!   results never enter shared caches.
//! - Warm cache hits must contribute their recorded transitive dependency
//!   signatures into the active fence via
//!   [`CompletionFence::merge_signature`]. Final-result validation is
//!   therefore transitive, not root-key-only.
//! - A fence is **not** a cache key, **not** a visibility filter, **not** an
//!   ambient view object.
//! - Singleflight joiners on the same cold entry wait for the winning
//!   builder's final published result, including any fence-triggered
//!   rebuilds.

use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use crate::semantic_query::{DepSignature, DepVersion};

/// Bounded completion fence attached to one top-level query.
///
/// Cheap to construct — intended for one live instance per in-flight
/// `get_component_meta` or shared exported-type query.
#[derive(Debug)]
pub struct CompletionFence {
    inner: Mutex<CompletionFenceInner>,
}

#[derive(Debug)]
struct CompletionFenceInner {
    /// Merged dependency signature observed so far this attempt. Keyed by
    /// `(canonical_id, DepKind)` to prevent duplicate facts from ballooning
    /// the signature.
    observed: FxHashMap<(Arc<str>, DepKindKey), DepVersion>,
    /// Attempt counter (1-indexed for user-facing error reports).
    attempts: u8,
}

/// Small normalized key over the shape of [`DepVersion`] so hash/eq works
/// without cloning the full variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DepKindKey {
    WholeHash,
    RouteGeneration,
    ProjectGeneration,
}

impl DepKindKey {
    fn from_version(v: &DepVersion) -> Self {
        match v {
            DepVersion::WholeHash(_) => DepKindKey::WholeHash,
            DepVersion::RouteGeneration(_) => DepKindKey::RouteGeneration,
            DepVersion::ProjectGeneration(_) => DepKindKey::ProjectGeneration,
        }
    }
}

/// Outcome of [`CompletionFence::run`].
#[derive(Debug, Clone)]
pub enum FenceOutcome<T> {
    /// The final revalidation matched the live host — safe to publish.
    Stable(T),
    /// Retries exhausted (`attempts == MAX_ATTEMPTS`). Nothing was published.
    Unstable { attempts: u8 },
}

/// Result of one inner fence attempt.
pub enum AttemptResult<T> {
    /// The builder produced a value and reports its observed dependency
    /// signature for revalidation.
    Built(T),
    /// The builder aborted internally (cancelled, budget exceeded, etc.).
    /// The fence drops the attempt without publishing.
    Abort,
}

/// Validator abstraction supplied by the caller. The concrete
/// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore) host
/// supplies an implementation that consults live whole-hashes, route
/// generations, and the project generation counter.
pub trait FenceValidator {
    fn validate(&self, canonical_id: &str, version: &DepVersion) -> bool;
}

impl CompletionFence {
    pub const MAX_ATTEMPTS: u8 = 3;

    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CompletionFenceInner {
                observed: FxHashMap::default(),
                attempts: 0,
            }),
        }
    }

    /// Current attempt count (1-indexed after the first attempt starts).
    pub fn attempts(&self) -> u8 {
        self.inner.lock().attempts
    }

    /// Merge a dependency-signature fragment — typically returned by a warm
    /// cache hit. Later-observed facts for the same `(canonical, kind)` pair
    /// overwrite prior ones so the fence always carries the most recently
    /// observed value at validation time.
    pub fn merge_signature(&self, signature: &DepSignature) {
        let mut inner = self.inner.lock();
        for (canonical, version) in signature.iter() {
            inner.observed.insert(
                (canonical.clone(), DepKindKey::from_version(version)),
                version.clone(),
            );
        }
    }

    /// Record a single dep-fact fragment. Useful for cold builders that
    /// observe facts one at a time.
    pub fn observe(&self, canonical: Arc<str>, version: DepVersion) {
        let mut inner = self.inner.lock();
        inner
            .observed
            .insert((canonical, DepKindKey::from_version(&version)), version);
    }

    /// Snapshot the currently-observed signature into a stable ordered
    /// array. Primarily used when publishing a cache entry so the stored
    /// dep-signature reflects what the builder actually touched.
    pub fn observed_signature(&self) -> DepSignature {
        let inner = self.inner.lock();
        let mut entries: Vec<(Arc<str>, DepVersion)> = inner
            .observed
            .iter()
            .map(|((canonical, _), version)| (canonical.clone(), version.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Arc::from(entries.into_boxed_slice())
    }

    /// Drive a top-level query through up to [`Self::MAX_ATTEMPTS`]
    /// attempts. On each attempt, `build` runs a fresh solve; the observed
    /// dep-signature is revalidated against `validator` before the result
    /// is accepted.
    pub fn run<T, V, F>(&self, validator: &V, mut build: F) -> FenceOutcome<T>
    where
        V: FenceValidator,
        F: FnMut(&CompletionFence) -> AttemptResult<T>,
    {
        for attempt in 1..=Self::MAX_ATTEMPTS {
            {
                let mut inner = self.inner.lock();
                inner.observed.clear();
                inner.attempts = attempt;
            }

            let produced = match build(self) {
                AttemptResult::Built(value) => value,
                AttemptResult::Abort => {
                    // Builder-reported abort is terminal — do not retry.
                    return FenceOutcome::Unstable { attempts: attempt };
                }
            };

            let revalidated = {
                let inner = self.inner.lock();
                inner
                    .observed
                    .iter()
                    .all(|((canonical, _), version)| validator.validate(canonical, version))
            };

            if revalidated {
                return FenceOutcome::Stable(produced);
            }
            // Fall through to retry — the observed set is cleared at the
            // top of the next iteration.
        }

        FenceOutcome::Unstable {
            attempts: Self::MAX_ATTEMPTS,
        }
    }
}

impl Default for CompletionFence {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConstValidator {
        verdict: bool,
    }

    impl FenceValidator for ConstValidator {
        fn validate(&self, _canonical_id: &str, _version: &DepVersion) -> bool {
            self.verdict
        }
    }

    #[test]
    fn stable_build_publishes_first_attempt() {
        let fence = CompletionFence::new();
        let validator = ConstValidator { verdict: true };
        let outcome = fence.run(&validator, |f| {
            f.observe(Arc::from("/w/a.ts"), DepVersion::WholeHash([1u8; 16]));
            AttemptResult::Built(42u32)
        });
        match outcome {
            FenceOutcome::Stable(v) => assert_eq!(v, 42),
            FenceOutcome::Unstable { .. } => panic!("expected stable"),
        }
        assert_eq!(fence.attempts(), 1);
    }

    #[test]
    fn retry_bound_is_three_on_persistent_invalidation() {
        let fence = CompletionFence::new();
        let validator = ConstValidator { verdict: false };
        let mut call_count = 0u32;
        let outcome = fence.run(&validator, |f| {
            call_count += 1;
            f.observe(Arc::from("/w/a.ts"), DepVersion::WholeHash([1u8; 16]));
            AttemptResult::Built(call_count)
        });
        match outcome {
            FenceOutcome::Unstable { attempts } => {
                assert_eq!(attempts, CompletionFence::MAX_ATTEMPTS);
            }
            FenceOutcome::Stable(_) => panic!("expected unstable"),
        }
        assert_eq!(call_count, 3);
    }

    #[test]
    fn abort_is_terminal_not_retried() {
        let fence = CompletionFence::new();
        let validator = ConstValidator { verdict: true };
        let mut call_count = 0u32;
        let outcome: FenceOutcome<u32> = fence.run(&validator, |_f| {
            call_count += 1;
            AttemptResult::Abort
        });
        match outcome {
            FenceOutcome::Unstable { attempts } => assert_eq!(attempts, 1),
            FenceOutcome::Stable(_) => panic!("expected unstable"),
        }
        assert_eq!(call_count, 1);
    }

    #[test]
    fn merge_signature_deduplicates_by_kind() {
        let fence = CompletionFence::new();
        let sig: DepSignature = Arc::from(
            vec![
                (
                    Arc::<str>::from("/w/a.ts"),
                    DepVersion::WholeHash([1u8; 16]),
                ),
                // Later fact for the same canonical+kind overwrites.
                (
                    Arc::<str>::from("/w/a.ts"),
                    DepVersion::WholeHash([2u8; 16]),
                ),
                (Arc::<str>::from("/w/a.ts"), DepVersion::RouteGeneration(5)),
            ]
            .into_boxed_slice(),
        );
        fence.merge_signature(&sig);
        let observed = fence.observed_signature();
        assert_eq!(observed.len(), 2);
        // Most recent WholeHash overwrite wins.
        let has_hash_v2 = observed.iter().any(|(c, v)| {
            c.as_ref() == "/w/a.ts" && matches!(v, DepVersion::WholeHash(h) if *h == [2u8; 16])
        });
        assert!(has_hash_v2);
        let has_route_gen = observed
            .iter()
            .any(|(_, v)| matches!(v, DepVersion::RouteGeneration(5)));
        assert!(has_route_gen);
    }

    #[test]
    fn observed_signature_sorted_by_canonical() {
        let fence = CompletionFence::new();
        fence.observe(Arc::from("/w/z.ts"), DepVersion::WholeHash([1u8; 16]));
        fence.observe(Arc::from("/w/a.ts"), DepVersion::WholeHash([2u8; 16]));
        let observed = fence.observed_signature();
        assert_eq!(observed[0].0.as_ref(), "/w/a.ts");
        assert_eq!(observed[1].0.as_ref(), "/w/z.ts");
    }
}
