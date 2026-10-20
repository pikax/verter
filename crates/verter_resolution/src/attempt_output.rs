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
use std::sync::Arc;
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
/// whole attempt is discarded. Immutable clones share the storage and its
/// reservations until mutation or the last clone's destruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptOutput {
    data: Arc<AttemptOutputData>,
}

impl AttemptOutput {
    /// A fresh, empty accumulator — one per attempt.
    #[must_use]
    pub fn new() -> Self {
        Self {
            data: Arc::new(AttemptOutputData::new()),
        }
    }

    /// Record one observed fact in first-occurrence order.
    pub fn record_fact(
        &mut self,
        fact: verter_session_query::facts::version::FactVersionRef,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
        // An idempotent write must not detach shared immutable witnesses.
        if Arc::strong_count(&self.data) > 1 && self.data.observed_fact_set.contains(&fact) {
            return Ok(());
        }
        Arc::make_mut(&mut self.data).record_fact(fact)
    }

    /// Record one ambient-dependency edge.
    pub fn record_ambient_dependency(
        &mut self,
        consumer_canonical: CanonicalId,
        virtual_id: CanonicalId,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
        if Arc::strong_count(&self.data) > 1
            && self
                .data
                .ambient_dependency_set
                .contains(&AmbientDependency {
                    consumer_canonical: consumer_canonical.clone(),
                    virtual_id: virtual_id.clone(),
                })
        {
            return Ok(());
        }
        Arc::make_mut(&mut self.data).record_ambient_dependency(consumer_canonical, virtual_id)
    }

    /// Record an input the kernel actually consumed before short-circuiting.
    pub fn record_consumed_resolution_observation(
        &mut self,
        key: ConsumedResolutionObservationKey,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
        if Arc::strong_count(&self.data) > 1
            && self.data.consumed_resolution_observation_set.contains(&key)
        {
            return Ok(());
        }
        Arc::make_mut(&mut self.data).record_consumed_resolution_observation(key)
    }

    #[must_use]
    pub fn observed_facts(&self) -> &[verter_session_query::facts::version::FactVersionRef] {
        &self.data.observed_facts
    }

    #[must_use]
    pub fn ambient_dependencies(&self) -> &[AmbientDependency] {
        &self.data.ambient_dependencies
    }

    #[must_use]
    pub fn consumed_resolution_observations(&self) -> &[ConsumedResolutionObservationKey] {
        &self.data.consumed_resolution_observations
    }

    /// Compose child witnesses in first-occurrence order. Immutable clones
    /// share one reservation owner; only mutation copies the storage. Unique
    /// child storage transfers its leases within the same operation, while a
    /// different operation still admits witnesses against its own budget.
    pub fn merge(
        &mut self,
        other: Self,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
        if Arc::ptr_eq(&self.data, &other.data) {
            return Ok(());
        }
        if self.is_empty() && self.data.retention.same_operation(&other.data.retention) {
            self.data = other.data;
            return Ok(());
        }
        if Arc::strong_count(&self.data) > 1
            && other
                .data
                .observed_facts
                .iter()
                .all(|fact| self.data.observed_fact_set.contains(fact))
            && other
                .data
                .ambient_dependencies
                .iter()
                .all(|dependency| self.data.ambient_dependency_set.contains(dependency))
            && other
                .data
                .consumed_resolution_observations
                .iter()
                .all(|observation| {
                    self.data
                        .consumed_resolution_observation_set
                        .contains(observation)
                })
        {
            return Ok(());
        }
        let parent = Arc::make_mut(&mut self.data);
        match Arc::try_unwrap(other.data) {
            Ok(child) => parent.merge(child),
            Err(child) => parent.merge_borrowed(&child),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl Default for AttemptOutput {
    fn default() -> Self {
        Self::new()
    }
}

// One owner of each reservation, shared by immutable output clones. Its
// destructor releases the reservations after the last clone goes away.
#[derive(Debug)]
struct AttemptOutputData {
    observed_facts: Vec<verter_session_query::facts::version::FactVersionRef>,
    ambient_dependencies: Vec<AmbientDependency>,
    consumed_resolution_observations: Vec<ConsumedResolutionObservationKey>,
    observed_fact_set: FxHashSet<verter_session_query::facts::version::FactVersionRef>,
    ambient_dependency_set: FxHashSet<AmbientDependency>,
    consumed_resolution_observation_set: FxHashSet<ConsumedResolutionObservationKey>,
    retention: super::input_resolution_budgets::InputResolutionRetention,
}

impl AttemptOutputData {
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

    /// Merge `other`'s recorded output into `self` — for composing a
    /// parent attempt's output from sub-attempts it delegated to (e.g. the
    /// recursive project-reference walk's per-node outputs folding into
    /// the enclosing resolution's output). Outputs from the same operation
    /// transfer their existing witness reservations; duplicates release only
    /// the child's reference. A different operation must admit each witness
    /// against the destination's budget before it is retained there.
    pub fn merge(
        &mut self,
        mut other: AttemptOutputData,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
        if self.retention.same_operation(&other.retention) {
            // The child already owns these reservations in our operation's
            // ledger. Move them rather than retaining and releasing each again.
            if self.is_empty() {
                std::mem::swap(self, &mut other);
                return Ok(());
            }
            for fact in other.observed_facts.drain(..) {
                if self.observed_fact_set.insert(fact.clone()) {
                    self.observed_facts.push(fact);
                } else {
                    self.retention.release_completed_witness(
                        &super::input_resolution_budgets::CompletedWitnessRetentionKey::Fact(fact),
                    );
                }
            }
            for dependency in other.ambient_dependencies.drain(..) {
                if self.ambient_dependency_set.insert(dependency.clone()) {
                    self.ambient_dependencies.push(dependency);
                } else {
                    self.retention.release_completed_witness(
                        &super::input_resolution_budgets::CompletedWitnessRetentionKey::AmbientDependency(dependency),
                    );
                }
            }
            for observation in other.consumed_resolution_observations.drain(..) {
                if self
                    .consumed_resolution_observation_set
                    .insert(observation.clone())
                {
                    self.consumed_resolution_observations.push(observation);
                } else {
                    self.retention.release_completed_witness(
                        &super::input_resolution_budgets::CompletedWitnessRetentionKey::ConsumedResolutionObservation(observation),
                    );
                }
            }
            return Ok(());
        }
        self.merge_borrowed(&other)
    }

    fn merge_borrowed(
        &mut self,
        other: &Self,
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

impl Default for AttemptOutputData {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AttemptOutputData {
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

impl PartialEq for AttemptOutputData {
    fn eq(&self, other: &Self) -> bool {
        self.observed_facts == other.observed_facts
            && self.ambient_dependencies == other.ambient_dependencies
            && self.consumed_resolution_observations == other.consumed_resolution_observations
    }
}

impl Eq for AttemptOutputData {}

impl Drop for AttemptOutputData {
    fn drop(&mut self) {
        for fact in self.observed_facts.drain(..) {
            self.retention.release_completed_witness(
                &super::input_resolution_budgets::CompletedWitnessRetentionKey::Fact(fact),
            );
        }
        for dependency in self.ambient_dependencies.drain(..) {
            self.retention.release_completed_witness(
                &super::input_resolution_budgets::CompletedWitnessRetentionKey::AmbientDependency(
                    dependency,
                ),
            );
        }
        for observation in self.consumed_resolution_observations.drain(..) {
            self.retention.release_completed_witness(
                &super::input_resolution_budgets::CompletedWitnessRetentionKey::ConsumedResolutionObservation(
                    observation,
                ),
            );
        }
    }
}

#[cfg(test)]
#[path = "attempt_output_tests.rs"]
mod attempt_output_tests;
