//! Explicit roots over the node arena's counted edges.
//!
//! A semantic node lives while something counts it: a live parent whose
//! payload names it, or one of the roots this module mints (see the
//! arena's **Ownership** docs). Three root kinds exist, one per holder:
//!
//! - [`NodeLease`]: one node, owned by a caller outside the store — a
//!   result handed across a public boundary. The node and everything it
//!   retains stay readable until the lease drops, whatever the store
//!   evicts or a document close sweeps meanwhile is never this node; a
//!   lease carries its arena's identity, so a lease read against another
//!   store is refused rather than aliasing an overlapping id there.
//! - [`NodeRootSet`]: the multiset of nodes one retained holder names (a
//!   memo candidate's key and value, a cache entry). Built from the
//!   holder's own enumeration of the ids it embeds; dropping the holder
//!   releases them.
//! - A root scope ([`SemanticRootScope`]): the nodes one computation
//!   touches. Every intern made on a thread inside a scope of the arena's
//!   store is rooted by that scope until the scope's last guard drops, so
//!   a computation's intermediate ids stay valid for the whole
//!   computation without each holding a handle. A scope entered while
//!   another of the same store is active on the thread JOINS it — ids
//!   returned from an inner computation to an outer one stay rooted — and
//!   a [`RootScopeHandle`] carries the same scope onto another thread
//!   (work a computation fans out).
//!
//! A root never resurrects: rooting an id whose node is released, dying,
//! or was never handed out is refused. Releasing a root is the only way a
//! count falls, and the last release queues the node for the arena's
//! iterative destruction.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashSet;
use verter_session_query::retention::RetentionCharge;

use super::arena::{ArenaCore, EdgeRefusal};
use super::SemanticGraphStore;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};

/// Why a node could not be rooted or read through a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
    /// The id was never handed out by this store.
    Unallocated,
    /// The node was released or is dying: a stale id.
    Stale,
    /// The handle was minted by another store.
    Foreign,
}

impl From<EdgeRefusal> for LeaseError {
    fn from(refusal: EdgeRefusal) -> Self {
        match refusal {
            EdgeRefusal::Unallocated => Self::Unallocated,
            EdgeRefusal::Stale => Self::Stale,
        }
    }
}

/// An owning root on one node of one store. See the module docs.
///
/// Not constructible from a raw id: only [`SemanticGraphStore::lease`]
/// mints one, and only over a live node. Its id stays readable for as long
/// as the lease is held.
pub struct NodeLease {
    core: Arc<ArenaCore>,
    id: SemanticNodeId,
    /// The handle's own bytes, pinned in the process retention account for
    /// as long as the handle lives: a live handle can never be refused.
    _charge: RetentionCharge,
}

impl NodeLease {
    /// The leased node's id, valid in its store while this lease is held.
    #[must_use]
    pub fn id(&self) -> SemanticNodeId {
        self.id
    }

    /// Whether this lease was minted by `store`.
    #[must_use]
    pub fn belongs_to(&self, store: &SemanticGraphStore) -> bool {
        Arc::ptr_eq(&self.core, store.arena.core())
    }
}

impl std::fmt::Debug for NodeLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeLease")
            .field("arena", &self.core.identity())
            .field("id", &self.id)
            .finish()
    }
}

impl Drop for NodeLease {
    fn drop(&mut self) {
        self.core.release(self.id);
    }
}

/// The nodes one retained holder names, each counted once per occurrence.
/// See the module docs.
pub struct NodeRootSet {
    core: Arc<ArenaCore>,
    ids: Box<[SemanticNodeId]>,
}

impl NodeRootSet {
    /// The rooted ids, in the order the holder enumerated them.
    #[must_use]
    pub fn ids(&self) -> &[SemanticNodeId] {
        &self.ids
    }
}

impl Clone for NodeRootSet {
    fn clone(&self) -> Self {
        for id in self.ids.iter() {
            // Every id is counted by `self`, so it cannot be dying.
            let counted = self.core.acquire(*id);
            verter_debug_assert!(counted.is_ok(), "a rooted node was not live");
        }
        Self {
            core: Arc::clone(&self.core),
            ids: self.ids.clone(),
        }
    }
}

impl Drop for NodeRootSet {
    fn drop(&mut self) {
        self.core.release_all(self.ids.iter().copied());
    }
}

impl std::fmt::Debug for NodeRootSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeRootSet")
            .field("arena", &self.core.identity())
            .field("ids", &self.ids)
            .finish()
    }
}

/// The root set of one scope. Released when its last guard or handle
/// drops.
pub(super) struct ScopeRoots {
    core: Arc<ArenaCore>,
    held: Mutex<FxHashSet<SemanticNodeId>>,
}

impl ScopeRoots {
    /// Root `id` in this scope: `true` when the scope already holds it, or
    /// `acquire` counted it now. The check and the insertion are one step
    /// under the scope's lock, so two threads of one scope rooting the same
    /// node count it once.
    pub(super) fn root_with(&self, id: SemanticNodeId, acquire: impl FnOnce() -> bool) -> bool {
        let mut held = self.held.lock();
        if held.contains(&id) {
            return true;
        }
        if !acquire() {
            return false;
        }
        held.insert(id);
        true
    }

    /// Record `id`, whose count this scope already took (a fresh intern
    /// allocated with it).
    pub(super) fn adopt_counted(&self, id: SemanticNodeId) {
        let fresh = self.held.lock().insert(id);
        verter_debug_assert!(fresh, "a freshly allocated node was already rooted");
    }

    fn len(&self) -> usize {
        self.held.lock().len()
    }
}

impl Drop for ScopeRoots {
    fn drop(&mut self) {
        let held = std::mem::take(self.held.get_mut());
        self.core.release_all(held);
    }
}

thread_local! {
    /// The root scopes active on this thread, innermost last.
    static SCOPES: RefCell<Vec<Arc<ScopeRoots>>> = const { RefCell::new(Vec::new()) };
}

/// The scope of `core`'s store active on this thread, if any.
pub(super) fn current_scope(core: &Arc<ArenaCore>) -> Option<Arc<ScopeRoots>> {
    SCOPES.with(|scopes| {
        scopes
            .borrow()
            .iter()
            .rev()
            .find(|scope| Arc::ptr_eq(&scope.core, core))
            .cloned()
    })
}

/// A root scope active on this thread. See the module docs. Bound to the
/// thread it was entered on; [`Self::handle`] carries the scope elsewhere.
#[must_use = "a root scope roots the computation only while it is alive"]
pub struct SemanticRootScope {
    roots: Arc<ScopeRoots>,
    _thread_bound: PhantomData<*const ()>,
}

impl SemanticRootScope {
    fn enter(roots: Arc<ScopeRoots>) -> Self {
        SCOPES.with(|scopes| scopes.borrow_mut().push(Arc::clone(&roots)));
        Self {
            roots,
            _thread_bound: PhantomData,
        }
    }

    /// A handle that enters this same scope on another thread.
    #[must_use]
    pub fn handle(&self) -> RootScopeHandle {
        RootScopeHandle(Arc::clone(&self.roots))
    }

    /// Nodes the scope roots right now.
    #[must_use]
    pub fn rooted_len(&self) -> usize {
        self.roots.len()
    }
}

impl Drop for SemanticRootScope {
    fn drop(&mut self) {
        let popped = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let at = scopes
                .iter()
                .rposition(|scope| Arc::ptr_eq(scope, &self.roots));
            at.map(|at| scopes.remove(at))
        });
        // The stack's reference drops here, outside the thread-local
        // borrow: if it was the scope's last, its release may destroy
        // nodes.
        drop(popped);
    }
}

impl std::fmt::Debug for SemanticRootScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticRootScope")
            .field("arena", &self.roots.core.identity())
            .field("rooted", &self.roots.len())
            .finish()
    }
}

/// A scope carried to another thread: [`Self::enter`] activates it there.
/// Holding a handle keeps the scope's roots alive.
#[derive(Clone)]
pub struct RootScopeHandle(Arc<ScopeRoots>);

impl RootScopeHandle {
    /// Activate the scope on the calling thread.
    pub fn enter(&self) -> SemanticRootScope {
        SemanticRootScope::enter(Arc::clone(&self.0))
    }
}

impl std::fmt::Debug for RootScopeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RootScopeHandle")
            .field(&self.0.core.identity())
            .finish()
    }
}

impl SemanticGraphStore {
    /// Enter a root scope of this store on the calling thread, joining the
    /// one already active here (see the module docs).
    pub fn enter_root_scope(&self) -> SemanticRootScope {
        let core = self.arena.core();
        let roots = current_scope(core).unwrap_or_else(|| {
            Arc::new(ScopeRoots {
                core: Arc::clone(core),
                held: Mutex::new(FxHashSet::default()),
            })
        });
        SemanticRootScope::enter(roots)
    }

    /// Root `id` in the scope active on this thread: `false` when no scope
    /// of this store is active or the node cannot be rooted (released,
    /// dying, never handed out).
    pub fn root_in_scope(&self, id: SemanticNodeId) -> bool {
        let core = self.arena.core();
        match current_scope(core) {
            Some(scope) => scope.root_with(id, || core.acquire(id).is_ok()),
            None => false,
        }
    }

    /// Lease `id`: an owning root a caller outside the store may hold for
    /// as long as it needs the node. Refused for a stale or never-handed-out
    /// id.
    pub fn lease(&self, id: SemanticNodeId) -> Result<NodeLease, LeaseError> {
        let core = self.arena.core();
        core.acquire(id)?;
        Ok(NodeLease {
            core: Arc::clone(core),
            id,
            _charge: self
                .retention_account()
                .pin(std::mem::size_of::<NodeLease>()),
        })
    }

    /// Read a leased node's payload. A lease minted by another store is
    /// refused, never read as whatever this store holds under its id.
    pub fn leased_node_data(&self, lease: &NodeLease) -> Result<Arc<SemanticNodeData>, LeaseError> {
        if !lease.belongs_to(self) {
            return Err(LeaseError::Foreign);
        }
        self.arena.get(lease.id).ok_or(LeaseError::Unallocated)
    }

    /// Root the multiset `ids` for one retained holder, all or none.
    pub fn root_set(
        &self,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Result<NodeRootSet, LeaseError> {
        let core = self.arena.core();
        let mut counted: Vec<SemanticNodeId> = Vec::new();
        for id in ids {
            if let Err(refusal) = core.acquire(id) {
                core.release_all(counted);
                return Err(refusal.into());
            }
            counted.push(id);
        }
        Ok(NodeRootSet {
            core: Arc::clone(core),
            ids: counted.into_boxed_slice(),
        })
    }

    /// `id`'s counted edges and roots: `None` once it holds no payload;
    /// `u32::MAX - 1` for a permanent vocabulary node; `u32::MAX` while it
    /// is dying.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn node_refs_for_tests(&self, id: SemanticNodeId) -> Option<u32> {
        self.arena.core().refs(id)
    }

    /// Dying nodes queued and not destroyed yet.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn pending_destruction_for_tests(&self) -> usize {
        self.arena.core().pending_len()
    }
}

#[cfg(test)]
#[path = "node_roots_tests.rs"]
mod tests;
