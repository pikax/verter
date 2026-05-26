use std::collections::HashMap;
use std::sync::Arc;

use verter_type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, LiteralValue, MappedModifier, MethodSignature,
    ObjectMember, ObjectProperty, TupleElement, TypeExpr, TypeParam, ValueRef,
};

use crate::graph::schema;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphConditionalFrame {
    /// 1 = true, 2 = false
    pub branch: u32,
    pub decided: bool,
    pub check: u32,
    pub extends: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphTupleElement {
    pub label: u32,
    pub ty: u32,
    pub optional: bool,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphObjectMember {
    pub kind: u32,
    pub name: u32,
    pub ty: u32,
    pub optional: bool,
    pub readonly: bool,
    pub key_name: u32,
    pub key_type: u32,
    pub value_type: u32,
    pub function: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphFunctionParam {
    pub name: u32,
    pub ty: u32,
    pub optional: bool,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ExprMemoKey {
    Primitive(verter_type_expr::PrimitiveName),
    Literal(verter_type_expr::LiteralValue),
    Union {
        ptr: usize,
        len: usize,
    },
    Intersection {
        ptr: usize,
        len: usize,
    },
    Array {
        element_ptr: usize,
        readonly: bool,
    },
    Tuple {
        ptr: usize,
        len: usize,
        readonly: bool,
    },
    Object {
        ptr: usize,
    },
    Function {
        ptr: usize,
    },
    Ref {
        name: Arc<str>,
        type_arguments_ptr: usize,
        type_arguments_len: usize,
    },
    TypeParameter(verter_type_expr::TypeParam),
    KeyOf {
        operand_ptr: usize,
    },
    TypeOf(verter_type_expr::ValueRef),
    IndexedAccess {
        object_ptr: usize,
        index_ptr: usize,
    },
    Conditional {
        check_ptr: usize,
        extends_ptr: usize,
        true_ptr: usize,
        false_ptr: usize,
    },
    Mapped {
        parameter: String,
        source_ptr: usize,
        value_ptr: usize,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type_ptr: usize,
    },
    TemplateLiteral {
        quasis: Vec<String>,
        expressions_ptr: usize,
        expressions_len: usize,
    },
    Infer {
        name: String,
    },
    Rest {
        inner_ptr: usize,
    },
    Parenthesized {
        inner_ptr: usize,
    },
    RecursiveRef {
        name: Arc<str>,
        type_arguments_ptr: usize,
        type_arguments_len: usize,
        conditional_context_ptr: usize,
        conditional_context_len: usize,
    },
    /// Synthetic slot-binding carrier — full structural identity. Two
    /// physically distinct `Arc<SyntheticCarrierKey>` values that hold
    /// the same logical key SHOULD share a memo entry, matching the
    /// `PartialEq + Eq + Hash` derive on `SyntheticCarrierKey`.
    SyntheticSlotBinding {
        key: Arc<verter_type_expr::SyntheticCarrierKey>,
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
            TypeExpr::Union(types) => Self::Union {
                ptr: slice_ptr_id(types),
                len: types.len(),
            },
            TypeExpr::Intersection(types) => Self::Intersection {
                ptr: slice_ptr_id(types),
                len: types.len(),
            },
            TypeExpr::Array { element, readonly } => Self::Array {
                element_ptr: arc_ptr_id(element),
                readonly: *readonly,
            },
            TypeExpr::Tuple { elements, readonly } => Self::Tuple {
                ptr: slice_ptr_id(elements),
                len: elements.len(),
                readonly: *readonly,
            },
            TypeExpr::Object(object) => Self::Object {
                ptr: arc_ptr_id(object),
            },
            TypeExpr::Function(function) => Self::Function {
                ptr: arc_ptr_id(function),
            },
            TypeExpr::Ref {
                name,
                type_arguments,
            } => Self::Ref {
                name: Arc::clone(name),
                type_arguments_ptr: slice_ptr_id(type_arguments),
                type_arguments_len: type_arguments.len(),
            },
            TypeExpr::TypeParameter(param) => Self::TypeParameter(param.clone()),
            TypeExpr::KeyOf(operand) => Self::KeyOf {
                operand_ptr: arc_ptr_id(operand),
            },
            TypeExpr::TypeOf(value) => Self::TypeOf(value.clone()),
            TypeExpr::IndexedAccess { object, index } => Self::IndexedAccess {
                object_ptr: arc_ptr_id(object),
                index_ptr: arc_ptr_id(index),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => Self::Conditional {
                check_ptr: arc_ptr_id(check),
                extends_ptr: arc_ptr_id(extends),
                true_ptr: arc_ptr_id(true_type),
                false_ptr: arc_ptr_id(false_type),
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
                source_ptr: arc_ptr_id(source),
                value_ptr: arc_ptr_id(value),
                optional: *optional,
                readonly: *readonly,
                name_type_ptr: option_arc_ptr_id(name_type.as_ref()),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => Self::TemplateLiteral {
                quasis: quasis.clone(),
                expressions_ptr: slice_ptr_id(expressions),
                expressions_len: expressions.len(),
            },
            TypeExpr::Infer { name } => Self::Infer { name: name.clone() },
            TypeExpr::Rest(inner) => Self::Rest {
                inner_ptr: arc_ptr_id(inner),
            },
            TypeExpr::Parenthesized(inner) => Self::Parenthesized {
                inner_ptr: arc_ptr_id(inner),
            },
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => Self::RecursiveRef {
                name: Arc::clone(name),
                type_arguments_ptr: slice_ptr_id(type_arguments),
                type_arguments_len: type_arguments.len(),
                conditional_context_ptr: conditional_context.as_ptr() as usize,
                conditional_context_len: conditional_context.len(),
            },
            TypeExpr::SyntheticSlotBinding(key) => Self::SyntheticSlotBinding {
                key: Arc::clone(key),
            },
            TypeExpr::Unknown { raw } => Self::Unknown { raw: raw.clone() },
        }
    }
}

fn arc_ptr_id<T>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value) as usize
}

fn slice_ptr_id<T>(value: &Arc<[T]>) -> usize {
    value.as_ptr() as usize
}

fn option_arc_ptr_id<T>(value: Option<&Arc<T>>) -> usize {
    value.map(arc_ptr_id).unwrap_or(0)
}

/// Compact pointer-based identity key for `TypeExpr` variants whose identity is
/// fully determined by Arc pointers (no owned/cloned data). Used as a fast-path
/// cache to avoid building the full `ExprMemoKey` and cloning value fields on
/// cache hits.
///
/// Variants that carry owned data (`Literal`, `TypeParameter`, `TypeOf`, `Mapped`,
/// `TemplateLiteral`, `Infer`, `Unknown`) are not eligible and fall through to
/// the full `ExprMemoKey` path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ExprPtrKey {
    /// Discriminant of the `TypeExpr` variant.
    tag: u8,
    /// Primary pointer (Arc::as_ptr or slice as_ptr). 0 when unused.
    p0: usize,
    /// Secondary pointer or small value. 0 when unused.
    p1: usize,
    /// Tertiary pointer. 0 when unused.
    p2: usize,
    /// Quaternary pointer. 0 when unused.
    p3: usize,
    /// Extra bits: readonly flag, len, etc. 0 when unused.
    extra: usize,
}

impl ExprPtrKey {
    /// Try to extract a pointer-based identity key from a `TypeExpr`.
    /// Returns `None` for variants whose identity depends on owned/cloned data.
    fn try_from_expr(expr: &TypeExpr) -> Option<Self> {
        Some(match expr {
            TypeExpr::Primitive(name) => Self {
                tag: 0,
                p0: *name as usize,
                p1: 0,
                p2: 0,
                p3: 0,
                extra: 0,
            },
            TypeExpr::Union(types) => Self {
                tag: 1,
                p0: types.as_ptr() as usize,
                p1: types.len(),
                p2: 0,
                p3: 0,
                extra: 0,
            },
            TypeExpr::Intersection(types) => Self {
                tag: 2,
                p0: types.as_ptr() as usize,
                p1: types.len(),
                p2: 0,
                p3: 0,
                extra: 0,
            },
            TypeExpr::Array { element, readonly } => Self {
                tag: 3,
                p0: Arc::as_ptr(element) as usize,
                p1: 0,
                p2: 0,
                p3: 0,
                extra: *readonly as usize,
            },
            TypeExpr::Tuple { elements, readonly } => Self {
                tag: 4,
                p0: elements.as_ptr() as usize,
                p1: elements.len(),
                p2: 0,
                p3: 0,
                extra: *readonly as usize,
            },
            TypeExpr::Object(object) => Self {
                tag: 5,
                p0: Arc::as_ptr(object) as usize,
                p1: 0,
                p2: 0,
                p3: 0,
                extra: 0,
            },
            TypeExpr::Function(function) => Self {
                tag: 6,
                p0: Arc::as_ptr(function) as usize,
                p1: 0,
                p2: 0,
                p3: 0,
                extra: 0,
            },
            TypeExpr::Ref {
                name,
                type_arguments,
            } => Self {
                tag: 7,
                p0: Arc::as_ptr(name) as *const u8 as usize,
                p1: type_arguments.as_ptr() as usize,
                p2: type_arguments.len(),
                p3: 0,
                extra: 0,
            },
            TypeExpr::KeyOf(operand) => Self {
                tag: 8,
                p0: Arc::as_ptr(operand) as usize,
                p1: 0,
                p2: 0,
                p3: 0,
                extra: 0,
            },
            TypeExpr::IndexedAccess { object, index } => Self {
                tag: 9,
                p0: Arc::as_ptr(object) as usize,
                p1: Arc::as_ptr(index) as usize,
                p2: 0,
                p3: 0,
                extra: 0,
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => Self {
                tag: 10,
                p0: Arc::as_ptr(check) as usize,
                p1: Arc::as_ptr(extends) as usize,
                p2: Arc::as_ptr(true_type) as usize,
                p3: Arc::as_ptr(false_type) as usize,
                extra: 0,
            },
            TypeExpr::Rest(inner) => Self {
                tag: 11,
                p0: Arc::as_ptr(inner) as usize,
                p1: 0,
                p2: 0,
                p3: 0,
                extra: 0,
            },
            TypeExpr::Parenthesized(inner) => Self {
                tag: 12,
                p0: Arc::as_ptr(inner) as usize,
                p1: 0,
                p2: 0,
                p3: 0,
                extra: 0,
            },
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => Self {
                tag: 13,
                p0: Arc::as_ptr(name) as *const u8 as usize,
                p1: type_arguments.as_ptr() as usize,
                p2: type_arguments.len(),
                p3: conditional_context.as_ptr() as usize,
                extra: conditional_context.len(),
            },
            // Synthetic slot-binding carrier — pointer-only identity. Two
            // physically identical Arcs are the same carrier (and share a
            // graph-node id); structurally identical carriers held in
            // distinct Arcs intentionally miss this fast path and fall
            // through to the full `ExprMemoKey` (which deduplicates by
            // structural equality on the `SyntheticCarrierKey` content).
            TypeExpr::SyntheticSlotBinding(key) => Self {
                tag: 14,
                p0: arc_ptr_id(key),
                p1: 0,
                p2: 0,
                p3: 0,
                extra: 0,
            },
            // Value-based variants that require cloning — fall through to full ExprMemoKey.
            TypeExpr::Literal(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => return None,
        })
    }
}

#[derive(Debug, Default)]
pub struct GraphBuilder {
    strings: Vec<String>,
    string_ids: HashMap<String, u32>,
    nodes: Vec<GraphNode>,
    node_ids: HashMap<GraphNode, u32>,
    expr_ids: HashMap<ExprMemoKey, u32>,
    /// Fast-path cache for pointer-based `TypeExpr` variants. Checked before
    /// building the full `ExprMemoKey`, avoiding clones on cache hits for the
    /// majority of expression types.
    expr_ptr_ids: HashMap<ExprPtrKey, u32>,
    #[cfg(test)]
    graph_node_build_count: usize,
    #[cfg(test)]
    expr_ptr_cache_hits: usize,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn string_id(&mut self, value: &str) -> u32 {
        if let Some(id) = self.string_ids.get(value) {
            return *id;
        }

        let id = self
            .strings
            .len()
            .checked_add(1)
            .and_then(|index| u32::try_from(index).ok())
            .expect("string table overflow");
        let owned = value.to_string();
        self.strings.push(owned.clone());
        self.string_ids.insert(owned, id);
        id
    }

    pub fn string_id_opt(&mut self, value: Option<&str>) -> u32 {
        value.map(|value| self.string_id(value)).unwrap_or(0)
    }

    pub fn node_id(&mut self, expr: &TypeExpr) -> u32 {
        // Fast path: for pointer-based variants, check the compact ptr cache
        // before building the full ExprMemoKey (avoids cloning on cache hits).
        let ptr_key = ExprPtrKey::try_from_expr(expr);
        if let Some(pk) = &ptr_key {
            if let Some(id) = self.expr_ptr_ids.get(pk) {
                #[cfg(test)]
                {
                    self.expr_ptr_cache_hits += 1;
                }
                return *id;
            }
        }

        // Slow path: build the full memo key (may clone value fields).
        let memo_key = ExprMemoKey::from_expr(expr);
        if let Some(id) = self.expr_ids.get(&memo_key) {
            // Populate the ptr cache so future hits for the same Arc take
            // the fast path.
            if let Some(pk) = ptr_key {
                self.expr_ptr_ids.insert(pk, *id);
            }
            return *id;
        }

        let node = self.graph_node(expr);
        if let Some(id) = self.node_ids.get(&node) {
            self.expr_ids.insert(memo_key, *id);
            if let Some(pk) = ptr_key {
                self.expr_ptr_ids.insert(pk, *id);
            }
            return *id;
        }

        let id = self
            .nodes
            .len()
            .checked_add(1)
            .and_then(|index| u32::try_from(index).ok())
            .expect("node table overflow");
        self.nodes.push(node.clone());
        self.node_ids.insert(node, id);
        self.expr_ids.insert(memo_key, id);
        if let Some(pk) = ptr_key {
            self.expr_ptr_ids.insert(pk, id);
        }
        id
    }

    pub fn strings(&self) -> &[String] {
        &self.strings
    }

    pub fn nodes(&self) -> &[GraphNode] {
        &self.nodes
    }

    #[cfg(test)]
    pub(crate) fn debug_graph_node_build_count(&self) -> usize {
        self.graph_node_build_count
    }

    #[cfg(test)]
    pub(crate) fn debug_expr_ptr_cache_hits(&self) -> usize {
        self.expr_ptr_cache_hits
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
                members: object
                    .properties
                    .iter()
                    .map(|member| self.object_member(member))
                    .collect(),
            },
            TypeExpr::Function(function) => self.function_node(function),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => GraphNode::Ref {
                name: self.string_id(name),
                type_arguments: type_arguments.iter().map(|ty| self.node_id(ty)).collect(),
            },
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
            TypeExpr::Unknown { raw } => GraphNode::Unknown {
                raw: self.string_id(raw),
            },
            TypeExpr::Infer { name } => GraphNode::Infer {
                name: self.string_id(name),
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
        }
    }

    fn property_member(&mut self, property: &ObjectProperty) -> GraphObjectMember {
        GraphObjectMember {
            kind: schema::MEMBER_PROPERTY,
            name: self.string_id(&property.name),
            ty: self.node_id(&property.ty),
            optional: property.optional,
            readonly: property.readonly,
            key_name: 0,
            key_type: 0,
            value_type: 0,
            function: 0,
        }
    }

    fn index_signature_member(&mut self, signature: &IndexSignature) -> GraphObjectMember {
        GraphObjectMember {
            kind: schema::MEMBER_INDEX_SIGNATURE,
            name: 0,
            ty: 0,
            optional: false,
            readonly: signature.readonly,
            key_name: self.string_id(&signature.key_name),
            key_type: self.node_id(&signature.key_type),
            value_type: self.node_id(&signature.value_type),
            function: 0,
        }
    }

    fn signature_member(&mut self, kind: u32, function: &FunctionExpr) -> GraphObjectMember {
        GraphObjectMember {
            kind,
            name: 0,
            ty: 0,
            optional: false,
            readonly: false,
            key_name: 0,
            key_type: 0,
            value_type: 0,
            function: self.node_id(&TypeExpr::Function(std::sync::Arc::new(function.clone()))),
        }
    }

    fn method_member(&mut self, method: &MethodSignature) -> GraphObjectMember {
        GraphObjectMember {
            kind: schema::MEMBER_METHOD,
            name: self.string_id(&method.name),
            ty: 0,
            optional: method.optional,
            readonly: false,
            key_name: 0,
            key_type: 0,
            value_type: 0,
            function: self.node_id(&TypeExpr::Function(std::sync::Arc::new(
                method.function.clone(),
            ))),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use verter_type_expr::{
        LiteralValue, PrimitiveName, RecursiveConditionalBranch, RecursiveConditionalFrame,
        TypeExpr,
    };

    #[test]
    fn graph_builder_encodes_recursive_ref_not_unknown() {
        let expr = TypeExpr::RecursiveRef {
            name: std::sync::Arc::from("Tree"),
            type_arguments: std::sync::Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
            conditional_context: std::sync::Arc::from(vec![RecursiveConditionalFrame {
                branch: RecursiveConditionalBranch::True,
                decided: true,
                check: std::sync::Arc::new(TypeExpr::named("T")),
                extends: std::sync::Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            }]),
        };

        let mut builder = GraphBuilder::new();
        let node_id = builder.node_id(&expr);
        let nodes = builder.nodes();
        let node = &nodes[(node_id - 1) as usize];

        assert!(
            matches!(node, GraphNode::RecursiveRef { .. }),
            "graph builder must produce GraphNode::RecursiveRef, got {:?}",
            std::mem::discriminant(node)
        );

        if let GraphNode::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } = node
        {
            assert!(*name > 0, "name string ID should be set");
            assert_eq!(type_arguments.len(), 1, "should have 1 type argument");
            assert_eq!(
                conditional_context.len(),
                1,
                "should have 1 conditional frame"
            );
            assert_eq!(conditional_context[0].branch, 1, "branch=true should be 1");
            assert!(conditional_context[0].decided);
        }
    }

    #[test]
    fn ptr_cache_fast_path_avoids_memo_key_on_pointer_based_variants() {
        // Build an expression tree with pointer-based variants (Union, Object,
        // Function, IndexedAccess, Array) and verify that repeat lookups hit
        // the ptr cache without building ExprMemoKey or GraphNode.
        let inner_obj = TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
            properties: vec![],
        }));
        let union = TypeExpr::Union(Arc::from(vec![
            inner_obj.clone(),
            TypeExpr::Primitive(PrimitiveName::String),
        ]));
        let array = TypeExpr::Array {
            element: Arc::new(union.clone()),
            readonly: false,
        };

        let mut builder = GraphBuilder::new();

        // First pass: builds everything.
        let id1 = builder.node_id(&array);
        let builds_after_first = builder.debug_graph_node_build_count();
        assert!(
            builds_after_first > 0,
            "first pass should build graph nodes"
        );
        assert_eq!(
            builder.debug_expr_ptr_cache_hits(),
            0,
            "first pass should have zero ptr cache hits"
        );

        // Second pass: should hit the ptr cache for the top-level Array.
        let id2 = builder.node_id(&array);
        assert_eq!(id1, id2, "same expression must return same node id");
        assert_eq!(
            builder.debug_expr_ptr_cache_hits(),
            1,
            "second lookup on a pointer-based variant should hit the ptr cache"
        );
        assert_eq!(
            builder.debug_graph_node_build_count(),
            builds_after_first,
            "ptr cache hit should not build any new graph nodes"
        );

        // Also verify that value-based variants (Literal) still work correctly.
        let lit = TypeExpr::Literal(LiteralValue::String("hello".to_string()));
        let lit_id1 = builder.node_id(&lit);
        let lit_id2 = builder.node_id(&lit);
        assert_eq!(
            lit_id1, lit_id2,
            "value-based variant should still deduplicate via ExprMemoKey"
        );
    }

    #[test]
    fn graph_builder_reuses_same_expr_reference_without_rewalking_subgraph() {
        let shared = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("Accordion")),
            index: Arc::new(TypeExpr::Literal(LiteralValue::String("slots".to_string()))),
        };
        let expr = TypeExpr::Array {
            element: Arc::new(TypeExpr::Union(Arc::from(vec![shared.clone(), shared]))),
            readonly: false,
        };

        let mut builder = GraphBuilder::new();
        let first_id = builder.node_id(&expr);
        let builds_after_first = builder.debug_graph_node_build_count();

        let second_id = builder.node_id(&expr);
        let builds_after_second = builder.debug_graph_node_build_count();

        assert_eq!(
            first_id, second_id,
            "same expression should reuse one node id"
        );
        assert_eq!(
            builds_after_second, builds_after_first,
            "repeat node_id() on the same expression should hit the front cache instead of rebuilding the graph node"
        );
    }
}
