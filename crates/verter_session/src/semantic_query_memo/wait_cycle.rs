//! Store-local cooperative wait-cycle detection.
//!
//! One execution owner represents one synchronous semantic-dispatch stack.
//! Nested queries on the same thread and store reuse that owner. A joiner
//! registers one temporary `waiter -> winner` edge before parking on an
//! in-flight entry. If that edge would close a cycle, the joiner returns the
//! normal recursion carrier through the memo's `ReturnOnly` path instead of
//! parking.

use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ExecutionOwner {
    id: usize,
    generation: u64,
}

impl ExecutionOwner {
    #[cfg(test)]
    pub(super) fn id(self) -> usize {
        self.id
    }

    #[cfg(test)]
    pub(super) fn generation(self) -> u64 {
        self.generation
    }
}

#[derive(Debug, Default)]
struct OwnerSlot {
    generation: u64,
    active: bool,
}

#[derive(Debug, Default)]
struct WaitForState {
    owners: Vec<OwnerSlot>,
    free_owner_ids: Vec<usize>,
    waits: FxHashMap<ExecutionOwner, ExecutionOwner>,
}

/// Store-local owner registry and wait-for graph.
#[derive(Clone, Debug, Default)]
pub(super) struct WaitForGraph {
    state: Arc<Mutex<WaitForState>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WaitCycle;

impl WaitForGraph {
    pub(super) fn register_owner(&self) -> ExecutionOwnerRegistration {
        let owner = {
            let mut state = self.state.lock();
            if let Some(id) = state.free_owner_ids.pop() {
                let slot = &mut state.owners[id];
                debug_assert!(!slot.active, "free execution-owner slot must be inactive");
                slot.generation = slot
                    .generation
                    .checked_add(1)
                    .expect("execution-owner generation exhausted");
                slot.active = true;
                ExecutionOwner {
                    id,
                    generation: slot.generation,
                }
            } else {
                let id = state.owners.len();
                state.owners.push(OwnerSlot {
                    generation: 1,
                    active: true,
                });
                ExecutionOwner { id, generation: 1 }
            }
        };
        ExecutionOwnerRegistration {
            lease: Arc::new(ExecutionOwnerLease {
                graph: self.clone(),
                owner,
            }),
        }
    }

    pub(super) fn register_wait(
        &self,
        waiter: ExecutionOwner,
        winner: ExecutionOwner,
    ) -> Result<WaitRegistration, WaitCycle> {
        let mut state = self.state.lock();
        if !Self::is_active(&state, waiter)
            || !Self::is_active(&state, winner)
            || state.waits.contains_key(&waiter)
        {
            // An inactive/stale owner or an already-waiting synchronous
            // execution cannot safely park. Treat it as a conservative cycle
            // refusal so it follows the same nonpublishing escape rail.
            return Err(WaitCycle);
        }

        let mut cursor = winner;
        let mut seen = FxHashSet::default();
        while seen.insert(cursor) {
            if cursor == waiter {
                return Err(WaitCycle);
            }
            let Some(next) = state.waits.get(&cursor).copied() else {
                break;
            };
            cursor = next;
        }

        state.waits.insert(waiter, winner);
        Ok(WaitRegistration {
            graph: self.clone(),
            waiter,
            winner,
        })
    }

    fn is_active(state: &WaitForState, owner: ExecutionOwner) -> bool {
        state
            .owners
            .get(owner.id)
            .is_some_and(|slot| slot.active && slot.generation == owner.generation)
    }

    fn unregister_owner(&self, owner: ExecutionOwner) {
        let mut state = self.state.lock();
        let Some(slot) = state.owners.get_mut(owner.id) else {
            return;
        };
        if !slot.active || slot.generation != owner.generation {
            return;
        }
        slot.active = false;
        state.waits.remove(&owner);
        state.waits.retain(|_, winner| *winner != owner);
        state.free_owner_ids.push(owner.id);
    }

    fn remove_wait(&self, waiter: ExecutionOwner, winner: ExecutionOwner) {
        let mut state = self.state.lock();
        if state
            .waits
            .get(&waiter)
            .is_some_and(|registered| *registered == winner)
        {
            state.waits.remove(&waiter);
        }
    }

    #[cfg(test)]
    pub(super) fn unregister_owner_for_tests(&self, owner: ExecutionOwner) {
        self.unregister_owner(owner);
    }

    #[cfg(test)]
    pub(super) fn remove_wait_for_tests(&self, waiter: ExecutionOwner, winner: ExecutionOwner) {
        self.remove_wait(waiter, winner);
    }

    #[cfg(test)]
    pub(super) fn is_active_for_tests(&self, owner: ExecutionOwner) -> bool {
        Self::is_active(&self.state.lock(), owner)
    }

    #[cfg(test)]
    pub(super) fn active_owner_count_for_tests(&self) -> usize {
        self.state
            .lock()
            .owners
            .iter()
            .filter(|slot| slot.active)
            .count()
    }

    #[cfg(test)]
    pub(super) fn wait_count_for_tests(&self) -> usize {
        self.state.lock().waits.len()
    }
}

/// RAII registration for one active execution owner.
#[derive(Clone)]
#[must_use = "dropping the registration retires the execution owner"]
pub(super) struct ExecutionOwnerRegistration {
    lease: Arc<ExecutionOwnerLease>,
}

struct ExecutionOwnerLease {
    graph: WaitForGraph,
    owner: ExecutionOwner,
}

impl ExecutionOwnerRegistration {
    pub(super) fn owner(&self) -> ExecutionOwner {
        self.lease.owner
    }
}

impl Drop for ExecutionOwnerLease {
    fn drop(&mut self) {
        self.graph.unregister_owner(self.owner);
    }
}

/// RAII registration for one temporary wait-for edge.
#[must_use = "dropping the registration removes the wait-for edge"]
pub(super) struct WaitRegistration {
    graph: WaitForGraph,
    waiter: ExecutionOwner,
    winner: ExecutionOwner,
}

impl Drop for WaitRegistration {
    fn drop(&mut self) {
        self.graph.remove_wait(self.waiter, self.winner);
    }
}

thread_local! {
    /// Active outermost execution owners on this thread. A vector rather than
    /// a single slot permits re-entrant calls through a second graph/store.
    static ACTIVE_EXECUTION_OWNERS: RefCell<Vec<(usize, ExecutionOwner)>> =
        const { RefCell::new(Vec::new()) };
}

/// Scope that reuses the current store's owner for nested semantic queries and
/// registers a fresh generation only for an outermost execution.
pub(super) struct ExecutionOwnerScope {
    graph_identity: usize,
    owner: ExecutionOwner,
    registration: Option<ExecutionOwnerRegistration>,
}

impl ExecutionOwnerScope {
    pub(super) fn current(graph: &WaitForGraph) -> Option<ExecutionOwner> {
        let graph_identity = Arc::as_ptr(&graph.state) as usize;
        ACTIVE_EXECUTION_OWNERS.with(|active| {
            active
                .borrow()
                .iter()
                .rev()
                .find_map(|(identity, owner)| (*identity == graph_identity).then_some(*owner))
        })
    }

    pub(super) fn enter(graph: &WaitForGraph) -> Self {
        let graph_identity = Arc::as_ptr(&graph.state) as usize;
        if let Some(owner) = Self::current(graph) {
            return Self {
                graph_identity,
                owner,
                registration: None,
            };
        }

        let registration = graph.register_owner();
        let owner = registration.owner();
        ACTIVE_EXECUTION_OWNERS.with(|active| {
            active.borrow_mut().push((graph_identity, owner));
        });
        Self {
            graph_identity,
            owner,
            registration: Some(registration),
        }
    }

    pub(super) fn owner(&self) -> ExecutionOwner {
        self.owner
    }
}

impl Drop for ExecutionOwnerScope {
    fn drop(&mut self) {
        if self.registration.is_none() {
            return;
        }
        ACTIVE_EXECUTION_OWNERS.with(|active| {
            let mut active = active.borrow_mut();
            if let Some(position) = active.iter().rposition(|(identity, owner)| {
                *identity == self.graph_identity && *owner == self.owner
            }) {
                active.remove(position);
            }
        });
        // `registration` drops after this body and releases the owner slot.
    }
}
