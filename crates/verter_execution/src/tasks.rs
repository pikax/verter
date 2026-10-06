//! Execution tasks: the identities that own producers and wait on one
//! another's producers, shared by every layer that runs one.
//!
//! A task is one execution. It owns the producers it has claimed — the
//! flights whose work it runs — for as long as each is open, and a
//! producer's ownership belongs to the task, never to a thread or a native
//! stack, so it survives the suspension of the frame that holds it. A task
//! is an id plus a refcounted handle with no thread affinity; the
//! per-thread [`ExecutionScope`] is only the adapter for synchronous entries.
//!
//! A [`TaskRegistry`] hands out generation-qualified task ids and keeps the
//! wait-for graph between tasks: a task that must wait on another task's
//! producer registers one `waiter -> producer` edge first, and an edge that
//! would close a cycle is refused, so the waiter takes its caller's
//! non-parking escape instead of deadlocking. One registry is ONE cycle
//! authority: layers whose producers may wait on one another share one.
//!
//! What a producer IS belongs to the layer that opens it: a task holds each
//! open producer as a type-erased [`ProducerIdentity`] and matches it by an
//! allocation-free hash-then-identity check, so a layer can ask whether a
//! task already produces its own key and downcast the producers it opened.

use std::any::Any;
use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

/// The identity of one producer a task opens, as the opening layer defines
/// it.
pub trait ProducerIdentity: Any + Send + Sync {
    /// A hash of this producer's identity: producers that are the same
    /// hash equal, so a mismatch rejects without comparing identities.
    fn producer_hash(&self) -> u64;

    /// Whether `other` is the same producer as this one. Called only when
    /// the hashes match; implementations downcast `other` to their own
    /// type.
    fn same_producer(&self, other: &dyn ProducerIdentity) -> bool;
}

impl dyn ProducerIdentity {
    /// This producer as its concrete type, when it is one.
    pub fn downcast_ref<T: ProducerIdentity>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }
}

/// Generation-qualified identity of one task in one registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId {
    id: usize,
    generation: u64,
}

impl TaskId {
    /// The registry slot this task occupies (reused once it retires).
    #[cfg(any(test, feature = "test-support"))]
    pub fn slot(self) -> usize {
        self.id
    }

    /// The generation that qualifies [`Self::slot`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn generation(self) -> u64 {
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

/// A task registry and its wait-for graph: one cycle authority.
#[derive(Clone, Debug, Default)]
pub struct TaskRegistry {
    state: Arc<Mutex<RegistryState>>,
}

/// A refused wait: registering it would close a cycle of waiting tasks (or
/// the waiter cannot wait at all).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitCycle;

impl TaskRegistry {
    /// Register a fresh task. It stays active while any handle to it lives.
    pub fn register_task(&self) -> ExecutionTask {
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
    pub fn register_wait(&self, waiter: TaskId, producer: TaskId) -> Result<WaitEdge, WaitCycle> {
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

    /// Retire `task` as its last handle would.
    #[cfg(any(test, feature = "test-support"))]
    pub fn retire_for_tests(&self, task: TaskId) {
        self.retire(task);
    }

    /// Remove `waiter -> producer` as its edge's drop would.
    #[cfg(any(test, feature = "test-support"))]
    pub fn remove_wait_for_tests(&self, waiter: TaskId, producer: TaskId) {
        self.remove_wait(waiter, producer);
    }

    /// Whether `task` is registered and not retired.
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_active_for_tests(&self, task: TaskId) -> bool {
        Self::is_active(&self.state.lock(), task)
    }

    /// How many tasks are active.
    #[cfg(any(test, feature = "test-support"))]
    pub fn active_task_count_for_tests(&self) -> usize {
        self.state
            .lock()
            .tasks
            .iter()
            .filter(|slot| slot.active)
            .count()
    }

    /// How many wait-for edges are registered.
    #[cfg(any(test, feature = "test-support"))]
    pub fn wait_count_for_tests(&self) -> usize {
        self.state.lock().waits.len()
    }
}

/// One open producer: its identity's hash (the fast reject) and the
/// identity itself.
struct OpenSlot {
    hash: u64,
    producer: Arc<dyn ProducerIdentity>,
}

struct TaskRecord {
    id: TaskId,
    registry: TaskRegistry,
    /// The producers this task has open, most recent last.
    producers: Mutex<SmallVec<[OpenSlot; 8]>>,
}

impl Drop for TaskRecord {
    fn drop(&mut self) {
        self.registry.retire(self.id);
    }
}

/// A handle on one active task. Clones share the task, which retires when
/// the last handle drops.
#[derive(Clone)]
pub struct ExecutionTask(Arc<TaskRecord>);

impl std::fmt::Debug for ExecutionTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ExecutionTask").field(&self.0.id).finish()
    }
}

impl ExecutionTask {
    /// This task's id.
    pub fn id(&self) -> TaskId {
        self.0.id
    }

    /// Open this task's `producer` until the registration drops.
    pub fn open(&self, producer: Arc<dyn ProducerIdentity>) -> OpenProducer {
        self.0.producers.lock().push(OpenSlot {
            hash: producer.producer_hash(),
            producer: Arc::clone(&producer),
        });
        OpenProducer {
            task: self.clone(),
            producer,
        }
    }

    /// Whether this task has `producer` open.
    pub fn produces_identity(&self, producer: &dyn ProducerIdentity) -> bool {
        let hash = producer.producer_hash();
        self.0
            .producers
            .lock()
            .iter()
            .any(|open| open.hash == hash && open.producer.same_producer(producer))
    }

    /// Whether this task has a producer open whose hash is `hash` and that
    /// `matches` accepts — the check a layer runs against its own key
    /// without building a producer for it.
    pub fn produces_matching(
        &self,
        hash: u64,
        mut matches: impl FnMut(&dyn ProducerIdentity) -> bool,
    ) -> bool {
        self.0
            .producers
            .lock()
            .iter()
            .any(|open| open.hash == hash && matches(open.producer.as_ref()))
    }
}

/// One open producer of one task; dropping it closes that producer (by
/// instance, never by equal identity).
#[must_use = "dropping the registration closes the producer"]
pub struct OpenProducer {
    task: ExecutionTask,
    producer: Arc<dyn ProducerIdentity>,
}

impl OpenProducer {
    /// The task that owns this producer.
    pub fn task(&self) -> &ExecutionTask {
        &self.task
    }
}

impl Drop for OpenProducer {
    fn drop(&mut self) {
        let mut producers = self.task.0.producers.lock();
        if let Some(position) = producers
            .iter()
            .rposition(|open| Arc::ptr_eq(&open.producer, &self.producer))
        {
            producers.remove(position);
        }
    }
}

/// One registered wait-for edge; dropping it removes the edge.
#[must_use = "dropping the registration removes the wait-for edge"]
pub struct WaitEdge {
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
/// its whole nested evaluation on the native stack, so every execution
/// nested inside it joins the one task the outermost entry installed; the
/// outermost entry registers that task and uninstalls it when it returns.
/// Installed tasks are keyed by registry identity.
pub struct ExecutionScope {
    registry_identity: usize,
    task: ExecutionTask,
    installed: bool,
}

impl ExecutionScope {
    /// The task installed on this thread for `registry`, if any.
    pub fn current(registry: &TaskRegistry) -> Option<ExecutionTask> {
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
    pub fn enter(registry: &TaskRegistry) -> Self {
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

    /// The task this scope runs as.
    pub fn task(&self) -> &ExecutionTask {
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
