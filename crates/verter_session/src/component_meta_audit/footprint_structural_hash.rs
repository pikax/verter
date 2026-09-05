//! Structural, content-only fingerprinting of semantic nodes for the
//! footprint miner. Extracted from the miner so the encoder (variant tags,
//! cycle/depth sentinels, recursive child fingerprints) is one focused unit.

use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::semantic_query::{
    AuthoredPropertyKey, DeclIdentity, IndexKey, IndexSignature, LiteralValue, NodeScopeId,
    ObjectConstructionEffect, ScopeId, SemanticNodeData, SemanticNodeId, SurfaceMember,
    ValueRootKey,
};
use crate::semantic_query_memo::SemanticGraphStore;
use crate::types::Hash16;

/// Depth backstop for the structural walk, secondary to the visited-set cycle
/// guard. The visited set already terminates every cycle reachable through the
/// interned DAG; this ceiling is a defensive bound against a pathologically deep
/// (but acyclic) chain. At the ceiling the walk encodes a fixed
/// [`TAG_DEPTH_CEILING`] sentinel — never an arena ordinal.
const STRUCTURAL_HASH_MAX_DEPTH: u32 = 64;

/// Content-only, recursive, variant-tagged structural fingerprint of a semantic
/// node.
///
/// The fingerprint is derived EXCLUSIVELY from semantic CONTENT — it never folds
/// a raw [`SemanticNodeId`] arena ordinal. A `SemanticNodeId` is allocated
/// sequentially on intern-miss and is only meaningful inside one store for one
/// project generation (see the contract on [`SemanticNodeId`]); folding it would
/// make two equivalent `Foo<String>` carriers hash differently whenever `String`
/// received a different ordinal, breaking the file-level content-determinism
/// contract. Instead, every child reference is replaced by the RECURSIVE
/// structural fingerprint of the node it points at, resolved through
/// `graph.node_data`.
///
/// Encoding shape:
///
/// - Each variant emits a one-byte [`VariantTag`] discriminant, so two different
///   variants can never collide on equal payload bytes.
/// - Scalars are emitted by their little-endian bytes; `Arc<str>` and string
///   collections are LENGTH-PREFIXED; child-reference fields are replaced by the
///   recursive child fingerprint, also length/order-prefixed where a collection.
/// - The three carriers (`TypeOf` / `BareRef` / `ImportType`) are fingerprinted
///   from their public HEAD (which exposes no args) PLUS the ordered recursive
///   fingerprints of their `type_args` children, reached through the sole
///   descent accessor [`SemanticNodeData::carrier_type_args`]. Their private
///   `type_args` layout is NEVER `Debug`-rendered.
///
/// Cycle safety: an in-progress visited set of `SemanticNodeId`s on the current
/// descent path terminates any cycle in the interned graph by emitting a fixed
/// [`TAG_CYCLE`] back-reference sentinel instead of recursing forever. A
/// secondary depth ceiling ([`STRUCTURAL_HASH_MAX_DEPTH`]) backstops a
/// pathologically deep acyclic chain with a fixed [`TAG_DEPTH_CEILING`]
/// sentinel. A child id that does not resolve in `graph` emits a fixed
/// [`TAG_UNRESOLVED_CHILD`] sentinel — never the ordinal value (two distinct but
/// equally-unresolved children collapsing is acceptable; two equal-content
/// children diverging by ordinal is not).
pub(crate) fn structural_hash_of(graph: &SemanticGraphStore, data: &SemanticNodeData) -> Hash16 {
    // TODO(follow-up): a per-mine `FxHashMap<SemanticNodeId, Hash16>` content-hash
    // memo would let `encode_child` reuse a child's already-computed fingerprint
    // instead of re-walking shared subtrees per reference. It is NOT a drop-in:
    // the encoder emits a node's FULL recursive bytes inline (not its 16-byte
    // hash), and the same node legitimately encodes as `TAG_CYCLE` vs full
    // content depending on the descent path — a naive `SemanticNodeId → bytes`
    // (or `→ Hash16`) memo is path-INSENSITIVE and would both change every
    // fingerprint's bytes and mis-handle the cycle sentinel. The durable shape is
    // an encode-once-then-splice cache that preserves byte-identity and respects
    // the visited-path sentinels; deferred so this fix does not risk any
    // fingerprint value. The depth-64 ceiling + visited set already bound the
    // current re-walk, so it terminates.
    let mut enc = StructuralEncoder {
        graph,
        buf: Vec::with_capacity(128),
        visited: Vec::new(),
    };
    enc.encode_node_data(data, 0);
    xxh3_128(&enc.buf).to_le_bytes()
}

/// Stateful structural encoder. Owns the growing byte `buf`, the in-progress
/// `visited` path set (for cycle detection), and a borrow of the graph for child
/// resolution.
struct StructuralEncoder<'g> {
    graph: &'g SemanticGraphStore,
    buf: Vec<u8>,
    /// `SemanticNodeId`s currently on the descent path. Used ONLY to detect a
    /// back-edge — never folded into the hash bytes.
    visited: Vec<SemanticNodeId>,
}

/// One-byte variant discriminants for the structural encoding. Each
/// `SemanticNodeData` variant — plus the descent sentinels — occupies a distinct
/// tag so disjoint variants live in disjoint hash-input spaces. Values are fixed
/// and independent of source declaration order; a new variant takes the next
/// free tag.
#[repr(u8)]
enum VariantTag {
    Alias = 1,
    Object = 2,
    Union = 3,
    Intersection = 4,
    Primitive = 5,
    Literal = 6,
    Opaque = 7,
    Array = 8,
    Tuple = 9,
    TemplateLiteral = 10,
    KeyOf = 11,
    IndexedAccess = 12,
    Mapped = 13,
    TypeOf = 14,
    TypeOfNominal = 19,
    TypeParam = 15,
    Infer = 16,
    Conditional = 17,
    // Tag 18 is retired and stays unassigned so surviving variants keep
    // stable tag values.
    DeclRef = 20,
    InstantiationRef = 21,
    MergedDecl = 22,
    BareRef = 23,
    ImportType = 24,
    RawFallback = 25,
    SyntheticBinding = 27,
    InferRef = 28,
    Signature = 29,
    ObjectSpreadProgram = 30,
    DeferredCallable = 31,
}

/// Descent sentinel: a child id currently on the descent path (a graph cycle).
const TAG_CYCLE: u8 = 0xF0;
/// Descent sentinel: a child id that did not resolve in `graph`.
const TAG_UNRESOLVED_CHILD: u8 = 0xF1;
/// Descent sentinel: the recursion depth backstop fired.
const TAG_DEPTH_CEILING: u8 = 0xF2;

impl StructuralEncoder<'_> {
    /// Push a length-prefixed string.
    fn push_str(&mut self, s: &str) {
        self.buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
        self.buf.extend_from_slice(s.as_bytes());
    }

    /// Push a length-prefixed list of strings (order-preserving).
    fn push_str_slice(&mut self, items: &[Arc<str>]) {
        self.buf
            .extend_from_slice(&(items.len() as u64).to_le_bytes());
        for item in items {
            self.push_str(item);
        }
    }

    /// Push an `Option<scalar-tag>`: `0` for `None`, `1` for `Some`.
    fn push_present(&mut self, present: bool) {
        self.buf.push(u8::from(present));
    }

    /// Resolve `id` and fold its RECURSIVE structural fingerprint into `buf`.
    /// Never folds the ordinal: a cycle, an unresolved id, or the depth backstop
    /// each emit a FIXED sentinel byte instead.
    fn encode_child(&mut self, id: SemanticNodeId, depth: u32) {
        if self.visited.contains(&id) {
            self.buf.push(TAG_CYCLE);
            return;
        }
        if depth >= STRUCTURAL_HASH_MAX_DEPTH {
            self.buf.push(TAG_DEPTH_CEILING);
            return;
        }
        match self.graph.node_data(id) {
            Some(child) => {
                self.visited.push(id);
                self.encode_node_data(&child, depth + 1);
                self.visited.pop();
            }
            None => self.buf.push(TAG_UNRESOLVED_CHILD),
        }
    }

    /// Encode every child id of an ordered id slice, length/order-prefixed.
    fn encode_child_slice(&mut self, ids: &[SemanticNodeId], depth: u32) {
        self.buf
            .extend_from_slice(&(ids.len() as u64).to_le_bytes());
        for id in ids {
            self.encode_child(*id, depth);
        }
    }

    /// Encode an `Option<SemanticNodeId>`: a presence byte followed by the child
    /// fingerprint when present.
    fn encode_child_opt(&mut self, id: Option<SemanticNodeId>, depth: u32) {
        match id {
            Some(id) => {
                self.push_present(true);
                self.encode_child(id, depth);
            }
            None => self.push_present(false),
        }
    }

    /// Encode an [`IndexKey`] structurally: a kind byte, then the key content
    /// (string / canonical integer bytes / recursive child fingerprint).
    fn encode_index_key(&mut self, index: &IndexKey, depth: u32) {
        match index {
            IndexKey::String(s) => {
                self.buf.push(0);
                self.push_str(s);
            }
            IndexKey::Number(n) => {
                self.buf.push(1);
                self.buf.extend_from_slice(&n.get().to_le_bytes());
            }
            IndexKey::UniqueSymbol(identity) => {
                self.buf.push(2);
                self.encode_value_decl_identity(identity);
            }
            IndexKey::Computed(id) => {
                self.buf.push(3);
                self.encode_child(*id, depth);
            }
        }
    }

    /// Encode a [`NodeScopeId`] by its SEMANTIC content (canonical id +
    /// whole-hash + local scope), never an arena ordinal.
    fn encode_node_scope(&mut self, scope: &NodeScopeId) {
        match scope {
            NodeScopeId::Global => self.buf.push(0),
            NodeScopeId::File {
                canonical_id,
                owner,
                whole_hash,
                local_scope,
            } => {
                self.buf.push(1);
                self.push_str(canonical_id);
                self.buf.push(match owner.kind() {
                    verter_type_expr::TopLevelOwnerKind::Module => 0,
                    verter_type_expr::TopLevelOwnerKind::Instance => 1,
                    verter_type_expr::TopLevelOwnerKind::Frontmatter => 2,
                });
                self.buf.extend_from_slice(&owner.ordinal().to_le_bytes());
                self.buf.extend_from_slice(whole_hash);
                match local_scope {
                    Some(s) => {
                        self.push_present(true);
                        self.buf.extend_from_slice(&s.to_le_bytes());
                    }
                    None => self.push_present(false),
                }
            }
        }
    }

    /// Encode a [`ScopeId`] by content (canonical id + optional local scope).
    fn encode_scope_id(&mut self, scope: &ScopeId) {
        self.push_str(&scope.canonical_id);
        match scope.local_scope {
            Some(s) => {
                self.push_present(true);
                self.buf.extend_from_slice(&s.to_le_bytes());
            }
            None => self.push_present(false),
        }
    }

    /// Encode a [`ValueRootKey`] by content (scope + name).
    fn encode_value_root(&mut self, root: &ValueRootKey) {
        self.encode_scope_id(&root.scope);
        self.push_str(&root.name);
    }

    /// Encode a [`DeclIdentity`] by content (canonical id + whole-hash + name).
    fn encode_decl_identity(&mut self, identity: &DeclIdentity) {
        self.push_str(&identity.canonical_id);
        self.buf.push(match identity.owner.kind() {
            verter_type_expr::TopLevelOwnerKind::Module => 0,
            verter_type_expr::TopLevelOwnerKind::Instance => 1,
            verter_type_expr::TopLevelOwnerKind::Frontmatter => 2,
        });
        self.buf
            .extend_from_slice(&identity.owner.ordinal().to_le_bytes());
        self.buf.extend_from_slice(&identity.whole_hash);
        self.push_str(&identity.decl_name);
    }

    fn encode_value_decl_identity(
        &mut self,
        identity: &verter_type_expr::facts::ValueDeclIdentityPart,
    ) {
        self.push_str(&identity.canonical_id);
        self.buf.push(match identity.owner.kind() {
            verter_type_expr::TopLevelOwnerKind::Module => 0,
            verter_type_expr::TopLevelOwnerKind::Instance => 1,
            verter_type_expr::TopLevelOwnerKind::Frontmatter => 2,
        });
        self.buf
            .extend_from_slice(&identity.owner.ordinal().to_le_bytes());
        self.push_str(&identity.symbol);
        self.buf
            .extend_from_slice(&(identity.member_path.len() as u64).to_le_bytes());
        for segment in identity.member_path.iter() {
            self.push_str(segment);
        }
    }

    fn encode_authored_property_key(&mut self, key: &AuthoredPropertyKey, depth: u32) {
        match key {
            AuthoredPropertyKey::String(name) => {
                self.buf.push(0);
                self.push_str(name);
            }
            AuthoredPropertyKey::Number(number) => {
                self.buf.push(1);
                self.buf.extend_from_slice(&number.get().to_le_bytes());
            }
            AuthoredPropertyKey::UniqueSymbol(identity) => {
                self.buf.push(2);
                self.encode_value_decl_identity(identity);
            }
            AuthoredPropertyKey::Computed(node) => {
                self.buf.push(3);
                self.encode_child(*node, depth);
            }
        }
    }

    /// Encode one [`SurfaceMember`]: scalar/string fields by content, the value
    /// type by recursive child fingerprint.
    fn encode_surface_member(&mut self, m: &SurfaceMember, depth: u32) {
        self.encode_authored_property_key(&m.key, depth);
        self.encode_child(m.value, depth);
        self.buf.push(u8::from(m.optional));
        self.buf.push(u8::from(m.readonly));
        self.push_str(&format!("{:?}", m.method_kind));
        // `visibility` / `merge_role` are id-free C-like enums; a Debug of them
        // cannot transitively print a `SemanticNodeId`, so a stable string of
        // their discriminant is content-deterministic.
        self.push_str(&format!("{:?}", m.visibility));
        self.push_str(&format!("{:?}", m.spans));
        match &m.declaration_origin {
            Some(o) => {
                self.push_present(true);
                self.push_str(o);
            }
            None => self.push_present(false),
        }
        self.buf.push(u8::from(m.declared_in_macro_type_arg.get()));
        self.push_str(&format!("{:?}", m.merge_role.role()));
    }

    /// Encode one [`IndexSignature`]: key / value types by recursive child
    /// fingerprint, the rest by content.
    fn encode_index_signature(&mut self, sig: &IndexSignature, depth: u32) {
        self.encode_child(sig.key_type, depth);
        self.encode_child(sig.value_type, depth);
        self.buf.push(u8::from(sig.readonly));
        self.push_str(&format!("{:?}", sig.spans));
        match &sig.declaration_origin {
            Some(o) => {
                self.push_present(true);
                self.push_str(o);
            }
            None => self.push_present(false),
        }
    }

    /// The structural encoder body. EXHAUSTIVE over every `SemanticNodeData`
    /// variant — NO `_` wildcard, so a new variant fails to compile here and
    /// must be classified (a content-bearing scalar, or a child-bearing variant
    /// whose ids are descended). `depth` is the current descent depth.
    fn encode_node_data(&mut self, data: &SemanticNodeData, depth: u32) {
        match data {
            // ── Single-child variants. ──
            SemanticNodeData::Alias(child) => {
                self.buf.push(VariantTag::Alias as u8);
                self.encode_child(*child, depth);
            }
            SemanticNodeData::Array { element, readonly } => {
                self.buf.push(VariantTag::Array as u8);
                self.buf.push(u8::from(*readonly));
                self.encode_child(*element, depth);
            }
            SemanticNodeData::KeyOf { base } => {
                self.buf.push(VariantTag::KeyOf as u8);
                self.encode_child(*base, depth);
            }
            // ── Child-list variants. ──
            SemanticNodeData::Union(arms) => {
                self.buf.push(VariantTag::Union as u8);
                self.encode_child_slice(arms, depth);
            }
            SemanticNodeData::Intersection(arms) => {
                self.buf.push(VariantTag::Intersection as u8);
                self.encode_child_slice(arms, depth);
            }
            SemanticNodeData::MergedDecl { contributors } => {
                self.buf.push(VariantTag::MergedDecl as u8);
                self.encode_child_slice(contributors, depth);
            }

            // ── Compound-payload variants carrying child ids. ──
            SemanticNodeData::Object(surface) => {
                self.buf.push(VariantTag::Object as u8);
                self.buf
                    .extend_from_slice(&(surface.positive_members().len() as u64).to_le_bytes());
                for m in surface.positive_members().iter() {
                    self.encode_surface_member(m, depth);
                }
                self.encode_child_slice(&surface.call_signatures, depth);
                self.encode_child_slice(&surface.construct_signatures, depth);
                self.buf
                    .extend_from_slice(&(surface.index_signatures.len() as u64).to_le_bytes());
                for sig in surface.index_signatures.iter() {
                    self.encode_index_signature(sig, depth);
                }
                self.encode_child_opt(surface.keyspace, depth);
                self.buf.push(u8::from(surface.has_known_index_signature()));
                self.push_present(false);
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                self.buf.push(VariantTag::ObjectSpreadProgram as u8);
                self.buf
                    .extend_from_slice(&(program.effects.len() as u64).to_le_bytes());
                for effect in program.effects.iter() {
                    match effect {
                        ObjectConstructionEffect::DirectProperty(effect) => {
                            self.buf.push(0);
                            self.encode_authored_property_key(&effect.key, depth);
                            self.encode_child(effect.value, depth);
                            self.buf.push(u8::from(effect.optional));
                            self.buf.push(u8::from(effect.readonly));
                            self.push_str(&format!("{:?}", effect.visibility));
                            self.push_str(&format!("{:?}", effect.spans));
                            self.push_str(&format!("{:?}", effect.declaration_origin));
                            self.buf
                                .push(u8::from(effect.declared_in_macro_type_arg.get()));
                            self.push_str(&format!("{:?}", effect.merge_role.role()));
                            self.push_str(&format!("{:?}", effect.excess_origin));
                        }
                        ObjectConstructionEffect::DirectMethod(effect) => {
                            self.buf.push(1);
                            self.encode_authored_property_key(&effect.key, depth);
                            self.encode_child(effect.signature, depth);
                            self.buf.push(u8::from(effect.optional));
                            self.buf.push(u8::from(effect.has_implementation_body));
                            self.push_str(&format!("{:?}", effect.visibility));
                            self.push_str(&format!("{:?}", effect.spans));
                            self.push_str(&format!("{:?}", effect.declaration_origin));
                            self.buf
                                .push(u8::from(effect.declared_in_macro_type_arg.get()));
                            self.push_str(&format!("{:?}", effect.merge_role.role()));
                            self.push_str(&format!("{:?}", effect.excess_origin));
                        }
                        ObjectConstructionEffect::DirectGet(effect) => {
                            self.buf.push(2);
                            self.encode_authored_property_key(&effect.key, depth);
                            self.encode_child(effect.signature, depth);
                            self.buf.push(u8::from(effect.optional));
                            self.buf.push(u8::from(effect.has_implementation_body));
                            self.push_str(&format!("{:?}", effect.visibility));
                            self.push_str(&format!("{:?}", effect.spans));
                            self.push_str(&format!("{:?}", effect.declaration_origin));
                            self.buf
                                .push(u8::from(effect.declared_in_macro_type_arg.get()));
                            self.push_str(&format!("{:?}", effect.merge_role.role()));
                            self.push_str(&format!("{:?}", effect.excess_origin));
                        }
                        ObjectConstructionEffect::DirectSet(effect) => {
                            self.buf.push(3);
                            self.encode_authored_property_key(&effect.key, depth);
                            self.encode_child(effect.signature, depth);
                            self.buf.push(u8::from(effect.optional));
                            self.buf.push(u8::from(effect.has_implementation_body));
                            self.push_str(&format!("{:?}", effect.visibility));
                            self.push_str(&format!("{:?}", effect.spans));
                            self.push_str(&format!("{:?}", effect.declaration_origin));
                            self.buf
                                .push(u8::from(effect.declared_in_macro_type_arg.get()));
                            self.push_str(&format!("{:?}", effect.merge_role.role()));
                            self.push_str(&format!("{:?}", effect.excess_origin));
                        }
                        ObjectConstructionEffect::DirectIndex(effect) => {
                            self.buf.push(4);
                            self.encode_child(effect.key_type, depth);
                            self.encode_child(effect.value_type, depth);
                            self.buf.push(u8::from(effect.readonly));
                            self.push_str(&format!("{:?}", effect.spans));
                            self.push_str(&format!("{:?}", effect.declaration_origin));
                        }
                        ObjectConstructionEffect::DirectCall(node) => {
                            self.buf.push(5);
                            self.encode_child(*node, depth);
                        }
                        ObjectConstructionEffect::DirectConstruct(node) => {
                            self.buf.push(6);
                            self.encode_child(*node, depth);
                        }
                        ObjectConstructionEffect::Spread(node) => {
                            self.buf.push(7);
                            self.encode_child(*node, depth);
                        }
                    }
                }
            }
            SemanticNodeData::Tuple { elements, readonly } => {
                self.buf.push(VariantTag::Tuple as u8);
                self.buf.push(u8::from(*readonly));
                self.buf
                    .extend_from_slice(&(elements.len() as u64).to_le_bytes());
                for el in elements.iter() {
                    match &el.label {
                        Some(l) => {
                            self.push_present(true);
                            self.push_str(l);
                        }
                        None => self.push_present(false),
                    }
                    self.encode_child(el.value, depth);
                    self.buf.push(u8::from(el.optional));
                    self.buf.push(u8::from(el.rest));
                }
            }
            SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            } => {
                self.buf.push(VariantTag::TemplateLiteral as u8);
                self.push_str_slice(quasis);
                self.encode_child_slice(expressions, depth);
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                self.buf.push(VariantTag::IndexedAccess as u8);
                self.encode_child(*object, depth);
                self.encode_index_key(index, depth);
            }
            SemanticNodeData::Mapped { source, mapper } => {
                self.buf.push(VariantTag::Mapped as u8);
                self.encode_child(*source, depth);
                self.encode_child(mapper.parameter_node, depth);
                self.encode_child(mapper.key_space, depth);
                self.encode_child(mapper.value_expr, depth);
                self.push_str(&format!("{:?}", mapper.optionality));
                self.push_str(&format!("{:?}", mapper.readonly));
                self.encode_child_opt(mapper.name_remap, depth);
                self.push_str(&format!("{:?}", mapper.kind));
            }
            SemanticNodeData::TypeParam {
                decl,
                param_index,
                constraint,
                default,
                display_name,
            } => {
                self.buf.push(VariantTag::TypeParam as u8);
                self.encode_decl_identity(decl);
                self.buf.extend_from_slice(&param_index.to_le_bytes());
                self.encode_child_opt(*constraint, depth);
                self.encode_child_opt(*default, depth);
                self.push_str(display_name);
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
            } => {
                self.buf.push(VariantTag::Conditional as u8);
                self.buf.push(u8::from(*distributive));
                self.encode_child(*check, depth);
                self.encode_child(*extends, depth);
                self.encode_child(*true_branch_ref, depth);
                self.encode_child(*false_branch_ref, depth);
            }
            SemanticNodeData::Signature {
                kind,
                params,
                return_type,
                type_parameters,
                occurrence,
                return_carrier,
                signature_span,
                return_type_span,
            } => {
                self.buf.push(VariantTag::Signature as u8);
                self.buf.push(match kind {
                    crate::semantic_query::SignatureKind::Call => 0,
                    crate::semantic_query::SignatureKind::Construct => 1,
                });
                self.buf
                    .extend_from_slice(&(params.len() as u64).to_le_bytes());
                for p in params.iter() {
                    match &p.name {
                        Some(n) => {
                            self.push_present(true);
                            self.push_str(n);
                        }
                        None => self.push_present(false),
                    }
                    self.encode_child(p.ty, depth);
                    self.buf.push(u8::from(p.optional));
                    self.buf.push(u8::from(p.rest));
                    self.push_str(&format!("{:?}", p.span));
                }
                self.encode_child(*return_type, depth);
                self.buf
                    .extend_from_slice(&(type_parameters.len() as u64).to_le_bytes());
                for tp in type_parameters.iter() {
                    self.push_str(&tp.name);
                    self.encode_child(tp.param, depth);
                    self.encode_child_opt(tp.constraint, depth);
                    self.encode_child_opt(tp.default, depth);
                    self.buf.push(u8::from(tp.is_const));
                }
                self.push_str(&format!("{occurrence:?}"));
                self.push_str(&format!("{return_carrier:?}"));
                self.push_str(&format!("{signature_span:?}"));
                self.push_str(&format!("{return_type_span:?}"));
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                self.buf.push(VariantTag::InstantiationRef as u8);
                self.encode_decl_identity(base);
                self.encode_child_slice(args, depth);
            }

            // ── Carrier arms: HEAD (no args) + ordered recursive child hashes.
            // NEVER `Debug`-render the carrier — its private `type_args` layout
            // is a representation detail; descend through the sole accessor. ──
            SemanticNodeData::TypeOf(_) => {
                self.buf.push(VariantTag::TypeOf as u8);
                let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                self.encode_value_root(value_root);
                self.push_str_slice(path);
                let args = data.carrier_type_args().to_vec();
                self.encode_child_slice(&args, depth);
            }
            // The nominal terminal's DECLARING IDENTITY participates in the
            // structural hash: it is the whole semantic content of the node,
            // so two same-headed carriers with different declaring symbols
            // must not share a structural fingerprint. The identity is
            // encoded field-by-field through the shared encoder
            // (`encode_value_decl_identity`) like every sibling arm — a
            // `Debug` render is not a stability contract and can reorder
            // fields without the fingerprint contract noticing.
            SemanticNodeData::TypeOfNominal(_) => {
                self.buf.push(VariantTag::TypeOfNominal as u8);
                let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                self.encode_value_root(value_root);
                self.push_str_slice(path);
                let identity = data
                    .typeof_nominal_identity()
                    .expect("TypeOfNominal carrier identity");
                self.encode_value_decl_identity(identity);
            }
            SemanticNodeData::BareRef(_) => {
                self.buf.push(VariantTag::BareRef as u8);
                let (name, scope) = data.bare_ref_head().expect("BareRef carrier head");
                self.push_str(name);
                self.encode_node_scope(scope);
                let args = data.carrier_type_args().to_vec();
                self.encode_child_slice(&args, depth);
            }
            SemanticNodeData::ImportType(_) => {
                self.buf.push(VariantTag::ImportType as u8);
                let (specifier, qualifier, typeof_query) =
                    data.import_type_head().expect("ImportType carrier head");
                self.push_str(specifier);
                self.push_str_slice(qualifier);
                self.buf.push(u8::from(typeof_query));
                let args = data.carrier_type_args().to_vec();
                self.encode_child_slice(&args, depth);
            }

            // ── Pure-scalar / id-free variants. Encoded by content. None of
            // these payloads can transitively hold a `SemanticNodeId`:
            // `PrimitiveKind` / `LiteralValue` / `QueryError` / the `Infer`
            // name / `RawFallback` raw text / the `DeclRef` identity are all
            // scalar/string or live in a lower crate than `SemanticNodeId`, so a
            // `Debug` of them is content-only. (`SyntheticBinding` is NOT in this
            // group: its `id` is scalar, but its `value_node` is a child node id
            // that is descended via `encode_child` — see that arm below.) ──
            SemanticNodeData::Primitive(p) => {
                self.buf.push(VariantTag::Primitive as u8);
                self.push_str(&format!("{p:?}"));
            }
            SemanticNodeData::Literal(lit) => {
                self.buf.push(VariantTag::Literal as u8);
                match lit {
                    LiteralValue::String(s) => {
                        self.buf.push(0);
                        self.push_str(s);
                    }
                    LiteralValue::Number(n) => {
                        self.buf.push(1);
                        // f64 by bit pattern — NaN folds to one stable encoding.
                        self.buf.extend_from_slice(&n.to_bits().to_le_bytes());
                    }
                    LiteralValue::Boolean(b) => {
                        self.buf.push(2);
                        self.buf.push(u8::from(*b));
                    }
                    LiteralValue::BigInt(s) => {
                        self.buf.push(3);
                        self.push_str(s);
                    }
                }
            }
            SemanticNodeData::Opaque(err) => {
                self.buf.push(VariantTag::Opaque as u8);
                // `QueryError` is an entirely scalar/string enum (no
                // `SemanticNodeId` in any arm), so its `Debug` is content-only.
                self.push_str(&format!("{err:?}"));
            }
            SemanticNodeData::Infer { name, binder } => {
                self.buf.push(VariantTag::Infer as u8);
                self.push_str(name);
                let fingerprint = binder.stable_fingerprint_bytes();
                self.buf
                    .extend_from_slice(&(fingerprint.len() as u64).to_le_bytes());
                self.buf.extend_from_slice(&fingerprint);
            }
            SemanticNodeData::InferRef { name, binder } => {
                self.buf.push(VariantTag::InferRef as u8);
                self.push_str(name);
                let fingerprint = binder.stable_fingerprint_bytes();
                self.buf
                    .extend_from_slice(&(fingerprint.len() as u64).to_le_bytes());
                self.buf.extend_from_slice(&fingerprint);
            }
            SemanticNodeData::RawFallback { value } => {
                self.buf.push(VariantTag::RawFallback as u8);
                self.push_str(value.raw());
            }
            SemanticNodeData::DeclRef { identity } => {
                self.buf.push(VariantTag::DeclRef as u8);
                self.encode_decl_identity(identity);
            }
            // The sealed callable carrier: its composed parts are readable
            // only by its two consumers, so the encoding is the tag alone.
            SemanticNodeData::DeferredCallable(_) => {
                self.buf.push(VariantTag::DeferredCallable as u8);
            }
            SemanticNodeData::SyntheticBinding { id, value_node } => {
                self.buf.push(VariantTag::SyntheticBinding as u8);
                // `SyntheticBindingId` is content-free (canonical id + surface
                // kind + slot/binding names); no `SemanticNodeId`.
                self.push_str(&id.scope_canonical_id);
                self.push_str(&format!("{:?}", id.surface_kind));
                match &id.slot_name {
                    Some(n) => {
                        self.push_present(true);
                        self.push_str(n);
                    }
                    None => self.push_present(false),
                }
                self.push_str(&id.binding_name);
                // `value_node` is a [`SemanticNodeId`] arena ordinal stored as a
                // raw `u64` on the payload — store/generation-relative, NOT
                // content. It is NEVER folded as the ordinal: it is descended as
                // a graph child via [`Self::encode_child`], so the fingerprint
                // carries the RECURSIVE CONTENT of the node it points at (or a
                // fixed cycle / unresolved / depth sentinel), exactly like every
                // other child id. `value_node` participates in node interning
                // Eq/Hash, so two bindings pointing at different value nodes stay
                // structurally DISTINCT (descending by content preserves that),
                // while two content-equivalent bindings whose target was interned
                // at a different ordinal hash IDENTICALLY (the cross-run
                // byte-identity contract this encoder establishes).
                self.encode_child(SemanticNodeId(*value_node), depth);
            }
        }
    }
}
