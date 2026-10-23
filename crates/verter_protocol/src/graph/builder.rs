use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::Arc;

use hashbrown::HashTable;

use verter_type_expr::{
    AuthoredPropertyKey, FunctionExpr, FunctionParam, IndexSignature, LiteralValue, MappedModifier,
    MethodSignature, ObjectMember, ObjectProperty, TupleElement, TypeAuthoredPropertyKey, TypeExpr,
    TypeParam, ValueRef,
};

use crate::graph::schema;
use crate::graph::snapshot::PersistedGraphNode;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum GraphNode {
    Primitive {
        primitive: u32,
    },
    LiteralString {
        value: u32,
    },
    LiteralNumber {
        bits: u64,
    },
    LiteralBoolean {
        value: bool,
    },
    LiteralBigInt {
        value: u32,
    },
    Union {
        types: Vec<u32>,
    },
    Intersection {
        types: Vec<u32>,
    },
    Array {
        element: u32,
        readonly: bool,
    },
    Tuple {
        readonly: bool,
        elements: Vec<GraphTupleElement>,
    },
    Object {
        members: Vec<GraphObjectMember>,
    },
    Function {
        parameters: Vec<GraphFunctionParam>,
        return_type: u32,
        type_parameters: Vec<u32>,
    },
    Ref {
        name: u32,
        type_arguments: Vec<u32>,
    },
    /// An applied compiler intrinsic. `op` is the shared op code (0 =
    /// unspecified, never emitted; 1 = Awaited), NOT a string id — an
    /// intrinsic names no declaration and must never reach the string
    /// table or be resolvable as a name.
    IntrinsicApplication {
        op: u32,
        arguments: Vec<u32>,
    },
    TypeParameter {
        name: u32,
        constraint: u32,
        default: u32,
    },
    KeyOf {
        operand: u32,
    },
    TypeOf {
        path: Vec<u32>,
    },
    IndexedAccess {
        object: u32,
        index: u32,
    },
    Conditional {
        check: u32,
        extends: u32,
        true_type: u32,
        false_type: u32,
    },
    Mapped {
        parameter: u32,
        source: u32,
        value: u32,
        optional: u32,
        readonly: u32,
        name_type: u32,
    },
    TemplateLiteral {
        quasis: Vec<u32>,
        expressions: Vec<u32>,
    },
    Parenthesized {
        inner: u32,
    },
    Unknown {
        raw: u32,
    },
    Infer {
        name: u32,
        constraint: u32,
    },
    Rest {
        inner: u32,
    },
    RecursiveRef {
        name: u32,
        type_arguments: Vec<u32>,
        conditional_context: Vec<GraphConditionalFrame>,
    },
    /// Synthetic slot-binding / `defineSlots` binding carrier — leaf node
    /// minted by `publish_merged_bindings`. Carrier identity is the full
    /// `(scope, surface_kind, slot_name, binding_name, value_node)` tuple.
    /// Consumers MUST NOT resolve `binding_name_id` as a type alias via
    /// `TypeRegistry` — this is a closed terminal.
    SyntheticSlotBinding {
        /// Semantic-node id of the binding-value node; serialised to FFI as
        /// a decimal STRING to avoid JS Number precision loss.
        value_node: u64,
        /// String-table id of the scope's canonical file id.
        scope_canonical_id_id: u32,
        /// 0 = `SlotBinding`, 1 = `Binding` (matches TS
        /// `SYNTHETIC_CARRIER_SURFACE_*` constants).
        surface_kind: u32,
        /// String-table id of the slot name; 0 = absent.
        slot_name_id: u32,
        /// String-table id of the binding name (always present).
        binding_name_id: u32,
    },
}

/// A conditional branch frame in the graph transport.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct GraphConditionalFrame {
    /// 1 = true, 2 = false
    pub branch: u32,
    pub decided: bool,
    pub check: u32,
    pub extends: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct GraphTupleElement {
    pub label: u32,
    pub ty: u32,
    pub optional: bool,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct GraphObjectMember {
    pub kind: u32,
    pub key: Option<GraphPropertyKey>,
    pub ty: u32,
    pub optional: bool,
    pub readonly: bool,
    pub key_name: u32,
    pub key_type: u32,
    pub value_type: u32,
    pub function: u32,
    pub method_kind: u32,
    pub has_implementation_body: bool,
}

/// Lossless graph-wire property-key oneof.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GraphPropertyKey {
    String {
        value: u32,
    },
    Number {
        value: i64,
    },
    UniqueSymbol {
        canonical_id: u32,
        owner_kind: u32,
        owner_ordinal: u32,
        symbol: u32,
        member_path: Vec<u32>,
    },
    Computed {
        node: u32,
    },
}

/// Whether an object member is publicly visible for the graph wire. Property
/// and Method members carry a `MemberVisibility`; an index / call / construct
/// signature has no accessibility concept and is always kept. The graph wire
/// is a public surface with no member-visibility field, so non-public
/// Property / Method members are filtered out before object-node encoding.
fn object_member_is_public(member: &ObjectMember) -> bool {
    match member {
        ObjectMember::Property(property) => property.visibility.is_public(),
        ObjectMember::Method(method) => method.visibility.is_public(),
        ObjectMember::IndexSignature(_)
        | ObjectMember::CallSignature(_)
        | ObjectMember::ConstructSignature(_)
        | ObjectMember::Spread(_) => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct GraphFunctionParam {
    pub name: u32,
    pub ty: u32,
    pub optional: bool,
    pub rest: bool,
}

/// An `Arc` held in a memo key, hashed and compared by the address of its
/// allocation rather than by its contents.
///
/// The key owns the `Arc`, so the allocation it names stays alive for as long
/// as the key exists: the address can never be freed and handed to another
/// value that would then compare equal to this key.
struct ArcAddr<T: ?Sized>(Arc<T>);

impl<T: ?Sized> ArcAddr<T> {
    fn of(value: &Arc<T>) -> Self {
        Self(Arc::clone(value))
    }

    fn addr(&self) -> usize {
        arc_addr(&self.0)
    }
}

impl<T: ?Sized> PartialEq for ArcAddr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.addr() == other.addr()
    }
}

impl<T: ?Sized> Eq for ArcAddr<T> {}

impl<T: ?Sized> std::hash::Hash for ArcAddr<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.addr().hash(state);
    }
}

impl<T: ?Sized> std::fmt::Debug for ArcAddr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ArcAddr({:#x})", self.addr())
    }
}

/// The address of an `Arc`'s allocation (the data pointer, without slice
/// metadata).
fn arc_addr<T: ?Sized>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value).cast::<()>() as usize
}

/// The full memo identity of a `TypeExpr`. Shared sub-expressions are named
/// by their allocation ([`ArcAddr`], which the key owns); names, literals and
/// other owned data compare by content.
#[derive(Debug, PartialEq, Eq, Hash)]
enum ExprMemoKey {
    Primitive(verter_type_expr::PrimitiveName),
    Literal(verter_type_expr::LiteralValue),
    Union(ArcAddr<[TypeExpr]>),
    Intersection(ArcAddr<[TypeExpr]>),
    Array {
        element: ArcAddr<TypeExpr>,
        readonly: bool,
    },
    Tuple {
        elements: ArcAddr<[TupleElement]>,
        readonly: bool,
    },
    Object(ArcAddr<verter_type_expr::ObjectExpr>),
    Function(ArcAddr<FunctionExpr>),
    /// Constructor type `new (...) => R`. A distinct memo variant from
    /// [`Function`](Self::Function): a constructor type and a function type are
    /// different types and must not share a memo entry even if they ever held
    /// the same `Arc<FunctionExpr>`.
    ConstructorType(ArcAddr<FunctionExpr>),
    Ref {
        name: Arc<str>,
        type_arguments: ArcAddr<[TypeExpr]>,
    },
    TypeParameter(verter_type_expr::TypeParam),
    KeyOf(ArcAddr<TypeExpr>),
    TypeOf(verter_type_expr::ValueRef),
    IndexedAccess {
        object: ArcAddr<TypeExpr>,
        index: ArcAddr<TypeExpr>,
    },
    Conditional {
        check: ArcAddr<TypeExpr>,
        extends: ArcAddr<TypeExpr>,
        true_type: ArcAddr<TypeExpr>,
        false_type: ArcAddr<TypeExpr>,
    },
    Mapped {
        parameter: String,
        source: ArcAddr<TypeExpr>,
        value: ArcAddr<TypeExpr>,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<ArcAddr<TypeExpr>>,
    },
    TemplateLiteral {
        quasis: Vec<String>,
        expressions: ArcAddr<[TypeExpr]>,
    },
    Infer {
        name: String,
        constraint: Option<ArcAddr<TypeExpr>>,
    },
    Rest(ArcAddr<TypeExpr>),
    Parenthesized(ArcAddr<TypeExpr>),
    RecursiveRef {
        name: Arc<str>,
        type_arguments: ArcAddr<[TypeExpr]>,
        conditional_context: ArcAddr<[verter_type_expr::RecursiveConditionalFrame]>,
    },
    /// Synthetic slot-binding carrier — full structural identity. Two
    /// physically distinct `Arc<SyntheticCarrierKey>` values that hold
    /// the same logical key SHOULD share a memo entry, matching the
    /// `PartialEq + Eq + Hash` derive on `SyntheticCarrierKey`.
    SyntheticSlotBinding {
        key: Arc<verter_type_expr::SyntheticCarrierKey>,
    },
    /// Dynamic-import type carrier — identity is the specifier + ordered
    /// qualifier segments + typeof flag (all by content) plus the
    /// instantiation-argument allocation. Two import-types with a different
    /// specifier or qualifier never share a memo entry.
    ImportType {
        specifier: Arc<str>,
        qualifier: Arc<[Arc<str>]>,
        typeof_query: bool,
        type_arguments: ArcAddr<[TypeExpr]>,
    },
    IntrinsicApplication {
        op: u8,
        arguments: ArcAddr<[TypeExpr]>,
    },
    Unknown {
        raw: String,
    },
}

impl ExprMemoKey {
    fn from_expr(expr: &TypeExpr) -> Self {
        match expr {
            TypeExpr::Primitive(name) => Self::Primitive(*name),
            TypeExpr::Literal(value) => Self::Literal(value.clone()),
            TypeExpr::IntrinsicApplication { op, arguments } => Self::IntrinsicApplication {
                op: op.stable_hash_tag(),
                arguments: ArcAddr::of(arguments),
            },
            TypeExpr::Union(types) => Self::Union(ArcAddr::of(types)),
            TypeExpr::Intersection(types) => Self::Intersection(ArcAddr::of(types)),
            TypeExpr::Array { element, readonly } => Self::Array {
                element: ArcAddr::of(element),
                readonly: *readonly,
            },
            TypeExpr::Tuple { elements, readonly } => Self::Tuple {
                elements: ArcAddr::of(elements),
                readonly: *readonly,
            },
            TypeExpr::Object(object) => Self::Object(ArcAddr::of(object)),
            TypeExpr::Function(function) => Self::Function(ArcAddr::of(function)),
            TypeExpr::ConstructorType(function) => Self::ConstructorType(ArcAddr::of(function)),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => Self::Ref {
                name: Arc::clone(name),
                type_arguments: ArcAddr::of(type_arguments),
            },
            TypeExpr::TypeParameter(param) => Self::TypeParameter(param.clone()),
            TypeExpr::KeyOf(operand) => Self::KeyOf(ArcAddr::of(operand)),
            TypeExpr::TypeOf(value) => Self::TypeOf(value.clone()),
            TypeExpr::IndexedAccess { object, index } => Self::IndexedAccess {
                object: ArcAddr::of(object),
                index: ArcAddr::of(index),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => Self::Conditional {
                check: ArcAddr::of(check),
                extends: ArcAddr::of(extends),
                true_type: ArcAddr::of(true_type),
                false_type: ArcAddr::of(false_type),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => Self::Mapped {
                parameter: parameter.clone(),
                source: ArcAddr::of(source),
                value: ArcAddr::of(value),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_ref().map(ArcAddr::of),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => Self::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: ArcAddr::of(expressions),
            },
            TypeExpr::Infer { name, constraint } => Self::Infer {
                name: name.clone(),
                constraint: constraint.as_ref().map(ArcAddr::of),
            },
            TypeExpr::Rest(inner) => Self::Rest(ArcAddr::of(inner)),
            TypeExpr::Parenthesized(inner) => Self::Parenthesized(ArcAddr::of(inner)),
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => Self::RecursiveRef {
                name: Arc::clone(name),
                type_arguments: ArcAddr::of(type_arguments),
                conditional_context: ArcAddr::of(conditional_context),
            },
            TypeExpr::SyntheticSlotBinding(key) => Self::SyntheticSlotBinding {
                key: Arc::clone(key),
            },
            TypeExpr::ImportType {
                specifier,
                qualifier,
                typeof_query,
                type_arguments,
            } => Self::ImportType {
                specifier: Arc::clone(specifier),
                qualifier: Arc::clone(qualifier),
                typeof_query: *typeof_query,
                type_arguments: ArcAddr::of(type_arguments),
            },
            TypeExpr::Unknown(value) => Self::Unknown {
                raw: value.raw().to_string(),
            },
        }
    }
}

/// Deterministic hash used as the bucket key for the id-only reverse indices
/// ([`GraphBuilder::string_ids`] / [`GraphBuilder::node_ids`]) and the
/// address fast path ([`GraphBuilder::expr_ptr_ids`]). The hash only steers
/// bucketing; dedup identity is decided by the entry's equality closure, so
/// the id-assignment order is hasher-independent.
#[inline]
fn table_hash<T: std::hash::Hash + ?Sized>(value: &T) -> u64 {
    rustc_hash::FxBuildHasher.hash_one(value)
}

/// The allocation identity of a `TypeExpr` whose variant holds only `Arc`s
/// and plain scalars: the fast-path memo identity, read off a borrowed
/// expression without cloning anything.
///
/// It is never stored. The fast-path table stores the expression itself and
/// recomputes this view from it, so every address compared belongs to an
/// expression the table owns and stays allocated while the entry exists.
/// Variants that carry owned data (`Literal`, `TypeParameter`, `TypeOf`,
/// `Mapped`, `TemplateLiteral`, `Infer`, `ImportType`, `Unknown`) have no
/// view and go to the full [`ExprMemoKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AddressIdentity {
    Primitive(verter_type_expr::PrimitiveName),
    IntrinsicApplication {
        op: u8,
        arguments: usize,
    },
    Union(usize),
    Intersection(usize),
    Array {
        element: usize,
        readonly: bool,
    },
    Tuple {
        elements: usize,
        readonly: bool,
    },
    Object(usize),
    Function(usize),
    /// A constructor type never shares a fast-path entry with a function
    /// type holding the same `Arc<FunctionExpr>`.
    ConstructorType(usize),
    Ref {
        name: usize,
        type_arguments: usize,
    },
    KeyOf(usize),
    IndexedAccess {
        object: usize,
        index: usize,
    },
    Conditional {
        check: usize,
        extends: usize,
        true_type: usize,
        false_type: usize,
    },
    Rest(usize),
    Parenthesized(usize),
    RecursiveRef {
        name: usize,
        type_arguments: usize,
        conditional_context: usize,
    },
    /// Synthetic slot-binding carrier by allocation. Structurally identical
    /// carriers held in distinct `Arc`s miss this fast path and meet in the
    /// full [`ExprMemoKey`], which compares the carrier by content.
    SyntheticSlotBinding(usize),
}

impl AddressIdentity {
    fn of(expr: &TypeExpr) -> Option<Self> {
        Some(match expr {
            TypeExpr::Primitive(name) => Self::Primitive(*name),
            TypeExpr::IntrinsicApplication { op, arguments } => Self::IntrinsicApplication {
                op: op.stable_hash_tag(),
                arguments: arc_addr(arguments),
            },
            TypeExpr::Union(types) => Self::Union(arc_addr(types)),
            TypeExpr::Intersection(types) => Self::Intersection(arc_addr(types)),
            TypeExpr::Array { element, readonly } => Self::Array {
                element: arc_addr(element),
                readonly: *readonly,
            },
            TypeExpr::Tuple { elements, readonly } => Self::Tuple {
                elements: arc_addr(elements),
                readonly: *readonly,
            },
            TypeExpr::Object(object) => Self::Object(arc_addr(object)),
            TypeExpr::Function(function) => Self::Function(arc_addr(function)),
            TypeExpr::ConstructorType(function) => Self::ConstructorType(arc_addr(function)),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => Self::Ref {
                name: arc_addr(name),
                type_arguments: arc_addr(type_arguments),
            },
            TypeExpr::KeyOf(operand) => Self::KeyOf(arc_addr(operand)),
            TypeExpr::IndexedAccess { object, index } => Self::IndexedAccess {
                object: arc_addr(object),
                index: arc_addr(index),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => Self::Conditional {
                check: arc_addr(check),
                extends: arc_addr(extends),
                true_type: arc_addr(true_type),
                false_type: arc_addr(false_type),
            },
            TypeExpr::Rest(inner) => Self::Rest(arc_addr(inner)),
            TypeExpr::Parenthesized(inner) => Self::Parenthesized(arc_addr(inner)),
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => Self::RecursiveRef {
                name: arc_addr(name),
                type_arguments: arc_addr(type_arguments),
                conditional_context: arc_addr(conditional_context),
            },
            TypeExpr::SyntheticSlotBinding(key) => Self::SyntheticSlotBinding(arc_addr(key)),
            TypeExpr::Literal(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::Infer { .. }
            | TypeExpr::ImportType { .. }
            | TypeExpr::Unknown(_) => return None,
        })
    }
}

/// A fast-path memo entry: the expression (which owns every `Arc` its
/// [`AddressIdentity`] names) and its node id.
struct AddressEntry {
    expr: TypeExpr,
    id: u32,
}

impl AddressEntry {
    fn identity(&self) -> AddressIdentity {
        AddressIdentity::of(&self.expr).expect("only address-identified expressions are stored")
    }
}

#[derive(Default)]
pub struct GraphBuilder {
    /// The sole owner of every interned string (own-once). Ids are 1-based
    /// (id 0 = absent), assigned in first-encounter order.
    strings: Vec<String>,
    /// Reverse index: hash of a string -> its 1-based id in [`Self::strings`].
    /// A `HashTable<u32>` stores ONLY ids and dedups by comparing the query
    /// against `strings[id - 1]`, so it never owns a second copy of the string.
    string_ids: HashTable<u32>,
    /// The sole owner of every interned wire node (own-once). Ids are 1-based
    /// (id 0 = absent), assigned in first-encounter order.
    nodes: Vec<GraphNode>,
    /// Reverse index: hash of a `GraphNode` -> its 1-based id in
    /// [`Self::nodes`]. Stores ONLY ids and dedups by comparing against
    /// `nodes[id - 1]`, so it never owns a second (deep) copy of the node.
    node_ids: HashTable<u32>,
    /// Full expression memo. A hit means an expression with the same
    /// identity: the same allocation for every shared sub-expression the
    /// key names ([`ArcAddr`]), and equal content for the parts compared by
    /// value. For a value-keyed variant (a primitive, literal, type
    /// parameter, `typeof`, synthetic carrier or unknown) that is any
    /// content-equal expression. Keys own the allocations they name.
    expr_ids: HashMap<ExprMemoKey, u32>,
    /// Fast path checked before building the full [`ExprMemoKey`]: the
    /// expressions identified by allocation alone ([`AddressIdentity`]),
    /// looked up by a borrowed expression without cloning. Each entry owns
    /// its expression, and with it every allocation the identity names.
    expr_ptr_ids: HashTable<AddressEntry>,
    #[cfg(test)]
    graph_node_build_count: usize,
    #[cfg(test)]
    expr_ptr_cache_hits: usize,
}

/// Table sizes only: the memo entries own whole expression trees, which a
/// debug dump must not walk.
impl std::fmt::Debug for GraphBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphBuilder")
            .field("strings", &self.strings.len())
            .field("nodes", &self.nodes.len())
            .field("expr_ids", &self.expr_ids.len())
            .field("expr_ptr_ids", &self.expr_ptr_ids.len())
            .finish_non_exhaustive()
    }
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn string_id(&mut self, value: &str) -> u32 {
        // Own-once: `strings` is the sole owner; `string_ids` holds only ids and
        // resolves collisions by comparing the query against `strings[id - 1]`.
        let Self {
            strings,
            string_ids,
            ..
        } = self;
        let hash = table_hash::<str>(value);
        match string_ids.entry(
            hash,
            |&id| strings[(id - 1) as usize].as_str() == value,
            |&id| table_hash::<str>(strings[(id - 1) as usize].as_str()),
        ) {
            hashbrown::hash_table::Entry::Occupied(entry) => *entry.get(),
            hashbrown::hash_table::Entry::Vacant(entry) => {
                let id = strings
                    .len()
                    .checked_add(1)
                    .and_then(|index| u32::try_from(index).ok())
                    .expect("string table overflow");
                strings.push(value.to_string());
                entry.insert(id);
                id
            }
        }
    }

    pub fn string_id_opt(&mut self, value: Option<&str>) -> u32 {
        value.map(|value| self.string_id(value)).unwrap_or(0)
    }

    pub fn node_id(&mut self, expr: &TypeExpr) -> u32 {
        // Fast path: an expression identified by allocation alone is looked
        // up by a borrowed view, before building the full memo key (which
        // clones owned fields).
        let address = AddressIdentity::of(expr);
        if let Some(address) = address {
            if let Some(entry) = self
                .expr_ptr_ids
                .find(table_hash(&address), |entry| entry.identity() == address)
            {
                #[cfg(test)]
                {
                    self.expr_ptr_cache_hits += 1;
                }
                return entry.id;
            }
        }

        // Slow path: the full memo key.
        let memo_key = ExprMemoKey::from_expr(expr);
        if let Some(&id) = self.expr_ids.get(&memo_key) {
            // Remember this allocation too, so its next lookup takes the
            // fast path.
            if let Some(address) = address {
                self.remember_address(address, expr, id);
            }
            return id;
        }

        // Terminal value-level dedup + own-once push (no `GraphNode` clone).
        // The memo maps update identically whether the node was already
        // interned or freshly pushed, so both cases share one tail.
        let node = self.graph_node(expr);
        let id = self.intern_node(node);
        self.expr_ids.insert(memo_key, id);
        if let Some(address) = address {
            self.remember_address(address, expr, id);
        }
        id
    }

    /// Record `expr` (identified by `address`) in the fast path. The entry
    /// keeps a clone of the expression — `Arc` reference counts only, since
    /// an address-identified variant holds no owned data — so the
    /// allocations `address` names outlive the entry's use.
    fn remember_address(&mut self, address: AddressIdentity, expr: &TypeExpr, id: u32) {
        if let hashbrown::hash_table::Entry::Vacant(slot) = self.expr_ptr_ids.entry(
            table_hash(&address),
            |entry| entry.identity() == address,
            |entry| table_hash(&entry.identity()),
        ) {
            slot.insert(AddressEntry {
                expr: expr.clone(),
                id,
            });
        }
    }

    /// The function node of a member signature (a method, call or construct
    /// signature). The signature lives inline in its member, in no `Arc` of
    /// its own, so it has no allocation to memoise on: its node is built and
    /// interned by value, which yields the same id a `TypeExpr::Function`
    /// holding an equal signature gets.
    fn signature_function_id(&mut self, function: &FunctionExpr) -> u32 {
        let node = self.function_node(function);
        self.intern_node(node)
    }

    pub fn strings(&self) -> &[String] {
        &self.strings
    }

    pub fn nodes(&self) -> &[GraphNode] {
        &self.nodes
    }

    /// Consume the builder into its raw `(strings, nodes)` tables — the
    /// snapshot-capture accessor
    /// ([`crate::graph::snapshot::ResolvedTypeGraphSnapshot::from_builder`]).
    /// Ids in both tables are 1-BASED (id 0 = absent), matching
    /// [`Self::string_id`] / [`Self::node_id`].
    #[must_use]
    pub fn into_tables(self) -> (Vec<String>, Vec<GraphNode>) {
        (self.strings, self.nodes)
    }

    /// Intern an ALREADY-BUILT wire node by VALUE, deduplicating against the
    /// existing node table — exactly the terminal dedup step of
    /// [`Self::node_id`] without the `TypeExpr` memo layers (there is no
    /// source expression here).
    fn intern_node(&mut self, node: GraphNode) -> u32 {
        // Own-once: `nodes` is the sole owner; `node_ids` holds only ids and
        // dedups by comparing against `nodes[id - 1]`. On a miss the node MOVES
        // into the table (no deep clone of its nested member/id vectors).
        let Self {
            nodes, node_ids, ..
        } = self;
        let hash = table_hash(&node);
        match node_ids.entry(
            hash,
            |&id| nodes[(id - 1) as usize] == node,
            |&id| table_hash(&nodes[(id - 1) as usize]),
        ) {
            hashbrown::hash_table::Entry::Occupied(entry) => *entry.get(),
            hashbrown::hash_table::Entry::Vacant(entry) => {
                let id = nodes
                    .len()
                    .checked_add(1)
                    .and_then(|index| u32::try_from(index).ok())
                    .expect("node table overflow");
                nodes.push(node);
                entry.insert(id);
                id
            }
        }
    }

    /// Append a producer-captured
    /// [`ResolvedTypeGraphSnapshot`](crate::graph::snapshot::ResolvedTypeGraphSnapshot)
    /// into this live builder, remapping the snapshot's arena-local string /
    /// node ids into this builder's tables, and return the REMAPPED root node
    /// id (0 when the snapshot has no root).
    ///
    /// WIRE-IDENTICAL to the legacy direct `node_id(&TypeExpr)` walk: a
    /// snapshot captured from a `GraphBuilder` stores its nodes in
    /// children-before-parents order (the recursive `node_id` interns every
    /// child before pushing its parent), so one in-order pass has every
    /// referenced id already remapped; each remapped node then interns through
    /// the SAME value-level dedup ([`Self::intern_node`]) the direct walk
    /// bottoms out in, so identical structural nodes collapse onto identical
    /// ids whether they arrived via a snapshot or a direct walk. A snapshot is
    /// well-formed by construction (the sealed module-private
    /// `ResolvedTypeGraphSnapshot::try_new` validated root and reference
    /// ranges up front), so the remap's fail-close-to-0 id lookups are pure
    /// defense-in-depth that a captured snapshot never triggers.
    pub fn append_snapshot(
        &mut self,
        snapshot: &crate::graph::snapshot::ResolvedTypeGraphSnapshot,
    ) -> u32 {
        if snapshot.root_node_id() == 0 {
            return 0;
        }
        // Old 1-based string id -> new string id; index 0 stays the absent 0.
        let mut string_map: Vec<u32> = Vec::with_capacity(snapshot.strings().len() + 1);
        string_map.push(0);
        for s in snapshot.strings() {
            string_map.push(self.string_id(s));
        }
        // Old 1-based node id -> new node id; index 0 stays the absent 0.
        let mut node_map: Vec<u32> = Vec::with_capacity(snapshot.nodes().len() + 1);
        node_map.push(0);
        for node in snapshot.nodes() {
            let remapped = remap_snapshot_node(node, &string_map, &node_map);
            node_map.push(self.intern_node(remapped));
        }
        node_map
            .get(snapshot.root_node_id() as usize)
            .copied()
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub(crate) fn debug_graph_node_build_count(&self) -> usize {
        self.graph_node_build_count
    }

    #[cfg(test)]
    pub(crate) fn debug_expr_ptr_cache_hits(&self) -> usize {
        self.expr_ptr_cache_hits
    }

    /// How many memo entries, fast path and full memo together, are for a
    /// `TypeExpr::Function`.
    #[cfg(test)]
    pub(crate) fn debug_function_memo_entries(&self) -> usize {
        let fast = self
            .expr_ptr_ids
            .iter()
            .filter(|entry| matches!(entry.expr, TypeExpr::Function(_)))
            .count();
        let full = self
            .expr_ids
            .keys()
            .filter(|key| matches!(key, ExprMemoKey::Function(_)))
            .count();
        fast + full
    }

    fn graph_node(&mut self, expr: &TypeExpr) -> GraphNode {
        #[cfg(test)]
        {
            self.graph_node_build_count += 1;
        }
        match expr {
            TypeExpr::Primitive(name) => GraphNode::Primitive {
                primitive: schema::primitive_to_tag(*name),
            },
            TypeExpr::Literal(literal) => self.literal_node(literal),
            TypeExpr::Union(types) => GraphNode::Union {
                types: types.iter().map(|ty| self.node_id(ty)).collect(),
            },
            TypeExpr::Intersection(types) => GraphNode::Intersection {
                types: types.iter().map(|ty| self.node_id(ty)).collect(),
            },
            TypeExpr::Array { element, readonly } => GraphNode::Array {
                element: self.node_id(element),
                readonly: *readonly,
            },
            TypeExpr::Tuple { elements, readonly } => GraphNode::Tuple {
                readonly: *readonly,
                elements: elements
                    .iter()
                    .map(|element| self.tuple_element(element))
                    .collect(),
            },
            TypeExpr::Object(object) => GraphNode::Object {
                // Public-surface sanitizer: the graph wire (component-meta graph
                // / type_registry AND the typeinfo graph) is a public surface and
                // carries NO member-visibility field, so a non-public
                // (private/protected) class member must never be encoded onto it
                // — a consumer decoding the wire could not distinguish or filter
                // it. Drop non-public Property / Method members here, the single
                // recursive chokepoint before object-node encoding (each member's
                // value type is serialized through this same `node_id` path, so
                // nested object surfaces are sanitized too). The keep-all native
                // surface (`native_props`) is serialized separately as
                // `ResolvedNativeProp` with its own visibility marker and does not
                // route through this object-node path, so it is unaffected.
                members: object
                    .properties
                    .iter()
                    .filter(|member| object_member_is_public(member))
                    .map(|member| self.object_member(member))
                    .collect(),
            },
            TypeExpr::Function(function) => self.function_node(function),
            // INTENTIONAL erasure: a constructor type serialises to the SAME
            // wire node as a function (`GraphNode::Function` — parameters /
            // return / type-parameters). A constructor type's structural wire
            // shape is exactly a function's, the typeinfo wire graph
            // (`GraphTypeNode.kind`) is a closed structural taxonomy with no
            // dedicated constructor-type kind, and the constructor-vs-function
            // distinction that matters for Vue runtime inference is carried by
            // the `TypeExpr::ConstructorType` variant itself and consumed by the
            // session-side semantic dispatch BEFORE
            // wire serialisation. Emitting the function node here is therefore
            // the contract-correct final-state shape — no schema-version bump or
            // new wire kind is required. The erasure is pinned (not accidental)
            // by `constructor_type_serialises_to_function_wire_node` below, which
            // also asserts the memo identity stays DISTINCT from a plain
            // function (`ExprMemoKey::ConstructorType`).
            TypeExpr::ConstructorType(function) => self.function_node(function),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => GraphNode::Ref {
                name: self.string_id(name),
                type_arguments: type_arguments.iter().map(|ty| self.node_id(ty)).collect(),
            },
            TypeExpr::IntrinsicApplication { op, arguments } => {
                GraphNode::IntrinsicApplication {
                    // Exhaustive on purpose: a new op must fail here until
                    // its wire code is stated.
                    op: match op {
                        verter_type_expr::CompilerIntrinsicTypeOp::Awaited => 1,
                        verter_type_expr::CompilerIntrinsicTypeOp::NoInfer => 2,
                    },
                    arguments: arguments.iter().map(|ty| self.node_id(ty)).collect(),
                }
            }
            TypeExpr::TypeParameter(param) => self.type_parameter_node(param),
            TypeExpr::KeyOf(operand) => GraphNode::KeyOf {
                operand: self.node_id(operand),
            },
            TypeExpr::TypeOf(value) => self.type_of_node(value),
            TypeExpr::IndexedAccess { object, index } => GraphNode::IndexedAccess {
                object: self.node_id(object),
                index: self.node_id(index),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => GraphNode::Conditional {
                check: self.node_id(check),
                extends: self.node_id(extends),
                true_type: self.node_id(true_type),
                false_type: self.node_id(false_type),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => GraphNode::Mapped {
                parameter: self.string_id(parameter),
                source: self.node_id(source),
                value: self.node_id(value),
                optional: mapped_modifier_tag(*optional),
                readonly: mapped_modifier_tag(*readonly),
                name_type: name_type.as_deref().map(|ty| self.node_id(ty)).unwrap_or(0),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => GraphNode::TemplateLiteral {
                quasis: quasis.iter().map(|quasi| self.string_id(quasi)).collect(),
                expressions: expressions.iter().map(|expr| self.node_id(expr)).collect(),
            },
            TypeExpr::Parenthesized(inner) => GraphNode::Parenthesized {
                inner: self.node_id(inner),
            },
            TypeExpr::Unknown(value) => GraphNode::Unknown {
                raw: self.string_id(value.raw()),
            },
            // An import-type carrier resolves in the dispatch before reaching
            // the wire surface; the closed typeinfo schema has no dedicated
            // import-type node (adding one would bump the schema version). On
            // the defensive path where an unresolved carrier reaches here, emit
            // the opaque `Unknown` wire node with a display-only diagnostic raw
            // (never re-parsed).
            TypeExpr::ImportType {
                specifier,
                qualifier,
                typeof_query,
                ..
            } => {
                let mut raw = String::new();
                if *typeof_query {
                    raw.push_str("typeof ");
                }
                raw.push_str("import(\"");
                raw.push_str(specifier);
                raw.push_str("\")");
                for seg in qualifier.iter() {
                    raw.push('.');
                    raw.push_str(seg);
                }
                GraphNode::Unknown {
                    raw: self.string_id(&raw),
                }
            }
            TypeExpr::Infer { name, constraint } => GraphNode::Infer {
                name: self.string_id(name),
                constraint: constraint
                    .as_ref()
                    .map(|value| self.node_id(value))
                    .unwrap_or(0),
            },
            TypeExpr::Rest(inner) => GraphNode::Rest {
                inner: self.node_id(inner),
            },
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => GraphNode::RecursiveRef {
                name: self.string_id(name),
                type_arguments: type_arguments.iter().map(|ty| self.node_id(ty)).collect(),
                conditional_context: conditional_context
                    .iter()
                    .map(|f| GraphConditionalFrame {
                        branch: match f.branch {
                            verter_type_expr::RecursiveConditionalBranch::True => 1,
                            verter_type_expr::RecursiveConditionalBranch::False => 2,
                        },
                        decided: f.decided,
                        check: self.node_id(&f.check),
                        extends: self.node_id(&f.extends),
                    })
                    .collect(),
            },
            TypeExpr::SyntheticSlotBinding(key) => {
                // Intern the carrier's string fields once through the
                // shared graph string table. The slot name is optional —
                // map `None` to id 0 (matching the proto `slot_name_id`
                // contract: 0 = absent).
                let scope_canonical_id_id = self.string_id(key.scope_canonical_id.as_ref());
                let surface_kind = match key.surface_kind {
                    verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding => 0,
                    verter_type_expr::SyntheticCarrierSurfaceKind::Binding => 1,
                };
                let slot_name_id = key
                    .slot_name
                    .as_deref()
                    .map(|name| self.string_id(name))
                    .unwrap_or(0);
                let binding_name_id = self.string_id(key.binding_name.as_ref());
                GraphNode::SyntheticSlotBinding {
                    value_node: key.value_node,
                    scope_canonical_id_id,
                    surface_kind,
                    slot_name_id,
                    binding_name_id,
                }
            }
        }
    }

    fn literal_node(&mut self, literal: &LiteralValue) -> GraphNode {
        match literal {
            LiteralValue::String(value) => GraphNode::LiteralString {
                value: self.string_id(value),
            },
            LiteralValue::Number(value) => GraphNode::LiteralNumber {
                bits: value.to_bits(),
            },
            LiteralValue::Boolean(value) => GraphNode::LiteralBoolean { value: *value },
            LiteralValue::BigInt(value) => GraphNode::LiteralBigInt {
                value: self.string_id(value),
            },
        }
    }

    fn tuple_element(&mut self, element: &TupleElement) -> GraphTupleElement {
        GraphTupleElement {
            label: self.string_id_opt(element.label.as_deref()),
            ty: self.node_id(&element.ty),
            optional: element.optional,
            rest: element.rest,
        }
    }

    fn object_member(&mut self, member: &ObjectMember) -> GraphObjectMember {
        match member {
            ObjectMember::Property(property) => self.property_member(property),
            ObjectMember::IndexSignature(signature) => self.index_signature_member(signature),
            ObjectMember::CallSignature(function) => {
                self.signature_member(schema::MEMBER_CALL_SIGNATURE, function)
            }
            ObjectMember::ConstructSignature(function) => {
                self.signature_member(schema::MEMBER_CONSTRUCT_SIGNATURE, function)
            }
            ObjectMember::Method(method) => self.method_member(method),
            ObjectMember::Spread(spread) => self.spread_member(spread),
        }
    }

    fn spread_member(&mut self, spread: &verter_type_expr::SpreadMember) -> GraphObjectMember {
        GraphObjectMember {
            kind: schema::MEMBER_SPREAD,
            key: None,
            // The spread OPERAND rides the member's `ty` slot.
            ty: self.node_id(&spread.ty),
            optional: false,
            readonly: false,
            key_name: 0,
            key_type: 0,
            value_type: 0,
            function: 0,
            method_kind: 0,
            has_implementation_body: false,
        }
    }

    fn property_member(&mut self, property: &ObjectProperty) -> GraphObjectMember {
        GraphObjectMember {
            kind: schema::MEMBER_PROPERTY,
            key: Some(self.property_key(&property.key)),
            ty: self.node_id(&property.ty),
            optional: property.optional,
            readonly: property.readonly,
            key_name: 0,
            key_type: 0,
            value_type: 0,
            function: 0,
            method_kind: 0,
            has_implementation_body: false,
        }
    }

    fn index_signature_member(&mut self, signature: &IndexSignature) -> GraphObjectMember {
        GraphObjectMember {
            kind: schema::MEMBER_INDEX_SIGNATURE,
            key: None,
            ty: 0,
            optional: false,
            readonly: signature.readonly,
            key_name: self.string_id(&signature.key_name),
            key_type: self.node_id(&signature.key_type),
            value_type: self.node_id(&signature.value_type),
            function: 0,
            method_kind: 0,
            has_implementation_body: false,
        }
    }

    fn signature_member(&mut self, kind: u32, function: &FunctionExpr) -> GraphObjectMember {
        GraphObjectMember {
            kind,
            key: None,
            ty: 0,
            optional: false,
            readonly: false,
            key_name: 0,
            key_type: 0,
            value_type: 0,
            function: self.signature_function_id(function),
            method_kind: 0,
            has_implementation_body: false,
        }
    }

    fn method_member(&mut self, method: &MethodSignature) -> GraphObjectMember {
        GraphObjectMember {
            kind: schema::MEMBER_METHOD,
            key: Some(self.property_key(&method.key)),
            ty: 0,
            optional: method.optional,
            readonly: false,
            key_name: 0,
            key_type: 0,
            value_type: 0,
            function: self.signature_function_id(&method.function),
            method_kind: match method.method_kind {
                verter_type_expr::ObjectMethodKind::Method => 0,
                verter_type_expr::ObjectMethodKind::Get => 1,
                verter_type_expr::ObjectMethodKind::Set => 2,
            },
            has_implementation_body: method.has_implementation_body,
        }
    }

    fn property_key(&mut self, key: &TypeAuthoredPropertyKey) -> GraphPropertyKey {
        match key {
            AuthoredPropertyKey::String(value) => GraphPropertyKey::String {
                value: self.string_id(value),
            },
            AuthoredPropertyKey::Number(value) => GraphPropertyKey::Number { value: value.get() },
            AuthoredPropertyKey::UniqueSymbol(identity) => GraphPropertyKey::UniqueSymbol {
                canonical_id: self.string_id(&identity.canonical_id),
                owner_kind: match identity.owner.kind() {
                    verter_type_expr::TopLevelOwnerKind::Module => 0,
                    verter_type_expr::TopLevelOwnerKind::Instance => 1,
                    verter_type_expr::TopLevelOwnerKind::Frontmatter => 2,
                },
                owner_ordinal: identity.owner.ordinal(),
                symbol: self.string_id(&identity.symbol),
                member_path: identity
                    .member_path
                    .iter()
                    .map(|segment| self.string_id(segment))
                    .collect(),
            },
            AuthoredPropertyKey::Computed(expression) => GraphPropertyKey::Computed {
                node: self.node_id(expression),
            },
        }
    }

    fn function_node(&mut self, function: &FunctionExpr) -> GraphNode {
        GraphNode::Function {
            parameters: function
                .parameters
                .iter()
                .map(|param| self.function_param(param))
                .collect(),
            return_type: function
                .return_type
                .as_deref()
                .map(|ty| self.node_id(ty))
                .unwrap_or(0),
            type_parameters: function
                .type_parameters
                .iter()
                .map(|param| self.node_id(&TypeExpr::TypeParameter(param.clone())))
                .collect(),
        }
    }

    fn function_param(&mut self, param: &FunctionParam) -> GraphFunctionParam {
        GraphFunctionParam {
            name: self.string_id_opt(param.name.as_deref()),
            ty: self.node_id(&param.ty),
            optional: param.optional,
            rest: param.rest,
        }
    }

    fn type_parameter_node(&mut self, param: &TypeParam) -> GraphNode {
        GraphNode::TypeParameter {
            name: self.string_id(&param.name),
            constraint: param
                .constraint
                .as_deref()
                .map(|constraint| self.node_id(constraint))
                .unwrap_or(0),
            default: param
                .default
                .as_deref()
                .map(|default| self.node_id(default))
                .unwrap_or(0),
        }
    }

    fn type_of_node(&mut self, value: &ValueRef) -> GraphNode {
        GraphNode::TypeOf {
            path: value
                .path
                .iter()
                .map(|segment| self.string_id(segment))
                .collect(),
        }
    }
}

fn mapped_modifier_tag(modifier: MappedModifier) -> u32 {
    schema::mapped_modifier_to_tag(modifier)
}

/// Widen one PERSISTED snapshot node back to a live wire node while
/// remapping its arena-local ids into a live builder's tables — the single
/// widening + remap pass [`GraphBuilder::append_snapshot`] re-interns
/// through.
///
/// `string_map` / `node_map` are 1-based old→new id maps whose index 0 carries
/// the absent id 0, so absent (`0`) fields round-trip unchanged through the
/// same lookup as present ones. A captured snapshot is validated in-range and
/// children-before-parents by construction
/// ([`crate::graph::snapshot::ResolvedTypeGraphSnapshot::try_new`]), so the
/// fail-close-to-0 lookups are defense-in-depth only.
///
/// EXHAUSTIVE over [`PersistedGraphNode`] (no wildcard): adding a persisted
/// node variant fails compilation here until its widening semantics (string
/// id vs node id vs copied tag) are stated explicitly. The live-wire-only
/// `SyntheticSlotBinding` carrier has no arm because the persisted
/// vocabulary structurally excludes it.
fn remap_snapshot_node(
    node: &PersistedGraphNode,
    string_map: &[u32],
    node_map: &[u32],
) -> GraphNode {
    let sid = |id: u32| -> u32 { string_map.get(id as usize).copied().unwrap_or(0) };
    let nid = |id: u32| -> u32 { node_map.get(id as usize).copied().unwrap_or(0) };
    match node {
        PersistedGraphNode::Primitive { primitive } => GraphNode::Primitive {
            primitive: *primitive,
        },
        PersistedGraphNode::LiteralString { value } => {
            GraphNode::LiteralString { value: sid(*value) }
        }
        PersistedGraphNode::LiteralNumber { bits } => GraphNode::LiteralNumber { bits: *bits },
        PersistedGraphNode::LiteralBoolean { value } => GraphNode::LiteralBoolean { value: *value },
        PersistedGraphNode::LiteralBigInt { value } => {
            GraphNode::LiteralBigInt { value: sid(*value) }
        }
        PersistedGraphNode::Union { types } => GraphNode::Union {
            types: types.iter().map(|ty| nid(*ty)).collect(),
        },
        PersistedGraphNode::Intersection { types } => GraphNode::Intersection {
            types: types.iter().map(|ty| nid(*ty)).collect(),
        },
        PersistedGraphNode::Array { element, readonly } => GraphNode::Array {
            element: nid(*element),
            readonly: *readonly,
        },
        PersistedGraphNode::Tuple { readonly, elements } => GraphNode::Tuple {
            readonly: *readonly,
            elements: elements
                .iter()
                .map(|element| GraphTupleElement {
                    label: sid(element.label),
                    ty: nid(element.ty),
                    optional: element.optional,
                    rest: element.rest,
                })
                .collect(),
        },
        PersistedGraphNode::Object { members } => GraphNode::Object {
            members: members
                .iter()
                .map(|member| GraphObjectMember {
                    kind: member.kind,
                    key: member.key.as_ref().map(|key| match key {
                        GraphPropertyKey::String { value } => {
                            GraphPropertyKey::String { value: sid(*value) }
                        }
                        GraphPropertyKey::Number { value } => {
                            GraphPropertyKey::Number { value: *value }
                        }
                        GraphPropertyKey::UniqueSymbol {
                            canonical_id,
                            owner_kind,
                            owner_ordinal,
                            symbol,
                            member_path,
                        } => GraphPropertyKey::UniqueSymbol {
                            canonical_id: sid(*canonical_id),
                            owner_kind: *owner_kind,
                            owner_ordinal: *owner_ordinal,
                            symbol: sid(*symbol),
                            member_path: member_path.iter().map(|id| sid(*id)).collect(),
                        },
                        GraphPropertyKey::Computed { node } => {
                            GraphPropertyKey::Computed { node: nid(*node) }
                        }
                    }),
                    ty: nid(member.ty),
                    optional: member.optional,
                    readonly: member.readonly,
                    key_name: sid(member.key_name),
                    key_type: nid(member.key_type),
                    value_type: nid(member.value_type),
                    function: nid(member.function),
                    method_kind: member.method_kind,
                    has_implementation_body: member.has_implementation_body,
                })
                .collect(),
        },
        PersistedGraphNode::Function {
            parameters,
            return_type,
            type_parameters,
        } => GraphNode::Function {
            parameters: parameters
                .iter()
                .map(|param| GraphFunctionParam {
                    name: sid(param.name),
                    ty: nid(param.ty),
                    optional: param.optional,
                    rest: param.rest,
                })
                .collect(),
            return_type: nid(*return_type),
            type_parameters: type_parameters.iter().map(|param| nid(*param)).collect(),
        },
        PersistedGraphNode::Ref {
            name,
            type_arguments,
        } => GraphNode::Ref {
            name: sid(*name),
            type_arguments: type_arguments.iter().map(|arg| nid(*arg)).collect(),
        },
        // `op` is a COPIED TAG, not a string id — no `sid` widening. The
        // operands are node ids and widen through `nid` like any child.
        PersistedGraphNode::IntrinsicApplication { op, arguments } => {
            GraphNode::IntrinsicApplication {
                op: *op,
                arguments: arguments.iter().map(|arg| nid(*arg)).collect(),
            }
        }
        PersistedGraphNode::TypeParameter {
            name,
            constraint,
            default,
        } => GraphNode::TypeParameter {
            name: sid(*name),
            constraint: nid(*constraint),
            default: nid(*default),
        },
        PersistedGraphNode::KeyOf { operand } => GraphNode::KeyOf {
            operand: nid(*operand),
        },
        PersistedGraphNode::TypeOf { path } => GraphNode::TypeOf {
            path: path.iter().map(|segment| sid(*segment)).collect(),
        },
        PersistedGraphNode::IndexedAccess { object, index } => GraphNode::IndexedAccess {
            object: nid(*object),
            index: nid(*index),
        },
        PersistedGraphNode::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => GraphNode::Conditional {
            check: nid(*check),
            extends: nid(*extends),
            true_type: nid(*true_type),
            false_type: nid(*false_type),
        },
        PersistedGraphNode::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => GraphNode::Mapped {
            parameter: sid(*parameter),
            source: nid(*source),
            value: nid(*value),
            optional: *optional,
            readonly: *readonly,
            name_type: nid(*name_type),
        },
        PersistedGraphNode::TemplateLiteral {
            quasis,
            expressions,
        } => GraphNode::TemplateLiteral {
            quasis: quasis.iter().map(|quasi| sid(*quasi)).collect(),
            expressions: expressions.iter().map(|expr| nid(*expr)).collect(),
        },
        PersistedGraphNode::Parenthesized { inner } => {
            GraphNode::Parenthesized { inner: nid(*inner) }
        }
        PersistedGraphNode::Unknown { raw } => GraphNode::Unknown { raw: sid(*raw) },
        PersistedGraphNode::Infer { name, constraint } => GraphNode::Infer {
            name: sid(*name),
            constraint: nid(*constraint),
        },
        PersistedGraphNode::Rest { inner } => GraphNode::Rest { inner: nid(*inner) },
        PersistedGraphNode::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => GraphNode::RecursiveRef {
            name: sid(*name),
            type_arguments: type_arguments.iter().map(|arg| nid(*arg)).collect(),
            conditional_context: conditional_context
                .iter()
                .map(|frame| GraphConditionalFrame {
                    branch: frame.branch,
                    decided: frame.decided,
                    check: nid(frame.check),
                    extends: nid(frame.extends),
                })
                .collect(),
        },
    }
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
