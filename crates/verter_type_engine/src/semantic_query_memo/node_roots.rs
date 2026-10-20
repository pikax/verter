//! Counted graph roots: every holder of a node owns one count on it, and
//! the last release destroys the node (see `arena.rs`, **Lifetime**).

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

impl verter_session_query::retention::owned::Measured for RootOwner {
    /// Its operand list. The arena it roots in is the graph's storage,
    /// charged with it.
    fn heap_bytes(
        &self,
        _aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        root_ids_bytes(&self.ids)
    }
}

/// The heap an operand list holds once it spills past its inline ids.
fn root_ids_bytes(ids: &RootIds) -> usize {
    if ids.spilled() {
        verter_session_query::retention::allocation::heap_block_bytes(
            ids.capacity() * std::mem::size_of::<SemanticNodeId>(),
        )
    } else {
        0
    }
}

impl Drop for RootOwner {
    fn drop(&mut self) {
        self.arena.release_roots(&self.ids);
    }
}

/// One immutable retained operand multiset. Clones share one root reservation;
/// separate owners acquire separate counts, including repeated operands.
#[derive(Clone, Default, verter_retention::heap_size::HeapSize)]
pub struct NodeRootSet(Option<verter_session_query::retention::owned::Owned<RootOwner>>);

/// An ownership attachment is transport, never semantic key identity.
/// Each wrapper holds its issuing arena's reservation even when the weak
/// process-wide interner shares equal content with another arena.
#[derive(Clone, Default, verter_retention::heap_size::HeapSize)]
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
    /// The root owner's block and operand list this set holds, counted
    /// without alias tracking: for a holder that is the set's only charged
    /// owner.
    pub(crate) fn heap_bytes(&self) -> usize {
        use verter_session_query::retention::allocation::heap_block_bytes;
        self.0.as_ref().map_or(0, |owner| {
            verter_session_query::retention::owned::Owned::<RootOwner>::cell_block_bytes()
                .max(heap_block_bytes(0))
                + root_ids_bytes(&owner.ids)
        })
    }

    /// The root owner's block and operand list, once per `aliases`: the
    /// ownership metadata a retained holder of this set keeps alive. The
    /// rooted nodes themselves are the arena's.
    pub(crate) fn owned_heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        // The root owner is an owned allocation, charged by its own cell.
        self.0.as_ref().map_or(0, |owner| owner.offer(aliases))
    }

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
        Ok(Self(Some(
            verter_session_query::retention::owned::Owned::new(RootOwner { arena, ids }),
        )))
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

    /// Read the payload under a new owning root. The borrow's owner keeps
    /// the node live, so the read cannot find it destroyed.
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
            roots: NodeRootSet(Some(verter_session_query::retention::owned::Owned::new(
                RootOwner {
                    arena,
                    ids: smallvec::smallvec![id],
                },
            ))),
        }
    }
    pub fn id(&self) -> SemanticNodeId {
        self.id
    }
    pub fn roots(&self) -> &NodeRootSet {
        &self.roots
    }
    /// Bytes the root set's own allocation is charged, and its class; `None`
    /// while no owner has charged it. An observation that retains nothing.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn root_charge_for_tests(
        &self,
    ) -> Option<(usize, verter_session_query::retention::ChargeClass)> {
        let owned = self.roots.0.as_ref()?;
        owned.charge_bytes().zip(owned.charge_class())
    }
    /// Borrow the rooted node as a read capability.
    pub fn node_ref(&self) -> Result<NodeRef<'_>, QueryError> {
        self.roots.node_ref(self.id)
    }
    /// The checker diagnostic code when this node is the checker's recovered
    /// error type (a TS2590 cross product, say) rather than a built type.
    pub fn checker_recovery_code(&self) -> Option<u32> {
        let data = self.node_ref().ok()?.read().ok()?;
        match &*data {
            crate::semantic_query::SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                diagnostic,
                ..
            }) => Some(diagnostic.code.code()),
            _ => None,
        }
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

    /// Hand everything a closing task owner holds — its construction
    /// counts, its roots, its deliveries and the carriers handed to it — to
    /// the newest owner of its arena open on this thread, which then holds
    /// them exactly as if they had been constructed under it. A dispatcher's
    /// lane owns nothing, so it is never the heir: with no owner open they
    /// are released.
    fn absorb(&mut self, mut owners: ConstructionOwners) -> Option<ConstructionOwners> {
        let arena = Arc::clone(&owners.arena);
        let Some(heir) = self
            .entries
            .iter_mut()
            .rev()
            .find(|candidate| candidate.is_owner() && Arc::ptr_eq(&candidate.arena, &arena))
        else {
            return Some(owners);
        };
        heir.constructed.append(&mut owners.constructed);
        heir.roots.append(&mut owners.roots);
        heir.held.append(&mut owners.held);
        for lease in owners.deliveries.drain(..) {
            // A repeat is another handle on an owner the heir keeps.
            let _ = heir.adopt(lease);
        }
        owners.delivered.clear();
        None
    }

    /// Remove a closing owner and hand its delivery attachments to the
    /// nearest remaining owner of the same arena: the one it was opened
    /// inside of first, else one opened after it that is still open. A
    /// nested dispatch's raw value can be consumed by its enclosing builder
    /// after the inner read envelope drops; the attachments stay owned until
    /// that builder seals its own delivery. A dispatcher's construction lane
    /// is never an heir: an operation that closes with no enclosing owner
    /// releases everything its result's own carrier does not hold.
    fn close(&mut self, token: u64) -> ConstructionOwners {
        let position = self.position(token);
        let mut owners = self.entries.remove(position);
        if let Some(heir) = self.heir(position, &owners.arena) {
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

    /// The owner of `arena` a closing entry at `position` hands on to: the
    /// one it was opened inside of first, else one opened after it that is
    /// still open. A dispatcher's lane is never one.
    fn heir(&self, position: usize, arena: &Arc<NodeArenaCore>) -> Option<usize> {
        let heir_of = |candidate: &ConstructionOwners| {
            candidate.is_owner() && Arc::ptr_eq(&candidate.arena, arena)
        };
        self.entries[..position]
            .iter()
            .rposition(heir_of)
            .or_else(|| {
                self.entries[position..]
                    .iter()
                    .position(heir_of)
                    .map(|offset| position + offset)
            })
    }

    /// Remove a carrier's construction owner and keep everything it holds
    /// for the carrier. A builder owner still open on the thread keeps a
    /// share of it too, as it keeps every construction made under it, and
    /// releases that share when it closes.
    fn seal(&mut self, token: u64) -> ConstructionOwners {
        let position = self.position(token);
        let owners = self.entries.remove(position);
        if let Some(heir) = self.heir(position, &owners.arena) {
            let heir = &mut self.entries[heir];
            if !owners.constructed.is_empty() {
                // Each intern count the carrier keeps is one acquisition;
                // the builder's share is another.
                heir.arena
                    .retain(&owners.constructed)
                    .expect("a sealed construction's operands are live");
                heir.constructed.extend_from_slice(&owners.constructed);
            }
            heir.roots.extend(owners.roots.iter().cloned());
            for lease in &owners.deliveries {
                let _ = heir.adopt(lease.share());
            }
        }
        owners
    }
}

struct ConstructionOwners {
    token: u64,
    arena: Arc<NodeArenaCore>,
    /// How many dispatchers share this entry as their construction lane;
    /// `0` for an owner. A lane is the authority to read and root
    /// construction ordinals of its graph on this thread; it holds nothing,
    /// so a dispatcher never keeps a result past the operation that
    /// produced it.
    dispatchers: usize,
    /// Whether this owner is a construction a result carrier is being built
    /// under ([`ConstructionSink`]): every delivery read while it is open is
    /// kept here, so the carrier it seals into owns it.
    carrier: bool,
    /// Whether this owner is a task's ([`TaskConstructionOwner`]) installed
    /// for one poll: owners the polling thread opened before it are not the
    /// task's, so what its steps read is never handed to them.
    task: bool,
    roots: Vec<NodeRoot>,
    /// The ordinals interned while this owner was the innermost one of its
    /// arena: each holds the one count its intern took, released together
    /// when the owner closes.
    constructed: Vec<SemanticNodeId>,
    deliveries: Vec<super::delivery_lease::SemanticDeliveryLease>,
    /// Result carriers handed to this owner ([`Constructed::into_value_in`]),
    /// kept until it closes.
    held: Vec<ConstructionHold>,
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
            carrier: false,
            task: false,
            roots: Vec::new(),
            constructed: Vec::new(),
            deliveries: Vec::new(),
            held: Vec::new(),
            delivered: Default::default(),
        }
    }

    /// Whether this entry owns what is captured into it: every scope and
    /// operation does, a dispatcher's construction lane does not.
    fn is_owner(&self) -> bool {
        self.dispatchers == 0
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

/// The construction owner of a task that may run on any thread. Between
/// polls it holds what the task constructed — the counts its interns took,
/// the roots and the deliveries its steps left — and while one poll runs it
/// is the innermost construction owner of its arena on the polling thread
/// ([`Self::install`]), so the task's construction ordinals stay rooted
/// across its suspensions whichever thread resumes it. It is an owner, never
/// a dispatcher's lane: an operation a poll enters finds it open and
/// constructs under it. Dropping it hands what it holds to the newest owner
/// of its arena open on the dropping thread.
pub(crate) struct TaskConstructionOwner {
    /// `None` only while installed on a thread.
    owners: Option<ConstructionOwners>,
}

impl TaskConstructionOwner {
    pub(crate) fn new(arena: &Arc<NodeArenaCore>) -> Self {
        let mut owners = ConstructionOwners::new(0, arena);
        owners.task = true;
        Self {
            owners: Some(owners),
        }
    }

    /// Make this the innermost construction owner of its arena on this
    /// thread until the returned installation drops.
    pub(crate) fn install(&mut self) -> InstalledTaskConstruction<'_> {
        let mut owners = self
            .owners
            .take()
            .expect("a task's construction owner is installed on one thread at a time");
        let token = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            owners.token = scopes.mint();
            let token = owners.token;
            scopes.entries.push(owners);
            token
        });
        InstalledTaskConstruction {
            owner: self,
            token,
            thread_bound: std::marker::PhantomData,
        }
    }
}

impl Drop for TaskConstructionOwner {
    fn drop(&mut self) {
        if let Some(owners) = self.owners.take() {
            let released = SCOPES.with(|scopes| scopes.borrow_mut().absorb(owners));
            drop(released);
        }
    }
}

/// One poll's installation of a [`TaskConstructionOwner`]; dropping it takes
/// the owner back off the thread, unwinding included.
pub(crate) struct InstalledTaskConstruction<'t> {
    owner: &'t mut TaskConstructionOwner,
    token: u64,
    thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Drop for InstalledTaskConstruction<'_> {
    fn drop(&mut self) {
        let owners = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let position = scopes.position(self.token);
            scopes.entries.remove(position)
        });
        self.owner.owners = Some(owners);
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

/// The construction lane every live dispatcher of one graph shares on its
/// thread: the authority to read and root that graph's construction
/// ordinals while any of them lives. A lane owns nothing. Each operation a
/// dispatcher runs opens its own construction owner, and what the operation
/// delivers is owned by the carrier it returns (or by an explicit builder
/// owner the caller holds open), so a discarded result releases its roots
/// at the end of its operation while the dispatcher stays alive, and a
/// retained result outlives every dispatcher. Dispatchers may be
/// interleaved and dropped in any order.
pub(crate) struct DispatchConstructionLane {
    token: u64,
    thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl DispatchConstructionLane {
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

impl Drop for DispatchConstructionLane {
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
                .find(|candidate| {
                    candidate.is_owner() && Arc::as_ptr(&candidate.arena) as usize == arena
                })
            {
                scope.roots.push(root.take().unwrap());
            }
        });
        drop(root);
    }

    /// Hand the count a fresh intern of `id` took in `arena` to the
    /// innermost construction owner of that arena on this thread, which
    /// releases it when it closes; with no such owner (a dispatcher's lane
    /// owns nothing) the count is released now.
    pub(crate) fn capture_constructed(arena: &Arc<NodeArenaCore>, id: SemanticNodeId) {
        let held = SCOPES.with(|scopes| {
            scopes
                .borrow_mut()
                .entries
                .iter_mut()
                .rev()
                .find(|candidate| candidate.is_owner() && Arc::ptr_eq(&candidate.arena, arena))
                .map(|scope| scope.constructed.push(id))
                .is_some()
        });
        if !held {
            arena.release_roots(&[id]);
        }
    }

    /// Hand a delivery lease to a construction owner on this thread: the
    /// OLDEST owner of its arena at or inside the newest installed task
    /// owner of that arena ([`TaskConstructionOwner::install`]), or the
    /// innermost owner when the lease names no arena. A closing owner hands its deliveries to another open
    /// owner of the arena, so a delivery stays held until no owner of its
    /// arena is open on the thread, whichever owner holds it; the oldest is
    /// the one least likely to close first, so the delivery is not handed on
    /// again at every nested scope's close. While a result carrier is being
    /// built ([`ConstructionSink`]) its owner keeps the delivery instead, so
    /// the carrier owns what its build read. A dispatcher's lane is not an
    /// owner: with no operation or builder open, the lease's own holder is
    /// its only owner.
    pub(crate) fn capture_delivery(
        lease: super::delivery_lease::SemanticDeliveryLease,
        arena: Option<usize>,
    ) {
        let mut lease = Some(lease);
        SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let owner = match arena {
                Some(arena) => {
                    let of_arena = |candidate: &ConstructionOwners| {
                        candidate.is_owner() && Arc::as_ptr(&candidate.arena) as usize == arena
                    };
                    // A task's poll owns what its steps read: an owner the
                    // polling thread opened before the task was installed
                    // may close while the task is pending, so the search
                    // starts at the newest installed task owner.
                    let floor = scopes
                        .entries
                        .iter()
                        .rposition(|candidate| candidate.task && of_arena(candidate))
                        .unwrap_or(0);
                    let entries = &mut scopes.entries[floor..];
                    // A carrier under construction keeps every delivery its
                    // build reads; otherwise the oldest owner does.
                    match entries
                        .iter()
                        .rposition(|candidate| candidate.carrier && of_arena(candidate))
                    {
                        Some(carrier) => entries.get_mut(carrier),
                        None => entries.iter_mut().find(|candidate| of_arena(candidate)),
                    }
                }
                None => scopes
                    .entries
                    .iter_mut()
                    .rev()
                    .find(|candidate| candidate.is_owner()),
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

    /// Whether this thread holds a construction owner of `arena`: an open
    /// operation or builder scope, never merely a dispatcher's lane.
    pub(crate) fn holds_owner(arena: &Arc<NodeArenaCore>) -> bool {
        SCOPES.with(|scopes| {
            scopes
                .borrow()
                .entries
                .iter()
                .any(|scope| scope.is_owner() && Arc::ptr_eq(&scope.arena, arena))
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
/// An operand the arena does not hold live — a synthetic ordinal no graph
/// allocated, or a stale one — owns nothing and is not rooted: the record
/// keeps naming it, and every read through it fails as stale or foreign.
pub(crate) fn root_in_arena(
    arena: Arc<NodeArenaCore>,
    ids: impl IntoIterator<Item = SemanticNodeId>,
) -> NodeRootSet {
    let mut ids: RootIds = ids
        .into_iter()
        .filter(|node| node.0 < super::UNALLOCATABLE_ID_FLOOR)
        .collect();
    if ids.is_empty() {
        return NodeRootSet::default();
    }
    arena.retain_live(&mut ids);
    if ids.is_empty() {
        return NodeRootSet::default();
    }
    NodeRootSet(Some(verter_session_query::retention::owned::Owned::new(
        RootOwner { arena, ids },
    )))
}

impl Drop for NodeRootScope {
    fn drop(&mut self) {
        let owners = SCOPES.with(|scopes| scopes.borrow_mut().close(self.token));
        drop(owners);
    }
}

/// The construction owner a dispatcher result carrier is built under: it
/// keeps everything the build interns, roots and reads on this thread, and
/// [`Self::seal`] closes it into the [`Constructed`] carrier that then owns
/// it. Dropped unsealed (an unwinding build), it closes like any scope.
pub(crate) struct ConstructionSink {
    /// `None` once sealed.
    token: Option<u64>,
    core: Arc<NodeArenaCore>,
    thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl ConstructionSink {
    pub(crate) fn enter(arena: &Arc<NodeArenaCore>) -> Self {
        let token = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let token = scopes.mint();
            let mut owners = ConstructionOwners::new(token, arena);
            owners.carrier = true;
            scopes.entries.push(owners);
            token
        });
        Self {
            token: Some(token),
            core: Arc::clone(arena),
            thread_bound: std::marker::PhantomData,
        }
    }

    /// Close the build into the carrier of `value`.
    pub(crate) fn seal<T>(mut self, value: T) -> Constructed<T> {
        let token = self.token.take().expect("a sink seals once");
        let owners = SCOPES.with(|scopes| scopes.borrow_mut().seal(token));
        let core = Arc::clone(&self.core);
        let empty = owners.constructed.is_empty()
            && owners.roots.is_empty()
            && owners.deliveries.is_empty()
            && owners.held.is_empty();
        Constructed {
            value,
            hold: ConstructionHold {
                core,
                owners: (!empty).then(|| Arc::new(owners)),
            },
        }
    }
}

impl Drop for ConstructionSink {
    fn drop(&mut self) {
        if let Some(token) = self.token {
            let owners = SCOPES.with(|scopes| scopes.borrow_mut().close(token));
            drop(owners);
        }
    }
}

/// The construction a [`Constructed`] carrier owns. Clones share it; the
/// last holder releases it.
#[derive(Clone)]
struct ConstructionHold {
    core: Arc<NodeArenaCore>,
    owners: Option<Arc<ConstructionOwners>>,
}

impl ConstructionHold {
    fn owns(&self, id: SemanticNodeId) -> bool {
        self.owners.as_ref().is_some_and(|owners| {
            owners.constructed.contains(&id)
                || owners
                    .roots
                    .iter()
                    .any(|root| root.roots.operands().contains(&id))
                || owners
                    .deliveries
                    .iter()
                    .any(|lease| lease.roots().contains(&id))
        })
    }
}

/// A dispatcher construction result in its owning carrier.
///
/// The carrier owns the construction that produced its value — every node
/// the build interned, rooted or read — so the value's operands stay live
/// exactly as long as this carrier or a clone of it does: past its
/// operation, its dispatcher, every builder scope and every cache. The
/// value is reached only through the carrier: borrowed while it is held
/// ([`Self::value`]), or handed to an explicit builder owner the caller
/// holds, which then keeps the construction until it closes
/// ([`Self::into_value_in`]). The value itself is an identity label that
/// authorizes nothing; a read borrows a capability from the carrier
/// ([`Self::node`]) that cannot outlive it. Only the dispatcher mints one.
pub struct Constructed<T> {
    value: T,
    hold: ConstructionHold,
}

impl<T: Clone> Clone for Constructed<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            hold: self.hold.clone(),
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Constructed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Constructed").field(&self.value).finish()
    }
}

/// Two carriers are equal when their values are; the construction they own
/// is ownership transport, never identity.
impl<T: PartialEq> PartialEq for Constructed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}
impl<T: Eq> Eq for Constructed<T> {}

impl<T> Constructed<T> {
    /// The carried value, borrowed for as long as the carrier — the owner of
    /// every node it names — is held.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Hand this construction to `builder`, which keeps it until the builder
    /// closes, and take the value: its labels name live nodes for exactly as
    /// long as that builder scope is open.
    pub fn into_value_in(self, builder: &NodeRootScope) -> T {
        SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let position = scopes.position(builder.token);
            scopes.entries[position].held.push(self.hold);
        });
        self.value
    }

    /// Borrow a node this carrier owns. An ordinal the carrier does not own
    /// is foreign: a copied label never authorizes a read.
    pub fn node(&self, id: SemanticNodeId) -> Result<NodeRef<'_>, QueryError> {
        if self.hold.owns(id) {
            Ok(NodeRef {
                id,
                arena: &self.hold.core,
            })
        } else {
            Err(QueryError::ForeignSemanticOperand)
        }
    }

    /// The same construction carrying a value derived from this one: the
    /// owner travels with the narrowed value.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Constructed<U> {
        Constructed {
            value: f(self.value),
            hold: self.hold,
        }
    }
}
