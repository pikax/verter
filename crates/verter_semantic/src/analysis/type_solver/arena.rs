//! Query-local type node arena.
//!
//! `QueryArena` is an append-only node store. Once nodes are allocated they
//! are never mutated, so `&QueryArena` is sufficient for reading. The
//! companion `SolverCaches` memoization tables were retired together with
//! the arena solver kernel.

use std::fmt;

// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------

/// Opaque handle into the query arena. Cheap to copy/compare.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    /// Sentinel for an unresolved / placeholder node.
    pub const UNRESOLVED: Self = Self(u32::MAX);

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn is_unresolved(self) -> bool {
        self == Self::UNRESOLVED
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_unresolved() {
            write!(f, "NodeId(UNRESOLVED)")
        } else {
            write!(f, "NodeId({})", self.0)
        }
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Node kinds
// ---------------------------------------------------------------------------

/// A type node in the solver arena.
#[derive(Debug, Clone)]
pub enum Node {
    // -- Terminals --
    Primitive(PrimitiveKind),
    Literal(SolverLiteral),

    // -- Compound --
    Union(Vec<NodeId>),
    Intersection(Vec<NodeId>),
    Array {
        element: NodeId,
        readonly: bool,
    },
    Tuple {
        elements: Vec<TupleNodeElement>,
        readonly: bool,
    },
    Object(ObjectNode),
    Function(FunctionNode),

    // -- References --
    /// A named type reference to a declaration (not yet applied/instantiated).
    Ref {
        name: String,
        type_arguments: Vec<NodeId>,
        scope_canonical_id: Option<String>,
    },
    /// An applied/instantiated declaration: decl identity + resolved args.
    Applied {
        identity: DeclIdentity,
        args: Vec<NodeId>,
    },
    /// A type parameter — either free (unresolved) or bound (in substitution env).
    TypeParam {
        name: String,
        constraint: Option<NodeId>,
        default: Option<NodeId>,
    },

    // -- Operators --
    KeyOf(NodeId),
    TypeOf {
        path: Vec<String>,
    },
    IndexedAccess {
        object: NodeId,
        index: NodeId,
    },
    Conditional {
        check: NodeId,
        extends: NodeId,
        true_branch: NodeId,
        false_branch: NodeId,
        /// Whether this conditional distributes over naked type parameters.
        distributive: bool,
    },
    Mapped {
        parameter: String,
        source: NodeId,
        value: NodeId,
        optional: MappedModifierKind,
        readonly: MappedModifierKind,
        name_type: Option<NodeId>,
    },
    TemplateLiteral {
        quasis: Vec<String>,
        expressions: Vec<NodeId>,
    },
    Infer {
        name: String,
    },
    Rest(NodeId),

    // -- Special --
    /// Recursive backedge / SCC placeholder. Preserves the recursive symbol
    /// name, applied type arguments, and active conditional context at the
    /// moment recursion was detected.
    RecursiveRef {
        symbol_name: String,
        type_arguments: Vec<NodeId>,
        conditional_context: Vec<ConditionalFrameSnapshot>,
    },
}

/// Primitive type kinds (mirrors TypeExpr::Primitive variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveKind {
    String,
    Number,
    Boolean,
    Symbol,
    BigInt,
    Any,
    Unknown,
    Void,
    Never,
    Null,
    Undefined,
    Object,
}

/// Literal values in the solver arena.
#[derive(Debug, Clone, PartialEq)]
pub enum SolverLiteral {
    String(String),
    Number(f64),
    Boolean(bool),
    BigInt(String),
}

impl Eq for SolverLiteral {}

impl std::hash::Hash for SolverLiteral {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::String(s) => s.hash(state),
            Self::Number(n) => n.to_bits().hash(state),
            Self::Boolean(b) => b.hash(state),
            Self::BigInt(s) => s.hash(state),
        }
    }
}

/// Mapped type modifier action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MappedModifierKind {
    Add,
    Remove,
    Unchanged,
}

/// A tuple element in the solver arena.
#[derive(Debug, Clone)]
pub struct TupleNodeElement {
    pub label: Option<String>,
    pub ty: NodeId,
    pub optional: bool,
    pub rest: bool,
}

/// Object node — properties, index signatures, call/construct signatures.
#[derive(Debug, Clone)]
pub struct ObjectNode {
    pub properties: Vec<PropertyNode>,
    pub index_signatures: Vec<IndexSignatureNode>,
    pub call_signatures: Vec<CallSignatureNode>,
    pub construct_signatures: Vec<CallSignatureNode>,
}

/// A property on an object node.
#[derive(Debug, Clone)]
pub struct PropertyNode {
    pub name: String,
    pub ty: NodeId,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
}

/// An index signature: `[key: K]: V`.
#[derive(Debug, Clone)]
pub struct IndexSignatureNode {
    pub key_type: NodeId,
    pub value_type: NodeId,
    pub readonly: bool,
}

/// A call or construct signature.
#[derive(Debug, Clone)]
pub struct CallSignatureNode {
    pub type_parameters: Vec<TypeParamNode>,
    pub parameters: Vec<ParamNode>,
    pub return_type: NodeId,
}

/// A type parameter declaration in the solver arena.
#[derive(Debug, Clone)]
pub struct TypeParamNode {
    pub name: String,
    pub constraint: Option<NodeId>,
    pub default: Option<NodeId>,
}

/// A function/method parameter.
#[derive(Debug, Clone)]
pub struct ParamNode {
    pub name: Option<String>,
    pub ty: NodeId,
    pub optional: bool,
    pub rest: bool,
}

/// Function node — overloaded call signatures.
#[derive(Debug, Clone)]
pub struct FunctionNode {
    pub signatures: Vec<CallSignatureNode>,
}

/// A snapshot of one conditional branch frame in the solver arena.
#[derive(Debug, Clone)]
pub struct ConditionalFrameSnapshot {
    pub branch: ConditionalBranch,
    pub decided: bool,
    pub check: NodeId,
    pub extends: NodeId,
}

/// Which branch of a conditional type was active in the solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionalBranch {
    True,
    False,
}

/// Declaration identity for applied/instantiated types — used as memoization
/// key together with applied type arguments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclIdentity {
    pub canonical_id: String,
    pub symbol_name: String,
}

// ---------------------------------------------------------------------------
// Query Arena — immutable node store
// ---------------------------------------------------------------------------

/// Append-only node store. Once nodes are allocated they are never mutated,
/// so `&QueryArena` is sufficient for reading during relation/projection.
pub struct QueryArena {
    nodes: Vec<Node>,
    total_allocations: u64,
}

impl QueryArena {
    pub fn new() -> Self {
        Self {
            nodes: Vec::with_capacity(256),
            total_allocations: 0,
        }
    }

    /// Intern a node, returning its stable `NodeId`.
    ///
    /// Panics in debug mode if the arena exceeds `u32::MAX - 1` nodes.
    pub fn alloc(&mut self, node: Node) -> NodeId {
        debug_assert!(
            self.nodes.len() < (u32::MAX - 1) as usize,
            "arena overflow: exceeded u32::MAX - 1 nodes"
        );
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        self.total_allocations += 1;
        id
    }

    /// Get a node by ID.
    ///
    /// Returns `Node::Primitive(PrimitiveKind::Unknown)` for `UNRESOLVED`
    /// node IDs instead of panicking. This handles the case where a type
    /// parameter has no explicit argument and no default — the solver
    /// produces `NodeId::UNRESOLVED` which must be safely dereferenced
    /// during subsequent type expansion.
    pub fn get(&self, id: NodeId) -> &Node {
        if id.is_unresolved() {
            // Static sentinel — avoids allocation for the common UNRESOLVED case.
            static UNKNOWN: Node = Node::Primitive(PrimitiveKind::Unknown);
            return &UNKNOWN;
        }
        &self.nodes[id.index()]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn total_allocations(&self) -> u64 {
        self.total_allocations
    }

    // -- Convenience constructors --

    pub fn primitive(&mut self, kind: PrimitiveKind) -> NodeId {
        self.alloc(Node::Primitive(kind))
    }

    pub fn literal(&mut self, lit: SolverLiteral) -> NodeId {
        self.alloc(Node::Literal(lit))
    }

    pub fn string_literal(&mut self, s: impl Into<String>) -> NodeId {
        self.literal(SolverLiteral::String(s.into()))
    }

    pub fn number_literal(&mut self, n: f64) -> NodeId {
        self.literal(SolverLiteral::Number(n))
    }

    pub fn boolean_literal(&mut self, b: bool) -> NodeId {
        self.literal(SolverLiteral::Boolean(b))
    }

    pub fn union(&mut self, members: Vec<NodeId>) -> NodeId {
        let mut flattened = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut seen_primitives = rustc_hash::FxHashSet::default();
        let mut seen_literals = rustc_hash::FxHashSet::default();
        for member in members {
            if member.is_unresolved() {
                if seen.insert(member) {
                    flattened.push(member);
                }
                continue;
            }

            match self.get(member) {
                Node::Union(inner) => {
                    for nested in inner {
                        if union_member_is_new(
                            self,
                            *nested,
                            &mut seen,
                            &mut seen_primitives,
                            &mut seen_literals,
                        ) {
                            flattened.push(*nested);
                        }
                    }
                }
                Node::Primitive(PrimitiveKind::Never) => {}
                _ => {
                    if union_member_is_new(
                        self,
                        member,
                        &mut seen,
                        &mut seen_primitives,
                        &mut seen_literals,
                    ) {
                        flattened.push(member);
                    }
                }
            }
        }

        match flattened.len() {
            0 => self.primitive(PrimitiveKind::Never),
            1 => flattened[0],
            _ => self.alloc(Node::Union(flattened)),
        }
    }

    pub fn intersection(&mut self, members: Vec<NodeId>) -> NodeId {
        match members.len() {
            0 => self.primitive(PrimitiveKind::Unknown),
            1 => members[0],
            _ => self.alloc(Node::Intersection(members)),
        }
    }

    pub fn array(&mut self, element: NodeId, readonly: bool) -> NodeId {
        self.alloc(Node::Array { element, readonly })
    }

    pub fn object(&mut self, obj: ObjectNode) -> NodeId {
        self.alloc(Node::Object(obj))
    }

    pub fn function(&mut self, func: FunctionNode) -> NodeId {
        self.alloc(Node::Function(func))
    }

    pub fn type_ref(&mut self, name: impl Into<String>, args: Vec<NodeId>) -> NodeId {
        self.scoped_type_ref(name, args, None)
    }

    pub fn scoped_type_ref(
        &mut self,
        name: impl Into<String>,
        args: Vec<NodeId>,
        scope_canonical_id: Option<String>,
    ) -> NodeId {
        self.alloc(Node::Ref {
            name: name.into(),
            type_arguments: args,
            scope_canonical_id,
        })
    }

    pub fn key_of(&mut self, operand: NodeId) -> NodeId {
        self.alloc(Node::KeyOf(operand))
    }

    pub fn indexed_access(&mut self, object: NodeId, index: NodeId) -> NodeId {
        self.alloc(Node::IndexedAccess { object, index })
    }

    pub fn conditional(
        &mut self,
        check: NodeId,
        extends: NodeId,
        true_branch: NodeId,
        false_branch: NodeId,
        distributive: bool,
    ) -> NodeId {
        self.alloc(Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
        })
    }

    pub fn mapped(
        &mut self,
        parameter: impl Into<String>,
        source: NodeId,
        value: NodeId,
        optional: MappedModifierKind,
        readonly: MappedModifierKind,
        name_type: Option<NodeId>,
    ) -> NodeId {
        self.alloc(Node::Mapped {
            parameter: parameter.into(),
            source,
            value,
            optional,
            readonly,
            name_type,
        })
    }
}

impl Default for QueryArena {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for QueryArena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueryArena")
            .field("nodes", &self.nodes.len())
            .finish()
    }
}

fn union_member_is_new(
    arena: &QueryArena,
    member: NodeId,
    seen: &mut rustc_hash::FxHashSet<NodeId>,
    seen_primitives: &mut rustc_hash::FxHashSet<PrimitiveKind>,
    seen_literals: &mut rustc_hash::FxHashSet<SolverLiteral>,
) -> bool {
    if member.is_unresolved() {
        return seen.insert(member);
    }

    match arena.get(member) {
        Node::Primitive(kind) => seen_primitives.insert(*kind),
        Node::Literal(literal) => seen_literals.insert(literal.clone()),
        _ => seen.insert(member),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_alloc_and_get() {
        let mut arena = QueryArena::new();
        let id = arena.primitive(PrimitiveKind::String);
        assert_eq!(id, NodeId(0));
        assert!(matches!(
            arena.get(id),
            Node::Primitive(PrimitiveKind::String)
        ));
        assert_eq!(arena.len(), 1);
        assert_eq!(arena.total_allocations(), 1);
    }

    #[test]
    fn arena_multiple_nodes() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let n = arena.primitive(PrimitiveKind::Number);
        let u = arena.union(vec![s, n]);
        assert_eq!(arena.len(), 3);
        assert!(matches!(arena.get(u), Node::Union(members) if members.len() == 2));
    }

    #[test]
    fn union_of_zero_is_never() {
        let mut arena = QueryArena::new();
        let u = arena.union(vec![]);
        assert!(matches!(
            arena.get(u),
            Node::Primitive(PrimitiveKind::Never)
        ));
    }

    #[test]
    fn union_of_one_is_identity() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let u = arena.union(vec![s]);
        assert_eq!(u, s);
    }

    #[test]
    fn union_preserves_unresolved_members_without_panicking() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let u = arena.union(vec![NodeId::UNRESOLVED, s]);

        match arena.get(u) {
            Node::Union(members) => {
                assert_eq!(members.len(), 2);
                assert_eq!(members[0], NodeId::UNRESOLVED);
                assert_eq!(members[1], s);
            }
            other => panic!("expected unresolved/string union, got {other:?}"),
        }
    }

    #[test]
    fn intersection_of_zero_is_unknown() {
        let mut arena = QueryArena::new();
        let i = arena.intersection(vec![]);
        assert!(matches!(
            arena.get(i),
            Node::Primitive(PrimitiveKind::Unknown)
        ));
    }

    #[test]
    fn intersection_of_one_is_identity() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let i = arena.intersection(vec![s]);
        assert_eq!(i, s);
    }

    #[test]
    fn node_id_unresolved_sentinel() {
        assert!(NodeId::UNRESOLVED.is_unresolved());
        assert!(!NodeId(0).is_unresolved());
        assert!(!NodeId(100).is_unresolved());
    }

    #[test]
    fn string_literal_convenience() {
        let mut arena = QueryArena::new();
        let id = arena.string_literal("hello");
        assert!(matches!(
            arena.get(id),
            Node::Literal(SolverLiteral::String(s)) if s == "hello"
        ));
    }

    #[test]
    fn complex_type_construction() {
        let mut arena = QueryArena::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let num_ty = arena.primitive(PrimitiveKind::Number);

        let prop = PropertyNode {
            name: "x".into(),
            ty: num_ty,
            optional: false,
            readonly: false,
            is_method: false,
        };
        let obj = arena.object(ObjectNode {
            properties: vec![prop],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let arr = arena.array(obj, true);
        let union = arena.union(vec![arr, str_ty]);

        assert_eq!(arena.len(), 5); // str, num, obj, arr, union
        assert!(matches!(arena.get(union), Node::Union(_)));
    }
}
