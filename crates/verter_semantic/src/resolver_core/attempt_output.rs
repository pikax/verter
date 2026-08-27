//! `AttemptOutput` is the attempt-local accumulator for OUTBOUND facts a
//! kernel attempt produces alongside a `Complete` answer.
//! Distinct from
//! `ResolverObservation`'s 13 INBOUND methods (the kernel asks the
//! session-provided implementor a question, gets an immutable answer) —
//! this is the OPPOSITE direction: things the kernel discovered while
//! answering, for the session-side driver to apply AFTER the attempt
//! reaches `Complete`. A `NeedInputs`/`Terminal` result discards its
//! attempt's accumulator entirely; only `Complete` transfers it to the
//! driver.
//!
//! `AttemptOutcome::Complete(T)` remains the inbound observation protocol;
//! attaching outbound effects to each observation response would invert
//! ownership. The top-level [`crate::resolver_core::CompletedAttempt`]
//! instead pairs the completed answer with this accumulator, and the
//! workspace driver applies it only after completion.

use crate::resolver_core::CanonicalId;
use rustc_hash::FxHashSet;

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
/// separately via [`AttemptOutput::record_fact`]); the session/workspace
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

/// The accumulator itself. Fields are private and there is no public
/// struct literal, so fields can grow additively without breaking callers.
///
/// `AttemptOutput` retains the first occurrence of each raw fact or edge in
/// candidate order. This idempotent normalization prevents repeated resolver
/// candidates from multiplying an identical witness. Every distinct entry is
/// retained against the operation ledger's shared tagged whole-output meter;
/// a prospective breach is terminal before the entry is inserted, and the
/// whole attempt is discarded.
#[derive(Debug)]
pub struct AttemptOutput {
    observed_facts: Vec<crate::facts::version::FactVersionRef>,
    ambient_dependencies: Vec<AmbientDependency>,
    consumed_resolution_observations: Vec<ConsumedResolutionObservationKey>,
    observed_fact_set: FxHashSet<crate::facts::version::FactVersionRef>,
    ambient_dependency_set: FxHashSet<AmbientDependency>,
    consumed_resolution_observation_set: FxHashSet<ConsumedResolutionObservationKey>,
    retention: super::input_resolution_budgets::InputResolutionRetention,
}

impl AttemptOutput {
    /// A fresh, empty accumulator — one per attempt.
    #[must_use]
    pub fn new() -> Self {
        Self {
            observed_facts: Vec::new(),
            ambient_dependencies: Vec::new(),
            consumed_resolution_observations: Vec::new(),
            observed_fact_set: FxHashSet::default(),
            ambient_dependency_set: FxHashSet::default(),
            consumed_resolution_observation_set: FxHashSet::default(),
            retention:
                super::input_resolution_budgets::InputResolutionRetention::current_or_default(),
        }
    }

    /// Record one observed fact — `observe_borrowed_signature`'s output
    /// shape.
    pub fn record_fact(
        &mut self,
        fact: crate::facts::version::FactVersionRef,
    ) -> Result<(), super::AttemptFailure> {
        if self.observed_fact_set.insert(fact.clone()) {
            if let Err(failure) = self.retention.retain_completed_witness(
                super::input_resolution_budgets::CompletedWitnessRetentionKey::Fact(fact.clone()),
            ) {
                self.observed_fact_set.remove(&fact);
                return Err(failure);
            }
            self.observed_facts.push(fact);
        }
        Ok(())
    }

    /// Record one ambient-dependency edge — `record_ambient_dependency`'s
    /// output shape.
    pub fn record_ambient_dependency(
        &mut self,
        consumer_canonical: CanonicalId,
        virtual_id: CanonicalId,
    ) -> Result<(), super::AttemptFailure> {
        let dependency = AmbientDependency {
            consumer_canonical,
            virtual_id,
        };
        if self.ambient_dependency_set.insert(dependency.clone()) {
            if let Err(failure) = self.retention.retain_completed_witness(
                super::input_resolution_budgets::CompletedWitnessRetentionKey::AmbientDependency(
                    dependency.clone(),
                ),
            ) {
                self.ambient_dependency_set.remove(&dependency);
                return Err(failure);
            }
            self.ambient_dependencies.push(dependency);
        }
        Ok(())
    }

    /// Record one module-resolution observation the kernel actually
    /// consumed (not merely speculatively prefetched) before
    /// short-circuiting.
    pub fn record_consumed_resolution_observation(
        &mut self,
        key: ConsumedResolutionObservationKey,
    ) -> Result<(), super::AttemptFailure> {
        if self.consumed_resolution_observation_set.insert(key.clone()) {
            if let Err(failure) = self.retention.retain_completed_witness(
                super::input_resolution_budgets::CompletedWitnessRetentionKey::ConsumedResolutionObservation(
                    key.clone(),
                ),
            ) {
                self.consumed_resolution_observation_set.remove(&key);
                return Err(failure);
            }
            self.consumed_resolution_observations.push(key);
        }
        Ok(())
    }

    #[must_use]
    pub fn observed_facts(&self) -> &[crate::facts::version::FactVersionRef] {
        &self.observed_facts
    }

    #[must_use]
    pub fn ambient_dependencies(&self) -> &[AmbientDependency] {
        &self.ambient_dependencies
    }

    #[must_use]
    pub fn consumed_resolution_observations(&self) -> &[ConsumedResolutionObservationKey] {
        &self.consumed_resolution_observations
    }

    /// Merge `other`'s recorded output into `self` — for composing a
    /// parent attempt's output from sub-attempts it delegated to (e.g. the
    /// recursive project-reference walk's per-node outputs folding into
    /// the enclosing resolution's output).
    pub fn merge(&mut self, other: AttemptOutput) -> Result<(), super::AttemptFailure> {
        for fact in &other.observed_facts {
            self.record_fact(fact.clone())?;
        }
        for dependency in &other.ambient_dependencies {
            self.record_ambient_dependency(
                dependency.consumer_canonical.clone(),
                dependency.virtual_id.clone(),
            )?;
        }
        for observation in &other.consumed_resolution_observations {
            self.record_consumed_resolution_observation(observation.clone())?;
        }
        Ok(())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observed_facts.is_empty()
            && self.ambient_dependencies.is_empty()
            && self.consumed_resolution_observations.is_empty()
    }
}

impl Default for AttemptOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AttemptOutput {
    fn clone(&self) -> Self {
        let retention = self.retention.clone();
        retention.scope(|| {
            let mut cloned = Self::new();
            for fact in &self.observed_facts {
                cloned
                    .record_fact(fact.clone())
                    .expect("duplicates fit the shared retention set");
            }
            for dependency in &self.ambient_dependencies {
                cloned
                    .record_ambient_dependency(
                        dependency.consumer_canonical.clone(),
                        dependency.virtual_id.clone(),
                    )
                    .expect("duplicates fit the shared retention set");
            }
            for observation in &self.consumed_resolution_observations {
                cloned
                    .record_consumed_resolution_observation(observation.clone())
                    .expect("duplicates fit the shared retention set");
            }
            cloned
        })
    }
}

impl PartialEq for AttemptOutput {
    fn eq(&self, other: &Self) -> bool {
        self.observed_facts == other.observed_facts
            && self.ambient_dependencies == other.ambient_dependencies
            && self.consumed_resolution_observations == other.consumed_resolution_observations
    }
}

impl Eq for AttemptOutput {}

impl Drop for AttemptOutput {
    fn drop(&mut self) {
        for fact in &self.observed_facts {
            self.retention.release_completed_witness(
                &super::input_resolution_budgets::CompletedWitnessRetentionKey::Fact(fact.clone()),
            );
        }
        for dependency in &self.ambient_dependencies {
            self.retention.release_completed_witness(
                &super::input_resolution_budgets::CompletedWitnessRetentionKey::AmbientDependency(
                    dependency.clone(),
                ),
            );
        }
        for observation in &self.consumed_resolution_observations {
            self.retention.release_completed_witness(
                &super::input_resolution_budgets::CompletedWitnessRetentionKey::ConsumedResolutionObservation(
                    observation.clone(),
                ),
            );
        }
    }
}

#[cfg(test)]
#[path = "attempt_output_tests.rs"]
mod attempt_output_tests;
