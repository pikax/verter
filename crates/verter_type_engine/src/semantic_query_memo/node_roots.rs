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
            depth,
            thread_bound: std::marker::PhantomData,
        }
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
