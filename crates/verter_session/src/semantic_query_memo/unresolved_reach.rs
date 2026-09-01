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

use super::*;

impl SemanticGraphStore {
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
    pub(crate) fn node_reaches_unresolved(&self, root: SemanticNodeId) -> bool {
        if let Some(&bit) = self.unresolved_reach.lock().get(&root) {
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
                    self.unresolved_reach.lock().insert(node, bit);
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
            let memoized = self.unresolved_reach.lock().get(&child).copied();
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
                children.push(mapper.value_expr);
                children.extend(mapper.name_remap);
                // A wildcard-free match covers VARIANTS, not struct FIELDS:
                // the mapper's own parameter node is a descent too.
                children.push(mapper.parameter_node);
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                children.push(*check);
                children.push(*extends);
                children.push(*true_branch_ref);
                children.push(*false_branch_ref);
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                ..
            } => {
                children.extend(params.iter().map(|param| param.ty));
                children.push(*return_type);
                for parameter in type_parameters.iter() {
                    children.extend(parameter.constraint);
                    children.extend(parameter.default);
                }
            }
            SemanticNodeData::InstantiationRef { args, .. } => children.extend_from_slice(args),
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
            | SemanticNodeData::TypeParam { .. }
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::MergedDecl { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::SyntheticBinding { .. } => {}
        }
        (children, unresolved)
    }
}
