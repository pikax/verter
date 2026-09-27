//! `VerterStableV1` representation-only stable keys.
//!
//! Category/sub-tag table and encodings are versioned integers with
//! explicit little-endian layout. The published order is the pair
//! `(fingerprint, exact key)`. Comparators never force bodies, resolve
//! names, run relations, instantiate, or reduce unions.

use std::collections::HashMap;
use std::sync::Arc;

use crate::semantic_query::composite::CompositeOriginCategory;
use crate::semantic_query::{
    AuthoredPropertyKey, LiteralValue, MapperKind, NodeScopeId, NullabilityPolicy, OptionalityMod,
    PredicateSubject, PrimitiveKind, QueryError, ReadonlyMod, ScopeId, SemanticNodeData,
    SemanticNodeId, SignatureKind, SurfaceEntry, SurfaceMember,
};
use crate::semantic_query_memo::SemanticGraphStore;

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

/// Encode a node. The walk is iterative over an explicit frame stack, so a
/// finite structure of any depth gets its complete key and the encoder
/// never recurses natively. A child already open on the walk's path is a
/// true cycle: it encodes as a back-reference to that frame's level (its
/// distance from this walk's root), never a node id, so recursion
/// terminates deterministically.
pub fn stable_key_for_node(graph: &SemanticGraphStore, id: SemanticNodeId) -> StableKey {
    let mut walk = KeyWalk {
        graph,
        out: Vec::with_capacity(64),
        frames: Vec::new(),
        open: HashMap::new(),
    };
    walk.enter(id, 0);
    walk.run();
    StableKey::from_exact(walk.out)
}

/// One node open on the walk's path.
struct Frame {
    node: SemanticNodeId,
    recipe: Recipe,
    /// Bytes of `recipe.buf` already written to the output.
    written: usize,
    /// The next hole to fill.
    next_hole: usize,
    /// Offset of this node's first byte in the output.
    start: usize,
    /// The set hole being filled, while one is open.
    set: Option<SetFill>,
}

/// An unordered child collection whose member keys are being collected.
struct SetFill {
    members: Vec<SemanticNodeId>,
    next: usize,
    keys: Vec<Vec<u8>>,
}

/// The iterative encoder: every open node is a heap frame, and every key is
/// written straight into one output buffer.
struct KeyWalk<'g> {
    graph: &'g SemanticGraphStore,
    out: Vec<u8>,
    frames: Vec<Frame>,
    /// Each node open on the path, with its frame level.
    open: HashMap<SemanticNodeId, u32>,
}

impl KeyWalk<'_> {
    /// Begin the key of `id` at output offset `start`: a back-reference or
    /// an absent node is written at once, anything else opens a frame.
    fn enter(&mut self, id: SemanticNodeId, start: usize) {
        if let Some(&level) = self.open.get(&id) {
            let mut enc = Recipe::new();
            enc.header(category::RECURSIVE, 1);
            enc.u32(level);
            self.out.extend_from_slice(&enc.buf);
            self.deliver(start);
            return;
        }
        let Some(data) = self.graph.node_data(id) else {
            let mut enc = Recipe::new();
            enc.header(category::INTRINSIC, subtag::OPAQUE);
            enc.u8(0xff);
            self.out.extend_from_slice(&enc.buf);
            self.deliver(start);
            return;
        };
        self.open.insert(id, self.frames.len() as u32);
        self.frames.push(Frame {
            node: id,
            recipe: encode_data(&data),
            written: 0,
            next_hole: 0,
            start,
            set: None,
        });
    }

    /// Hand the finished key at `start..` to the enclosing frame: a set
    /// member is kept aside for sorting, a single child gets its length.
    fn deliver(&mut self, start: usize) {
        let Some(parent) = self.frames.last_mut() else {
            return;
        };
        match &mut parent.set {
            Some(set) => set.keys.push(self.out.split_off(start)),
            None => {
                let len = (self.out.len() - start) as u32;
                self.out[start - 4..start].copy_from_slice(&len.to_le_bytes());
            }
        }
    }

    fn run(&mut self) {
        while let Some(frame) = self.frames.last_mut() {
            if let Some(set) = &mut frame.set {
                if let Some(&member) = set.members.get(set.next) {
                    set.next += 1;
                    let start = self.out.len();
                    self.enter(member, start);
                    continue;
                }
                let mut keys = std::mem::take(&mut set.keys);
                frame.set = None;
                keys.sort();
                keys.dedup();
                self.out
                    .extend_from_slice(&(keys.len() as u16).to_le_bytes());
                for key in keys {
                    self.out
                        .extend_from_slice(&(key.len() as u32).to_le_bytes());
                    self.out.extend_from_slice(&key);
                }
                continue;
            }
            if let Some((at, hole)) = frame.recipe.holes.get_mut(frame.next_hole) {
                frame.next_hole += 1;
                self.out
                    .extend_from_slice(&frame.recipe.buf[frame.written..*at]);
                frame.written = *at;
                match hole {
                    Hole::Child(child) => {
                        let child = *child;
                        self.out.extend_from_slice(&[0; 4]);
                        let start = self.out.len();
                        self.enter(child, start);
                    }
                    Hole::Set(members) => {
                        frame.set = Some(SetFill {
                            members: std::mem::take(members),
                            next: 0,
                            keys: Vec::new(),
                        });
                    }
                }
                continue;
            }
            self.out
                .extend_from_slice(&frame.recipe.buf[frame.written..]);
            let Some(frame) = self.frames.pop() else {
                break;
            };
            self.open.remove(&frame.node);
            self.deliver(frame.start);
        }
    }
}

/// The node's encoding recipe: its own bytes with a hole at every child.
/// Reads the node's payload only — never another node.
fn encode_data(data: &SemanticNodeData) -> Recipe {
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
            enc.u16(*param_index);
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
            return_carrier: _,
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
            enc.u16(params.len() as u16);
            for p in params.iter() {
                match &p.name {
                    None => enc.u8(0),
                    Some(n) => {
                        enc.u8(1);
                        enc.str(n);
                    }
                }
                // One byte for optionality and the literal-declared fact:
                // a parameter that is not literal-declared encodes exactly
                // as its optionality alone.
                enc.u8(u8::from(p.optional) | (u8::from(p.declared_literal) << 1));
                enc.bool(p.rest);
                enc.child(p.ty);
            }
            enc.child(*return_type);
            enc.u16(type_parameters.len() as u16);
            for tp in type_parameters.iter() {
                enc.str(&tp.name);
                enc.child(tp.param);
            }
            match occurrence {
                None => enc.u8(0),
                Some(occ) => {
                    enc.u8(1);
                    enc.bytes(&format!("{occ:?}").into_bytes());
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
                enc.bytes(&format!("{identity:?}").into_bytes());
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
        SemanticNodeData::RawFallback { value } => {
            enc.header(category::SYNTHETIC, subtag::RAW_FALLBACK);
            enc.bytes(&format!("{value:?}").into_bytes());
        }
        SemanticNodeData::SyntheticBinding { id, value_node: _ } => {
            enc.header(category::BINDER, subtag::SYNTHETIC_BINDING);
            enc.bytes(&format!("{id:?}").into_bytes());
        }
        SemanticNodeData::DeferredCallable(_) => {
            enc.header(category::SYNTHETIC, subtag::DEFERRED_CALLABLE);
            for child in data.carrier_type_args() {
                enc.child(*child);
            }
        }
    }
    enc
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
            enc.bytes(&format!("{n:?}").into_bytes());
        }
        AuthoredPropertyKey::UniqueSymbol(id) => {
            enc.u8(3);
            enc.bytes(&format!("{id:?}").into_bytes());
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
