//! Counted graph roots. Production canonical sweeps may retire these roots;
//! dropping an old handle then observes the tombstone and does nothing.

use std::cell::RefCell;
use std::sync::Arc;

use super::arena::NodeArenaCore;
use crate::semantic_query::{QueryError, SemanticNodeId};

struct RootOwner {
    arena: Arc<NodeArenaCore>,
    ids: RootIds,
}

/// The operands one root set holds. Most sets hold a node or two, so they
/// are stored in place rather than in an allocation of their own.
pub(crate) type RootIds = smallvec::SmallVec<[SemanticNodeId; 4]>;

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
    /// Root operands in the innermost construction scope on this thread.
    /// Only a fixture with no issuing graph to name uses this: a record
    /// whose graph is known roots there ([`Self::in_graph`]), because the
    /// innermost scope may belong to another graph's dispatcher.
    pub(crate) fn scoped(ids: impl IntoIterator<Item = SemanticNodeId>) -> Self {
        Self(NodeRootScope::root_current(ids))
    }

    /// Root a record's operands in the graph that issued it, whatever
    /// graph's scope is innermost on this thread.
    pub(crate) fn in_graph(
        graph: &super::SemanticGraphStore,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Self {
        Self::in_arena(&graph.arena.core, ids)
    }

    /// Root a record's operands in the arena of the graph that issued it.
    pub(crate) fn in_arena(
        arena: &Arc<NodeArenaCore>,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Self {
        Self(root_in_arena(Arc::clone(arena), ids))
    }

    /// Root a record derived from `sources` in the graph that issued them:
    /// the first source holding an attachment names it. A derivation whose
    /// sources own no graph operand has none to root.
    pub(crate) fn derived_from<'a>(
        sources: impl IntoIterator<Item = &'a Self>,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Self {
        sources
            .into_iter()
            .find_map(|source| source.0.arena())
            .map_or_else(Self::default, |arena| Self::in_arena(&arena, ids))
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

    fn arena(&self) -> Option<Arc<NodeArenaCore>> {
        self.0.as_ref().map(|owner| Arc::clone(&owner.arena))
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
    pub(crate) fn acquire(arena: Arc<NodeArenaCore>, ids: RootIds) -> Result<Self, QueryError> {
        if ids.is_empty() {
            return Ok(Self::default());
        }
        arena.retain(&ids)?;
        Ok(Self(Some(Arc::new(RootOwner { arena, ids }))))
    }

    /// Retain the live operands of `ids` from `arena`, dropping any id that
    /// names no live node.
    pub(crate) fn acquire_live(arena: Arc<NodeArenaCore>, mut ids: RootIds) -> Self {
        if ids.is_empty() {
            return Self::default();
        }
        arena.retain_live(&mut ids);
        if ids.is_empty() {
            return Self::default();
        }
        Self(Some(Arc::new(RootOwner { arena, ids })))
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
        let roots = NodeRootSet::acquire(Arc::clone(self.arena), smallvec::smallvec![self.id])?;
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
        let roots = NodeRootSet::acquire(Arc::clone(self.arena), smallvec::smallvec![self.id])?;
        Ok(NodeRoot::from_roots(self.id, roots))
    }
}

/// A rooted node, qualified by its arena. The integer alone is not an owner.
#[derive(Clone, Debug)]
pub struct NodeRoot {
    id: SemanticNodeId,
    roots: NodeRootSet,
}

/// Two roots are equal when they name the same node of the same graph.
impl PartialEq for NodeRoot {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.roots.arena_identity() == other.roots.arena_identity()
    }
}
impl Eq for NodeRoot {}

impl NodeRoot {
    pub(crate) fn from_roots(id: SemanticNodeId, roots: NodeRootSet) -> Self {
        Self { id, roots }
    }
    pub(crate) fn adopt(arena: Arc<NodeArenaCore>, id: SemanticNodeId) -> Self {
        Self {
            id,
            roots: NodeRootSet(Some(Arc::new(RootOwner {
                arena,
                ids: smallvec::smallvec![id],
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

/// The authority to open construction scopes on one graph: the producer
/// lane in which a bare construction ordinal may be read or rooted.
///
/// Non-forgeable: its field is private, it has no public constructor and no
/// `Default`. Only [`crate::project_semantic_dispatch::engine_resources::EngineStores`]
/// mints one, together with the graph it is bound to; holding the graph
/// alone grants no construction lane. Clones are held by the host's store
/// and by each request binding that composes a dispatcher.
#[derive(Clone)]
pub struct NodeConstructionGrant {
    arena: Arc<NodeArenaCore>,
}

impl std::fmt::Debug for NodeConstructionGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeConstructionGrant")
            .finish_non_exhaustive()
    }
}

impl NodeConstructionGrant {
    pub(crate) fn mint(graph: &super::SemanticGraphStore) -> Self {
        Self {
            arena: Arc::clone(&graph.arena.core),
        }
    }

    pub(crate) fn arena(&self) -> &Arc<NodeArenaCore> {
        &self.arena
    }

    /// Whether this grant authorizes construction on `graph`.
    #[must_use]
    pub fn authorizes(&self, graph: &super::SemanticGraphStore) -> bool {
        Arc::ptr_eq(&self.arena, &graph.arena.core)
    }

    /// Open a construction scope of the granted graph on this thread.
    #[must_use]
    pub fn enter_scope(&self) -> NodeRootScope {
        NodeRootScope::enter(&self.arena)
    }

    /// Root construction operands of the granted graph for delivery.
    pub fn root_nodes(
        &self,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Result<NodeRootSet, QueryError> {
        NodeRootSet::acquire(Arc::clone(&self.arena), ids.into_iter().collect())
    }
}

std::thread_local! {
    static SCOPES: RefCell<ScopeStack> = const {
        RefCell::new(ScopeStack {
            entries: Vec::new(),
            next_token: 0,
        })
    };
}

/// The construction owners open on this thread, oldest first. An entry is
/// addressed by its token, never by its position, so owners may close in any
/// order.
struct ScopeStack {
    entries: Vec<ConstructionOwners>,
    next_token: u64,
}

impl ScopeStack {
    fn mint(&mut self) -> u64 {
        self.next_token += 1;
        self.next_token
    }

    /// Owners usually close innermost first, so the search starts at the
    /// newest.
    fn position(&self, token: u64) -> usize {
        self.entries
            .iter()
            .rposition(|entry| entry.token == token)
            .expect("an open construction owner is registered on its thread")
    }

    /// Remove a closing owner and hand its delivery attachments to the
    /// nearest remaining owner of the same arena: the one it was opened
    /// inside of first, else one opened after it that is still open. A
    /// nested dispatch's raw value can be consumed by its enclosing builder
    /// after the inner read envelope drops; the attachments stay owned until
    /// that builder seals its own delivery.
    fn close(&mut self, token: u64) -> ConstructionOwners {
        let position = self.position(token);
        let mut owners = self.entries.remove(position);
        let arena = &owners.arena;
        let heir = self.entries[..position]
            .iter()
            .rposition(|candidate| Arc::ptr_eq(&candidate.arena, arena))
            .or_else(|| {
                self.entries[position..]
                    .iter()
                    .position(|candidate| Arc::ptr_eq(&candidate.arena, arena))
                    .map(|offset| position + offset)
            });
        if let Some(heir) = heir {
            let heir = &mut self.entries[heir];
            // The heir keeps the union, so merge the smaller holding into
            // the larger: an attachment that rises through every enclosing
            // owner of a deep evaluation is moved, not re-adopted, at each.
            if heir.deliveries.len() < owners.deliveries.len() {
                std::mem::swap(&mut heir.deliveries, &mut owners.deliveries);
                std::mem::swap(&mut heir.delivered, &mut owners.delivered);
            }
            for lease in owners.deliveries.drain(..) {
                // A repeat is another handle on an owner the heir keeps.
                let _ = heir.adopt(lease);
            }
            owners.delivered.clear();
        }
        owners
    }
}

struct ConstructionOwners {
    token: u64,
    arena: Arc<NodeArenaCore>,
    /// How many dispatchers share this owner; `0` for a lexical scope.
    dispatchers: usize,
    roots: Vec<NodeRoot>,
    /// The ordinals interned while this owner was the innermost one of its
    /// arena: each holds the one count its intern took, released together
    /// when the owner closes.
    constructed: Vec<SemanticNodeId>,
    deliveries: Vec<super::delivery_lease::SemanticDeliveryLease>,
    /// The shared delivery owners `deliveries` already holds. Every clone
    /// of one delivery names the same owner, so an owner keeps it once
    /// however often it is read or cloned under this owner.
    delivered: rustc_hash::FxHashSet<usize>,
}

impl Drop for ConstructionOwners {
    fn drop(&mut self) {
        if !self.constructed.is_empty() {
            self.arena.release_roots(&self.constructed);
        }
    }
}

impl ConstructionOwners {
    fn new(token: u64, arena: &Arc<NodeArenaCore>) -> Self {
        Self {
            token,
            arena: Arc::clone(arena),
            dispatchers: 0,
            roots: Vec::new(),
            constructed: Vec::new(),
            deliveries: Vec::new(),
            delivered: Default::default(),
        }
    }

    /// Keep `lease` unless this owner already holds its delivery; a
    /// repeat is handed back for the caller to drop.
    fn adopt(
        &mut self,
        lease: super::delivery_lease::SemanticDeliveryLease,
    ) -> Option<super::delivery_lease::SemanticDeliveryLease> {
        match lease.owner_identity() {
            Some(owner) if !self.delivered.insert(owner) => Some(lease),
            _ => {
                self.deliveries.push(lease);
                None
            }
        }
    }
}

/// A construction scope owns every interned intermediate on this thread,
/// independently of audit capture. Delivery acquires its own roots before
/// the scope releases its intermediates. Scopes close in any order: each
/// releases exactly the owners it holds.
pub struct NodeRootScope {
    token: u64,
    core: Arc<NodeArenaCore>,
    thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

/// The construction owner every live dispatcher of one graph shares on its
/// thread. A construction ordinal a dispatcher returns stays rooted while
/// any dispatcher of that graph on the thread is alive, so interleaved
/// dispatchers never release one another's operands, and they may be
/// dropped in any order.
pub(crate) struct DispatchConstructionScope {
    token: u64,
    thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl DispatchConstructionScope {
    pub(crate) fn enter(arena: &Arc<NodeArenaCore>) -> Self {
        let token = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let shared = scopes
                .entries
                .iter()
                .position(|entry| entry.dispatchers > 0 && Arc::ptr_eq(&entry.arena, arena));
            // The newest dispatcher's graph is the thread's current one.
            let mut owners = match shared {
                Some(position) => scopes.entries.remove(position),
                None => {
                    let token = scopes.mint();
                    ConstructionOwners::new(token, arena)
                }
            };
            owners.dispatchers += 1;
            let token = owners.token;
            scopes.entries.push(owners);
            token
        });
        Self {
            token,
            thread_bound: std::marker::PhantomData,
        }
    }
}

impl Drop for DispatchConstructionScope {
    fn drop(&mut self) {
        let released = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let position = scopes.position(self.token);
            let entry = &mut scopes.entries[position];
            entry.dispatchers -= 1;
            (entry.dispatchers == 0).then(|| scopes.close(self.token))
        });
        drop(released);
    }
}

impl NodeRootScope {
    pub(crate) fn current_arena_identity() -> Option<usize> {
        SCOPES.with(|scopes| {
            scopes
                .borrow()
                .entries
                .last()
                .map(|scope| Arc::as_ptr(&scope.arena) as usize)
        })
    }
    pub(crate) fn enter(arena: &Arc<NodeArenaCore>) -> Self {
        let token = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let token = scopes.mint();
            scopes.entries.push(ConstructionOwners::new(token, arena));
            token
        });
        Self {
            token,
            core: Arc::clone(arena),
            thread_bound: std::marker::PhantomData,
        }
    }

    /// Borrow a construction ordinal of this scope's arena. The scope roots
    /// the node until it closes, so the capability cannot outlive its owner.
    pub fn node_ref(&self, id: SemanticNodeId) -> Result<NodeRef<'_>, QueryError> {
        let roots = NodeRootSet::acquire(Arc::clone(&self.core), smallvec::smallvec![id])?;
        // Root it in THIS scope, not an inner one of the same arena that
        // could close while the borrow is still live.
        SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let position = scopes.position(self.token);
            scopes.entries[position]
                .roots
                .push(NodeRoot::from_roots(id, roots));
        });
        Ok(NodeRef {
            id,
            arena: &self.core,
        })
    }

    /// Root construction operands of this scope's arena for delivery beyond
    /// the scope.
    pub fn root_nodes(
        &self,
        ids: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Result<NodeRootSet, QueryError> {
        NodeRootSet::acquire(Arc::clone(&self.core), ids.into_iter().collect())
    }

    pub(crate) fn capture(root: NodeRoot) {
        let Some(arena) = root.roots.arena_identity() else {
            return;
        };
        let mut root = Some(root);
        SCOPES.with(|scopes| {
            if let Some(scope) = scopes
                .borrow_mut()
                .entries
                .iter_mut()
                .rev()
                .find(|candidate| Arc::as_ptr(&candidate.arena) as usize == arena)
            {
                scope.roots.push(root.take().unwrap());
            }
        });
        drop(root);
    }

    /// Hand the count a fresh intern of `id` took in `arena` to the
    /// innermost construction owner of that arena on this thread, which
    /// releases it when it closes; with no such owner the count is released
    /// now.
    pub(crate) fn capture_constructed(arena: &Arc<NodeArenaCore>, id: SemanticNodeId) {
        let held = SCOPES.with(|scopes| {
            scopes
                .borrow_mut()
                .entries
                .iter_mut()
                .rev()
                .find(|candidate| Arc::ptr_eq(&candidate.arena, arena))
                .map(|scope| scope.constructed.push(id))
                .is_some()
        });
        if !held {
            arena.release_roots(&[id]);
        }
    }

    /// Hand a delivery lease to a construction owner on this thread: the
    /// OLDEST owner of its arena, or the innermost owner when the lease
    /// names no arena. A closing owner hands its deliveries to another open
    /// owner of the arena, so a delivery stays held until no owner of its
    /// arena is open on the thread, whichever owner holds it; the oldest is
    /// the one least likely to close first, so the delivery is not handed on
    /// again at every nested scope's close.
    pub(crate) fn capture_delivery(
        lease: super::delivery_lease::SemanticDeliveryLease,
        arena: Option<usize>,
    ) {
        let mut lease = Some(lease);
        SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let owner = match arena {
                Some(arena) => scopes
                    .entries
                    .iter_mut()
                    .find(|candidate| Arc::as_ptr(&candidate.arena) as usize == arena),
                None => scopes.entries.last_mut(),
            };
            if let Some(owner) = owner {
                lease = owner.adopt(lease.take().unwrap());
            }
        });
        drop(lease);
    }

    /// Delivery leases the construction owners on this thread hold.
    #[cfg(test)]
    pub(crate) fn retained_deliveries_for_tests() -> usize {
        SCOPES.with(|scopes| {
            scopes
                .borrow()
                .entries
                .iter()
                .map(|owners| owners.deliveries.len())
                .sum()
        })
    }

    /// Whether this thread holds a construction scope of `arena`.
    pub(super) fn holds(arena: &Arc<NodeArenaCore>) -> bool {
        SCOPES.with(|scopes| {
            scopes
                .borrow()
                .entries
                .iter()
                .any(|scope| Arc::ptr_eq(&scope.arena, arena))
        })
    }

    /// Attach operands to a record minted within its issuing graph scope.
    /// Raw public fixtures outside a scope have no arena authority.
    pub(crate) fn root_current(ids: impl IntoIterator<Item = SemanticNodeId>) -> NodeRootSet {
        let arena = SCOPES.with(|scopes| {
            scopes
                .borrow()
                .entries
                .last()
                .map(|scope| Arc::clone(&scope.arena))
        });
        let Some(arena) = arena else {
            return NodeRootSet::default();
        };
        root_in_arena(arena, ids)
    }
}

/// Root a record's operands against the arena of the graph that issued it.
pub(crate) fn root_in_arena(
    arena: Arc<NodeArenaCore>,
    ids: impl IntoIterator<Item = SemanticNodeId>,
) -> NodeRootSet {
    let ids = ids
        .into_iter()
        .filter(|node| {
            node.0 < super::UNALLOCATABLE_ID_FLOOR && (arena.counted() || arena.is_live(*node))
        })
        .collect();
    NodeRootSet::acquire(arena, ids).expect("scoped record operands are live")
}

impl Drop for NodeRootScope {
    fn drop(&mut self) {
        let owners = SCOPES.with(|scopes| scopes.borrow_mut().close(self.token));
        drop(owners);
    }
}
