//! The memoized `reaches an unresolved carrier` verdict over the
//! hash-consed semantic graph.
//!
//! One question, asked by the flow-return success carrier and by the
//! fail-closed test rail: does this VALUE reach a carrier whose own
//! resolution answered "not known"?
//!
//! It lives in its own module because the answer is an INDUCTIVE function
//! of the node id — `self_bit(n) || any(bit(child))` — over an immutable,
//! hash-consed DAG, memoized per id. That shape is what removes the need
//! for a traversal bound: a bounded walk that reports "unresolved" on
//! exhaustion computes a DIFFERENT function, and a 4,100-arm literal
//! union with zero misses is fully known.
//!
//! ## The memo needs no invalidation
//!
//! The node arena is APPEND-ONLY: `invalidate_all` deliberately does not
//! reset it, and `invalidate_canonical` only drops shard-dedup entries so
//! a later intern mints a fresh id — every `SemanticNodeId` already handed
//! out keeps resolving to the same payload forever. This bit is a pure
//! function of that payload plus its children's bits, and it STOPS at
//! every shallow carrier, so it never depends on content behind a carrier
//! the way the `(node_id, ctx) -> result_id` hash-cons memos do. Those are
//! cleared on every edit; this one must not be, and clearing it would only
//! cost recomputation.
//!
//! The one exception is a node's destruction: its bits are pure retention
//! once its payload is gone, so the store drops them with the arena's
//! destroyed log (`release.rs`). A destroyed id asked again reads the
//! `Opaque(Miss)` placeholder and answers `true` — the conservative answer
//! — without memoizing it.

use super::*;

pub(super) const STRUCTURE_BITS_STORAGE_GRANULE: usize = 4 * 1024;

/// The single per-node bit table and its pinned backing. Removal can reduce
/// HashMap::capacity through tombstones without freeing its allocation, so
/// backing_capacity changes only on growth or an explicit geometric shrink.
pub(super) struct StructureBits {
    entries: FxHashMap<SemanticNodeId, NodeStructureBits>,
    backing_capacity: usize,
    storage: verter_session_query::retention::granular::GranularPin,
    #[cfg(test)]
    retirement_work: usize,
}

impl StructureBits {
    pub(super) fn new(
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self {
            entries: FxHashMap::default(),
            backing_capacity: 0,
            storage: verter_session_query::retention::granular::GranularPin::new(
                account,
                STRUCTURE_BITS_STORAGE_GRANULE,
            ),
            #[cfg(test)]
            retirement_work: 0,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn get(&self, node: &SemanticNodeId) -> Option<&NodeStructureBits> {
        self.entries.get(node)
    }

    fn set_unresolved(&mut self, node: SemanticNodeId, bit: bool) {
        self.entries.entry(node).or_default().unresolved = Some(bit);
        self.settle_growth();
    }

    fn set_inert(&mut self, node: SemanticNodeId, bit: bool) {
        self.entries.entry(node).or_default().inert = Some(bit);
        self.settle_growth();
    }

    fn set_versioned_ref(&mut self, node: SemanticNodeId, bit: bool) {
        self.entries.entry(node).or_default().versioned_ref = Some(bit);
        self.settle_growth();
    }

    fn settle_growth(&mut self) {
        let capacity = self.entries.capacity();
        if capacity > self.backing_capacity {
            self.settle_capacity(capacity);
        }
    }

    fn settle_capacity(&mut self, capacity: usize) {
        let bytes = |capacity| {
            verter_retention::heap_size::hash_table_bytes(
                capacity,
                std::mem::size_of::<(SemanticNodeId, NodeStructureBits)>(),
            )
        };
        let (before, after) = (bytes(self.backing_capacity), bytes(capacity));
        if after > before {
            self.storage.add(after - before);
        } else {
            self.storage.sub(before - after);
        }
        self.backing_capacity = capacity;
    }

    pub(super) fn remove(&mut self, node: &SemanticNodeId) -> bool {
        #[cfg(test)]
        {
            self.retirement_work += 1;
        }
        let removed = self.entries.remove(node).is_some();
        if !removed {
            return false;
        }
        if self.entries.is_empty() {
            self.entries = FxHashMap::default();
            self.settle_capacity(0);
        } else if self.backing_capacity > self.entries.len().saturating_mul(4) {
            #[cfg(test)]
            {
                self.retirement_work += self.entries.len();
            }
            self.entries.shrink_to(self.entries.len().saturating_mul(2));
            self.settle_capacity(self.entries.capacity());
        }
        removed
    }

    #[cfg(test)]
    pub(super) fn retirement_work(&self) -> usize {
        self.retirement_work
    }

    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.backing_capacity
    }

    /// Bytes the table's backing is pinned for.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn storage_charge_bytes(&self) -> usize {
        self.storage.charged()
    }
}

/// The structural bits memoized for ONE node id — the value of the store's
/// single per-node sidecar. Each bit is an inductive function of the node's
/// immutable payload, decided on first demand; sharing one entry gives them
/// one lifecycle, so a release of the node id drops them together.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeStructureBits {
    /// [`SemanticGraphStore::node_reaches_unresolved`], once decided.
    pub unresolved: Option<bool>,
    /// [`SemanticGraphStore::node_is_inert_structure`], once decided.
    pub inert: Option<bool>,
    /// [`SemanticGraphStore::node_reaches_versioned_reference`], once
    /// decided.
    pub versioned_ref: Option<bool>,
}

impl SemanticGraphStore {
    construction_ordinal_seam! {
        /// Whether the VALUE `root` denotes REACHES a semantic-miss carrier —
        /// a node whose own resolution answered "not known".
        ///
        /// The bit is a pure INDUCTIVE function of the node id over the
        /// hash-consed, immutable graph (`self_bit(n) || any(bit(child))`),
        /// memoized per id, so it is decided ONCE per node for the lifetime of
        /// the store and is O(1) amortized at every later read. There is no
        /// budget and no exhaustion arm: a bound whose trip reports
        /// "unresolved" computes a DIFFERENT function — a 4,100-arm literal
        /// union with zero misses is fully known, and a factually false
        /// "unresolved" verdict over it propagates as a permanent warm refusal
        /// into every enclosing result.
        ///
        /// The walk descends the structure a value COMPOSES or lowers inline
        /// and STOPS at every shallow carrier (`DeclRef`, `InstantiationRef`'s
        /// base, `BareRef`, `ImportType`, `TypeOf`, `MergedDecl`). Descending a
        /// carrier would be materialisation, which the shallow-by-default rule
        /// forbids — and a miss INSIDE a referenced declaration is that
        /// declaration's own admission problem, gated by its own query.
        /// Carrier TYPE ARGUMENTS are locally-supplied structure and do
        /// descend, through the one sanctioned accessor.
        fn node_reaches_unresolved(&self, root: SemanticNodeId) -> bool {
            if let Some(bit) = self
                .unresolved_reach
                .lock()
                .get(&root)
                .and_then(|bits| bits.unresolved)
            {
                return bit;
            }
            struct Frame {
                node: SemanticNodeId,
                children: Vec<SemanticNodeId>,
                next: usize,
                bit: bool,
            }
            let mut local: FxHashMap<SemanticNodeId, bool> = FxHashMap::default();
            let mut on_path: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
            // A back edge would make a memoized `false` an assumption rather
            // than a fact. The interner cannot produce one (a node's children
            // are interned before it), so this never fires in production — but
            // if it ever did, the computation answers without POISONING the
            // store's memo.
            let mut cycle_seen = false;
            let (children, bit) = self.node_unresolved_step(root);
            let mut stack: Vec<Frame> = vec![Frame {
                node: root,
                children,
                next: 0,
                bit,
            }];
            on_path.insert(root);
            while let Some(frame) = stack.last_mut() {
                // Short-circuit: a decided-true frame needs no further child.
                if frame.bit || frame.next >= frame.children.len() {
                    let (node, bit) = (frame.node, frame.bit);
                    stack.pop();
                    on_path.remove(&node);
                    local.insert(node, bit);
                    if !cycle_seen {
                        self.unresolved_reach.lock().set_unresolved(node, bit);
                        self.track_structure_bits(node);
                    }
                    if let Some(parent) = stack.last_mut() {
                        parent.bit |= bit;
                    }
                    continue;
                }
                let child = frame.children[frame.next];
                frame.next += 1;
                if let Some(&known) = local.get(&child) {
                    frame.bit |= known;
                    continue;
                }
                let memoized = self
                    .unresolved_reach
                    .lock()
                    .get(&child)
                    .and_then(|bits| bits.unresolved);
                if let Some(known) = memoized {
                    frame.bit |= known;
                    continue;
                }
                if on_path.contains(&child) {
                    cycle_seen = true;
                    continue;
                }
                let (children, bit) = self.node_unresolved_step(child);
                on_path.insert(child);
                stack.push(Frame {
                    node: child,
                    children,
                    next: 0,
                    bit,
                });
            }
            local.get(&root).copied().unwrap_or(true)
        }
    }

    construction_ordinal_seam! {
        /// Whether the VALUE `root` denotes REACHES a versioned reference —
        /// a `DeclRef` or `InstantiationRef` whose embedded identity carries
        /// a non-zero whole-file content version.
        ///
        /// The retained-reference observation at a cold-build close uses
        /// this to walk only the spine of a value that can intern one, in
        /// the same retained topology it descends
        /// ([`SemanticNodeData::for_each_retained_child`]), so a value of
        /// pure local structure — a deferred-operator chain over
        /// primitives, for instance — costs the close one memo read, not a
        /// walk of its whole node graph. Like the unresolved bit, this is a
        /// pure INDUCTIVE function of the immutable node id
        /// (`self_bit(n) || any(bit(child))`), memoized per id in the same
        /// per-node entry ([`NodeStructureBits`]) and decided once for the
        /// lifetime of the store. A carrier's own head sets the bit (its
        /// identity is the versioned thing); its locally-supplied type
        /// arguments still descend.
        fn node_reaches_versioned_reference(&self, root: SemanticNodeId) -> bool {
            if let Some(bit) = self
                .unresolved_reach
                .lock()
                .get(&root)
                .and_then(|bits| bits.versioned_ref)
            {
                return bit;
            }
            struct Frame {
                node: SemanticNodeId,
                children: Vec<SemanticNodeId>,
                next: usize,
                bit: bool,
            }
            let mut local: FxHashMap<SemanticNodeId, bool> = FxHashMap::default();
            let mut on_path: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
            // The interner cannot produce a back edge (a node's children
            // are interned before it), so this never fires in production;
            // if it ever did, the computation answers without poisoning
            // the store's memo.
            let mut cycle_seen = false;
            let (children, bit) = self.node_versioned_ref_step(root);
            let mut stack: Vec<Frame> = vec![Frame {
                node: root,
                children,
                next: 0,
                bit,
            }];
            on_path.insert(root);
            while let Some(frame) = stack.last_mut() {
                // Short-circuit: a decided-true frame needs no further child.
                if frame.bit || frame.next >= frame.children.len() {
                    let (node, bit) = (frame.node, frame.bit);
                    stack.pop();
                    on_path.remove(&node);
                    local.insert(node, bit);
                    if !cycle_seen {
                        self.unresolved_reach.lock().set_versioned_ref(node, bit);
                        self.track_structure_bits(node);
                    }
                    if let Some(parent) = stack.last_mut() {
                        parent.bit |= bit;
                    }
                    continue;
                }
                let child = frame.children[frame.next];
                frame.next += 1;
                if let Some(&known) = local.get(&child) {
                    frame.bit |= known;
                    continue;
                }
                let memoized = self
                    .unresolved_reach
                    .lock()
                    .get(&child)
                    .and_then(|bits| bits.versioned_ref);
                if let Some(known) = memoized {
                    frame.bit |= known;
                    continue;
                }
                if on_path.contains(&child) {
                    cycle_seen = true;
                    continue;
                }
                let (children, bit) = self.node_versioned_ref_step(child);
                on_path.insert(child);
                stack.push(Frame {
                    node: child,
                    children,
                    next: 0,
                    bit,
                });
            }
            local.get(&root).copied().unwrap_or(false)
        }
    }

    /// One node's own versioned-reference bit plus the retained children
    /// the reference-version walk descends into — exactly the topology
    /// [`SemanticNodeData::for_each_retained_child`] enumerates, so the bit
    /// and the walk can never disagree about what a value retains.
    fn node_versioned_ref_step(&self, node: SemanticNodeId) -> (Vec<SemanticNodeId>, bool) {
        let Some(data) = self.node_data(node) else {
            return (Vec::new(), false);
        };
        let mut versioned = false;
        match data.as_ref() {
            SemanticNodeData::DeclRef { identity } => {
                versioned = identity.whole_hash != [0u8; 16];
            }
            SemanticNodeData::InstantiationRef { base, .. } => {
                versioned = base.whole_hash != [0u8; 16];
            }
            _ => {}
        }
        let mut children = Vec::new();
        data.for_each_retained_child(|child| children.push(child));
        (children, versioned)
    }

    /// One node's own unresolved bit plus the children the reach walk
    /// descends into.
    ///
    /// The match is exhaustive with no wildcard: a new [`SemanticNodeData`]
    /// variant does not compile until it is dispositioned as
    /// descend-or-stop here.
    fn node_unresolved_step(&self, node: SemanticNodeId) -> (Vec<SemanticNodeId>, bool) {
        let Some(data) = self.node_data(node) else {
            // A node id the graph cannot resolve is not proof of a known
            // value.
            return (Vec::new(), true);
        };
        // The three structural carriers' arguments are locally-supplied
        // structure: they descend through the ONE sanctioned accessor (the
        // carriers' own heads do not).
        let mut children: Vec<SemanticNodeId> = data.carrier_type_args().to_vec();
        let mut unresolved = false;
        match data.as_ref() {
            SemanticNodeData::Opaque(error) => {
                unresolved = error.means_type_is_not_yet_known();
            }
            // A `RawFallback` is a display-only raw-text passthrough with
            // no typed content behind it. `SemanticNodeData::
            // means_type_is_not_yet_known` already classifies it as
            // not-known, and it is not a carrier a later query resolves, so
            // it is not-known here too.
            SemanticNodeData::RawFallback { .. } => unresolved = true,
            // -- Composed / inline structure: descend --------------------
            SemanticNodeData::Alias(inner) => children.push(*inner),
            SemanticNodeData::Object(surface) => {
                for member in surface.positive_members() {
                    children.extend(crate::semantic_query::authored_property_key_child(
                        &member.key,
                    ));
                    children.push(member.value);
                }
                children.extend_from_slice(&surface.call_signatures);
                children.extend_from_slice(&surface.construct_signatures);
                for index in surface.index_signatures.iter() {
                    children.push(index.key_type);
                    children.push(index.value_type);
                }
                children.extend(surface.keyspace);
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                children.extend(program.child_nodes());
            }
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let members = composite.composite_members().expect("composite arm");
                children.extend_from_slice(members);
            }
            SemanticNodeData::Array { element, .. } => children.push(*element),
            SemanticNodeData::Tuple { elements, .. } => {
                children.extend(elements.iter().map(|element| element.value));
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                children.extend_from_slice(expressions);
            }
            SemanticNodeData::KeyOf { base } => children.push(*base),
            SemanticNodeData::IndexedAccess { object, index } => {
                children.push(*object);
                children.extend(crate::semantic_query::authored_property_key_child(index));
            }
            SemanticNodeData::Mapped { source, mapper } => {
                children.push(*source);
                children.push(mapper.key_space);
                // Identity mappings read the source member itself. Their
                // dormant template slot is not an adopted value dependency.
                if mapper.kind != crate::semantic_query::MapperKind::Identity {
                    children.push(mapper.value_expr);
                }
                children.extend(mapper.name_remap);
                children.extend(mapper.modifiers_type);
                // A wildcard-free match covers VARIANTS, not struct FIELDS:
                // the mapper's own parameter node is a descent too.
                children.push(mapper.parameter_node);
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                pending,
                ..
            } => {
                if let Some(pending) = pending {
                    children.extend(pending.argument_nodes());
                }
                children.push(*check);
                children.push(*extends);
                children.push(*true_branch_ref);
                children.push(*false_branch_ref);
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                predicate,
                return_carrier,
                ..
            } => {
                children.extend(params.iter().map(|param| param.ty));
                // A dormant owned source is not an evaluated missing value.
                // Once its return is actually demanded, the rebuilt slot
                // carries the value or its typed gap and participates here.
                if !(matches!(return_carrier, crate::semantic_query::SignatureReturnCarrier::DeferredFlow(_))
                    && matches!(self.node_data(*return_type).as_deref(), Some(SemanticNodeData::Opaque(QueryError::Miss)))) {
                    children.push(*return_type);
                }
                for parameter in type_parameters.iter() {
                    children.extend(parameter.constraint);
                    children.extend(parameter.default);
                }
                children.extend(predicate.and_then(|predicate| predicate.ty));
            }
            SemanticNodeData::InstantiationRef { args, .. } => children.extend_from_slice(args),
            // A class expression's type arguments and instance surface are
            // structure the class expression's own evaluation produced —
            // never a declaration a later query materialises — so a miss
            // inside them is this value's.
            SemanticNodeData::ClassExpressionInstance {
                type_arguments,
                surface,
                ..
            } => {
                children.extend_from_slice(type_arguments);
                children.push(*surface);
            }
            // A deferred intrinsic application is a KNOWN value whose operands
            // are locally-supplied structure: descend them, and never set the
            // unresolved bit for the application itself.
            SemanticNodeData::IntrinsicApplication { args, .. } => {
                children.extend_from_slice(args);
            }
            // A sealed callable carrier opens only to its two sanctioned
            // consumers — terminal, and decided by construction.
            SemanticNodeData::DeferredCallable(_) => {}
            // -- Settled leaves and SHALLOW CARRIERS: stop ---------------
            //
            // `Primitive` / `Literal` / `Infer` / `InferRef` /
            // `SyntheticBinding` are settled values. `TypeParam` is a
            // binder, and its constraint / default are the DECLARATION's
            // meaning, not this value's. `DeclRef`, `InstantiationRef`'s
            // base, `MergedDecl`, `BareRef`, `ImportType` and `TypeOf` are
            // shallow carriers: descending one would materialise a
            // referenced declaration, which the shallow-by-default rule
            // forbids, and a miss inside it is that declaration's own
            // admission problem.
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_)
            | SemanticNodeData::TypeParam { .. }
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::MergedDecl { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::TypeOf(_)
            // The nominal terminal is a RESOLVED scalar — its declaring
            // identity is known content, never an unresolved name.
            | SemanticNodeData::TypeOfNominal(_)
            | SemanticNodeData::SyntheticBinding { .. } => {}
        }
        (children, unresolved)
    }

    construction_ordinal_seam! {
        /// Whether `root` is an INERT STRUCTURE: a primitive or literal, or an
        /// array or rest-free tuple whose elements are inert structures.
        ///
        /// Nothing in such a node names a declaration, a binder or a deferred
        /// operator, so the declaration-body projection rebuilds it unchanged
        /// under every context and need not walk into it. The same inductive,
        /// per-id memo as [`Self::node_reaches_unresolved`] (`self_bit(n) &&
        /// all(bit(child))` over the append-only arena), in the same per-node
        /// entry ([`NodeStructureBits`]), so an argument that a generic
        /// instantiation grows by one level at every step costs one step to
        /// classify, not its whole depth.
        fn node_is_inert_structure(&self, root: SemanticNodeId) -> bool {
            let mut stack: Vec<(SemanticNodeId, bool)> = vec![(root, false)];
            while let Some((node, expanded)) = stack.pop() {
                if self.inert_bit(node).is_some() {
                    continue;
                }
                let children = match self.node_data(node).as_deref() {
                    Some(SemanticNodeData::Primitive(_) | SemanticNodeData::Literal(_)) => {
                        Some(Vec::new())
                    }
                    Some(SemanticNodeData::Array { element, .. }) => Some(vec![*element]),
                    Some(SemanticNodeData::Tuple { elements, .. })
                        if elements.iter().all(|element| !element.rest) =>
                    {
                        Some(elements.iter().map(|element| element.value).collect())
                    }
                    _ => None,
                };
                let Some(children) = children else {
                    self.set_inert_bit(node, false);
                    continue;
                };
                if expanded {
                    let bit = children
                        .iter()
                        .all(|child| self.inert_bit(*child).unwrap_or(false));
                    self.set_inert_bit(node, bit);
                    continue;
                }
                stack.push((node, true));
                stack.extend(children.into_iter().map(|child| (child, false)));
            }
            self.inert_bit(root).unwrap_or(false)
        }
    }

    fn inert_bit(&self, node: SemanticNodeId) -> Option<bool> {
        self.unresolved_reach
            .lock()
            .get(&node)
            .and_then(|bits| bits.inert)
    }

    fn set_inert_bit(&self, node: SemanticNodeId, bit: bool) {
        self.unresolved_reach.lock().set_inert(node, bit);
        self.track_structure_bits(node);
    }

    /// Mark `node`'s just-written structure bits for cleanup when it is
    /// destroyed. A destroyed id is answered (it reads the placeholder) but
    /// never memoized: no later destruction names it, so its entry would be
    /// a residue for the life of the store.
    fn track_structure_bits(&self, node: SemanticNodeId) {
        if !self.track_sidecar(node) {
            self.unresolved_reach.lock().remove(&node);
        }
    }

    /// The memoized structural bits of `node`, if any was decided.
    #[cfg(any(test, feature = "test-support"))]
    pub fn node_structure_bits_for_tests(&self, node: SemanticNodeId) -> Option<NodeStructureBits> {
        self.unresolved_reach.lock().get(&node).copied()
    }
}
