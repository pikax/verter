//! Execution tasks: the identities that own semantic producers and wait on
//! one another's producers.
//!
//! A task is one semantic execution. It owns the producers it has claimed —
//! the flights whose builds it runs — for as long as each is open, and a
//! producer's ownership belongs to the task, never to a thread or a native
//! stack, so it survives the suspension of the frame that holds it.
//!
//! The store's [`TaskRegistry`] hands out generation-qualified task ids and
//! keeps the wait-for graph between tasks: a task that must wait on another
//! task's producer registers one `waiter -> producer` edge first, and an
//! edge that would close a cycle is refused so the waiter answers with the
//! recursion carrier through the memo's ReturnOnly path instead of parking.
//!
//! Same-path recursion is a property of the task, not of a thread: a demand
//! is same-path exactly when the demanding task already has a producer open
//! for the same full key.

use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::semantic_query::SemanticQueryKey;

use super::prepared::PreparedKeyHandle;

/// Generation-qualified identity of one task in one registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct TaskId {
    id: usize,
    generation: u64,
}

impl TaskId {
    #[cfg(test)]
    pub(super) fn slot(self) -> usize {
        self.id
    }

    #[cfg(test)]
    pub(super) fn generation(self) -> u64 {
        self.generation
    }
}

#[derive(Debug, Default)]
struct TaskSlot {
    generation: u64,
    active: bool,
}

#[derive(Debug, Default)]
struct RegistryState {
    tasks: Vec<TaskSlot>,
    free_slots: Vec<usize>,
    waits: FxHashMap<TaskId, TaskId>,
}

/// Store-local task registry and wait-for graph.
#[derive(Clone, Debug, Default)]
pub(super) struct TaskRegistry {
    state: Arc<Mutex<RegistryState>>,
}

/// A refused wait: registering it would close a cycle of waiting tasks (or
/// the waiter cannot wait at all).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WaitCycle;

impl TaskRegistry {
    /// Register a fresh task. It stays active while any handle to it lives.
    pub(super) fn register_task(&self) -> ExecutionTask {
        let id = {
            let mut state = self.state.lock();
            if let Some(id) = state.free_slots.pop() {
                let slot = &mut state.tasks[id];
                verter_debug_assert!(!slot.active, "a free task slot must be inactive");
                slot.generation = slot
                    .generation
                    .checked_add(1)
                    .expect("task generation exhausted");
                slot.active = true;
                TaskId {
                    id,
                    generation: slot.generation,
                }
            } else {
                let id = state.tasks.len();
                state.tasks.push(TaskSlot {
                    generation: 1,
                    active: true,
                });
                TaskId { id, generation: 1 }
            }
        };
        ExecutionTask(Arc::new(TaskRecord {
            id,
            registry: self.clone(),
            producers: Mutex::new(SmallVec::new()),
        }))
    }

    /// Register `waiter -> producer` before `waiter` parks on one of
    /// `producer`'s flights. Refused when the edge would close a cycle, when
    /// either task is gone, or when the waiter already waits.
    pub(super) fn register_wait(
        &self,
        waiter: TaskId,
        producer: TaskId,
    ) -> Result<WaitEdge, WaitCycle> {
        let mut state = self.state.lock();
        if !Self::is_active(&state, waiter)
            || !Self::is_active(&state, producer)
            || state.waits.contains_key(&waiter)
        {
            // A task that is gone, or one already waiting, cannot safely
            // park: refuse it onto the same nonpublishing escape rail.
            return Err(WaitCycle);
        }
        let mut cursor = producer;
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
        state.waits.insert(waiter, producer);
        Ok(WaitEdge {
            registry: self.clone(),
            waiter,
            producer,
        })
    }

    fn is_active(state: &RegistryState, task: TaskId) -> bool {
        state
            .tasks
            .get(task.id)
            .is_some_and(|slot| slot.active && slot.generation == task.generation)
    }

    fn retire(&self, task: TaskId) {
        let mut state = self.state.lock();
        let Some(slot) = state.tasks.get_mut(task.id) else {
            return;
        };
        if !slot.active || slot.generation != task.generation {
            return;
        }
        slot.active = false;
        state.waits.remove(&task);
        state.waits.retain(|_, producer| *producer != task);
        state.free_slots.push(task.id);
    }

    fn remove_wait(&self, waiter: TaskId, producer: TaskId) {
        let mut state = self.state.lock();
        if state
            .waits
            .get(&waiter)
            .is_some_and(|registered| *registered == producer)
        {
            state.waits.remove(&waiter);
        }
    }

    /// Identity of this registry, for the per-thread entry adapter.
    fn identity(&self) -> usize {
        Arc::as_ptr(&self.state) as usize
    }

    #[cfg(test)]
    pub(super) fn retire_for_tests(&self, task: TaskId) {
        self.retire(task);
    }

    #[cfg(test)]
    pub(super) fn remove_wait_for_tests(&self, waiter: TaskId, producer: TaskId) {
        self.remove_wait(waiter, producer);
    }

    #[cfg(test)]
    pub(super) fn is_active_for_tests(&self, task: TaskId) -> bool {
        Self::is_active(&self.state.lock(), task)
    }

    #[cfg(test)]
    pub(super) fn active_task_count_for_tests(&self) -> usize {
        self.state
            .lock()
            .tasks
            .iter()
            .filter(|slot| slot.active)
            .count()
    }

    #[cfg(test)]
    pub(super) fn wait_count_for_tests(&self) -> usize {
        self.state.lock().waits.len()
    }
}

struct TaskRecord {
    id: TaskId,
    registry: TaskRegistry,
    /// The keys whose producers this task has open, most recent last.
    producers: Mutex<SmallVec<[PreparedKeyHandle; 8]>>,
}

impl Drop for TaskRecord {
    fn drop(&mut self) {
        self.registry.retire(self.id);
    }
}

/// A handle on one active task. Clones share the task, which retires when
/// the last handle drops.
#[derive(Clone)]
pub(crate) struct ExecutionTask(Arc<TaskRecord>);

impl std::fmt::Debug for ExecutionTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ExecutionTask").field(&self.0.id).finish()
    }
}

impl ExecutionTask {
    pub(super) fn id(&self) -> TaskId {
        self.0.id
    }

    /// Open this task's producer for `key` until the registration drops.
    pub(super) fn open_producer(&self, key: PreparedKeyHandle) -> OpenProducer {
        self.0.producers.lock().push(key.clone());
        OpenProducer {
            task: self.clone(),
            key,
        }
    }

    /// Whether this task has a producer open for exactly `key`.
    pub(super) fn produces(&self, key: &PreparedKeyHandle) -> bool {
        self.0.producers.lock().iter().any(|open| open == key)
    }

    /// Whether this task has a producer open for exactly `key`, whose
    /// prepared hash is `key_hash`.
    pub(super) fn produces_key(&self, key: &SemanticQueryKey, key_hash: u64) -> bool {
        self.0
            .producers
            .lock()
            .iter()
            .any(|open| open.key_matches(key, key_hash))
    }
}

/// One open producer of one task; dropping it closes the producer.
#[must_use = "dropping the registration closes the producer"]
pub(super) struct OpenProducer {
    task: ExecutionTask,
    key: PreparedKeyHandle,
}

impl Drop for OpenProducer {
    fn drop(&mut self) {
        let mut producers = self.task.0.producers.lock();
        if let Some(position) = producers
            .iter()
            .rposition(|open| open.same_instance(&self.key))
        {
            producers.remove(position);
        }
    }
}

/// One registered wait-for edge; dropping it removes the edge.
#[must_use = "dropping the registration removes the wait-for edge"]
pub(super) struct WaitEdge {
    registry: TaskRegistry,
    waiter: TaskId,
    producer: TaskId,
}

impl Drop for WaitEdge {
    fn drop(&mut self) {
        self.registry.remove_wait(self.waiter, self.producer);
    }
}

thread_local! {
    /// The task each registry's synchronous entry installed on this thread.
    static INSTALLED_TASKS: RefCell<Vec<(usize, ExecutionTask)>> =
        const { RefCell::new(Vec::new()) };
}

/// The synchronous entry adapter. An entry that is not a continuation runs
/// its whole nested evaluation on the native stack, so every query nested
/// inside it joins the one task the outermost entry installed; the outermost
/// entry registers that task and uninstalls it when it returns.
pub(crate) struct ExecutionScope {
    registry_identity: usize,
    task: ExecutionTask,
    installed: bool,
}

impl ExecutionScope {
    /// The task installed on this thread for `registry`, if any.
    pub(super) fn current(registry: &TaskRegistry) -> Option<ExecutionTask> {
        let identity = registry.identity();
        INSTALLED_TASKS.with(|installed| {
            installed
                .borrow()
                .iter()
                .rev()
                .find(|(owner, _)| *owner == identity)
                .map(|(_, task)| task.clone())
        })
    }

    /// Join the task installed for `registry`, or register and install a
    /// fresh one for the extent of this scope.
    pub(super) fn enter(registry: &TaskRegistry) -> Self {
        let registry_identity = registry.identity();
        if let Some(task) = Self::current(registry) {
            return Self {
                registry_identity,
                task,
                installed: false,
            };
        }
        let task = registry.register_task();
        INSTALLED_TASKS.with(|installed| {
            installed
                .borrow_mut()
                .push((registry_identity, task.clone()));
        });
        Self {
            registry_identity,
            task,
            installed: true,
        }
    }

    pub(crate) fn task(&self) -> &ExecutionTask {
        &self.task
    }
}

impl Drop for ExecutionScope {
    fn drop(&mut self) {
        if !self.installed {
            return;
        }
        let task = self.task.id();
        INSTALLED_TASKS.with(|installed| {
            let mut installed = installed.borrow_mut();
            if let Some(position) = installed
                .iter()
                .rposition(|(owner, open)| *owner == self.registry_identity && open.id() == task)
            {
                installed.remove(position);
            }
        });
    }
}
