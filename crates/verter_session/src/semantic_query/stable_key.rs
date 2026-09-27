//! `VerterStableV1` representation-only stable keys.
//!
//! Category/sub-tag table and encodings are versioned integers with
//! explicit little-endian layout. The published order is the pair
//! `(fingerprint, exact key)`. Comparators never force bodies, resolve
//! names, run relations, instantiate, or reduce unions.

use std::cmp::Ordering;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::semantic_query::composite::CompositeOriginCategory;
use crate::semantic_query::{
    AuthoredPropertyKey, FunctionParam, LiteralValue, MapperKind, NodeScopeId, NullabilityPolicy,
    OptionalityMod, PredicateSubject, PrimitiveKind, QueryError, ReadonlyMod, ScopeId,
    SemanticNodeData, SemanticNodeId, SignatureKind, SignatureNodeOccurrence,
    SignatureReturnCarrier, SurfaceEntry, SurfaceMember, TypeParamDecl,
};
use crate::semantic_query_memo::SemanticGraphStore;
use verter_type_expr::facts::{
    FlowFunctionReturnIdentity, FunctionPartIdentity, FunctionReturnSource, ValueDeclIdentityPart,
};
use verter_type_expr::locators::{
    AuthoredAnchor, FunctionReturnLocator, LocatorSymbolSpace, TypeBodyPathStep,
    TypeParamBoundPosition,
};
use verter_type_expr::SyntheticCarrierSurfaceKind;

use super::semantic_context::{
    project_order_domain, project_union_order, OrderDomainId, SemanticContext, SemanticContextId,
    SemanticOrderPolicyId, SemanticUnionMembersKey,
};

/// Schema version mixed into every exact key.
pub const VERTER_STABLE_V1: u8 = 1;

/// Domain-separated FNV-1a offset for the fingerprint half of the pair.
const FINGERPRINT_OFFSET: u64 = 0xcbf2_9ce4_8422_2325 ^ 0x5634_5354_4142_4c45;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

/// Versioned category tags (not Rust discriminants).
pub mod category {
    pub const INTRINSIC: u8 = 1;
    pub const LITERAL: u8 = 2;
    pub const AUTHORED: u8 = 3;
    pub const ANONYMOUS: u8 = 4;
    pub const BINDER: u8 = 5;
    pub const SYNTHETIC: u8 = 6;
    pub const RECURSIVE: u8 = 7;
}

/// Versioned sub-tags within each category.
pub mod subtag {
    pub const PRIM_STRING: u8 = 1;
    pub const PRIM_NUMBER: u8 = 2;
    pub const PRIM_BOOLEAN: u8 = 3;
    pub const PRIM_SYMBOL: u8 = 4;
    pub const PRIM_BIGINT: u8 = 5;
    pub const PRIM_ANY: u8 = 6;
    pub const PRIM_UNKNOWN: u8 = 7;
    pub const PRIM_VOID: u8 = 8;
    pub const PRIM_NEVER: u8 = 9;
    pub const PRIM_NULL: u8 = 10;
    pub const PRIM_UNDEFINED: u8 = 11;
    pub const PRIM_OBJECT: u8 = 12;
    pub const LIT_STRING: u8 = 1;
    pub const LIT_NUMBER: u8 = 2;
    pub const LIT_BOOLEAN: u8 = 3;
    pub const LIT_BIGINT: u8 = 4;
    pub const OPAQUE: u8 = 20;
    pub const INTRINSIC_APP: u8 = 21;
    pub const DECL_REF: u8 = 1;
    pub const INSTANTIATION_REF: u8 = 2;
    pub const TYPEOF: u8 = 3;
    pub const TYPEOF_NOMINAL: u8 = 4;
    pub const BARE_REF: u8 = 5;
    pub const IMPORT_TYPE: u8 = 6;
    pub const ALIAS: u8 = 7;
    pub const SIGNATURE: u8 = 8;
    pub const OBJECT: u8 = 9;
    pub const MERGED_DECL: u8 = 10;
    pub const CLASS_EXPRESSION_INSTANCE: u8 = 11;
    pub const ENUM_LITERAL: u8 = 12;
    pub const TYPE_PARAM: u8 = 1;
    pub const INFER: u8 = 2;
    pub const INFER_REF: u8 = 3;
    pub const SYNTHETIC_BINDING: u8 = 4;
    pub const UNION: u8 = 1;
    pub const INTERSECTION: u8 = 2;
    pub const ARRAY: u8 = 3;
    pub const TUPLE: u8 = 4;
    pub const TEMPLATE: u8 = 5;
    pub const KEYOF: u8 = 6;
    pub const INDEXED_ACCESS: u8 = 7;
    pub const MAPPED: u8 = 8;
    pub const CONDITIONAL: u8 = 9;
    pub const OBJECT_SPREAD: u8 = 10;
    pub const DEFERRED_CALLABLE: u8 = 11;
    pub const RAW_FALLBACK: u8 = 12;
    // Recursive references. Sub-tags 1 (a cycle back-reference) and 2 (the
    // depth-exhausted marker) are retired: the arena is acyclic by contract.
    pub const SHARED_SUBTREE: u8 = 3;
}

/// Exact key plus versioned fingerprint. Order is the whole pair
/// `(fingerprint, exact)` and equality is over the same two fields, so two
/// keys compare `Equal` exactly when they are equal.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct StableKey {
    fingerprint: u64,
    exact: Vec<u8>,
}

impl StableKey {
    /// Build from exact encoding bytes. Fingerprint is FNV-1a over those bytes.
    #[must_use]
    pub fn from_exact(exact: Vec<u8>) -> Self {
        Self {
            fingerprint: fingerprint_v1(&exact),
            exact,
        }
    }

    /// Test-only: inject a fingerprint collision while keeping distinct exact bytes.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn with_forced_fingerprint(exact: Vec<u8>, fingerprint: u64) -> Self {
        Self { fingerprint, exact }
    }

    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    #[must_use]
    pub fn exact(&self) -> &[u8] {
        &self.exact
    }
}

/// Versioned FNV-1a 64 of `exact`, domain-separated from storage hashes.
#[must_use]
pub fn fingerprint_v1(exact: &[u8]) -> u64 {
    let mut hash = FINGERPRINT_OFFSET;
    for byte in exact {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Where one child's key goes in its parent's encoding.
enum Hole {
    /// One child key, written as a `u32` length and its bytes.
    Child(SemanticNodeId),
    /// An unordered child collection: its member keys sorted and
    /// deduplicated by exact bytes, written as a `u16` count and each key.
    Set(Vec<SemanticNodeId>),
    /// A child written, after its `lead` bytes, only when its structure
    /// differs from the child of the earlier single-child hole `same_as`.
    Unless {
        child: SemanticNodeId,
        same_as: usize,
        lead: Vec<u8>,
    },
}

/// One node's encoding with its children left as holes. Scalar fields are
/// written inline; each hole is filled by the walk with the child's own
/// complete key, so building a recipe never reads another node.
struct Recipe {
    buf: Vec<u8>,
    /// Holes in write order, each at the `buf` offset it follows.
    holes: Vec<(usize, Hole)>,
}

impl Recipe {
    fn new() -> Self {
        let mut enc = Self {
            buf: Vec::with_capacity(32),
            holes: Vec::new(),
        };
        enc.u8(VERTER_STABLE_V1);
        enc
    }

    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }

    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }

    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    fn header(&mut self, category: u8, subtag: u8) {
        self.u8(category);
        self.u8(subtag);
    }

    /// A child whose key is embedded in place.
    fn child(&mut self, id: SemanticNodeId) {
        self.holes.push((self.buf.len(), Hole::Child(id)));
    }

    /// An unordered child collection, normalized as a set of keys.
    fn child_set(&mut self, members: Vec<SemanticNodeId>) {
        self.holes.push((self.buf.len(), Hole::Set(members)));
    }

    /// A child written, after `lead`, only when its structure differs from
    /// the child of the earlier single-child hole `same_as` (a hole index).
    fn child_unless_same(&mut self, child: SemanticNodeId, same_as: usize, lead: &[u8]) {
        self.holes.push((
            self.buf.len(),
            Hole::Unless {
                child,
                same_as,
                lead: lead.to_vec(),
            },
        ));
    }
}

fn primitive_subtag(kind: PrimitiveKind) -> u8 {
    match kind {
        PrimitiveKind::String => subtag::PRIM_STRING,
        PrimitiveKind::Number => subtag::PRIM_NUMBER,
        PrimitiveKind::Boolean => subtag::PRIM_BOOLEAN,
        PrimitiveKind::Symbol => subtag::PRIM_SYMBOL,
        PrimitiveKind::BigInt => subtag::PRIM_BIGINT,
        PrimitiveKind::Any => subtag::PRIM_ANY,
        PrimitiveKind::Unknown => subtag::PRIM_UNKNOWN,
        PrimitiveKind::Void => subtag::PRIM_VOID,
        PrimitiveKind::Never => subtag::PRIM_NEVER,
        PrimitiveKind::Null => subtag::PRIM_NULL,
        PrimitiveKind::Undefined => subtag::PRIM_UNDEFINED,
        PrimitiveKind::Object => subtag::PRIM_OBJECT,
    }
}

fn origin_tag(category: CompositeOriginCategory) -> u8 {
    match category {
        CompositeOriginCategory::Canonical(NullabilityPolicy::Strict) => 1,
        CompositeOriginCategory::Canonical(NullabilityPolicy::Erased) => 8,
        CompositeOriginCategory::CanonicalUnproven => 2,
        CompositeOriginCategory::AuthoredShell => 3,
        CompositeOriginCategory::OrderedCarrier => 4,
        CompositeOriginCategory::PreservingRebuild => 5,
        CompositeOriginCategory::QuerySubject => 6,
        #[cfg(any(test, feature = "test-support"))]
        CompositeOriginCategory::TestFixture => 7,
        CompositeOriginCategory::Heritage => 9,
        CompositeOriginCategory::OverloadGroup => 10,
        CompositeOriginCategory::MergedOverloadGroup => 11,
    }
}

fn encode_owner(enc: &mut Recipe, owner: verter_type_expr::TopLevelOwnerId) {
    enc.u8(match owner.kind() {
        verter_type_expr::TopLevelOwnerKind::Module => 1,
        verter_type_expr::TopLevelOwnerKind::Instance => 2,
        verter_type_expr::TopLevelOwnerKind::Frontmatter => 3,
    });
    enc.u32(owner.ordinal());
}

fn encode_scope(enc: &mut Recipe, scope: &NodeScopeId) {
    match scope {
        NodeScopeId::Global => enc.u8(0),
        NodeScopeId::File {
            canonical_id,
            owner,
            local_scope,
            whole_hash: _,
        } => {
            enc.u8(1);
            enc.str(canonical_id);
            encode_owner(enc, *owner);
            match local_scope {
                None => enc.u8(0),
                Some(id) => {
                    enc.u8(1);
                    enc.u32(*id);
                }
            }
        }
    }
}

fn encode_scope_id(enc: &mut Recipe, scope: &ScopeId) {
    enc.str(&scope.canonical_id);
    encode_owner(enc, scope.owner);
    match scope.local_scope {
        None => enc.u8(0),
        Some(id) => {
            enc.u8(1);
            enc.u32(id);
        }
    }
    enc.bytes(&scope.binder_scope_id.structural_hash);
}

/// Encode a node.
///
/// The key is the node's exact structure written out. The arena is acyclic
/// by contract (every child id is below its parent's), so the structure is
/// a DAG and the key is finite; the walk descends only to children below
/// their parent and treats any other child as absent, so it terminates on
/// any arena. A subtree whose written-out form is longer than
/// [`SHARED_SUBTREE_MIN_BYTES`] and equal to one already written earlier in
/// the key encodes as a reference to that earlier subtree, so a key is
/// linear in the graph it describes however often a subtree is shared.
///
/// Sharing is a function of the structure alone, so equal structures get
/// equal keys and distinct structures distinct ones. A key with no repeated
/// subtree over that length writes every subtree in full. The encoder works
/// in two passes over heap stacks and never recurses natively: it first
/// reduces the reachable structure to a table of distinct subtrees
/// (classes), classifying each node once, then writes the root's class.
pub fn stable_key_for_node(graph: &SemanticGraphStore, id: SemanticNodeId) -> StableKey {
    let mut table = ClassTable::new(graph);
    let root = table.classify(id);
    StableKey::from_exact(table.write(root))
}

// Test-only: classification frames opened on this thread.
#[cfg(test)]
thread_local! {
    pub(crate) static CLASSIFICATION_FRAMES: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

/// Shortest written-out subtree a key refers back to rather than repeats.
/// A reference costs seven bytes; shorter subtrees are written each time.
pub const SHARED_SUBTREE_MIN_BYTES: u64 = 256;

type ClassId = u32;

/// One piece of a class's written-out form.
#[derive(Clone, Copy)]
enum Part {
    /// Literal bytes, a range of the class's `lit`.
    Lit(u32, u32),
    /// A child subtree, written as its length and its bytes.
    Child(ClassId),
}

/// One distinct subtree: its literal bytes and child classes, in order.
struct Class {
    lit: Vec<u8>,
    parts: Vec<Part>,
    /// Length of the subtree written out in full, every occurrence inline
    /// (saturating).
    full_len: u64,
}

/// A child a frame has classified, per hole.
enum Filled {
    One(ClassId),
    Set(Vec<ClassId>),
}

/// One node open on the classification path.
struct OpenFrame {
    node: SemanticNodeId,
    recipe: Recipe,
    next_hole: usize,
    filled: Vec<Filled>,
    /// The set hole being filled: its next member and the classes so far.
    set: Option<(usize, Vec<ClassId>)>,
}

/// Classification state for one key. It lives for one key computation and
/// is dropped with it.
struct ClassTable<'g> {
    graph: &'g SemanticGraphStore,
    classes: Vec<Class>,
    interned: FxHashMap<Box<[u8]>, ClassId>,
    /// Each classified node's class: a node's subtree is the same at every
    /// occurrence, so it is classified once.
    classified: FxHashMap<SemanticNodeId, ClassId>,
    order: FxHashMap<(ClassId, ClassId), Ordering>,
    frames: Vec<OpenFrame>,
}

impl<'g> ClassTable<'g> {
    fn new(graph: &'g SemanticGraphStore) -> Self {
        Self {
            graph,
            classes: Vec::new(),
            interned: FxHashMap::default(),
            classified: FxHashMap::default(),
            order: FxHashMap::default(),
            frames: Vec::new(),
        }
    }

    /// Reduce the structure reachable from `root` to classes and return the
    /// root's.
    fn classify(&mut self, root: SemanticNodeId) -> ClassId {
        if let Some(class) = self.enter(root) {
            return class;
        }
        loop {
            let Some(frame) = self.frames.last_mut() else {
                unreachable!("the root frame completes with a class");
            };
            let parent = frame.node;
            let hole = frame
                .recipe
                .holes
                .get(frame.next_hole)
                .map(|(_, hole)| hole);
            let next = match hole {
                Some(Hole::Set(members)) => match &mut frame.set {
                    None => {
                        frame.set = Some((0, Vec::new()));
                        continue;
                    }
                    Some((next, _)) => match members.get(*next) {
                        Some(&member) => {
                            *next += 1;
                            member
                        }
                        None => {
                            if let Some((_, classes)) = frame.set.take() {
                                frame.filled.push(Filled::Set(classes));
                            }
                            frame.next_hole += 1;
                            continue;
                        }
                    },
                },
                Some(Hole::Child(child) | Hole::Unless { child, .. }) => {
                    let child = *child;
                    frame.next_hole += 1;
                    child
                }
                None => {
                    if let Some(class) = self.complete() {
                        return class;
                    }
                    continue;
                }
            };
            // The arena holds every child below its parent; any other child
            // is absent here, so the walk can never meet a cycle.
            let class = if next.0 < parent.0 {
                self.enter(next)
            } else {
                Some(self.absent())
            };
            if let Some(class) = class {
                self.deliver(class);
            }
        }
    }

    /// Close the innermost frame: build and remember its class, and hand it
    /// to the enclosing frame, or return it when the root closes.
    fn complete(&mut self) -> Option<ClassId> {
        let frame = self.frames.pop()?;
        let class = self.build(&frame.recipe, frame.filled);
        self.classified.insert(frame.node, class);
        if self.frames.is_empty() {
            return Some(class);
        }
        self.deliver(class);
        None
    }

    /// Hand a classified child to the enclosing frame.
    fn deliver(&mut self, class: ClassId) {
        let Some(parent) = self.frames.last_mut() else {
            return;
        };
        match &mut parent.set {
            Some((_, classes)) => classes.push(class),
            None => parent.filled.push(Filled::One(class)),
        }
    }

    /// Begin classifying `id`: an absent node or an already classified one
    /// answers at once; anything else opens a frame.
    fn enter(&mut self, id: SemanticNodeId) -> Option<ClassId> {
        if let Some(&class) = self.classified.get(&id) {
            return Some(class);
        }
        let Some(data) = self.graph.node_data(id) else {
            return Some(self.absent());
        };
        #[cfg(test)]
        CLASSIFICATION_FRAMES.with(|frames| frames.set(frames.get() + 1));
        self.frames.push(OpenFrame {
            node: id,
            recipe: encode_data(self.graph, id, &data),
            next_hole: 0,
            filled: Vec::new(),
            set: None,
        });
        None
    }

    /// The class of an absent child.
    fn absent(&mut self) -> ClassId {
        let mut enc = Recipe::new();
        enc.header(category::INTRINSIC, subtag::OPAQUE);
        enc.u8(0xff);
        let len = enc.buf.len() as u32;
        self.intern(enc.buf, vec![Part::Lit(0, len)])
    }

    /// The class of a completed frame: its recipe with every hole filled,
    /// set members in their canonical order without repeats.
    fn build(&mut self, recipe: &Recipe, filled: Vec<Filled>) -> ClassId {
        let mut lit: Vec<u8> = Vec::new();
        let mut parts: Vec<Part> = Vec::new();
        let mut written = 0;
        let one = |filled: &[Filled], hole: usize| match filled.get(hole) {
            Some(Filled::One(class)) => Some(*class),
            _ => None,
        };
        let mut kept: Vec<Filled> = Vec::with_capacity(filled.len());
        for ((at, hole), fill) in recipe.holes.iter().zip(filled) {
            push_lit(&mut lit, &mut parts, &recipe.buf[written..*at]);
            written = *at;
            match fill {
                Filled::One(class) => {
                    match hole {
                        Hole::Unless { same_as, lead, .. } => {
                            if one(&kept, *same_as) != Some(class) {
                                push_lit(&mut lit, &mut parts, lead);
                                parts.push(Part::Child(class));
                            }
                        }
                        _ => parts.push(Part::Child(class)),
                    }
                    kept.push(Filled::One(class));
                    continue;
                }
                Filled::Set(mut members) => {
                    members.sort_by(|a, b| self.compare(*a, *b));
                    members.dedup();
                    push_lit(&mut lit, &mut parts, &(members.len() as u16).to_le_bytes());
                    parts.extend(members.iter().copied().map(Part::Child));
                    kept.push(Filled::Set(members));
                }
            }
        }
        push_lit(&mut lit, &mut parts, &recipe.buf[written..]);
        self.intern(lit, parts)
    }

    fn intern(&mut self, lit: Vec<u8>, parts: Vec<Part>) -> ClassId {
        let mut repr = Vec::with_capacity(lit.len() + parts.len() * 5);
        for part in &parts {
            match *part {
                Part::Lit(start, end) => {
                    repr.push(0);
                    repr.extend_from_slice(&(end - start).to_le_bytes());
                    repr.extend_from_slice(&lit[start as usize..end as usize]);
                }
                Part::Child(class) => {
                    repr.push(1);
                    repr.extend_from_slice(&class.to_le_bytes());
                }
            }
        }
        if let Some(&class) = self.interned.get(repr.as_slice()) {
            return class;
        }
        let mut full_len = 0u64;
        for part in &parts {
            match *part {
                Part::Lit(start, end) => {
                    full_len = full_len.saturating_add(u64::from(end - start));
                }
                Part::Child(class) => {
                    let child = &self.classes[class as usize];
                    full_len = full_len
                        .saturating_add(length_prefix(child.full_len).1 as u64)
                        .saturating_add(child.full_len);
                }
            }
        }
        let class = self.classes.len() as ClassId;
        self.classes.push(Class {
            lit,
            parts,
            full_len,
        });
        self.interned.insert(repr.into_boxed_slice(), class);
        class
    }

    /// The canonical order of two classes: their written-out-in-full byte
    /// streams, compared lexicographically. Equal children at one offset
    /// are skipped whole; only the first differing path is descended.
    fn compare(&mut self, a: ClassId, b: ClassId) -> Ordering {
        if a == b {
            return Ordering::Equal;
        }
        if let Some(&order) = self.order.get(&(a, b)) {
            return order;
        }
        let mut left = FullStream::new(a);
        let mut right = FullStream::new(b);
        let order = loop {
            match (left.peek(&self.classes), right.peek(&self.classes)) {
                (Event::End, Event::End) => break Ordering::Equal,
                (Event::End, _) => break Ordering::Less,
                (_, Event::End) => break Ordering::Greater,
                (Event::Child(x), Event::Child(y)) if x == y => {
                    left.skip();
                    right.skip();
                }
                (Event::Child(x), _) => left.descend(x, &self.classes),
                (_, Event::Child(y)) => right.descend(y, &self.classes),
                (Event::Byte(x), Event::Byte(y)) => {
                    if x != y {
                        break x.cmp(&y);
                    }
                    left.advance();
                    right.advance();
                }
            }
        };
        self.order.insert((a, b), order);
        self.order.insert((b, a), order.reverse());
        order
    }

    /// Write `root`'s class: every subtree in full the first time, and a
    /// reference to that first time whenever an equal subtree longer than
    /// [`SHARED_SUBTREE_MIN_BYTES`] recurs. References number the shared
    /// subtrees in the order they finish being written.
    fn write(&self, root: ClassId) -> Vec<u8> {
        struct Writing {
            class: ClassId,
            part: usize,
            length_slot: Option<usize>,
        }
        let patch = |out: &mut Vec<u8>, slot: usize| {
            let len = (out.len() - slot - 4) as u32;
            out[slot..slot + 4].copy_from_slice(&len.to_le_bytes());
        };
        let mut out: Vec<u8> = Vec::with_capacity(64);
        let mut written: FxHashMap<ClassId, u32> = FxHashMap::default();
        let mut stack = vec![Writing {
            class: root,
            part: 0,
            length_slot: None,
        }];
        while let Some(top) = stack.last_mut() {
            let class = &self.classes[top.class as usize];
            let part = class.parts.get(top.part).copied();
            if part.is_some() {
                top.part += 1;
            }
            match part {
                Some(Part::Lit(start, end)) => {
                    out.extend_from_slice(&class.lit[start as usize..end as usize]);
                }
                Some(Part::Child(child)) => {
                    let slot = out.len();
                    out.extend_from_slice(&[0; 4]);
                    match written.get(&child) {
                        Some(&index) => {
                            let mut enc = Recipe::new();
                            enc.header(category::RECURSIVE, subtag::SHARED_SUBTREE);
                            enc.u32(index);
                            out.extend_from_slice(&enc.buf);
                            patch(&mut out, slot);
                        }
                        None => stack.push(Writing {
                            class: child,
                            part: 0,
                            length_slot: Some(slot),
                        }),
                    }
                }
                None => {
                    let Some(done) = stack.pop() else {
                        break;
                    };
                    if self.classes[done.class as usize].full_len > SHARED_SUBTREE_MIN_BYTES {
                        let index = written.len() as u32;
                        written.insert(done.class, index);
                    }
                    if let Some(slot) = done.length_slot {
                        patch(&mut out, slot);
                    }
                }
            }
        }
        out
    }
}

/// Append literal bytes, merging them into a trailing literal part.
fn push_lit(lit: &mut Vec<u8>, parts: &mut Vec<Part>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let start = lit.len() as u32;
    lit.extend_from_slice(bytes);
    let end = lit.len() as u32;
    match parts.last_mut() {
        Some(Part::Lit(_, last_end)) if *last_end == start => *last_end = end,
        _ => parts.push(Part::Lit(start, end)),
    }
}

/// The length prefix of a subtree written out in full: four little-endian
/// bytes, or, from `u32::MAX` up, four `0xFF` bytes and eight
/// little-endian bytes, so every length has one prefix.
fn length_prefix(full_len: u64) -> ([u8; 12], usize) {
    let mut prefix = [0u8; 12];
    match u32::try_from(full_len) {
        Ok(short) if short != u32::MAX => {
            prefix[..4].copy_from_slice(&short.to_le_bytes());
            (prefix, 4)
        }
        _ => {
            prefix[..4].copy_from_slice(&[0xFF; 4]);
            prefix[4..].copy_from_slice(&full_len.to_le_bytes());
            (prefix, 12)
        }
    }
}

/// What a [`FullStream`] shows next.
enum Event {
    Byte(u8),
    /// A child subtree begins (before its length prefix).
    Child(ClassId),
    End,
}

/// A class's written-out-in-full byte stream, expanded on demand.
struct FullStream {
    /// Open classes with their next part and the offset within it.
    stack: Vec<(ClassId, usize, u32)>,
    /// A pending length prefix: its bytes, its length and how much of it is
    /// consumed.
    prefix: ([u8; 12], usize, usize),
}

impl FullStream {
    fn new(class: ClassId) -> Self {
        Self {
            stack: vec![(class, 0, 0)],
            prefix: ([0; 12], 0, 0),
        }
    }

    fn peek(&mut self, classes: &[Class]) -> Event {
        let (bytes, len, at) = self.prefix;
        if at < len {
            return Event::Byte(bytes[at]);
        }
        loop {
            let Some(top) = self.stack.last_mut() else {
                return Event::End;
            };
            let class = &classes[top.0 as usize];
            match class.parts.get(top.1).copied() {
                None => {
                    self.stack.pop();
                }
                Some(Part::Lit(start, end)) => {
                    if start + top.2 < end {
                        return Event::Byte(class.lit[(start + top.2) as usize]);
                    }
                    top.1 += 1;
                    top.2 = 0;
                }
                Some(Part::Child(child)) => return Event::Child(child),
            }
        }
    }

    fn advance(&mut self) {
        let (_, len, at) = &mut self.prefix;
        if *at < *len {
            *at += 1;
        } else if let Some(top) = self.stack.last_mut() {
            top.2 += 1;
        }
    }

    /// Step over the child at the cursor.
    fn skip(&mut self) {
        if let Some(top) = self.stack.last_mut() {
            top.1 += 1;
        }
    }

    /// Step into the child at the cursor: its length prefix, then its bytes.
    fn descend(&mut self, child: ClassId, classes: &[Class]) {
        self.skip();
        let (prefix, len) = length_prefix(classes[child as usize].full_len);
        self.prefix = (prefix, len, 0);
        self.stack.push((child, 0, 0));
    }
}

/// The node's encoding recipe: its own bytes with a hole at every child.
/// Reads the node's own payload and, for an import carrier, its own
/// interning scope — never another node.
fn encode_data(graph: &SemanticGraphStore, id: SemanticNodeId, data: &SemanticNodeData) -> Recipe {
    let mut enc = Recipe::new();
    match data {
        SemanticNodeData::Primitive(kind) => {
            enc.header(category::INTRINSIC, primitive_subtag(*kind));
        }
        SemanticNodeData::Literal(value) => match value {
            LiteralValue::String(s) => {
                enc.header(category::LITERAL, subtag::LIT_STRING);
                enc.str(s);
            }
            LiteralValue::Number(n) => {
                enc.header(category::LITERAL, subtag::LIT_NUMBER);
                // Canonical numeric identity: SameValueZero, then bit pattern
                // for NaN payloads. -0 and +0 share one key.
                let bits = if *n == 0.0 { 0 } else { n.to_bits() };
                enc.u64(bits);
            }
            LiteralValue::Boolean(b) => {
                enc.header(category::LITERAL, subtag::LIT_BOOLEAN);
                enc.bool(*b);
            }
            LiteralValue::BigInt(s) => {
                enc.header(category::LITERAL, subtag::LIT_BIGINT);
                enc.str(s);
            }
        },
        SemanticNodeData::Opaque(err) => {
            enc.header(category::INTRINSIC, subtag::OPAQUE);
            encode_query_error(&mut enc, err);
            // A recursive back-edge names the instantiation it stands for.
            if let QueryError::RecursiveRef { args, .. } = err {
                enc.u16(args.len() as u16);
                for arg in args.iter() {
                    enc.child(*arg);
                }
            }
        }
        SemanticNodeData::IntrinsicApplication { op, args } => {
            enc.header(category::SYNTHETIC, subtag::INTRINSIC_APP);
            // The frozen, append-only op tag: never a derived discriminant or a
            // spelling.
            enc.u8(op.stable_hash_tag());
            enc.u16(args.len() as u16);
            for arg in args.iter() {
                enc.child(*arg);
            }
        }
        SemanticNodeData::Alias(inner) => {
            enc.header(category::AUTHORED, subtag::ALIAS);
            enc.child(*inner);
        }
        SemanticNodeData::Union(members) => {
            enc.header(category::SYNTHETIC, subtag::UNION);
            enc.u8(origin_tag(members.origin_category()));
            enc.child_set(members.iter().copied().collect());
        }
        SemanticNodeData::Intersection(members) => {
            enc.header(category::SYNTHETIC, subtag::INTERSECTION);
            enc.u8(origin_tag(members.origin_category()));
            enc.u16(members.len() as u16);
            for id in members.iter() {
                enc.child(*id);
            }
        }
        SemanticNodeData::Array { element, readonly } => {
            enc.header(category::SYNTHETIC, subtag::ARRAY);
            enc.bool(*readonly);
            enc.child(*element);
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            enc.header(category::SYNTHETIC, subtag::TUPLE);
            enc.bool(*readonly);
            enc.u16(elements.len() as u16);
            for el in elements.iter() {
                match &el.label {
                    None => enc.u8(0),
                    Some(label) => {
                        enc.u8(1);
                        enc.str(label);
                    }
                }
                enc.bool(el.optional);
                enc.bool(el.rest);
                enc.child(el.value);
            }
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => {
            enc.header(category::SYNTHETIC, subtag::TEMPLATE);
            enc.u16(quasis.len() as u16);
            for q in quasis.iter() {
                enc.str(q);
            }
            enc.u16(expressions.len() as u16);
            for expr in expressions.iter() {
                enc.child(*expr);
            }
        }
        SemanticNodeData::KeyOf { base } => {
            enc.header(category::SYNTHETIC, subtag::KEYOF);
            enc.child(*base);
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            enc.header(category::SYNTHETIC, subtag::INDEXED_ACCESS);
            enc.child(*object);
            encode_property_key(&mut enc, index);
        }
        SemanticNodeData::Mapped { source, mapper } => {
            enc.header(category::SYNTHETIC, subtag::MAPPED);
            enc.child(*source);
            enc.child(mapper.parameter_node);
            enc.child(mapper.key_space);
            enc.child(mapper.value_expr);
            enc.u8(match mapper.optionality {
                OptionalityMod::Add => 1,
                OptionalityMod::Remove => 2,
                OptionalityMod::Keep => 3,
            });
            enc.u8(match mapper.readonly {
                ReadonlyMod::Add => 1,
                ReadonlyMod::Remove => 2,
                ReadonlyMod::Keep => 3,
            });
            enc.u8(match mapper.kind {
                MapperKind::Identity => 1,
                MapperKind::Computed => 2,
            });
            enc.bool(mapper.over_type_variable);
            match mapper.name_remap {
                None => enc.u8(0),
                Some(remap) => {
                    enc.u8(1);
                    enc.child(remap);
                }
            }
        }
        SemanticNodeData::TypeParam {
            decl,
            param_index,
            constraint,
            default,
            display_name: _,
        } => {
            enc.header(category::BINDER, subtag::TYPE_PARAM);
            enc.str(&decl.canonical_id);
            encode_owner(&mut enc, decl.owner);
            enc.str(&decl.decl_name);
            // A mapped binder's declaration name is the identity of the
            // mapping that binds it; its index is an interning ordinal handed
            // out in discovery order, so it never enters the key.
            if !crate::mapper_binder_registry::is_mapper_binder_decl_name(&decl.decl_name) {
                enc.u16(*param_index);
            }
            match constraint {
                None => enc.u8(0),
                Some(c) => {
                    enc.u8(1);
                    enc.child(*c);
                }
            }
            match default {
                None => enc.u8(0),
                Some(d) => {
                    enc.u8(1);
                    enc.child(*d);
                }
            }
        }
        SemanticNodeData::Infer { name, binder } => {
            enc.header(category::BINDER, subtag::INFER);
            enc.str(name);
            enc.bytes(&binder.stable_fingerprint_bytes());
        }
        SemanticNodeData::InferRef { name, binder } => {
            enc.header(category::BINDER, subtag::INFER_REF);
            enc.str(name);
            enc.bytes(&binder.stable_fingerprint_bytes());
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive,
            pending,
        } => {
            enc.header(category::SYNTHETIC, subtag::CONDITIONAL);
            enc.bool(*distributive);
            enc.child(*check);
            enc.child(*extends);
            enc.child(*true_branch_ref);
            enc.child(*false_branch_ref);
            // The pending substitution is part of the shell's identity: a
            // different parameter binder is a different binding, never a
            // duplicate. Binders and arguments both descend as children.
            match pending {
                None => enc.u8(0),
                Some(pending) => {
                    enc.u8(1);
                    for (tag, frame) in
                        [(1u8, pending.true_branch()), (2u8, pending.false_branch())]
                    {
                        enc.u8(tag);
                        enc.u16(frame.pairs().len() as u16);
                        for (parameter, argument) in frame.pairs().iter() {
                            enc.child(*parameter);
                            enc.child(*argument);
                        }
                    }
                }
            }
        }
        SemanticNodeData::DeclRef { identity } => {
            enc.header(category::AUTHORED, subtag::DECL_REF);
            enc.str(&identity.canonical_id);
            encode_owner(&mut enc, identity.owner);
            enc.str(&identity.decl_name);
        }
        // An enum member's literal is identified by its enum's declaration
        // and its name; its base value descends as a child.
        SemanticNodeData::EnumLiteral(literal) => {
            enc.header(category::AUTHORED, subtag::ENUM_LITERAL);
            enc.str(&literal.enum_decl.canonical_id);
            encode_owner(&mut enc, literal.enum_decl.owner);
            enc.str(&literal.enum_decl.decl_name);
            enc.str(&literal.member);
            enc.u64(u64::from(literal.member_count));
            enc.child(literal.base);
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            enc.header(category::AUTHORED, subtag::INSTANTIATION_REF);
            enc.str(&base.canonical_id);
            encode_owner(&mut enc, base.owner);
            enc.str(&base.decl_name);
            enc.u16(args.len() as u16);
            for arg in args.iter() {
                enc.child(*arg);
            }
        }
        // The class identity is the authored position (file, owner, offset);
        // the printed name and the enclosing clauses ride along, and the
        // reference's type arguments and the instance surface descend as
        // children, so two instantiations of one class expression stay
        // distinct.
        SemanticNodeData::ClassExpressionInstance {
            identity,
            type_arguments,
            surface,
        } => {
            enc.header(category::AUTHORED, subtag::CLASS_EXPRESSION_INSTANCE);
            enc.str(&identity.canonical_id);
            encode_owner(&mut enc, identity.owner);
            enc.u32(identity.offset);
            enc.str(&identity.name);
            enc.u16(identity.outer_clauses.len() as u16);
            for clause in identity.outer_clauses.iter() {
                enc.str(&clause.container);
                enc.u16(clause.parameters.len() as u16);
                for parameter in clause.parameters.iter() {
                    enc.str(parameter);
                }
            }
            enc.u32(identity.own_arity);
            enc.u8(u8::from(identity.object_literal));
            enc.u16(type_arguments.len() as u16);
            for argument in type_arguments.iter() {
                enc.child(*argument);
            }
            enc.child(*surface);
        }
        SemanticNodeData::MergedDecl { contributors } => {
            enc.header(category::AUTHORED, subtag::MERGED_DECL);
            enc.u16(contributors.len() as u16);
            for c in contributors.iter() {
                enc.child(*c);
            }
        }
        SemanticNodeData::Signature {
            kind,
            params,
            return_type,
            type_parameters,
            occurrence,
            return_carrier,
            signature_span: _,
            return_type_span: _,
            predicate,
            is_abstract,
        } => {
            enc.header(category::AUTHORED, subtag::SIGNATURE);
            enc.u8(match kind {
                SignatureKind::Call => 1,
                SignatureKind::Construct if *is_abstract => 3,
                SignatureKind::Construct => 2,
            });
            encode_params(&mut enc, params);
            let return_hole = enc.holes.len();
            enc.child(*return_type);
            encode_type_parameters(&mut enc, type_parameters);
            match occurrence {
                None => enc.u8(0),
                Some(occ) => {
                    enc.u8(1);
                    encode_signature_occurrence(&mut enc, occ);
                }
            }
            // The predicate is a trailing section, present only on a
            // predicate signature: every predicate-less signature keeps its
            // key bytes, and a predicate key extends that prefix, so the two
            // never collide.
            if let Some(predicate) = predicate {
                match predicate.subject {
                    PredicateSubject::This => enc.u8(1),
                    PredicateSubject::Parameter(index) => {
                        enc.u8(2);
                        enc.u32(index);
                    }
                }
                enc.bool(predicate.asserts);
                match predicate.ty {
                    None => enc.u8(0),
                    Some(ty) => {
                        enc.u8(1);
                        enc.child(ty);
                    }
                }
            }
            // The return carrier is a trailing section too, present only
            // when it is not structurally the declared return type: the
            // common signature keeps its key bytes, whichever node its
            // carrier names, and the section's leading tag (never a
            // predicate subject tag) keeps the two sections apart.
            match return_carrier {
                SignatureReturnCarrier::Declared(node) => {
                    enc.child_unless_same(*node, return_hole, &[3, 1]);
                }
                SignatureReturnCarrier::Function(_) => {
                    enc.u8(3);
                    encode_return_carrier(&mut enc, return_carrier);
                }
            }
        }
        SemanticNodeData::Object(surface) => {
            enc.header(category::AUTHORED, subtag::OBJECT);
            enc.u16(surface.entries.len() as u16);
            for entry in surface.entries.iter() {
                encode_surface_entry(&mut enc, entry);
            }
            enc.bool(surface.has_known_index_signature());
            match surface.keyspace {
                None => enc.u8(0),
                Some(ks) => {
                    enc.u8(1);
                    enc.child(ks);
                }
            }
            // The derived positive-members index participates in arena
            // identity and CAN diverge from `entries` (`call_shape_transform`
            // rebuilds it via `with_positive_members`), so the key must
            // discriminate every member field `Eq` carries. Spans and
            // `declaration_origin` stay deliberately excluded:
            // source-location-only differences collapse under `T | T = T`.
            let members = surface.positive_members();
            enc.u16(members.len() as u16);
            for member in members.iter() {
                encode_surface_member(&mut enc, member);
            }
        }
        SemanticNodeData::ObjectSpreadProgram(program) => {
            enc.header(category::SYNTHETIC, subtag::OBJECT_SPREAD);
            let children: Vec<SemanticNodeId> = program.child_nodes().collect();
            enc.u16(children.len() as u16);
            for child in children {
                enc.child(child);
            }
        }
        SemanticNodeData::TypeOf(_) => {
            enc.header(category::AUTHORED, subtag::TYPEOF);
            if let Some((root, path)) = data.typeof_head() {
                encode_scope_id(&mut enc, &root.scope);
                enc.str(&root.name);
                enc.u16(path.len() as u16);
                for seg in path.iter() {
                    enc.str(seg);
                }
            }
            let args = data.carrier_type_args();
            enc.u16(args.len() as u16);
            for arg in args {
                enc.child(*arg);
            }
        }
        SemanticNodeData::TypeOfNominal(_) => {
            enc.header(category::AUTHORED, subtag::TYPEOF_NOMINAL);
            if let Some(identity) = data.typeof_nominal_identity() {
                encode_value_decl(&mut enc, identity);
            }
            if let Some((root, path)) = data.typeof_head() {
                encode_scope_id(&mut enc, &root.scope);
                enc.str(&root.name);
                enc.u16(path.len() as u16);
                for seg in path.iter() {
                    enc.str(seg);
                }
            }
        }
        SemanticNodeData::BareRef(_) => {
            enc.header(category::AUTHORED, subtag::BARE_REF);
            if let Some((name, scope)) = data.bare_ref_head() {
                enc.str(name);
                encode_scope(&mut enc, scope);
            }
            let args = data.carrier_type_args();
            enc.u16(args.len() as u16);
            for arg in args {
                enc.child(*arg);
            }
        }
        SemanticNodeData::ImportType(_) => {
            enc.header(category::AUTHORED, subtag::IMPORT_TYPE);
            // The specifier resolves against the logical source unit the
            // carrier is scoped to: the importing unit and the specifier are
            // the whole resolver input, so one spelling from two importers
            // stays two identities.
            match graph
                .node_scope(id)
                .as_ref()
                .and_then(NodeScopeId::canonical_file)
            {
                None => enc.u8(0),
                Some(importer) => {
                    enc.u8(1);
                    enc.str(&importer);
                }
            }
            if let Some((spec, qual, typeof_query)) = data.import_type_head() {
                enc.str(spec);
                enc.u16(qual.len() as u16);
                for q in qual.iter() {
                    enc.str(q);
                }
                enc.bool(typeof_query);
            }
            let args = data.carrier_type_args();
            enc.u16(args.len() as u16);
            for arg in args {
                enc.child(*arg);
            }
        }
        // The raw text is the payload's whole equality identity; its
        // provenance is diagnostic, so two payloads that intern as one node
        // share one key.
        SemanticNodeData::RawFallback { value } => {
            enc.header(category::SYNTHETIC, subtag::RAW_FALLBACK);
            enc.str(value.raw());
        }
        // The synthesizing owner (the component scope), the role (the
        // binding surface), the binder position (slot and bound name), and
        // the bound value as a child key, never its arena ordinal.
        SemanticNodeData::SyntheticBinding { id, value_node } => {
            enc.header(category::BINDER, subtag::SYNTHETIC_BINDING);
            enc.str(&id.scope_canonical_id);
            enc.u8(match id.surface_kind {
                SyntheticCarrierSurfaceKind::SlotBinding => 1,
                SyntheticCarrierSurfaceKind::Binding => 2,
            });
            match &id.slot_name {
                None => enc.u8(0),
                Some(slot) => {
                    enc.u8(1);
                    enc.str(slot);
                }
            }
            enc.str(&id.binding_name);
            enc.child(SemanticNodeId(*value_node));
        }
        // The closed carrier recipe: the bucket, the positional parameter
        // model, the binder declarations, the served position the callable
        // was composed at, and where its deferred return comes from.
        SemanticNodeData::DeferredCallable(callable) => {
            enc.header(category::SYNTHETIC, subtag::DEFERRED_CALLABLE);
            let parts = callable.stable_identity_parts();
            enc.u8(match parts.kind {
                SignatureKind::Call => 1,
                SignatureKind::Construct => 2,
            });
            encode_params(&mut enc, parts.params);
            encode_type_parameters(&mut enc, parts.type_parameters);
            encode_signature_occurrence(&mut enc, parts.occurrence);
            encode_return_carrier(&mut enc, parts.return_carrier);
        }
    }
    enc
}

/// A signature's positional parameters, in order: each one's name, its
/// optionality and literal-declared fact in one byte, its rest flag and its
/// type.
fn encode_params(enc: &mut Recipe, params: &[FunctionParam]) {
    enc.u16(params.len() as u16);
    for p in params {
        match &p.name {
            None => enc.u8(0),
            Some(n) => {
                enc.u8(1);
                enc.str(n);
            }
        }
        // One byte for optionality and the literal-declared fact: a
        // parameter that is not literal-declared encodes exactly as its
        // optionality alone.
        enc.u8(u8::from(p.optional) | (u8::from(p.declared_literal) << 1));
        enc.bool(p.rest);
        enc.child(p.ty);
    }
}

/// A signature's own binder declarations, in order: each one's name and
/// binder, then its constraint and default (their presence and their keys)
/// and its `const` modifier. A declaration's bounds live here, not on its
/// binder node, which a signature lowering interns bound-free.
fn encode_type_parameters(enc: &mut Recipe, type_parameters: &[TypeParamDecl]) {
    enc.u16(type_parameters.len() as u16);
    for tp in type_parameters {
        enc.str(&tp.name);
        enc.child(tp.param);
        for bound in [tp.constraint, tp.default] {
            match bound {
                None => enc.u8(0),
                Some(bound) => {
                    enc.u8(1);
                    enc.child(bound);
                }
            }
        }
        enc.bool(tp.is_const);
    }
}

fn encode_symbol_space(enc: &mut Recipe, space: LocatorSymbolSpace) {
    enc.u8(match space {
        LocatorSymbolSpace::Type => 1,
        LocatorSymbolSpace::Value => 2,
        LocatorSymbolSpace::Namespace => 3,
    });
}

/// An authored declaration anchor: its logical source unit, lexical owner,
/// merged symbol name and symbol space.
fn encode_authored_anchor(enc: &mut Recipe, anchor: &AuthoredAnchor) {
    enc.str(&anchor.canonical_id);
    encode_owner(enc, anchor.owner);
    enc.str(&anchor.symbol);
    encode_symbol_space(enc, anchor.space);
}

/// A value declaration's identity: its logical source unit, lexical owner,
/// symbol name and member path.
fn encode_value_decl(enc: &mut Recipe, identity: &ValueDeclIdentityPart) {
    enc.str(&identity.canonical_id);
    encode_owner(enc, identity.owner);
    enc.str(&identity.symbol);
    enc.u16(identity.member_path.len() as u16);
    for segment in identity.member_path.iter() {
        enc.str(segment);
    }
}

/// A served function position: its declaration anchor, the authored part
/// of the declaration it occupies and its overload ordinal.
fn encode_function_position(enc: &mut Recipe, function: &FlowFunctionReturnIdentity) {
    encode_authored_anchor(enc, &function.anchor);
    match &function.function_part {
        FunctionPartIdentity::DeclarationBody => enc.u8(1),
        FunctionPartIdentity::Member { member_path } => {
            enc.u8(2);
            enc.u16(member_path.len() as u16);
            for ordinal in member_path.iter() {
                enc.u32(*ordinal);
            }
        }
        FunctionPartIdentity::Initializer => enc.u8(3),
        FunctionPartIdentity::Other { ordinal } => {
            enc.u8(4);
            enc.u32(*ordinal);
        }
    }
    enc.u32(function.overload_ordinal);
}

/// The authored occurrence of a signature: its served function position
/// and its ordinal in that position's call or construct bucket.
fn encode_signature_occurrence(enc: &mut Recipe, occurrence: &SignatureNodeOccurrence) {
    encode_function_position(enc, &occurrence.function);
    enc.u32(occurrence.signature_ordinal);
}

/// One step of a declaration-body locator path.
fn encode_body_step(enc: &mut Recipe, step: TypeBodyPathStep) {
    let (tag, ordinal) = match step {
        TypeBodyPathStep::MergedContributor { ordinal } => (1, Some(ordinal)),
        TypeBodyPathStep::IntersectionArm { ordinal } => (2, Some(ordinal)),
        TypeBodyPathStep::TypeArgument { ordinal } => (3, Some(ordinal)),
        TypeBodyPathStep::Member { ordinal } => (4, Some(ordinal)),
        TypeBodyPathStep::MemberKey => (5, None),
        TypeBodyPathStep::MemberValue => (6, None),
        TypeBodyPathStep::TypeParamBound { ordinal, position } => {
            enc.u8(7);
            enc.u32(ordinal);
            enc.u8(match position {
                TypeParamBoundPosition::Constraint => 1,
                TypeParamBoundPosition::Default => 2,
            });
            return;
        }
        TypeBodyPathStep::FunctionParam { ordinal } => (8, Some(ordinal)),
        TypeBodyPathStep::FunctionReturn => (9, None),
        TypeBodyPathStep::ValueSignature { ordinal } => (10, Some(ordinal)),
        TypeBodyPathStep::MappedSource => (11, None),
        TypeBodyPathStep::MappedValue => (12, None),
        TypeBodyPathStep::MappedNameType => (13, None),
        TypeBodyPathStep::ConditionalCheck => (14, None),
        TypeBodyPathStep::ConditionalExtends => (15, None),
        TypeBodyPathStep::ConditionalTrue => (16, None),
        TypeBodyPathStep::ConditionalFalse => (17, None),
        TypeBodyPathStep::UnionArm { ordinal } => (18, Some(ordinal)),
        TypeBodyPathStep::IndexedAccessObject => (19, None),
        TypeBodyPathStep::IndexedAccessIndex => (20, None),
        TypeBodyPathStep::IndexSignatureKey => (21, None),
        TypeBodyPathStep::IndexSignatureValue => (22, None),
        TypeBodyPathStep::TupleElement { ordinal } => (23, Some(ordinal)),
    };
    enc.u8(tag);
    if let Some(ordinal) = ordinal {
        enc.u32(ordinal);
    }
}

/// Where a deferred callable's return comes from: a declared node, a
/// declared annotation's locator, a body-derived position, or nothing.
fn encode_return_carrier(enc: &mut Recipe, carrier: &SignatureReturnCarrier) {
    match carrier {
        SignatureReturnCarrier::Declared(node) => {
            enc.u8(1);
            enc.child(*node);
        }
        SignatureReturnCarrier::Function(source) => {
            enc.u8(2);
            match source {
                FunctionReturnSource::Declared(locator) => {
                    enc.u8(1);
                    let slot = match locator {
                        FunctionReturnLocator::Authored(slot) => {
                            enc.u8(1);
                            slot
                        }
                        FunctionReturnLocator::Jsdoc(slot) => {
                            enc.u8(2);
                            slot
                        }
                    };
                    encode_authored_anchor(enc, &slot.anchor);
                    enc.u16(slot.path.len() as u16);
                    for step in slot.path.iter() {
                        encode_body_step(enc, *step);
                    }
                }
                FunctionReturnSource::Flow(function) => {
                    enc.u8(2);
                    encode_function_position(enc, function);
                }
                FunctionReturnSource::Absent => enc.u8(3),
            }
        }
    }
}

fn encode_surface_entry(enc: &mut Recipe, entry: &SurfaceEntry) {
    match entry {
        SurfaceEntry::Member(member) => {
            enc.u8(1);
            encode_property_key(enc, &member.key);
            enc.bool(member.optional);
            enc.bool(member.readonly);
            enc.u8(match member.visibility {
                verter_type_expr::MemberVisibility::Public => 1,
                verter_type_expr::MemberVisibility::Protected => 2,
                verter_type_expr::MemberVisibility::Private => 3,
            });
            enc.child(member.value);
        }
        SurfaceEntry::CallSignature(id) => {
            enc.u8(2);
            enc.child(*id);
        }
        SurfaceEntry::ConstructSignature(id) => {
            enc.u8(3);
            enc.child(*id);
        }
        SurfaceEntry::IndexSignature(sig) => {
            enc.u8(4);
            enc.bool(sig.readonly);
            enc.child(sig.key_type);
            enc.child(sig.value_type);
        }
    }
}

/// Encode one derived positive member — every `SurfaceMember` field the
/// arena's `Eq` carries except the deliberately excluded `spans` and
/// `declaration_origin` (source-location-only differences collapse under
/// `T | T = T`).
fn encode_surface_member(enc: &mut Recipe, member: &SurfaceMember) {
    encode_property_key(enc, &member.key);
    enc.bool(member.optional);
    enc.bool(member.readonly);
    match member.method_kind {
        None => enc.u8(0),
        Some(verter_type_expr::ObjectMethodKind::Method) => enc.u8(1),
        Some(verter_type_expr::ObjectMethodKind::Get) => enc.u8(2),
        Some(verter_type_expr::ObjectMethodKind::Set) => enc.u8(3),
    }
    enc.bool(member.has_implementation_body);
    match member.visibility {
        verter_type_expr::MemberVisibility::Public => enc.u8(1),
        verter_type_expr::MemberVisibility::Protected => enc.u8(2),
        verter_type_expr::MemberVisibility::Private => enc.u8(3),
    }
    match member.merge_role.role() {
        crate::semantic_query::MemberMergeRole::Authored => enc.u8(1),
        crate::semantic_query::MemberMergeRole::OwnBody => enc.u8(2),
        crate::semantic_query::MemberMergeRole::Heritage => enc.u8(3),
    }
    enc.bool(member.declared_in_macro_type_arg.get());
    match member.excess_origin {
        verter_type_expr::ExcessPropertyOrigin::FreshOwn => enc.u8(1),
        verter_type_expr::ExcessPropertyOrigin::SpreadTainted => enc.u8(2),
        verter_type_expr::ExcessPropertyOrigin::NonLiteral => enc.u8(3),
    }
    enc.child(member.value);
}

fn encode_property_key(enc: &mut Recipe, key: &AuthoredPropertyKey) {
    match key {
        AuthoredPropertyKey::String(s) => {
            enc.u8(1);
            enc.str(s);
        }
        AuthoredPropertyKey::Number(n) => {
            enc.u8(2);
            enc.u64(n.get() as u64);
        }
        AuthoredPropertyKey::UniqueSymbol(id) => {
            enc.u8(3);
            encode_value_decl(enc, id);
        }
        AuthoredPropertyKey::Computed(node) => {
            enc.u8(4);
            enc.child(*node);
        }
    }
}

fn encode_query_error(enc: &mut Recipe, err: &QueryError) {
    let tag: u8 = match err {
        QueryError::Miss => 1,
        QueryError::UnsupportedIntrinsic { .. } => 2,
        QueryError::BudgetExceeded(_) => 3,
        QueryError::Cancelled => 4,
        QueryError::UnstableState { .. } => 5,
        QueryError::SignatureOverflow => 6,
        QueryError::ForeignSemanticOperand => 7,
        QueryError::StaleSemanticOperand => 8,
        QueryError::IncompleteSemanticOperand { .. } => 9,
        QueryError::AliasCycle { .. } => 10,
        QueryError::RecursiveRef { .. } => 11,
        QueryError::Other(_) => 12,
        QueryError::DeclPlaceholder { .. } => 13,
        QueryError::ValueDomainMismatch { .. } => 14,
        QueryError::RaiseAliasCycle => 15,
        QueryError::TypeParamCycle => 16,
        QueryError::RaiseMiss => 17,
        QueryError::UnrepresentableSurface => 18,
        QueryError::UnrepresentableSurfaceMember => 19,
        QueryError::OpenSurface => 20,
        QueryError::UnmodeledPosition => 21,
        QueryError::CheckerRecovery(_) => 22,
    };
    enc.u8(tag);
    match err {
        QueryError::CheckerRecovery(diagnostic) => {
            enc.u16(u16::try_from(diagnostic.code.code()).unwrap_or(u16::MAX));
            enc.u8(match diagnostic.operation {
                crate::semantic_query::CheckerDiagnosticOperation::LibAwaited => 1,
                crate::semantic_query::CheckerDiagnosticOperation::AwaitOperand => 2,
                crate::semantic_query::CheckerDiagnosticOperation::AsyncReturnPayload => 3,
                crate::semantic_query::CheckerDiagnosticOperation::CallResolution => 4,
            });
        }
        QueryError::UnsupportedIntrinsic { name } => enc.str(name),
        QueryError::RecursiveRef { name, .. } => enc.str(name),
        QueryError::Other(s) => enc.str(s),
        QueryError::DeclPlaceholder {
            canonical_id,
            name,
            owner,
            whole_hash: _,
        } => {
            enc.str(canonical_id);
            encode_owner(enc, *owner);
            enc.str(name);
        }
        QueryError::AliasCycle { chain } => {
            enc.u16(chain.len() as u16);
            for c in chain.iter() {
                enc.str(c);
            }
        }
        _ => {}
    }
}

/// Sort `members` by `VerterStableV1`. Equal keys stay in input order
/// only when they are indistinguishable; distinguishable members never
/// compare equal.
pub fn sort_by_stable_key(graph: &SemanticGraphStore, members: &mut [SemanticNodeId]) {
    members.sort_by_cached_key(|id| stable_key_for_node(graph, *id));
}

/// Stable-key equality, which may license a collapse: every key encodes
/// its whole structure, so equal keys name one encoded structure.
pub fn provably_equal(graph: &SemanticGraphStore, a: SemanticNodeId, b: SemanticNodeId) -> bool {
    let key_a = stable_key_for_node(graph, a);
    let key_b = stable_key_for_node(graph, b);
    key_a == key_b
}

/// Union-set canonicalization: sort by stable key and drop repeated node ids.
pub fn canonicalize_union_members(
    graph: &SemanticGraphStore,
    members: &[SemanticNodeId],
) -> Arc<[SemanticNodeId]> {
    let mut keyed: Vec<(StableKey, SemanticNodeId)> = members
        .iter()
        .map(|&id| (stable_key_for_node(graph, id), id))
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    if graph.union_order_reversed() {
        keyed.reverse();
    }
    // Admission-time convergence is ORDER only: structurally equal but
    // arena-distinct members stay — the build's budgeted comparator owns
    // the collapse and its discard-evidence discipline.
    keyed.dedup_by(|a, b| a.1 == b.1);
    keyed.into_iter().map(|(_, id)| id).collect()
}

/// Order a union's members by the `VerterStableV1` stable key — THE union
/// order. Every union construction sorts through here, so the one order has
/// one site (and one test-only counterfactual, a store that reverses it).
pub fn sort_union_members_by_stable_key(
    graph: &SemanticGraphStore,
    members: &mut [SemanticNodeId],
) {
    sort_by_stable_key(graph, members);
    if graph.union_order_reversed() {
        members.reverse();
    }
}

/// Order a union's members as [`sort_union_members_by_stable_key`] does and
/// drop each member whose key equals its neighbour's (the key-equality
/// collapse [`provably_equal`] licenses), keying every member once.
pub fn sort_and_collapse_union_members(
    graph: &SemanticGraphStore,
    members: &mut Vec<SemanticNodeId>,
) {
    let mut keyed: Vec<(StableKey, SemanticNodeId)> = members
        .iter()
        .map(|&id| (stable_key_for_node(graph, id), id))
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    if graph.union_order_reversed() {
        keyed.reverse();
    }
    keyed.dedup_by(|a, b| a.0 == b.0);
    members.clear();
    members.extend(keyed.into_iter().map(|(_, id)| id));
}

/// Lazy `SemanticUnionMembers` view. A resident valid view is not re-sorted.
/// Views live in the store whose arena the union's id indexes.
pub fn semantic_union_members(
    graph: &SemanticGraphStore,
    union: SemanticNodeId,
    ctx: &SemanticContext,
) -> Arc<[SemanticNodeId]> {
    let key = SemanticUnionMembersKey::from_context(union, ctx);
    if let Some(view) = graph.union_view(&key) {
        return view;
    }
    let view = build_union_view(graph, union);
    graph.keep_union_view(key, &view)
}

fn build_union_view(graph: &SemanticGraphStore, union: SemanticNodeId) -> Arc<[SemanticNodeId]> {
    match graph.node_data(union).as_deref() {
        Some(SemanticNodeData::Union(members)) => {
            canonicalize_union_members(graph, members.as_ref())
        }
        _ => Arc::from([union]),
    }
}

/// Project a view from a context id when the interned context is available.
pub fn semantic_union_members_for_id(
    graph: &SemanticGraphStore,
    union: SemanticNodeId,
    ctx: SemanticContextId,
) -> Arc<[SemanticNodeId]> {
    match ctx.lookup() {
        Some(context) => semantic_union_members(graph, union, &context),
        None => build_union_view(graph, union),
    }
}

pub fn order_policy_of(ctx: &SemanticContext) -> SemanticOrderPolicyId {
    project_union_order(ctx)
}

pub fn order_domain_of(ctx: &SemanticContext) -> OrderDomainId {
    project_order_domain(ctx)
}
