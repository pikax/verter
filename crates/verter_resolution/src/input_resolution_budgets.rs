//! Operation-owned retention accounting shared by resolver attempts.

use std::cell::RefCell;
use std::collections::HashMap;
#[cfg(test)]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use verter_session_query::resolution::InputResolutionBudgets;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum CompletedWitnessRetentionKey {
    Fact(verter_session_query::facts::version::FactVersionRef),
    AmbientDependency(verter_session_query::resolution::attempt_output::AmbientDependency),
    ConsumedResolutionObservation(
        verter_session_query::resolution::attempt_output::ConsumedResolutionObservationKey,
    ),
}

#[derive(Debug)]
struct InputResolutionRetentionState {
    budgets: InputResolutionBudgets,
    alias_geometry: AtomicU32,
    alias_geometry_high_water: AtomicU32,
    completed_witnesses: Mutex<HashMap<CompletedWitnessRetentionKey, u64>>,
    #[cfg(test)]
    forced_completed_witness_count: AtomicU32,
    #[cfg(test)]
    has_forced_completed_witness_count: AtomicBool,
}

/// Shared handles to the two live-retention counters physically owned by one
/// workspace operation ledger. Attempt views clone this handle; no policy or
/// counter is copied into a per-frontier table.
#[derive(Debug, Clone)]
pub struct InputResolutionRetention {
    state: Arc<InputResolutionRetentionState>,
}

impl InputResolutionRetention {
    #[doc(hidden)]
    pub fn new(budgets: InputResolutionBudgets) -> Self {
        Self {
            state: Arc::new(InputResolutionRetentionState {
                budgets,
                alias_geometry: AtomicU32::new(0),
                alias_geometry_high_water: AtomicU32::new(0),
                completed_witnesses: Mutex::new(HashMap::new()),
                #[cfg(test)]
                forced_completed_witness_count: AtomicU32::new(0),
                #[cfg(test)]
                has_forced_completed_witness_count: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) fn retain_alias_geometry(
        &self,
    ) -> Result<AliasGeometryRetentionLease, verter_session_query::resolution::AttemptFailure> {
        let maximum = self.state.budgets.alias_geometry_retention();
        loop {
            let retained = self.state.alias_geometry.load(Ordering::Acquire);
            let Some(prospective) = retained.checked_add(1) else {
                return Err(
                    verter_session_query::resolution::AttemptFailure::InputResolutionAliasGeometryRetentionLimit {
                        retained,
                        prospective: u32::MAX,
                        maximum,
                    },
                );
            };
            if prospective > maximum {
                return Err(
                    verter_session_query::resolution::AttemptFailure::InputResolutionAliasGeometryRetentionLimit {
                        retained,
                        prospective,
                        maximum,
                    },
                );
            }
            if self
                .state
                .alias_geometry
                .compare_exchange_weak(retained, prospective, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.state
                    .alias_geometry_high_water
                    .fetch_max(prospective, Ordering::AcqRel);
                return Ok(AliasGeometryRetentionLease {
                    retention: self.clone(),
                });
            }
        }
    }

    pub(crate) fn retain_completed_witness(
        &self,
        key: CompletedWitnessRetentionKey,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
        let mut retained = self
            .state
            .completed_witnesses
            .lock()
            .expect("completed-witness retention lock poisoned");
        if let Some(references) = retained.get_mut(&key) {
            *references = references.checked_add(1).unwrap_or(u64::MAX);
            return Ok(());
        }
        #[cfg(test)]
        let current = if self
            .state
            .has_forced_completed_witness_count
            .load(Ordering::Acquire)
        {
            self.state
                .forced_completed_witness_count
                .load(Ordering::Acquire)
        } else {
            u32::try_from(retained.len()).unwrap_or(u32::MAX)
        };
        #[cfg(not(test))]
        let current = u32::try_from(retained.len()).unwrap_or(u32::MAX);
        let maximum = self.state.budgets.completed_witness_retention();
        let Some(prospective) = current.checked_add(1) else {
            return Err(
                verter_session_query::resolution::AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
                    retained: current,
                    prospective: u32::MAX,
                    maximum,
                },
            );
        };
        if prospective > maximum {
            return Err(
                verter_session_query::resolution::AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
                    retained: current,
                    prospective,
                    maximum,
                },
            );
        }
        retained.insert(key, 1);
        Ok(())
    }

    pub(crate) fn release_completed_witness(&self, key: &CompletedWitnessRetentionKey) {
        let mut retained = self
            .state
            .completed_witnesses
            .lock()
            .expect("completed-witness retention lock poisoned");
        let remove = {
            let references = retained
                .get_mut(key)
                .expect("released completed witness must be retained");
            *references = references
                .checked_sub(1)
                .expect("completed witness reference count underflow");
            *references == 0
        };
        if remove {
            retained.remove(key);
        }
    }

    #[cfg(test)]
    pub(crate) fn retained_for_test(&self) -> (u32, u32, u32) {
        (
            self.state.alias_geometry.load(Ordering::Acquire),
            self.state.alias_geometry_high_water.load(Ordering::Acquire),
            u32::try_from(
                self.state
                    .completed_witnesses
                    .lock()
                    .expect("completed-witness retention lock poisoned")
                    .len(),
            )
            .unwrap_or(u32::MAX),
        )
    }

    #[cfg(test)]
    pub(crate) fn force_alias_retained_for_test(&self, retained: u32) {
        self.state.alias_geometry.store(retained, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn force_completed_witness_retained_for_test(&self, retained: Option<u32>) {
        match retained {
            Some(retained) => {
                self.state
                    .forced_completed_witness_count
                    .store(retained, Ordering::Release);
                self.state
                    .has_forced_completed_witness_count
                    .store(true, Ordering::Release);
            }
            None => self
                .state
                .has_forced_completed_witness_count
                .store(false, Ordering::Release),
        }
    }

    #[doc(hidden)]
    pub fn scope<T>(&self, run: impl FnOnce() -> T) -> T {
        CURRENT_RETENTION.with(|current| current.borrow_mut().push(self.clone()));
        struct Pop;
        impl Drop for Pop {
            fn drop(&mut self) {
                CURRENT_RETENTION.with(|current| {
                    current
                        .borrow_mut()
                        .pop()
                        .expect("retention scope is balanced");
                });
            }
        }
        let pop = Pop;
        let result = run();
        drop(pop);
        result
    }

    pub(crate) fn current_or_default() -> Self {
        CURRENT_RETENTION
            .with(|current| current.borrow().last().cloned())
            .unwrap_or_else(|| Self::new(InputResolutionBudgets::default()))
    }
}

impl Default for InputResolutionRetention {
    fn default() -> Self {
        Self::new(InputResolutionBudgets::default())
    }
}

thread_local! {
    static CURRENT_RETENTION: RefCell<Vec<InputResolutionRetention>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug)]
pub(crate) struct AliasGeometryRetentionLease {
    retention: InputResolutionRetention,
}

impl Drop for AliasGeometryRetentionLease {
    fn drop(&mut self) {
        let prior = self
            .retention
            .state
            .alias_geometry
            .fetch_sub(1, Ordering::AcqRel);
        assert!(prior > 0, "alias-geometry retention underflow");
    }
}
