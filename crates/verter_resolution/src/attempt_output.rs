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
//! ownership. The top-level [`crate::CompletedAttempt`]
//! instead pairs the completed answer with this accumulator, and the
//! workspace driver applies it only after completion.

use rustc_hash::FxHashSet;
use verter_session_query::resolution::{
    AmbientDependency, CanonicalId, ConsumedResolutionObservationKey,
};

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
    observed_facts: Vec<verter_session_query::facts::version::FactVersionRef>,
    ambient_dependencies: Vec<AmbientDependency>,
    consumed_resolution_observations: Vec<ConsumedResolutionObservationKey>,
    observed_fact_set: FxHashSet<verter_session_query::facts::version::FactVersionRef>,
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
        fact: verter_session_query::facts::version::FactVersionRef,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
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
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
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
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
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
    pub fn observed_facts(&self) -> &[verter_session_query::facts::version::FactVersionRef] {
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
    pub fn merge(
        &mut self,
        other: AttemptOutput,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
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
