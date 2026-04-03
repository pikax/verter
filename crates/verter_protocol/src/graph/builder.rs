use std::collections::HashMap;

use verter_semantic::analysis::type_expr::{
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

#[derive(Debug, Default)]
pub struct GraphBuilder {
    strings: Vec<String>,
    string_ids: HashMap<String, u32>,
    nodes: Vec<GraphNode>,
    node_ids: HashMap<GraphNode, u32>,
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
        let node = self.graph_node(expr);
        if let Some(id) = self.node_ids.get(&node) {
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
        id
    }

    pub fn strings(&self) -> &[String] {
        &self.strings
    }

    pub fn nodes(&self) -> &[GraphNode] {
        &self.nodes
    }

    fn graph_node(&mut self, expr: &TypeExpr) -> GraphNode {
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
                            verter_semantic::analysis::type_expr::RecursiveConditionalBranch::True => 1,
                            verter_semantic::analysis::type_expr::RecursiveConditionalBranch::False => 2,
                        },
                        decided: f.decided,
                        check: self.node_id(&f.check),
                        extends: self.node_id(&f.extends),
                    })
                    .collect(),
            },
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
    use verter_semantic::analysis::type_expr::{
        PrimitiveName, RecursiveConditionalBranch, RecursiveConditionalFrame, TypeExpr,
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
}
