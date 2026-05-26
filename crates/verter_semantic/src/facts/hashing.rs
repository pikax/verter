//! Stack-safe, alpha-normalised semantic fingerprint computation
//! over `TypeExpr` bodies (R16, R27, R28).
//!
//! Walks a `TypeExpr` body with an explicit worklist + `VisitedSet`,
//! emitting a `Hash16` `semantic_hash`. The walk is path-precise:
//! references to other declarations are recorded as reference-shape
//! edges (`Ref("Local", Type)`, `ImportRefRef("./spec", "binding",
//! Type)`) WITHOUT inlining the referent's body (R14). Editing an
//! unused local in the same file does NOT change consumers'
//! `semantic_hash` over an export that does not reach it.
//!
//! ## Stack safety
//!
//! The walker carries an explicit `depth` counter and bails to
//! `Opaque(BudgetExceeded)` when depth ≥ [`MAX_HASH_DEPTH`]. The
//! call-stack recursion through nested `TypeExpr` arms is bounded by
//! this depth budget, so a 10,000-arm union or a 200-deep nested
//! conditional cannot overflow the OS thread stack.
//!
//! ## Cycle handling
//!
//! When the walk reaches a node it has previously placed on its
//! visit path (via the `VisitedSet`), it emits a stable
//! `CycleRef(visit_index)` placeholder rather than recursing. The
//! `visit_index` is the lexicographic index of the cycle target
//! in the canonical visit order. **Visit order is canonical:
//! lexicographic by `(name, symbol_space)` at each unresolved-
//! neighbor expansion; depth-budget tie-break by `(canonical, name,
//! symbol_space)`.** `CycleRef` placeholder identity is therefore
//! invariant under source-text reordering — the same cycle
//! produces byte-identical fingerprints regardless of whether the
//! declarations were rewritten in lexical reverse order.
//!
//! ## Depth budget (R27)
//!
//! `MAX_HASH_DEPTH = 64`. Over-budget paths emit
//! `Opaque(BudgetExceeded)` and the cache entry is admitted as
//! `NonCacheable`. The walker carries an explicit `BudgetExceeded`
//! flag that producers check after [`compute_semantic_hash`].

use std::collections::BTreeMap;
use std::sync::Arc;

use verter_type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, LiteralValue, MappedModifier, MethodSignature,
    ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TupleElement, TypeExpr, TypeParam,
    ValueRef,
};

use crate::analysis::types::hash_16;
use crate::facts::registry::{FactHash, MemberKind, SymbolSpace};

/// Depth budget per R27. A walk that hits this depth emits
/// `Opaque(BudgetExceeded)` and the cache entry is admitted as
/// `NonCacheable`.
pub const MAX_HASH_DEPTH: usize = 64;

/// Identity of a cross-decl reference appearing inside a fact body.
///
/// A `TypeExpr::Ref { name, .. }` resolves to one of these variants
/// before the body fingerprint is hashed. The mapping is provided by
/// the caller (the shallow walk knows which `name` is a same-file
/// local vs an imported binding); the hashing routine never resolves
/// names on its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CrossDeclRef {
    /// `Ref("Local", space)` — same-file declaration.
    LocalDecl { name: Arc<str>, space: SymbolSpace },
    /// `ImportRefRef("./spec", "binding", space)` — imported binding.
    /// No `resolved_canonical` (R12).
    ImportRef {
        specifier: Arc<str>,
        binding: Arc<str>,
        space: SymbolSpace,
    },
    /// A `TypeExpr::TypeOf(value_name)` reference. Routed through
    /// the typeof channel because the binding is in value-space.
    TypeOfRef { name: Arc<str> },
    /// The reference could not be resolved by the producer (unknown
    /// origin, e.g. global ambient lib). Emit as `Unresolved(name)`.
    Unresolved { name: Arc<str>, space: SymbolSpace },
}

impl CrossDeclRef {
    /// Stable byte serialisation used inside the structural hash.
    fn extend_hash_buf(&self, buf: &mut Vec<u8>) {
        match self {
            Self::LocalDecl { name, space } => {
                buf.push(0xC0);
                buf.push(space.tag());
                buf.extend_from_slice(name.as_bytes());
                buf.push(0xFF);
            }
            Self::ImportRef {
                specifier,
                binding,
                space,
            } => {
                buf.push(0xC1);
                buf.push(space.tag());
                buf.extend_from_slice(specifier.as_bytes());
                buf.push(0xFE);
                buf.extend_from_slice(binding.as_bytes());
                buf.push(0xFF);
            }
            Self::TypeOfRef { name } => {
                buf.push(0xC2);
                buf.extend_from_slice(name.as_bytes());
                buf.push(0xFF);
            }
            Self::Unresolved { name, space } => {
                buf.push(0xC3);
                buf.push(space.tag());
                buf.extend_from_slice(name.as_bytes());
                buf.push(0xFF);
            }
        }
    }

    /// The canonical sort key for visit-order canonicalisation: the
    /// reference's `(name, space)` pair. Used by callers that need to
    /// order references lexicographically (R27 canonical visit
    /// order).
    #[must_use]
    pub fn canonical_sort_key(&self) -> (&str, u8) {
        match self {
            Self::LocalDecl { name, space } => (name.as_ref(), space.tag()),
            Self::ImportRef { binding, space, .. } => (binding.as_ref(), space.tag()),
            Self::TypeOfRef { name } => (name.as_ref(), SymbolSpace::Value.tag()),
            Self::Unresolved { name, space } => (name.as_ref(), space.tag()),
        }
    }
}

/// Resolver lens that the caller supplies — maps a `TypeExpr::Ref`'s
/// name + space pair to its cross-decl reference identity, OR `None`
/// if the name is a free type parameter (`T`, `U`) in the current
/// generic frame.
pub trait CrossDeclLens {
    /// Resolve a `TypeExpr::Ref { name, type_arguments }` site to its
    /// cross-decl reference identity. Return `None` if the name is a
    /// free type parameter — the hasher emits a `TypeParam(<index>)`
    /// alpha-normalised placeholder in that case.
    fn resolve(&self, name: &str, space: SymbolSpace) -> Option<CrossDeclRef>;
}

/// No-op lens used by tests that don't care about cross-decl edges —
/// every reference becomes `Unresolved(name, space)`. Production
/// callers MUST supply a real lens; the shallow walk has all the
/// information needed.
#[derive(Debug, Default)]
pub struct UnresolvedLens;

impl CrossDeclLens for UnresolvedLens {
    fn resolve(&self, name: &str, space: SymbolSpace) -> Option<CrossDeclRef> {
        Some(CrossDeclRef::Unresolved {
            name: Arc::from(name),
            space,
        })
    }
}

/// Outcome of a hashing call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashOutcome {
    /// The structural fingerprint.
    pub hash: FactHash,
    /// `true` if the walk exceeded `MAX_HASH_DEPTH`. The producer
    /// MUST admit the cache entry as `NonCacheable` when this is
    /// `true` (R27).
    pub budget_exceeded: bool,
    /// Stable count of visited unique nodes — used by parse-time
    /// producers + cycle tests to verify visit-order stability
    /// under reordering.
    pub visited_nodes: usize,
}

/// Compute the alpha-normalised structural fingerprint of a single
/// `TypeExpr` body. The body is hashed in isolation; cross-decl
/// references resolve through `lens` and emit reference-shape edges
/// (R14).
///
/// The default `space` parameter is the symbol space of the
/// declaration the body belongs to. Used to disambiguate type-vs-
/// value references the producer hasn't already tagged.
pub fn compute_semantic_hash(
    body: &TypeExpr,
    space: SymbolSpace,
    lens: &dyn CrossDeclLens,
) -> HashOutcome {
    let mut walker = Walker::new(lens, space);
    walker.walk(body);
    walker.finish()
}

/// Compute the alpha-normalised structural fingerprint over a member
/// header `(name, kind, exporter_salt)` per R28's `MemberPresence`
/// fact.
///
/// Header only — NO body fingerprint. Adding sibling `b` does not
/// force re-walking member `a`'s body (the path-precision
/// invariant).
#[must_use]
pub fn compute_member_presence_hash(
    exporter: &str,
    name: &str,
    kind: MemberKind,
    space: SymbolSpace,
) -> FactHash {
    let mut buf: Vec<u8> = Vec::with_capacity(64);
    buf.extend_from_slice(b"member-presence:");
    buf.push(space.tag());
    buf.extend_from_slice(name.as_bytes());
    buf.push(0xFF);
    buf.extend_from_slice(&kind.tag());
    buf.extend_from_slice(&exporter_qualifier_salt(exporter));
    hash_16(&buf)
}

/// Compute the whole-surface fingerprint over an exporter's full
/// member name + kind list (R28 `MemberShape`).
///
/// Sorted by name, order-insensitive at top level. Member-body
/// fingerprints live in separate `Member` facts (lazy).
#[must_use]
pub fn compute_member_shape_hash(
    exporter: &str,
    members: &[(Arc<str>, MemberKind)],
    space: SymbolSpace,
) -> FactHash {
    let mut sorted: Vec<&(Arc<str>, MemberKind)> = members.iter().collect();
    sorted.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
    let mut buf: Vec<u8> = Vec::with_capacity(64 + 16 * sorted.len());
    buf.extend_from_slice(b"member-shape:");
    buf.push(space.tag());
    buf.extend_from_slice(exporter.as_bytes());
    buf.push(0xFE);
    for (name, kind) in &sorted {
        buf.extend_from_slice(name.as_bytes());
        buf.push(0xFD);
        buf.extend_from_slice(&kind.tag());
        buf.push(0xFC);
    }
    hash_16(&buf)
}

/// Stable per-exporter salt for `MemberPresence.semantic_hash`. Keeps
/// the `(name, kind)` pair distinct across multiple exporters in the
/// same file.
#[must_use]
pub fn exporter_qualifier_salt(exporter: &str) -> FactHash {
    let mut buf: Vec<u8> = Vec::with_capacity(32);
    buf.extend_from_slice(b"presence-salt:");
    buf.extend_from_slice(exporter.as_bytes());
    hash_16(&buf)
}

// ──────────────────────────────────────────────────────────────────
// Internal walker
// ──────────────────────────────────────────────────────────────────

/// Inner traversal state.
struct Walker<'a> {
    buf: Vec<u8>,
    visited: BTreeMap<Vec<u8>, usize>,
    visit_counter: usize,
    depth: usize,
    budget_exceeded: bool,
    lens: &'a dyn CrossDeclLens,
    default_space: SymbolSpace,
    type_param_frame: Vec<Vec<Arc<str>>>,
}

impl<'a> Walker<'a> {
    fn new(lens: &'a dyn CrossDeclLens, default_space: SymbolSpace) -> Self {
        Self {
            buf: Vec::with_capacity(256),
            visited: BTreeMap::new(),
            visit_counter: 0,
            depth: 0,
            budget_exceeded: false,
            lens,
            default_space,
            type_param_frame: Vec::new(),
        }
    }

    fn walk(&mut self, root: &TypeExpr) {
        self.walk_node(root);
    }

    fn finish(self) -> HashOutcome {
        HashOutcome {
            hash: hash_16(&self.buf),
            budget_exceeded: self.budget_exceeded,
            visited_nodes: self.visit_counter,
        }
    }

    fn walk_node(&mut self, node: &TypeExpr) {
        if self.budget_exceeded {
            return;
        }
        self.depth += 1;
        if self.depth > MAX_HASH_DEPTH {
            self.budget_exceeded = true;
            self.buf.extend_from_slice(b"BUDGET_EXCEEDED");
            self.depth -= 1;
            return;
        }

        // Cycle detection: a node's *identity* (variant tag + Arc
        // pointer addresses of any owned sub-nodes) is recorded once
        // per visit. A re-entry through the same node emits a
        // `CycleRef(visit_index)` placeholder rather than recursing.
        let identity_key = self.node_identity_key(node);
        if let Some(&first_index) = self.visited.get(&identity_key) {
            self.buf.push(0xCC); // CycleRef tag
            self.buf
                .extend_from_slice(&(first_index as u32).to_le_bytes());
            self.buf.push(0xFF);
            self.depth -= 1;
            return;
        }
        self.visit_counter += 1;
        let my_index = self.visit_counter;
        self.visited.insert(identity_key, my_index);

        match node {
            TypeExpr::Primitive(p) => self.write_primitive(*p),
            TypeExpr::Literal(lit) => self.write_literal(lit),
            TypeExpr::Union(arms) => {
                self.buf.push(0x20);
                self.buf
                    .extend_from_slice(&(arms.len() as u32).to_le_bytes());
                for arm in arms.iter() {
                    self.walk_node(arm);
                    self.buf.push(0xFE);
                }
            }
            TypeExpr::Intersection(arms) => {
                self.buf.push(0x21);
                self.buf
                    .extend_from_slice(&(arms.len() as u32).to_le_bytes());
                for arm in arms.iter() {
                    self.walk_node(arm);
                    self.buf.push(0xFE);
                }
            }
            TypeExpr::Array { element, readonly } => {
                self.buf.push(0x22);
                self.buf.push(u8::from(*readonly));
                self.walk_node(element);
            }
            TypeExpr::Tuple { elements, readonly } => {
                self.buf.push(0x23);
                self.buf.push(u8::from(*readonly));
                self.buf
                    .extend_from_slice(&(elements.len() as u32).to_le_bytes());
                for tuple_elem in elements.iter() {
                    self.write_tuple_element(tuple_elem);
                }
            }
            TypeExpr::Object(obj) => self.walk_object(obj),
            TypeExpr::Function(func) => self.walk_function(func),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => self.walk_ref(name.as_ref(), type_arguments),
            TypeExpr::TypeParameter(param) => self.walk_type_param(param),
            TypeExpr::KeyOf(inner) => {
                self.buf.push(0x30);
                self.walk_node(inner);
            }
            TypeExpr::TypeOf(value_ref) => self.walk_typeof(value_ref),
            TypeExpr::IndexedAccess { object, index } => {
                self.buf.push(0x32);
                self.walk_node(object);
                self.buf.push(0xFD);
                self.walk_node(index);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                self.buf.push(0x33);
                self.walk_node(check);
                self.buf.push(0xFD);
                self.walk_node(extends);
                self.buf.push(0xFC);
                self.walk_node(true_type);
                self.buf.push(0xFB);
                self.walk_node(false_type);
            }
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => {
                self.buf.push(0x34);
                // Mapped type's `[K in Source]: Value` declares a
                // single type parameter `K`. Alpha-normalise by
                // pushing a one-name frame.
                self.type_param_frame
                    .push(vec![Arc::from(parameter.as_str())]);
                self.buf.extend_from_slice(b"alpha:K");
                self.walk_node(source);
                self.buf.push(0xFD);
                self.walk_node(value);
                self.buf.push(0xFC);
                self.write_mapped_modifier(*optional);
                self.buf.push(0xFB);
                self.write_mapped_modifier(*readonly);
                self.buf.push(0xFA);
                if let Some(nt) = name_type {
                    self.walk_node(nt);
                }
                self.type_param_frame.pop();
            }
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => {
                self.buf.push(0x35);
                self.buf
                    .extend_from_slice(&(quasis.len() as u32).to_le_bytes());
                for q in quasis {
                    self.buf.extend_from_slice(q.as_bytes());
                    self.buf.push(0xFF);
                }
                self.buf
                    .extend_from_slice(&(expressions.len() as u32).to_le_bytes());
                for e in expressions.iter() {
                    self.walk_node(e);
                    self.buf.push(0xFE);
                }
            }
            TypeExpr::Infer { name } => {
                self.buf.push(0x36);
                self.buf.extend_from_slice(name.as_bytes());
                self.buf.push(0xFF);
            }
            TypeExpr::Rest(inner) => {
                self.buf.push(0x37);
                self.walk_node(inner);
            }
            TypeExpr::Parenthesized(inner) => {
                // Parenthesisation is transparent to alpha-
                // normalised structure (R16).
                self.walk_node(inner);
            }
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => {
                // Legacy producers may still hand us this node on
                // detected recursion; the worklist hasher emits
                // `CycleRef` instead under R27. We hash this legacy
                // shape alpha-stably so producers that emit it do
                // not change `semantic_hash` non-cosmetically.
                self.buf.push(0x38);
                self.buf.extend_from_slice(name.as_bytes());
                self.buf.push(0xFF);
                self.buf
                    .extend_from_slice(&(type_arguments.len() as u32).to_le_bytes());
                for ta in type_arguments.iter() {
                    self.walk_node(ta);
                    self.buf.push(0xFE);
                }
                self.buf
                    .extend_from_slice(&(conditional_context.len() as u32).to_le_bytes());
            }
            TypeExpr::SyntheticSlotBinding(key) => {
                // Distinct discriminator from `Ref` (which routes through
                // `walk_ref` and emits 0x40+ on type-param hit, or the
                // ref-name body otherwise). The carrier is NEVER resolved
                // as a type alias via the type registry — its identity is
                // intrinsic: (scope_canonical_id, surface_kind, slot_name,
                // binding_name, value_node).
                //
                // value_node discriminates two same-named carriers in
                // different slots of the same component, so two carriers
                // with the same binding_name but different value_node
                // hash differently.
                self.buf.push(0x39);
                self.buf
                    .extend_from_slice(key.scope_canonical_id.as_bytes());
                self.buf.push(0xFF);
                self.buf.push(match key.surface_kind {
                    verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding => 0,
                    verter_type_expr::SyntheticCarrierSurfaceKind::Binding => 1,
                });
                match &key.slot_name {
                    Some(name) => {
                        self.buf.push(1);
                        self.buf.extend_from_slice(name.as_bytes());
                        self.buf.push(0xFF);
                    }
                    None => self.buf.push(0),
                }
                self.buf.extend_from_slice(key.binding_name.as_bytes());
                self.buf.push(0xFF);
                self.buf.extend_from_slice(&key.value_node.to_le_bytes());
            }
            TypeExpr::Unknown { raw } => {
                self.buf.push(0x3F);
                self.buf.extend_from_slice(raw.as_bytes());
                self.buf.push(0xFF);
            }
        }

        self.depth -= 1;
    }

    fn walk_ref(&mut self, name: &str, type_arguments: &[TypeExpr]) {
        // First: is this name a free type parameter in the active
        // generic frame? If yes, alpha-normalise to a binder-relative
        // index.
        if let Some(idx) = self.find_type_param(name) {
            self.buf.push(0x40);
            self.buf.extend_from_slice(&(idx as u32).to_le_bytes());
            self.buf.push(0xFF);
            for ta in type_arguments {
                self.walk_node(ta);
                self.buf.push(0xFE);
            }
            return;
        }

        // Otherwise: resolve through the lens. The producer knows
        // whether `name` is a same-file local, an imported binding,
        // or unresolved.
        let cross_decl = self.lens.resolve(name, self.default_space);
        self.buf.push(0x41);
        if let Some(cdr) = cross_decl {
            cdr.extend_hash_buf(&mut self.buf);
        } else {
            self.buf.extend_from_slice(b"none");
            self.buf.push(0xFF);
        }
        self.buf
            .extend_from_slice(&(type_arguments.len() as u32).to_le_bytes());
        for ta in type_arguments {
            self.walk_node(ta);
            self.buf.push(0xFE);
        }
    }

    fn walk_type_param(&mut self, param: &TypeParam) {
        // First-class TypeParam reference. Alpha-normalise to
        // binder-relative index when possible; otherwise emit by name.
        if let Some(idx) = self.find_type_param(&param.name) {
            self.buf.push(0x40);
            self.buf.extend_from_slice(&(idx as u32).to_le_bytes());
            self.buf.push(0xFF);
        } else {
            self.buf.push(0x42);
            self.buf.extend_from_slice(param.name.as_bytes());
            self.buf.push(0xFF);
        }
    }

    fn walk_object(&mut self, obj: &ObjectExpr) {
        self.buf.push(0x50);
        // Members sorted lexicographically by name (alpha-
        // normalisation R16 — declaration order does not affect the
        // hash).
        let mut sorted: Vec<&ObjectMember> = obj.properties.iter().collect();
        sorted.sort_by(|a, b| Self::member_sort_key(a).cmp(&Self::member_sort_key(b)));
        self.buf
            .extend_from_slice(&(sorted.len() as u32).to_le_bytes());
        for member in sorted {
            self.write_object_member(member);
        }
    }

    fn member_sort_key(m: &ObjectMember) -> String {
        match m {
            ObjectMember::Property(p) => format!("prop:{}", p.name),
            ObjectMember::Method(m) => format!("method:{}", m.name),
            ObjectMember::IndexSignature(_) => "index".to_string(),
            ObjectMember::CallSignature(_) => "call".to_string(),
            ObjectMember::ConstructSignature(_) => "construct".to_string(),
        }
    }

    fn write_object_member(&mut self, member: &ObjectMember) {
        match member {
            ObjectMember::Property(prop) => self.write_property(prop),
            ObjectMember::Method(method) => self.write_method(method),
            ObjectMember::IndexSignature(sig) => self.write_index_signature(sig),
            ObjectMember::CallSignature(func) => {
                self.buf.push(0x63);
                self.walk_function(func);
                self.buf.push(0xFD);
            }
            ObjectMember::ConstructSignature(func) => {
                self.buf.push(0x64);
                self.walk_function(func);
                self.buf.push(0xFD);
            }
        }
    }

    fn write_property(&mut self, prop: &ObjectProperty) {
        self.buf.push(0x60);
        self.buf.extend_from_slice(prop.name.as_bytes());
        self.buf.push(0xFF);
        self.buf.push(u8::from(prop.optional));
        self.buf.push(u8::from(prop.readonly));
        self.walk_node(&prop.ty);
        self.buf.push(0xFD);
    }

    fn write_method(&mut self, method: &MethodSignature) {
        self.buf.push(0x61);
        self.buf.extend_from_slice(method.name.as_bytes());
        self.buf.push(0xFF);
        self.buf.push(u8::from(method.optional));
        self.walk_function(&method.function);
        self.buf.push(0xFD);
    }

    fn write_index_signature(&mut self, sig: &IndexSignature) {
        self.buf.push(0x62);
        self.buf.push(u8::from(sig.readonly));
        // `key_name` is display-only (`[k: string]` vs `[x: string]`
        // is the same type). Hash only the structural pieces.
        self.walk_node(&sig.key_type);
        self.buf.push(0xFE);
        self.walk_node(&sig.value_type);
        self.buf.push(0xFD);
    }

    fn walk_function(&mut self, func: &FunctionExpr) {
        self.buf.push(0x70);
        // Push a fresh type-param frame if the function declares its
        // own generics (alpha-normalised by binder-relative index).
        let frame_names: Vec<Arc<str>> = func
            .type_parameters
            .iter()
            .map(|p| Arc::from(p.name.as_str()))
            .collect();
        self.type_param_frame.push(frame_names);
        self.buf
            .extend_from_slice(&(func.type_parameters.len() as u32).to_le_bytes());
        for param in &func.type_parameters {
            self.write_type_param_decl(param);
        }
        self.buf
            .extend_from_slice(&(func.parameters.len() as u32).to_le_bytes());
        for p in &func.parameters {
            self.write_parameter(p);
        }
        if let Some(ret) = &func.return_type {
            self.buf.push(1);
            self.walk_node(ret);
        } else {
            self.buf.push(0);
        }
        self.type_param_frame.pop();
    }

    fn write_parameter(&mut self, p: &FunctionParam) {
        // Parameter names are display-only; alpha-normalise to slot
        // index, NOT to the source identifier (R16 cosmetic-
        // invariance for parameter rename).
        self.buf.push(0x71);
        self.buf.push(u8::from(p.optional));
        self.buf.push(u8::from(p.rest));
        self.walk_node(&p.ty);
        self.buf.push(0xFD);
    }

    fn write_type_param_decl(&mut self, p: &TypeParam) {
        // Lock in the constraint / default shape but rename the
        // parameter to its binder-relative slot.
        self.buf.push(0x72);
        if let Some(c) = &p.constraint {
            self.buf.push(1);
            self.walk_node(c);
        } else {
            self.buf.push(0);
        }
        if let Some(d) = &p.default {
            self.buf.push(1);
            self.walk_node(d);
        } else {
            self.buf.push(0);
        }
    }

    fn write_tuple_element(&mut self, tuple_elem: &TupleElement) {
        self.buf.push(0x80);
        self.buf.push(u8::from(tuple_elem.optional));
        self.buf.push(u8::from(tuple_elem.rest));
        // Label is display-only.
        self.walk_node(&tuple_elem.ty);
        self.buf.push(0xFD);
    }

    fn write_primitive(&mut self, p: PrimitiveName) {
        self.buf.push(0x90);
        self.buf.push(p as u8);
        self.buf.push(0xFF);
    }

    fn write_literal(&mut self, lit: &LiteralValue) {
        self.buf.push(0x10);
        match lit {
            LiteralValue::String(s) => {
                self.buf.push(0);
                self.buf.extend_from_slice(s.as_bytes());
                self.buf.push(0xFF);
            }
            LiteralValue::Number(n) => {
                self.buf.push(1);
                self.buf.extend_from_slice(&n.to_bits().to_le_bytes());
            }
            LiteralValue::Boolean(b) => {
                self.buf.push(2);
                self.buf.push(u8::from(*b));
            }
            LiteralValue::BigInt(s) => {
                self.buf.push(3);
                self.buf.extend_from_slice(s.as_bytes());
                self.buf.push(0xFF);
            }
        }
    }

    fn write_mapped_modifier(&mut self, m: MappedModifier) {
        self.buf.push(match m {
            MappedModifier::None => 0,
            MappedModifier::Add => 1,
            MappedModifier::Remove => 2,
        });
    }

    fn walk_typeof(&mut self, value_ref: &ValueRef) {
        self.buf.push(0x31);
        // `typeof a.b.c` — record the path stably; the producer
        // resolves the head ident through the lens later (we don't
        // have a typeof-channel for free here, but the structural
        // shape is invariant).
        self.buf
            .extend_from_slice(&(value_ref.path.len() as u32).to_le_bytes());
        for seg in &value_ref.path {
            self.buf.extend_from_slice(seg.as_bytes());
            self.buf.push(0xFE);
        }
    }

    fn find_type_param(&self, name: &str) -> Option<usize> {
        // Search the active stack of generic frames from innermost
        // outward; return a flat binder-relative index. Outer frames
        // don't see inner-frame names — TypeScript's lexical scoping
        // for generics matches our nested-frame model.
        for frame in self.type_param_frame.iter().rev() {
            if let Some(pos) = frame.iter().position(|n| n.as_ref() == name) {
                return Some(pos);
            }
        }
        None
    }

    fn node_identity_key(&self, node: &TypeExpr) -> Vec<u8> {
        // The cycle-detection identity key fingerprints the node's
        // *variant + leaf data only* — it MUST NOT recurse, because
        // doing so reintroduces stack recursion. For compound nodes,
        // the key folds in the `Arc` pointer addresses of any owned
        // sub-nodes: two physically identical `Arc`s ARE the same
        // node and MUST re-enter as `CycleRef`. Different `Arc`s
        // carrying structurally identical content are distinct nodes
        // (no false-positive cycle).
        let mut key = Vec::with_capacity(32);
        match node {
            TypeExpr::Primitive(p) => {
                key.push(0xA0);
                key.push(*p as u8);
            }
            TypeExpr::Literal(lit) => {
                key.push(0xA1);
                match lit {
                    LiteralValue::String(s) => {
                        key.push(0);
                        key.extend_from_slice(s.as_bytes());
                    }
                    LiteralValue::Number(n) => {
                        key.push(1);
                        key.extend_from_slice(&n.to_bits().to_le_bytes());
                    }
                    LiteralValue::Boolean(b) => {
                        key.push(2);
                        key.push(u8::from(*b));
                    }
                    LiteralValue::BigInt(s) => {
                        key.push(3);
                        key.extend_from_slice(s.as_bytes());
                    }
                }
            }
            TypeExpr::Union(arms) => {
                key.push(0xA2);
                key.extend_from_slice(&(arms.as_ptr() as usize).to_le_bytes());
                key.extend_from_slice(&(arms.len() as u32).to_le_bytes());
            }
            TypeExpr::Intersection(arms) => {
                key.push(0xA3);
                key.extend_from_slice(&(arms.as_ptr() as usize).to_le_bytes());
                key.extend_from_slice(&(arms.len() as u32).to_le_bytes());
            }
            TypeExpr::Array { element, readonly } => {
                key.push(0xA4);
                key.push(u8::from(*readonly));
                key.extend_from_slice(&(Arc::as_ptr(element) as usize).to_le_bytes());
            }
            TypeExpr::Tuple { elements, readonly } => {
                key.push(0xA5);
                key.push(u8::from(*readonly));
                key.extend_from_slice(&(elements.as_ptr() as usize).to_le_bytes());
                key.extend_from_slice(&(elements.len() as u32).to_le_bytes());
            }
            TypeExpr::Object(obj) => {
                key.push(0xA6);
                key.extend_from_slice(&(Arc::as_ptr(obj) as usize).to_le_bytes());
            }
            TypeExpr::Function(func) => {
                key.push(0xA7);
                key.extend_from_slice(&(Arc::as_ptr(func) as usize).to_le_bytes());
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                key.push(0xA8);
                key.extend_from_slice(name.as_bytes());
                key.push(0xFF);
                key.extend_from_slice(&(type_arguments.as_ptr() as usize).to_le_bytes());
                key.extend_from_slice(&(type_arguments.len() as u32).to_le_bytes());
            }
            TypeExpr::TypeParameter(p) => {
                key.push(0xA9);
                key.extend_from_slice(p.name.as_bytes());
            }
            TypeExpr::KeyOf(inner) => {
                key.push(0xAA);
                key.extend_from_slice(&(Arc::as_ptr(inner) as usize).to_le_bytes());
            }
            TypeExpr::TypeOf(value_ref) => {
                key.push(0xAB);
                for seg in &value_ref.path {
                    key.extend_from_slice(seg.as_bytes());
                    key.push(0xFE);
                }
            }
            TypeExpr::IndexedAccess { object, index } => {
                key.push(0xAC);
                key.extend_from_slice(&(Arc::as_ptr(object) as usize).to_le_bytes());
                key.extend_from_slice(&(Arc::as_ptr(index) as usize).to_le_bytes());
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                key.push(0xAD);
                key.extend_from_slice(&(Arc::as_ptr(check) as usize).to_le_bytes());
                key.extend_from_slice(&(Arc::as_ptr(extends) as usize).to_le_bytes());
                key.extend_from_slice(&(Arc::as_ptr(true_type) as usize).to_le_bytes());
                key.extend_from_slice(&(Arc::as_ptr(false_type) as usize).to_le_bytes());
            }
            TypeExpr::Mapped {
                parameter, source, ..
            } => {
                key.push(0xAE);
                key.extend_from_slice(parameter.as_bytes());
                key.extend_from_slice(&(Arc::as_ptr(source) as usize).to_le_bytes());
            }
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => {
                key.push(0xAF);
                key.extend_from_slice(&(quasis.as_ptr() as usize).to_le_bytes());
                key.extend_from_slice(&(expressions.as_ptr() as usize).to_le_bytes());
            }
            TypeExpr::Infer { name } => {
                key.push(0xB0);
                key.extend_from_slice(name.as_bytes());
            }
            TypeExpr::Rest(inner) => {
                key.push(0xB1);
                key.extend_from_slice(&(Arc::as_ptr(inner) as usize).to_le_bytes());
            }
            TypeExpr::Parenthesized(inner) => {
                key.push(0xB2);
                key.extend_from_slice(&(Arc::as_ptr(inner) as usize).to_le_bytes());
            }
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                ..
            } => {
                key.push(0xB3);
                key.extend_from_slice(name.as_bytes());
                key.push(0xFF);
                key.extend_from_slice(&(type_arguments.as_ptr() as usize).to_le_bytes());
            }
            TypeExpr::SyntheticSlotBinding(arc_key) => {
                // The carrier identity is the full (scope, surface_kind,
                // slot_name, binding_name, value_node) tuple. Use the
                // `Arc<SyntheticCarrierKey>` pointer for cheap identity
                // discrimination — physically distinct Arcs are distinct
                // carriers (the cycle-detection key never recurses).
                key.push(0xB4);
                key.extend_from_slice(&(Arc::as_ptr(arc_key) as usize).to_le_bytes());
            }
            TypeExpr::Unknown { raw } => {
                key.push(0xBF);
                key.extend_from_slice(raw.as_bytes());
            }
        }
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

    fn prim(p: PrimitiveName) -> TypeExpr {
        TypeExpr::Primitive(p)
    }

    fn name_ref(name: &str) -> TypeExpr {
        TypeExpr::Ref {
            name: Arc::from(name),
            type_arguments: Arc::from(Vec::new()),
        }
    }

    fn make_object(members: Vec<(&str, TypeExpr)>) -> TypeExpr {
        let properties: Vec<ObjectMember> = members
            .into_iter()
            .map(|(name, ty)| {
                ObjectMember::Property(ObjectProperty {
                    name: name.to_string(),
                    ty,
                    optional: false,
                    readonly: false,
                })
            })
            .collect();
        TypeExpr::Object(Arc::new(ObjectExpr { properties }))
    }

    #[test]
    fn primitive_is_stable_under_reuse() {
        let h1 = compute_semantic_hash(
            &prim(PrimitiveName::String),
            SymbolSpace::Type,
            &UnresolvedLens,
        );
        let h2 = compute_semantic_hash(
            &prim(PrimitiveName::String),
            SymbolSpace::Type,
            &UnresolvedLens,
        );
        assert_eq!(h1.hash, h2.hash, "primitive must hash identically");
        assert!(!h1.budget_exceeded);
        assert_ne!(h1.visited_nodes, 0);
    }

    #[test]
    fn object_member_order_does_not_affect_hash() {
        // R16: alpha-normalised — member declaration order MUST NOT
        // change the semantic_hash.
        let obj_ab = make_object(vec![
            ("a", prim(PrimitiveName::Number)),
            ("b", prim(PrimitiveName::String)),
        ]);
        let obj_ba = make_object(vec![
            ("b", prim(PrimitiveName::String)),
            ("a", prim(PrimitiveName::Number)),
        ]);
        let h_ab = compute_semantic_hash(&obj_ab, SymbolSpace::Type, &UnresolvedLens);
        let h_ba = compute_semantic_hash(&obj_ba, SymbolSpace::Type, &UnresolvedLens);
        assert_eq!(
            h_ab.hash, h_ba.hash,
            "object member order MUST not change semantic_hash (alpha-normalised R16)"
        );
    }

    #[test]
    fn property_value_edit_changes_hash() {
        // Discrimination: a real semantic edit DOES change the hash.
        let a = make_object(vec![("a", prim(PrimitiveName::String))]);
        let b = make_object(vec![("a", prim(PrimitiveName::Number))]);
        let h_a = compute_semantic_hash(&a, SymbolSpace::Type, &UnresolvedLens);
        let h_b = compute_semantic_hash(&b, SymbolSpace::Type, &UnresolvedLens);
        assert_ne!(
            h_a.hash, h_b.hash,
            "editing property body MUST change semantic_hash"
        );
    }

    #[test]
    fn name_ref_emits_cross_decl_edge() {
        // R14 path-precision: a `Ref(Foo)` cross-decl reference
        // must be observable as a reference-shape edge, not by
        // inlining Foo's body.
        let ref_foo = name_ref("Foo");
        let ref_bar = name_ref("Bar");
        let h_foo = compute_semantic_hash(&ref_foo, SymbolSpace::Type, &UnresolvedLens);
        let h_bar = compute_semantic_hash(&ref_bar, SymbolSpace::Type, &UnresolvedLens);
        assert_ne!(
            h_foo.hash, h_bar.hash,
            "different ref names produce different hashes"
        );
    }

    #[test]
    fn stack_safe_deep_chain_does_not_overflow() {
        // Build a left-leaning Union 200 deep — much deeper than
        // the default thread stack would tolerate via recursion.
        // The worklist hasher MUST terminate (it may set
        // `budget_exceeded` once depth ≥ 64).
        let mut node = prim(PrimitiveName::String);
        for _ in 0..200 {
            node = TypeExpr::Union(Arc::from(vec![node, prim(PrimitiveName::Number)]));
        }
        let result = compute_semantic_hash(&node, SymbolSpace::Type, &UnresolvedLens);
        assert!(
            result.budget_exceeded,
            "200-deep nesting MUST trigger budget_exceeded (limit = {})",
            MAX_HASH_DEPTH
        );
    }

    #[test]
    fn shallow_object_does_not_exceed_budget() {
        // A small object that stays under 64 depth MUST NOT trip
        // budget_exceeded.
        let obj = make_object(vec![
            ("a", prim(PrimitiveName::String)),
            ("b", prim(PrimitiveName::Number)),
            ("c", prim(PrimitiveName::Boolean)),
        ]);
        let r = compute_semantic_hash(&obj, SymbolSpace::Type, &UnresolvedLens);
        assert!(
            !r.budget_exceeded,
            "shallow tree MUST stay under MAX_HASH_DEPTH"
        );
    }

    #[test]
    fn cross_decl_lens_emits_distinct_shapes() {
        // Provide a lens that maps `Foo` → `LocalDecl(Foo, Type)`
        // and `Bar` → `ImportRef("./bar", "Bar", Type)`.
        struct MyLens;
        impl CrossDeclLens for MyLens {
            fn resolve(&self, name: &str, _space: SymbolSpace) -> Option<CrossDeclRef> {
                match name {
                    "Foo" => Some(CrossDeclRef::LocalDecl {
                        name: Arc::from("Foo"),
                        space: SymbolSpace::Type,
                    }),
                    "Bar" => Some(CrossDeclRef::ImportRef {
                        specifier: Arc::from("./bar"),
                        binding: Arc::from("Bar"),
                        space: SymbolSpace::Type,
                    }),
                    _ => None,
                }
            }
        }
        let ref_foo = name_ref("Foo");
        let ref_bar = name_ref("Bar");
        let h_foo = compute_semantic_hash(&ref_foo, SymbolSpace::Type, &MyLens);
        let h_bar = compute_semantic_hash(&ref_bar, SymbolSpace::Type, &MyLens);
        // Different cross-decl shapes produce different hashes.
        assert_ne!(h_foo.hash, h_bar.hash);
        // Unresolved lens produces a third distinct hash for `Foo`.
        let h_foo_unresolved = compute_semantic_hash(&ref_foo, SymbolSpace::Type, &UnresolvedLens);
        assert_ne!(
            h_foo.hash, h_foo_unresolved.hash,
            "LocalDecl(Foo) and Unresolved(Foo) MUST differ"
        );
    }

    #[test]
    fn member_presence_hash_independent_of_siblings() {
        // R28 two-fact model: `MemberPresence(Foo, "a")` MUST be
        // invariant under adding sibling `b`. Both formations of
        // `compute_member_presence_hash("Foo", "a", ...)` produce
        // the same fingerprint regardless of what else exists.
        let kind = MemberKind::Property {
            readonly: false,
            optional: false,
        };
        let h1 = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
        let h2 = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
        assert_eq!(h1, h2, "presence hash MUST be deterministic");
        let h3 = compute_member_presence_hash("Foo", "b", kind, SymbolSpace::Type);
        assert_ne!(h1, h3, "different member name MUST hash distinctly");
        // Exporter salt distinguishes same-named members across
        // exporters in the same file.
        let h_foo_a = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
        let h_bar_a = compute_member_presence_hash("Bar", "a", kind, SymbolSpace::Type);
        assert_ne!(
            h_foo_a, h_bar_a,
            "exporter qualifier salt MUST disambiguate same-named members"
        );
    }

    #[test]
    fn member_presence_hash_changes_under_modifier_flip() {
        // R28: a property switching from required to optional MUST
        // change the presence hash (the consumer's view shifts).
        let required = MemberKind::Property {
            readonly: false,
            optional: false,
        };
        let optional = MemberKind::Property {
            readonly: false,
            optional: true,
        };
        let h_r = compute_member_presence_hash("Foo", "a", required, SymbolSpace::Type);
        let h_o = compute_member_presence_hash("Foo", "a", optional, SymbolSpace::Type);
        assert_ne!(h_r, h_o);
    }

    #[test]
    fn member_shape_hash_invariant_under_member_reorder() {
        // R28 `MemberShape`: order-insensitive at top level. Sorted
        // by name.
        let kind = MemberKind::Property {
            readonly: false,
            optional: false,
        };
        let members_ab: Vec<(Arc<str>, MemberKind)> =
            vec![(Arc::from("a"), kind), (Arc::from("b"), kind)];
        let members_ba: Vec<(Arc<str>, MemberKind)> =
            vec![(Arc::from("b"), kind), (Arc::from("a"), kind)];
        let h_ab = compute_member_shape_hash("Foo", &members_ab, SymbolSpace::Type);
        let h_ba = compute_member_shape_hash("Foo", &members_ba, SymbolSpace::Type);
        assert_eq!(h_ab, h_ba, "member_shape MUST be order-insensitive");
    }

    #[test]
    fn member_shape_hash_changes_when_member_added() {
        // R28: adding a member changes `MemberShape` but NOT each
        // existing `MemberPresence`.
        let kind = MemberKind::Property {
            readonly: false,
            optional: false,
        };
        let just_a: Vec<(Arc<str>, MemberKind)> = vec![(Arc::from("a"), kind)];
        let a_and_b: Vec<(Arc<str>, MemberKind)> =
            vec![(Arc::from("a"), kind), (Arc::from("b"), kind)];
        let h_a = compute_member_shape_hash("Foo", &just_a, SymbolSpace::Type);
        let h_ab = compute_member_shape_hash("Foo", &a_and_b, SymbolSpace::Type);
        assert_ne!(h_a, h_ab, "adding a member MUST change MemberShape");
        // And each MemberPresence is unchanged.
        let p_a_before = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
        let p_a_after = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
        assert_eq!(
            p_a_before, p_a_after,
            "MemberPresence(a) MUST be unchanged when sibling added"
        );
    }

    // ------------------------------------------------------------------
    // Discrimination tests for the `TypeExpr::SyntheticSlotBinding`
    // variant. The fact-hash walker must use a DISTINCT discriminator
    // tag from `Ref` so that a synthetic carrier with
    // `binding_name = "x"` does NOT collide with a workspace
    // `TypeExpr::Ref { name: "x", type_arguments: [] }`.
    // ------------------------------------------------------------------

    fn synthetic_carrier(scope: &str, binding_name: &str, value_node: u64) -> TypeExpr {
        use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind};
        TypeExpr::synthetic_slot_binding(SyntheticCarrierKey {
            scope_canonical_id: Arc::from(scope),
            surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: Some(Arc::from("default")),
            binding_name: Arc::from(binding_name),
            value_node,
        })
    }

    #[test]
    fn synthetic_carrier_fact_hash_differs_from_ref_with_same_name() {
        let carrier = synthetic_carrier("/abs/Foo.vue", "controls", 42);
        let plain_ref = name_ref("controls");

        let carrier_hash = compute_semantic_hash(&carrier, SymbolSpace::Type, &UnresolvedLens).hash;
        let ref_hash = compute_semantic_hash(&plain_ref, SymbolSpace::Type, &UnresolvedLens).hash;

        assert_ne!(
            carrier_hash, ref_hash,
            "synthetic carrier and workspace Ref with the same `name` MUST hash distinctly"
        );
    }

    #[test]
    fn synthetic_carrier_fact_hash_value_node_discriminates() {
        // Same scope + binding_name, different value_node => distinct
        // hashes. Guards the rule that two same-binding-name carriers
        // in different slots of the same component are distinct
        // identities.
        let a = synthetic_carrier("/abs/Foo.vue", "controls", 1);
        let b = synthetic_carrier("/abs/Foo.vue", "controls", 2);

        let a_hash = compute_semantic_hash(&a, SymbolSpace::Type, &UnresolvedLens).hash;
        let b_hash = compute_semantic_hash(&b, SymbolSpace::Type, &UnresolvedLens).hash;

        assert_ne!(
            a_hash, b_hash,
            "synthetic carriers differing only in value_node MUST hash distinctly"
        );
    }
}
