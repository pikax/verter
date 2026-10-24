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
use std::sync::atomic::{AtomicU64, Ordering};
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
    namespace: u64,
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
struct TaskPool {
    tasks: Vec<TaskSlot>,
    free_slots: Vec<usize>,
}

#[derive(Debug, Default)]
struct RegistryState {
    // Composition joins graphs, never task identities or registration pools.
    pools: FxHashMap<u64, TaskPool>,
    parent: Option<Arc<Mutex<RegistryState>>>,
    rank: usize,
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

/// A task registry and its wait-for graph: one cycle authority. Composition
/// can join authorities when hosts share a dependency owner. Existing tasks,
/// subscriptions and scopes keep their identities and follow the joined graph.
#[derive(Clone, Debug)]
pub struct TaskRegistry {
    owner: Arc<RegistryOwner>,
}
#[derive(Debug)]
struct RegistryOwner {
    namespace: u64,
    graph: Arc<Mutex<RegistryState>>,
}
impl Default for TaskRegistry {
    fn default() -> Self {
        static NEXT_NAMESPACE: AtomicU64 = AtomicU64::new(1);
        let namespace = NEXT_NAMESPACE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("task registry namespace exhausted");
        Self {
            owner: Arc::new(RegistryOwner {
                namespace,
                graph: Arc::new(Mutex::new(RegistryState::default())),
            }),
        }
    }
}

fn with_graph<R>(
    graph: &Arc<Mutex<RegistryState>>,
    apply: impl FnOnce(&mut RegistryState) -> R,
) -> R {
    let mut graph = graph.clone();
    loop {
        let mut state = graph.lock();
        if let Some(parent) = state.parent.clone() {
            drop(state);
            graph = parent;
        } else {
            return apply(&mut state);
        }
    }
}
fn root_graph(graph: &Arc<Mutex<RegistryState>>) -> Arc<Mutex<RegistryState>> {
    let mut graph = graph.clone();
    loop {
        let parent = graph.lock().parent.clone();
        match parent {
            Some(parent) => graph = parent,
            None => return graph,
        }
    }
}
impl Drop for RegistryOwner {
    fn drop(&mut self) {
        // Tasks and edges pin their registration owner. Its final release
        // retires the empty pool too; repeated workspace swaps retain no pool
        // history or backing arrays in the surviving authority.
        let pool = with_graph(&self.graph, |state| state.pools.remove(&self.namespace));
        drop(pool);
    }
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
    /// Join two composition authorities without replacing any live task. All
    /// old handles and scopes subsequently use one cycle graph. Graph locks
    /// are acquired in address order, and a concurrent join is rechecked;
    /// union by rank bounds forwarding depth. No task work or callback runs
    /// while joining. This grants no CPU service, worker or admission permit.
    pub fn share_authority_with(&self, other: &Self) {
        loop {
            let left = root_graph(&self.owner.graph);
            let right = root_graph(&other.owner.graph);
            if Arc::ptr_eq(&left, &right) {
                return;
            }
            let (left, right) = if Arc::as_ptr(&left) < Arc::as_ptr(&right) {
                (left, right)
            } else {
                (right, left)
            };
            let mut a = left.lock();
            let mut b = right.lock();
            if a.parent.is_some() || b.parent.is_some() {
                continue;
            }
            if a.rank >= b.rank {
                if a.rank == b.rank {
                    a.rank += 1;
                }
                Self::absorb_graph(&mut a, &mut b);
                b.parent = Some(left.clone());
            } else {
                Self::absorb_graph(&mut b, &mut a);
                a.parent = Some(right.clone());
            }
            return;
        }
    }

    fn absorb_graph(target: &mut RegistryState, source: &mut RegistryState) {
        // Disjoint authorities have disjoint namespaces, including slots
        // with identical ordinals/generations. Their live waits cannot form
        // a cycle until a subsequent cross-authority edge is registered.
        target.pools.extend(std::mem::take(&mut source.pools));
        target.waits.extend(std::mem::take(&mut source.waits));
        target.waiters.extend(std::mem::take(&mut source.waiters));
    }

    fn same_authority(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
            || Arc::ptr_eq(
                &root_graph(&self.owner.graph),
                &root_graph(&other.owner.graph),
            )
    }

    /// Register a fresh task. It stays active while any handle to it lives.
    pub fn register_task(&self) -> ExecutionTask {
        let id = with_graph(&self.owner.graph, |state| {
            let pool = state.pools.entry(self.owner.namespace).or_default();
            if let Some(id) = pool.free_slots.pop() {
                let slot = &mut pool.tasks[id];
                verter_debug_assert!(!slot.active, "a free task slot must be inactive");
                slot.generation = slot
                    .generation
                    .checked_add(1)
                    .expect("task generation exhausted");
                slot.active = true;
                TaskId {
                    namespace: self.owner.namespace,
                    id,
                    generation: slot.generation,
                }
            } else {
                let id = pool.tasks.len();
                pool.tasks.push(TaskSlot {
                    generation: 1,
                    active: true,
                });
                TaskId {
                    namespace: self.owner.namespace,
                    id,
                    generation: 1,
                }
            }
        });
        ExecutionTask(Arc::new(TaskRecord {
            id,
            registry: self.clone(),
            producers: Mutex::new(SmallVec::new()),
        }))
    }

    /// Register `waiter -> producer` before waiting. Refused when it would
    /// close a cycle, either task is gone, or the waiter already waits.
    pub fn register_wait(&self, waiter: TaskId, producer: TaskId) -> Result<WaitEdge, WaitCycle> {
        with_graph(&self.owner.graph, |state| {
            if !Self::is_active(state, waiter)
                || !Self::is_active(state, producer)
                || state.waits.contains_key(&waiter)
            {
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
        })
    }
    fn is_active(state: &RegistryState, task: TaskId) -> bool {
        state
            .pools
            .get(&task.namespace)
            .and_then(|pool| pool.tasks.get(task.id))
            .is_some_and(|slot| slot.active && slot.generation == task.generation)
    }
    fn retire(&self, task: TaskId) {
        with_graph(&self.owner.graph, |state| {
            let Some(pool) = state.pools.get_mut(&task.namespace) else {
                return;
            };
            let Some(slot) = pool.tasks.get_mut(task.id) else {
                return;
            };
            if !slot.active || slot.generation != task.generation {
                return;
            }
            slot.active = false;
            pool.free_slots.push(task.id);
            state.unlink_wait(task);
            for waiter in state.waiters.remove(&task).unwrap_or_default() {
                state.waits.remove(&waiter);
            }
        });
    }
    fn remove_wait(&self, waiter: TaskId, producer: TaskId) {
        with_graph(&self.owner.graph, |state| {
            if state
                .waits
                .get(&waiter)
                .is_some_and(|registered| *registered == producer)
            {
                state.unlink_wait(waiter);
            }
        });
    }
    fn identity(&self) -> u64 {
        self.owner.namespace
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

    /// Current occupancy of the shared cycle graph and its reverse index.
    pub fn wait_graph_occupancy(&self) -> WaitGraphOccupancy {
        with_graph(&self.owner.graph, |state| WaitGraphOccupancy {
            edges: state.waits.len(),
            edges_capacity: state.waits.capacity(),
            producers: state.waiters.len(),
            reverse_entries: state.waiters.values().map(FxHashSet::len).sum(),
            reverse_capacity: state.waiters.capacity(),
            reverse_entries_capacity: state.waiters.values().map(FxHashSet::capacity).sum(),
        })
    }
    /// Whether `task` is registered and not retired in this authority.
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_active_for_tests(&self, task: TaskId) -> bool {
        with_graph(&self.owner.graph, |state| Self::is_active(state, task))
    }
    /// How many tasks are active in this authority.
    #[cfg(any(test, feature = "test-support"))]
    pub fn active_task_count_for_tests(&self) -> usize {
        with_graph(&self.owner.graph, |state| {
            state
                .pools
                .values()
                .flat_map(|pool| &pool.tasks)
                .filter(|slot| slot.active)
                .count()
        })
    }
    /// How many wait-for edges are registered.
    #[cfg(any(test, feature = "test-support"))]
    pub fn wait_count_for_tests(&self) -> usize {
        with_graph(&self.owner.graph, |state| {
            verter_debug_assert!(
                state.waits.len() == state.waiters.values().map(FxHashSet::len).sum::<usize>(),
                "both sides of every wait-for edge are registered"
            );
            state.waits.len()
        })
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
    /// Whether both handles name the same live task and execution authority.
    pub fn same_task(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// The cycle authority that minted this task. Dependency adapters use
    /// this handle instead of creating a private registry.
    pub fn registry(&self) -> &TaskRegistry {
        &self.0.registry
    }

    /// Register a dependency on another task from this same authority.
    /// Cross-registry identities must never alias by slot and generation.
    pub fn wait_for(&self, producer: &ExecutionTask) -> Result<WaitEdge, WaitCycle> {
        if !self.0.registry.same_authority(&producer.0.registry) {
            return Err(WaitCycle);
        }
        self.0.registry.register_wait(self.id(), producer.id())
    }

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
    static INSTALLED_TASKS: RefCell<Vec<(u64, ExecutionTask)>> =
        const { RefCell::new(Vec::new()) };
}

/// The synchronous entry adapter. An entry that is not a continuation runs
/// its whole nested evaluation on the native stack, so every execution
/// nested inside it joins the one task the outermost entry installed; the
/// outermost entry registers that task and uninstalls it when it returns.
/// Installed scopes follow joined authorities while retaining their exact
/// registration identity for removal.
pub struct ExecutionScope {
    registry_identity: u64,
    task: ExecutionTask,
    installed: bool,
}

impl ExecutionScope {
    /// The innermost installed task, for synchronous adapters that inherit
    /// their caller's execution authority without creating another registry.
    pub fn current_task() -> Option<ExecutionTask> {
        INSTALLED_TASKS.with(|installed| installed.borrow().last().map(|(_, task)| task.clone()))
    }

    /// The task installed on this thread for `registry`, if any.
    pub fn current(registry: &TaskRegistry) -> Option<ExecutionTask> {
        INSTALLED_TASKS.with(|installed| {
            installed
                .borrow()
                .iter()
                .rev()
                .find(|(_, task)| task.registry().same_authority(registry))
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

    /// Install `task` — a task that may run on any thread — as the one
    /// `registry`'s entries on this thread join, for the extent of the
    /// returned scope: the scope a suspendable execution opens around each
    /// of its polls, on whichever thread runs it.
    pub fn install(registry: &TaskRegistry, task: &ExecutionTask) -> Self {
        let registry_identity = registry.identity();
        INSTALLED_TASKS.with(|installed| {
            installed
                .borrow_mut()
                .push((registry_identity, task.clone()));
        });
        Self {
            registry_identity,
            task: task.clone(),
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
    mod wait_cycles;

    use super::*;

    fn reverse_index(registry: &TaskRegistry) -> FxHashMap<TaskId, FxHashSet<TaskId>> {
        with_graph(&registry.owner.graph, |state| state.waiters.clone())
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

#[cfg(test)]
mod composition_tests {
    use super::*;

    #[test]
    fn joining_live_authorities_preserves_edges_scopes_and_namespaced_task_ids() {
        let a = TaskRegistry::default();
        let b = TaskRegistry::default();
        let a0 = a.register_task();
        let a1 = a.register_task();
        let b0 = b.register_task();
        let b1 = b.register_task();
        assert_eq!(a0.id().slot(), b0.id().slot());
        assert_eq!(a0.id().generation(), b0.id().generation());
        assert_ne!(
            a0.id(),
            b0.id(),
            "task identities survive joining equal local slots"
        );
        let a_edge = a1.wait_for(&a0).unwrap();
        let b_edge = b1.wait_for(&b0).unwrap();
        let scope = ExecutionScope::enter(&a);
        assert!(ExecutionScope::current(&b).is_none());
        a.share_authority_with(&b);
        assert!(ExecutionScope::current(&b).unwrap().same_task(scope.task()));
        assert_eq!(a.wait_count_for_tests(), 2);
        assert_eq!(b.wait_count_for_tests(), 2);
        let connecting = a0.wait_for(&b1).unwrap();
        assert!(b0.wait_for(&a1).is_err(), "cross-owner cycles are refused");
        drop(a_edge);
        assert_eq!(b.wait_count_for_tests(), 2);
        drop((connecting, b_edge, scope, a0, a1, b0, b1));
        assert_eq!(a.wait_count_for_tests(), 0);
        assert_eq!(b.active_task_count_for_tests(), 0);
    }

    #[test]
    fn concurrent_composition_joins_have_one_cycle_authority() {
        for _ in 0..8 {
            let registries = [
                TaskRegistry::default(),
                TaskRegistry::default(),
                TaskRegistry::default(),
            ];
            let tasks: Vec<_> = registries.iter().map(TaskRegistry::register_task).collect();
            let start = std::sync::Barrier::new(3);
            std::thread::scope(|scope| {
                let joins: Vec<_> = (0..3)
                    .map(|i| {
                        let registries = &registries;
                        let start = &start;
                        scope.spawn(move || {
                            start.wait();
                            registries[i].share_authority_with(&registries[(i + 1) % 3]);
                        })
                    })
                    .collect();
                for join in joins {
                    join.join().unwrap();
                }
            });
            let first = tasks[0].wait_for(&tasks[1]).unwrap();
            let second = tasks[1].wait_for(&tasks[2]).unwrap();
            assert!(tasks[2].wait_for(&tasks[0]).is_err());
            drop((first, second, tasks));
            for registry in &registries {
                assert_eq!(registry.active_task_count_for_tests(), 0);
            }
        }
    }

    #[test]
    fn released_registration_owners_retain_no_pool_history_after_composition() {
        let surviving = TaskRegistry::default();
        let task = surviving.register_task();
        for _ in 0..32 {
            let joined = TaskRegistry::default();
            let temporary = joined.register_task();
            let retired = temporary.id();
            surviving.share_authority_with(&joined);
            drop((temporary, joined));
            assert!(!surviving.is_active_for_tests(retired));
            assert_eq!(
                with_graph(&surviving.owner.graph, |state| state.pools.len()),
                1,
                "only the live registration owner's backing remains"
            );
        }
        drop(task);
        assert_eq!(surviving.active_task_count_for_tests(), 0);
    }
}
