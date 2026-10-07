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
    /// Waiter → the producer it waits on. A task waits on at most one.
    waits: FxHashMap<TaskId, TaskId>,
    /// Producer → the waiters registered on it: the reverse side of
    /// `waits`, changed in the same step, so retiring a task removes the
    /// edges into it without scanning every edge.
    waiters: FxHashMap<TaskId, FxHashSet<TaskId>>,
}

impl RegistryState {
    fn link_wait(&mut self, waiter: TaskId, producer: TaskId) {
        self.waits.insert(waiter, producer);
        self.waiters.entry(producer).or_default().insert(waiter);
    }

    /// Remove `waiter`'s edge, returning the producer it waited on.
    fn unlink_wait(&mut self, waiter: TaskId) -> Option<TaskId> {
        let producer = self.waits.remove(&waiter)?;
        if let Some(waiters) = self.waiters.get_mut(&producer) {
            waiters.remove(&waiter);
            if waiters.is_empty() {
                self.waiters.remove(&producer);
            }
        }
        Some(producer)
    }
}

/// A task registry and its wait-for graph: one cycle authority.
#[derive(Clone, Debug, Default)]
pub struct TaskRegistry {
    state: Arc<Mutex<RegistryState>>,
}

/// Current occupancy of a registry's wait-for graph, read under one lock.
/// Always available; every count is derived from the resident tables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WaitGraphOccupancy {
    /// Registered `waiter -> producer` edges.
    pub edges: usize,
    /// Backing capacity of the waiter → producer table.
    pub edges_capacity: usize,
    /// Producers with at least one registered waiter.
    pub producers: usize,
    /// Waiter entries in the producer → waiters reverse index.
    pub reverse_entries: usize,
    /// Backing capacity of the producer → waiters reverse index.
    pub reverse_capacity: usize,
    /// Backing capacity summed over the per-producer waiter sets. A set
    /// keeps its capacity while any waiter survives, so this can exceed
    /// `reverse_entries`.
    pub reverse_entries_capacity: usize,
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
        state.link_wait(waiter, producer);
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
        state.unlink_wait(task);
        for waiter in state.waiters.remove(&task).unwrap_or_default() {
            state.waits.remove(&waiter);
        }
        state.free_slots.push(task.id);
    }

    fn remove_wait(&self, waiter: TaskId, producer: TaskId) {
        let mut state = self.state.lock();
        if state
            .waits
            .get(&waiter)
            .is_some_and(|registered| *registered == producer)
        {
            state.unlink_wait(waiter);
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

    /// Current occupancy of the wait-for graph and its reverse index.
    pub fn wait_graph_occupancy(&self) -> WaitGraphOccupancy {
        let state = self.state.lock();
        WaitGraphOccupancy {
            edges: state.waits.len(),
            edges_capacity: state.waits.capacity(),
            producers: state.waiters.len(),
            reverse_entries: state.waiters.values().map(FxHashSet::len).sum(),
            reverse_capacity: state.waiters.capacity(),
            reverse_entries_capacity: state.waiters.values().map(FxHashSet::capacity).sum(),
        }
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
        let state = self.state.lock();
        verter_debug_assert!(
            state.waits.len() == state.waiters.values().map(FxHashSet::len).sum::<usize>(),
            "both sides of every wait-for edge are registered",
        );
        state.waits.len()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn reverse_index(registry: &TaskRegistry) -> FxHashMap<TaskId, FxHashSet<TaskId>> {
        registry.state.lock().waiters.clone()
    }

    /// Retiring a producer removes exactly the edges into it, through the
    /// reverse index, and leaves unrelated edges and no empty bucket.
    #[test]
    fn retiring_a_producer_removes_only_its_incoming_edges() {
        let registry = TaskRegistry::default();
        let producer = registry.register_task();
        let other = registry.register_task();
        let waiters: Vec<ExecutionTask> = (0..64).map(|_| registry.register_task()).collect();
        let unrelated = registry.register_task();
        let mut edges: Vec<WaitEdge> = waiters
            .iter()
            .map(|waiter| {
                registry
                    .register_wait(waiter.id(), producer.id())
                    .expect("acyclic wait registers")
            })
            .collect();
        let unrelated_edge = registry
            .register_wait(unrelated.id(), other.id())
            .expect("acyclic wait registers");
        assert_eq!(registry.wait_count_for_tests(), 65);
        let occupancy = registry.wait_graph_occupancy();
        assert_eq!((occupancy.edges, occupancy.producers), (65, 2));
        assert_eq!(occupancy.reverse_entries, 65);

        let producer_id = producer.id();
        drop(producer);
        assert!(!registry.is_active_for_tests(producer_id));
        assert_eq!(
            registry.wait_count_for_tests(),
            1,
            "only the unrelated edge remains"
        );
        let index = reverse_index(&registry);
        assert!(!index.contains_key(&producer_id));
        assert_eq!(index.get(&other.id()).map(FxHashSet::len), Some(1));

        // Stale edge drops after retirement are no-ops, and a waiter freed
        // by the retirement can wait again.
        edges.pop();
        let rewait = registry
            .register_wait(waiters[0].id(), other.id())
            .expect("a released waiter can wait again");
        assert_eq!(registry.wait_count_for_tests(), 2);
        drop(rewait);
        drop(unrelated_edge);
        drop(edges);
        assert_eq!(registry.wait_count_for_tests(), 0);
        assert!(
            reverse_index(&registry).is_empty(),
            "the reverse index drains"
        );
        let drained = registry.wait_graph_occupancy();
        assert_eq!(
            (drained.edges, drained.producers, drained.reverse_entries),
            (0, 0, 0),
            "occupancy drains with the edges",
        );
        assert!(
            drained.edges_capacity >= 65,
            "backing capacity is reported separately from membership",
        );
    }

    /// A producer's waiter set emptied down to one survivor still reports
    /// the backing capacity its former waiters left behind, and retiring
    /// the producer releases it.
    #[test]
    fn producer_waiter_set_capacity_is_reported_until_it_drains() {
        let registry = TaskRegistry::default();
        let producer = registry.register_task();
        let waiters: Vec<ExecutionTask> = (0..64).map(|_| registry.register_task()).collect();
        let mut edges: Vec<WaitEdge> = waiters
            .iter()
            .map(|waiter| {
                registry
                    .register_wait(waiter.id(), producer.id())
                    .expect("acyclic wait registers")
            })
            .collect();
        let populated = registry.wait_graph_occupancy();
        assert_eq!((populated.producers, populated.reverse_entries), (1, 64));
        assert!(populated.reverse_entries_capacity >= 64);

        edges.truncate(1);
        let shrunk = registry.wait_graph_occupancy();
        assert_eq!((shrunk.producers, shrunk.reverse_entries), (1, 1));
        assert_eq!(
            shrunk.reverse_entries_capacity, populated.reverse_entries_capacity,
            "the surviving set keeps the backing its former waiters used",
        );

        drop(producer);
        let retired = registry.wait_graph_occupancy();
        assert_eq!(
            (
                retired.producers,
                retired.reverse_entries,
                retired.reverse_entries_capacity
            ),
            (0, 0, 0),
        );
        drop(edges);
    }

    /// Retiring a waiter removes its own outgoing edge from both sides.
    #[test]
    fn retiring_a_waiter_unlinks_its_edge() {
        let registry = TaskRegistry::default();
        let producer = registry.register_task();
        let waiter = registry.register_task();
        let edge = registry
            .register_wait(waiter.id(), producer.id())
            .expect("acyclic wait registers");
        drop(waiter);
        assert_eq!(registry.wait_count_for_tests(), 0);
        assert!(reverse_index(&registry).is_empty());
        drop(edge);
        assert_eq!(registry.wait_count_for_tests(), 0);
    }
}
