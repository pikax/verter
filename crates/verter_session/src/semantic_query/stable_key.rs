//! `VerterStableV1` representation-only stable keys.
//!
//! Category/sub-tag table and encodings are versioned integers with
//! explicit little-endian layout. The published order is the pair
//! `(fingerprint, exact key)`. Comparators never force bodies, resolve
//! names, run relations, instantiate, or reduce unions.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use crate::semantic_query::composite::CompositeOriginCategory;
use crate::semantic_query::{
    AuthoredPropertyKey, LiteralValue, MapperKind, NodeScopeId, NullabilityPolicy, OptionalityMod,
    PredicateSubject, PrimitiveKind, QueryError, ReadonlyMod, ScopeId, SemanticNodeData,
    SemanticNodeId, SignatureKind, SurfaceEntry, SurfaceMember,
};
use crate::semantic_query_memo::SemanticGraphStore;
use verter_type_expr::CompilerIntrinsicTypeOp;

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

/// Exact key plus versioned fingerprint. Order is the whole pair.
/// `complete` rides alongside: an incomplete key (its encoding tripped
/// [`MAX_ENCODE_DEPTH`]) still orders deterministically but never
/// licenses a structural collapse.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct StableKey {
    fingerprint: u64,
    exact: Vec<u8>,
    complete: bool,
}

impl StableKey {
    /// Build from exact encoding bytes. Fingerprint is FNV-1a over those bytes.
    #[must_use]
    pub fn from_exact(exact: Vec<u8>) -> Self {
        Self {
            fingerprint: fingerprint_v1(&exact),
            exact,
            complete: true,
        }
    }

    /// Test-only: inject a fingerprint collision while keeping distinct exact bytes.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn with_forced_fingerprint(exact: Vec<u8>, fingerprint: u64) -> Self {
        Self {
            fingerprint,
            exact,
            complete: true,
        }
    }

    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    #[must_use]
    pub fn exact(&self) -> &[u8] {
        &self.exact
    }

    /// Whether the encoding finished within [`MAX_ENCODE_DEPTH`]. Only a
    /// complete key proves structural identity; the depth-exhausted
    /// marker is shared by every over-deep structure and must never
    /// collapse anything.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.complete
    }
}

impl Ord for StableKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.fingerprint
            .cmp(&other.fingerprint)
            .then_with(|| self.exact.cmp(&other.exact))
    }
}

impl PartialOrd for StableKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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

struct Encoder {
    buf: Vec<u8>,
    /// Whether any embedded child key was the incomplete depth-exhausted
    /// marker: completeness must propagate to every enclosing key.
    saw_incomplete: bool,
}

impl Encoder {
    fn new() -> Self {
        let mut enc = Self {
            buf: Vec::with_capacity(32),
            saw_incomplete: false,
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

    fn finish(self) -> StableKey {
        StableKey {
            fingerprint: fingerprint_v1(&self.buf),
            complete: !self.saw_incomplete,
            exact: self.buf,
        }
    }

    /// Propagate a child key's incompleteness into this enclosing key.
    fn absorb_incomplete(&mut self, child: &StableKey) {
        self.saw_incomplete |= !child.is_complete();
    }

    /// Finish with `complete: false` — the depth-exhausted marker.
    fn finish_incomplete(self) -> StableKey {
        StableKey {
            fingerprint: fingerprint_v1(&self.buf),
            exact: self.buf,
            complete: false,
        }
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
    }
}

fn encode_owner(enc: &mut Encoder, owner: verter_type_expr::TopLevelOwnerId) {
    enc.u8(match owner.kind() {
        verter_type_expr::TopLevelOwnerKind::Module => 1,
        verter_type_expr::TopLevelOwnerKind::Instance => 2,
        verter_type_expr::TopLevelOwnerKind::Frontmatter => 3,
    });
    enc.u32(owner.ordinal());
}

fn encode_scope(enc: &mut Encoder, scope: &NodeScopeId) {
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

fn encode_scope_id(enc: &mut Encoder, scope: &ScopeId) {
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

/// Encode a node. Recursion terminates at a prior occurrence in this walk
/// (relative ordinal), never by publishing a node id. Nesting beyond
/// [`MAX_ENCODE_DEPTH`] publishes the shared depth-exhausted marker — an
/// INCOMPLETE key that never licenses a collapse.
pub fn stable_key_for_node(graph: &SemanticGraphStore, id: SemanticNodeId) -> StableKey {
    let mut seen: HashMap<SemanticNodeId, u32> = HashMap::new();
    encode_node(graph, id, &mut seen, 0)
}

/// Deepest nesting the recursive encoder may descend before it stops and
/// publishes the incomplete marker. Bounds both the encoder's stack and
/// the collapse-proving work to a constant, mirroring the canonical
/// comparator's comparison budget: a structural collapse that cannot be
/// proven within bounded work must not happen.
const MAX_ENCODE_DEPTH: u16 = 256;

fn encode_node(
    graph: &SemanticGraphStore,
    id: SemanticNodeId,
    seen: &mut HashMap<SemanticNodeId, u32>,
    depth: u16,
) -> StableKey {
    if let Some(&ordinal) = seen.get(&id) {
        let mut enc = Encoder::new();
        enc.header(category::RECURSIVE, 1);
        enc.u32(ordinal);
        return enc.finish();
    }
    if depth >= MAX_ENCODE_DEPTH {
        let mut enc = Encoder::new();
        enc.header(category::RECURSIVE, 2);
        return enc.finish_incomplete();
    }
    let ordinal = seen.len() as u32;
    seen.insert(id, ordinal);
    let Some(data) = graph.node_data(id) else {
        let mut enc = Encoder::new();
        enc.header(category::INTRINSIC, subtag::OPAQUE);
        enc.u8(0xff);
        return enc.finish();
    };
    let key = encode_data(graph, data.as_ref(), seen, depth);
    seen.remove(&id);
    key
}

fn encode_child(
    graph: &SemanticGraphStore,
    id: SemanticNodeId,
    seen: &mut HashMap<SemanticNodeId, u32>,
    enc: &mut Encoder,
    depth: u16,
) {
    let child = encode_node(graph, id, seen, depth + 1);
    enc.absorb_incomplete(&child);
    enc.bytes(&child.exact);
}

fn encode_data(
    graph: &SemanticGraphStore,
    data: &SemanticNodeData,
    seen: &mut HashMap<SemanticNodeId, u32>,
    depth: u16,
) -> StableKey {
    let mut enc = Encoder::new();
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
        }
        SemanticNodeData::IntrinsicApplication { op, args } => {
            enc.header(category::SYNTHETIC, subtag::INTRINSIC_APP);
            enc.u8(intrinsic_op_tag(*op));
            enc.u16(args.len() as u16);
            for arg in args.iter() {
                encode_child(graph, *arg, seen, &mut enc, depth);
            }
        }
        SemanticNodeData::Alias(inner) => {
            enc.header(category::AUTHORED, subtag::ALIAS);
            encode_child(graph, *inner, seen, &mut enc, depth);
        }
        SemanticNodeData::Union(members) => {
            enc.header(category::SYNTHETIC, subtag::UNION);
            enc.u8(origin_tag(members.origin_category()));
            let mut child_keys: Vec<(Vec<u8>, bool)> = members
                .iter()
                .map(|id| {
                    let key = encode_node(graph, *id, seen, depth + 1);
                    let complete = key.is_complete();
                    (key.exact, complete)
                })
                .collect();
            child_keys.sort();
            child_keys.dedup();
            enc.saw_incomplete |= child_keys.iter().any(|(_, complete)| !*complete);
            let child_keys: Vec<Vec<u8>> = child_keys.into_iter().map(|(exact, _)| exact).collect();
            enc.u16(child_keys.len() as u16);
            for key in child_keys {
                enc.bytes(&key);
            }
        }
        SemanticNodeData::Intersection(members) => {
            enc.header(category::SYNTHETIC, subtag::INTERSECTION);
            enc.u8(origin_tag(members.origin_category()));
            enc.u16(members.len() as u16);
            for id in members.iter() {
                encode_child(graph, *id, seen, &mut enc, depth);
            }
        }
        SemanticNodeData::Array { element, readonly } => {
            enc.header(category::SYNTHETIC, subtag::ARRAY);
            enc.bool(*readonly);
            encode_child(graph, *element, seen, &mut enc, depth);
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
                encode_child(graph, el.value, seen, &mut enc, depth);
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
                encode_child(graph, *expr, seen, &mut enc, depth);
            }
        }
        SemanticNodeData::KeyOf { base } => {
            enc.header(category::SYNTHETIC, subtag::KEYOF);
            encode_child(graph, *base, seen, &mut enc, depth);
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            enc.header(category::SYNTHETIC, subtag::INDEXED_ACCESS);
            encode_child(graph, *object, seen, &mut enc, depth);
            encode_property_key(graph, index, seen, &mut enc, depth);
        }
        SemanticNodeData::Mapped { source, mapper } => {
            enc.header(category::SYNTHETIC, subtag::MAPPED);
            encode_child(graph, *source, seen, &mut enc, depth);
            encode_child(graph, mapper.parameter_node, seen, &mut enc, depth);
            encode_child(graph, mapper.key_space, seen, &mut enc, depth);
            encode_child(graph, mapper.value_expr, seen, &mut enc, depth);
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
            match mapper.name_remap {
                None => enc.u8(0),
                Some(remap) => {
                    enc.u8(1);
                    encode_child(graph, remap, seen, &mut enc, depth);
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
                    encode_child(graph, *c, seen, &mut enc, depth);
                }
            }
            match default {
                None => enc.u8(0),
                Some(d) => {
                    enc.u8(1);
                    encode_child(graph, *d, seen, &mut enc, depth);
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
            encode_child(graph, *check, seen, &mut enc, depth);
            encode_child(graph, *extends, seen, &mut enc, depth);
            encode_child(graph, *true_branch_ref, seen, &mut enc, depth);
            encode_child(graph, *false_branch_ref, seen, &mut enc, depth);
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
                            encode_child(graph, *parameter, seen, &mut enc, depth);
                            encode_child(graph, *argument, seen, &mut enc, depth);
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
        SemanticNodeData::InstantiationRef { base, args } => {
            enc.header(category::AUTHORED, subtag::INSTANTIATION_REF);
            enc.str(&base.canonical_id);
            encode_owner(&mut enc, base.owner);
            enc.str(&base.decl_name);
            enc.u16(args.len() as u16);
            for arg in args.iter() {
                encode_child(graph, *arg, seen, &mut enc, depth);
            }
        }
        // The class identity is the authored position (file, owner, offset);
        // the printed name and qualifier ride along, and the instance
        // surface descends as a child, so two instantiations of one class
        // expression stay distinct.
        SemanticNodeData::ClassExpressionInstance { identity, surface } => {
            enc.header(category::AUTHORED, subtag::CLASS_EXPRESSION_INSTANCE);
            enc.str(&identity.canonical_id);
            encode_owner(&mut enc, identity.owner);
            enc.u32(identity.offset);
            enc.str(&identity.name);
            match &identity.qualifier {
                None => enc.u8(0),
                Some(qualifier) => {
                    enc.u8(1);
                    enc.str(qualifier);
                }
            }
            encode_child(graph, *surface, seen, &mut enc, depth);
        }
        SemanticNodeData::MergedDecl { contributors } => {
            enc.header(category::AUTHORED, subtag::MERGED_DECL);
            enc.u16(contributors.len() as u16);
            for c in contributors.iter() {
                encode_child(graph, *c, seen, &mut enc, depth);
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
        } => {
            enc.header(category::AUTHORED, subtag::SIGNATURE);
            enc.u8(match kind {
                SignatureKind::Call => 1,
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
                enc.bool(p.optional);
                enc.bool(p.rest);
                encode_child(graph, p.ty, seen, &mut enc, depth);
            }
            encode_child(graph, *return_type, seen, &mut enc, depth);
            enc.u16(type_parameters.len() as u16);
            for tp in type_parameters.iter() {
                enc.str(&tp.name);
                encode_child(graph, tp.param, seen, &mut enc, depth);
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
                        encode_child(graph, ty, seen, &mut enc, depth);
                    }
                }
            }
        }
        SemanticNodeData::Object(surface) => {
            enc.header(category::AUTHORED, subtag::OBJECT);
            enc.u16(surface.entries.len() as u16);
            for entry in surface.entries.iter() {
                encode_surface_entry(graph, entry, seen, &mut enc, depth);
            }
            enc.bool(surface.has_known_index_signature());
            match surface.keyspace {
                None => enc.u8(0),
                Some(ks) => {
                    enc.u8(1);
                    encode_child(graph, ks, seen, &mut enc, depth);
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
                encode_surface_member(graph, member, seen, &mut enc, depth);
            }
        }
        SemanticNodeData::ObjectSpreadProgram(program) => {
            enc.header(category::SYNTHETIC, subtag::OBJECT_SPREAD);
            let children: Vec<SemanticNodeId> = program.child_nodes().collect();
            enc.u16(children.len() as u16);
            for child in children {
                encode_child(graph, child, seen, &mut enc, depth);
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
                encode_child(graph, *arg, seen, &mut enc, depth);
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
                encode_child(graph, *arg, seen, &mut enc, depth);
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
                encode_child(graph, *arg, seen, &mut enc, depth);
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
                encode_child(graph, *child, seen, &mut enc, depth);
            }
        }
    }
    enc.finish()
}

fn encode_surface_entry(
    graph: &SemanticGraphStore,
    entry: &SurfaceEntry,
    seen: &mut HashMap<SemanticNodeId, u32>,
    enc: &mut Encoder,
    depth: u16,
) {
    match entry {
        SurfaceEntry::Member(member) => {
            enc.u8(1);
            encode_property_key(graph, &member.key, seen, enc, depth);
            enc.bool(member.optional);
            enc.bool(member.readonly);
            enc.u8(match member.visibility {
                verter_type_expr::MemberVisibility::Public => 1,
                verter_type_expr::MemberVisibility::Protected => 2,
                verter_type_expr::MemberVisibility::Private => 3,
            });
            encode_child(graph, member.value, seen, enc, depth);
        }
        SurfaceEntry::CallSignature(id) => {
            enc.u8(2);
            encode_child(graph, *id, seen, enc, depth);
        }
        SurfaceEntry::ConstructSignature(id) => {
            enc.u8(3);
            encode_child(graph, *id, seen, enc, depth);
        }
        SurfaceEntry::IndexSignature(sig) => {
            enc.u8(4);
            enc.bool(sig.readonly);
            encode_child(graph, sig.key_type, seen, enc, depth);
            encode_child(graph, sig.value_type, seen, enc, depth);
        }
    }
}

/// Encode one derived positive member — every `SurfaceMember` field the
/// arena's `Eq` carries except the deliberately excluded `spans` and
/// `declaration_origin` (source-location-only differences collapse under
/// `T | T = T`).
fn encode_surface_member(
    graph: &SemanticGraphStore,
    member: &SurfaceMember,
    seen: &mut HashMap<SemanticNodeId, u32>,
    enc: &mut Encoder,
    depth: u16,
) {
    encode_property_key(graph, &member.key, seen, enc, depth);
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
    encode_child(graph, member.value, seen, enc, depth);
}

fn encode_property_key(
    graph: &SemanticGraphStore,
    key: &AuthoredPropertyKey,
    seen: &mut HashMap<SemanticNodeId, u32>,
    enc: &mut Encoder,
    depth: u16,
) {
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
            encode_child(graph, *node, seen, enc, depth);
        }
    }
}

fn encode_query_error(enc: &mut Encoder, err: &QueryError) {
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
    };
    enc.u8(tag);
    match err {
        QueryError::UnsupportedIntrinsic { name } => enc.str(name),
        QueryError::RecursiveRef { name } => enc.str(name),
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

fn intrinsic_op_tag(op: CompilerIntrinsicTypeOp) -> u8 {
    // Stable ordinal from Debug spelling so adding an op cannot silently
    // reuse a previous tag without a schema bump.
    let name = format!("{op:?}");
    let mut h = 1u8;
    for b in name.bytes() {
        h = h.wrapping_add(b).wrapping_mul(31);
    }
    h
}

/// Sort `members` by `VerterStableV1`. Equal keys stay in input order
/// only when they are indistinguishable; distinguishable members never
/// compare equal.
pub fn sort_by_stable_key(graph: &SemanticGraphStore, members: &mut [SemanticNodeId]) {
    members.sort_by_cached_key(|id| stable_key_for_node(graph, *id));
}

/// Stable-key equality that may license a collapse: BOTH keys must be
/// complete. The depth-exhausted marker is shared by every over-deep
/// structure, so an incomplete key never proves structural identity —
/// mirroring the canonical comparator's budgeted refusal to collapse.
pub fn provably_equal(graph: &SemanticGraphStore, a: SemanticNodeId, b: SemanticNodeId) -> bool {
    let key_a = stable_key_for_node(graph, a);
    let key_b = stable_key_for_node(graph, b);
    key_a.is_complete() && key_b.is_complete() && key_a == key_b
}

/// Union-set canonicalization: sort by stable key and drop exact duplicates.
/// An incomplete (depth-exhausted) key never drops anything — the shared
/// marker would collapse distinct over-deep structures into one.
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
