//! Counted graph roots. Production canonical sweeps may retire these roots;
//! dropping an old handle then observes the tombstone and does nothing.

use std::cell::RefCell;
use std::sync::Arc;

use super::arena::NodeArenaCore;
use crate::semantic_query::{QueryError, SemanticNodeId};

struct RootOwner {
    arena: Arc<NodeArenaCore>,
    ids: Vec<SemanticNodeId>,
}

/// Storage acquired for a freshly interned node's construction root: the
/// Arc owner and its one-element operand allocation, plus the scope vector's
/// entry and geometric growth slack.
pub(crate) fn construction_root_bytes() -> usize {
    std::mem::size_of::<RootOwner>()
        + 2 * std::mem::size_of::<usize>()
        + std::mem::size_of::<SemanticNodeId>()
        + 2 * std::mem::size_of::<NodeRoot>()
}

impl Drop for RootOwner {
    fn drop(&mut self) {
        self.arena.release_roots(&self.ids);
    }
}

/// One immutable retained operand multiset. Clones share one root reservation;
/// separate owners acquire separate counts, including repeated operands.
#[derive(Clone, Default)]
pub struct NodeRootSet(Option<Arc<RootOwner>>);

/// An ownership attachment is transport, never semantic key identity.
/// Each wrapper holds its issuing arena's reservation even when the weak
/// process-wide interner shares equal content with another arena.
#[derive(Clone, Default)]
pub(crate) struct IdentityNodeRoots(pub(crate) NodeRootSet);

impl IdentityNodeRoots {
    pub(crate) fn scoped(ids: impl IntoIterator<Item = SemanticNodeId>) -> Self {
        Self(NodeRootScope::root_current(ids))
    }
}
impl PartialEq for IdentityNodeRoots {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for IdentityNodeRoots {}
impl std::hash::Hash for IdentityNodeRoots {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}
impl std::fmt::Debug for IdentityNodeRoots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityNodeRoots")
            .field("operands", &self.0.operands().len())
            .finish()
    }
}

impl std::fmt::Debug for NodeRootSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("NodeRootSet")
            .field(&self.operands())
            .finish()
    }
}

impl NodeRootSet {
    /// Keep a cache hit alive in the active issuing graph construction scope.
    pub fn retain_for_construction(&self) {
        self.capture_in_scope();
    }

    pub(crate) fn arena_identity(&self) -> Option<usize> {
        self.0
            .as_ref()
            .map(|owner| Arc::as_ptr(&owner.arena) as usize)
    }

    pub(crate) fn capture_in_scope(&self) {
        let Some(id) = self.operands().first().copied() else {
            return;
        };
        NodeRootScope::capture(NodeRoot::from_roots(id, self.clone()));
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn live_nodes(&self) -> usize {
        self.0
            .as_ref()
            .map_or(0, |owner| owner.arena.ownership_counts().live_nodes)
    }
    pub(crate) fn acquire(
        arena: Arc<NodeArenaCore>,
        ids: Vec<SemanticNodeId>,
    ) -> Result<Self, QueryError> {
        if ids.is_empty() {
            return Ok(Self::default());
        }
        arena.retain(&ids)?;
        Ok(Self(Some(Arc::new(RootOwner { arena, ids }))))
    }

    pub fn operands(&self) -> &[SemanticNodeId] {
        self.0.as_ref().map_or(&[], |owner| owner.ids.as_slice())
    }

    pub(crate) fn belongs_to(&self, arena: &Arc<NodeArenaCore>) -> bool {
        self.0
            .as_ref()
            .is_none_or(|owner| Arc::ptr_eq(&owner.arena, arena))
    }

    /// Borrow one operand this set owns. An ordinal the set does not hold is
    /// a foreign operand: a copied integer never authorizes a read.
    pub fn node_ref(&self, id: SemanticNodeId) -> Result<NodeRef<'_>, QueryError> {
        let owner = self.0.as_ref().ok_or(QueryError::ForeignSemanticOperand)?;
        if !owner.ids.contains(&id) {
            return Err(QueryError::ForeignSemanticOperand);
        }
        Ok(NodeRef {
            id,
            arena: &owner.arena,
        })
    }
}

/// A borrowed, arena-qualified capability to read one live node.
///
/// Only an owner mints it — a [`NodeRoot`], a [`NodeRootSet`] operand, a
/// [`super::node_data_lease::NodeDataLease`] child, a delivered
/// [`crate::semantic_query::CacheRead`] or an active [`NodeRootScope`] — and
/// it cannot outlive that owner. The node's ordinal alone authorizes nothing.
#[derive(Clone, Copy)]
pub struct NodeRef<'a> {
    id: SemanticNodeId,
    arena: &'a Arc<NodeArenaCore>,
}

impl std::fmt::Debug for NodeRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("NodeRef").field(&self.id).finish()
    }
}

impl<'a> NodeRef<'a> {
    pub(super) fn new(id: SemanticNodeId, arena: &'a Arc<NodeArenaCore>) -> Self {
        Self { id, arena }
    }

    /// The node's ordinal: an identity label that authorizes no read.
    #[must_use]
    pub fn id(self) -> SemanticNodeId {
        self.id
    }

    pub(super) fn arena(self) -> &'a Arc<NodeArenaCore> {
        self.arena
    }

    /// Read the payload under a new owning root. A slot a legacy close
    /// already released is stale; it never reads as a live payload.
    pub fn read(self) -> Result<super::node_data_lease::NodeDataLease, QueryError> {
        let roots = NodeRootSet::acquire(Arc::clone(self.arena), vec![self.id])?;
        let data = self
            .arena
            .live_payload(self.id)
            .ok_or(QueryError::StaleSemanticOperand)?;
        Ok(super::node_data_lease::NodeDataLease {
            data,
            root: NodeRoot::from_roots(self.id, roots),
        })
    }

    /// Acquire an owning root that outlives this borrow.
    pub fn to_root(self) -> Result<NodeRoot, QueryError> {
        let roots = NodeRootSet::acquire(Arc::clone(self.arena), vec![self.id])?;
        Ok(NodeRoot::from_roots(self.id, roots))
    }
}

/// A rooted node, qualified by its arena. The integer alone is not an owner.
#[derive(Clone, Debug)]
pub struct NodeRoot {
    id: SemanticNodeId,
    roots: NodeRootSet,
}

impl NodeRoot {
    pub(crate) fn from_roots(id: SemanticNodeId, roots: NodeRootSet) -> Self {
        Self { id, roots }
    }
    pub(crate) fn adopt(arena: Arc<NodeArenaCore>, id: SemanticNodeId) -> Self {
        Self {
            id,
            roots: NodeRootSet(Some(Arc::new(RootOwner {
                arena,
                ids: vec![id],
            }))),
        }
    }
    pub fn id(&self) -> SemanticNodeId {
        self.id
    }
    pub fn roots(&self) -> &NodeRootSet {
        &self.roots
    }
    /// Borrow the rooted node as a read capability.
    pub fn node_ref(&self) -> Result<NodeRef<'_>, QueryError> {
        self.roots.node_ref(self.id)
    }
}

std::thread_local! {
    static SCOPES: RefCell<Vec<ConstructionOwners>> = const { RefCell::new(Vec::new()) };
}

struct ConstructionOwners {
    arena: Arc<NodeArenaCore>,
    roots: Vec<NodeRoot>,
    deliveries: Vec<super::delivery_lease::SemanticDeliveryLease>,
}

/// A construction scope owns every interned intermediate on this thread,
/// independently of audit capture. Delivery acquires its own roots before
/// the scope releases its intermediates.
pub struct NodeRootScope {
    arena: usize,
    core: Arc<NodeArenaCore>,
    depth: usize,
    thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl NodeRootScope {
    pub(crate) fn current_arena_identity() -> Option<usize> {
        SCOPES.with(|scopes| {
            scopes
                .borrow()
                .last()
                .map(|scope| Arc::as_ptr(&scope.arena) as usize)
        })
    }
    pub(crate) fn enter(arena: &Arc<NodeArenaCore>) -> Self {
        let identity = Arc::as_ptr(arena) as usize;
        let depth = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let depth = scopes.len();
            scopes.push(ConstructionOwners {
                arena: Arc::clone(arena),
                roots: Vec::new(),
                deliveries: Vec::new(),
            });
            depth
        });
        Self {
            arena: identity,
            core: Arc::clone(arena),
            depth,
            thread_bound: std::marker::PhantomData,
        }
    }

    /// Borrow a construction ordinal of this scope's arena. The scope roots
    /// the node until it closes, so the capability cannot outlive its owner.
    pub fn node_ref(&self, id: SemanticNodeId) -> Result<NodeRef<'_>, QueryError> {
        let roots = NodeRootSet::acquire(Arc::clone(&self.core), vec![id])?;
        // Root it in THIS scope, not an inner one of the same arena that
        // could close while the borrow is still live.
        SCOPES.with(|scopes| {
            scopes.borrow_mut()[self.depth]
                .roots
                .push(NodeRoot::from_roots(id, roots));
        });
        Ok(NodeRef {
            id,
            arena: &self.core,
        })
    }

    pub(crate) fn capture(root: NodeRoot) {
        let Some(arena) = root.roots.arena_identity() else {
            return;
        };
        let mut root = Some(root);
        SCOPES.with(|scopes| {
            if let Some(scope) = scopes
                .borrow_mut()
                .iter_mut()
                .rev()
                .find(|candidate| Arc::as_ptr(&candidate.arena) as usize == arena)
            {
                scope.roots.push(root.take().unwrap());
            }
        });
        drop(root);
    }

    pub(crate) fn capture_delivery(
        lease: super::delivery_lease::SemanticDeliveryLease,
        arena: Option<usize>,
    ) {
        let mut lease = Some(lease);
        SCOPES.with(|scopes| {
            if let Some(scope) = scopes.borrow_mut().iter_mut().rev().find(|candidate| {
                arena.is_none_or(|arena| Arc::as_ptr(&candidate.arena) as usize == arena)
            }) {
                scope.deliveries.push(lease.take().unwrap());
            }
        });
        drop(lease);
    }

    /// Whether this thread holds a construction scope of `arena`.
    pub(super) fn holds(arena: &Arc<NodeArenaCore>) -> bool {
        SCOPES.with(|scopes| {
            scopes
                .borrow()
                .iter()
                .any(|scope| Arc::ptr_eq(&scope.arena, arena))
        })
    }

    /// Attach operands to a record minted within its issuing graph scope.
    /// Raw public fixtures outside a scope have no arena authority.
    pub(crate) fn root_current(ids: impl IntoIterator<Item = SemanticNodeId>) -> NodeRootSet {
        let arena =
            SCOPES.with(|scopes| scopes.borrow().last().map(|scope| Arc::clone(&scope.arena)));
        let Some(arena) = arena else {
            return NodeRootSet::default();
        };
        let ids = ids
            .into_iter()
            .filter(|node| {
                node.0 < super::UNALLOCATABLE_ID_FLOOR && (arena.counted() || arena.is_live(*node))
            })
            .collect();
        NodeRootSet::acquire(arena, ids).expect("scoped record operands are live")
    }
}

impl Drop for NodeRootScope {
    fn drop(&mut self) {
        let roots = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            assert_eq!(
                scopes.len(),
                self.depth + 1,
                "construction scopes must close in stack order"
            );
            let mut owners = scopes.pop().unwrap();
            assert_eq!(Arc::as_ptr(&owners.arena) as usize, self.arena);
            // A nested dispatch's raw value can be consumed by its enclosing
            // builder after the inner read envelope drops. Carry those
            // attachments to that builder until it seals its own delivery.
            if let Some(parent) = scopes
                .iter_mut()
                .rev()
                .find(|candidate| Arc::ptr_eq(&candidate.arena, &owners.arena))
            {
                parent.deliveries.append(&mut owners.deliveries);
            }
            owners
        });
        drop(roots);
    }
}
