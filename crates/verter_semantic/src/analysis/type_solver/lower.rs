//! Lower `TypeExpr` from prepared declarations into solver arena `NodeId`s.
//!
//! This is the bridge between the host's prepared declarations (which store
//! symbolic `TypeExpr` bodies) and the solver's query arena (which uses
//! interned `NodeId`s for memoization and relation checking).

use super::arena::{
    CallSignatureNode, FunctionNode, IndexSignatureNode, MappedModifierKind, Node, NodeId,
    ObjectNode, ParamNode, PrimitiveKind, PropertyNode, QueryArena, SolverLiteral,
    TupleNodeElement, TypeParamNode,
};
use crate::analysis::type_expr::{
    FunctionExpr, LiteralValue, MappedModifier, ObjectExpr, ObjectMember, PrimitiveName, TypeExpr,
    TypeParam,
};

// ---------------------------------------------------------------------------
// TypeExpr → NodeId lowering
// ---------------------------------------------------------------------------

/// Lower a `TypeExpr` into the solver arena, returning its `NodeId`.
///
/// This is a straightforward structural conversion — it does NOT resolve
/// references, instantiate generics, or evaluate operators. Those are the
/// solver's job. This simply interns the declaration's symbolic body into
/// the arena's node graph.
pub fn lower_type_expr(arena: &mut QueryArena, expr: &TypeExpr) -> NodeId {
    match expr {
        TypeExpr::Primitive(prim) => arena.primitive(lower_primitive(*prim)),

        TypeExpr::Literal(lit) => arena.literal(lower_literal(lit)),

        TypeExpr::Union(members) => {
            let ids: Vec<NodeId> = members.iter().map(|m| lower_type_expr(arena, m)).collect();
            arena.union(ids)
        }

        TypeExpr::Intersection(members) => {
            let ids: Vec<NodeId> = members.iter().map(|m| lower_type_expr(arena, m)).collect();
            arena.intersection(ids)
        }

        TypeExpr::Array { element, readonly } => {
            let el = lower_type_expr(arena, element);
            arena.array(el, *readonly)
        }

        TypeExpr::Tuple { elements, readonly } => {
            let els: Vec<TupleNodeElement> = elements
                .iter()
                .map(|el| TupleNodeElement {
                    label: el.label.clone(),
                    ty: lower_type_expr(arena, &el.ty),
                    optional: el.optional,
                    rest: el.rest,
                })
                .collect();
            arena.alloc(Node::Tuple {
                elements: els,
                readonly: *readonly,
            })
        }

        TypeExpr::Object(obj) => lower_object(arena, obj),

        TypeExpr::Function(func) => lower_function(arena, func),

        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            let args: Vec<NodeId> = type_arguments
                .iter()
                .map(|a| lower_type_expr(arena, a))
                .collect();
            arena.type_ref(name.as_ref(), args)
        }

        TypeExpr::TypeParameter(param) => {
            let constraint = param.constraint.as_ref().map(|c| lower_type_expr(arena, c));
            let default = param.default.as_ref().map(|d| lower_type_expr(arena, d));
            arena.alloc(Node::TypeParam {
                name: param.name.clone(),
                constraint,
                default,
            })
        }

        TypeExpr::KeyOf(operand) => {
            let op = lower_type_expr(arena, operand);
            arena.key_of(op)
        }

        TypeExpr::TypeOf(value_ref) => arena.alloc(Node::TypeOf {
            path: value_ref.path.clone(),
        }),

        TypeExpr::IndexedAccess { object, index } => {
            let obj = lower_type_expr(arena, object);
            let idx = lower_type_expr(arena, index);
            arena.indexed_access(obj, idx)
        }

        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_id = lower_type_expr(arena, check);
            let extends_id = lower_type_expr(arena, extends);
            let true_id = lower_type_expr(arena, true_type);
            let false_id = lower_type_expr(arena, false_type);
            // Distributive if the check type is a bare type parameter
            let distributive = matches!(check.as_ref(), TypeExpr::TypeParameter(_));
            arena.conditional(check_id, extends_id, true_id, false_id, distributive)
        }

        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            let src = lower_type_expr(arena, source);
            let val = lower_type_expr(arena, value);
            let nt = name_type.as_ref().map(|n| lower_type_expr(arena, n));
            arena.mapped(
                parameter.clone(),
                src,
                val,
                lower_mapped_modifier(*optional),
                lower_mapped_modifier(*readonly),
                nt,
            )
        }

        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => {
            let exprs: Vec<NodeId> = expressions
                .iter()
                .map(|e| lower_type_expr(arena, e))
                .collect();
            arena.alloc(Node::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: exprs,
            })
        }

        TypeExpr::Infer { name } => arena.alloc(Node::Infer { name: name.clone() }),

        TypeExpr::Rest(inner) => {
            let id = lower_type_expr(arena, inner);
            arena.alloc(Node::Rest(id))
        }

        TypeExpr::Parenthesized(inner) => lower_type_expr(arena, inner),

        TypeExpr::Unknown { raw } => arena.error(raw.clone()),
    }
}

// ---------------------------------------------------------------------------
// Helper conversions
// ---------------------------------------------------------------------------

fn lower_primitive(prim: PrimitiveName) -> PrimitiveKind {
    match prim {
        PrimitiveName::String => PrimitiveKind::String,
        PrimitiveName::Number => PrimitiveKind::Number,
        PrimitiveName::Boolean => PrimitiveKind::Boolean,
        PrimitiveName::Symbol => PrimitiveKind::Symbol,
        PrimitiveName::BigInt => PrimitiveKind::BigInt,
        PrimitiveName::Any => PrimitiveKind::Any,
        PrimitiveName::Unknown => PrimitiveKind::Unknown,
        PrimitiveName::Void => PrimitiveKind::Void,
        PrimitiveName::Never => PrimitiveKind::Never,
        PrimitiveName::Null => PrimitiveKind::Null,
        PrimitiveName::Undefined => PrimitiveKind::Undefined,
        PrimitiveName::Object => PrimitiveKind::Object,
    }
}

fn lower_literal(lit: &LiteralValue) -> SolverLiteral {
    match lit {
        LiteralValue::String(s) => SolverLiteral::String(s.clone()),
        LiteralValue::Number(n) => SolverLiteral::Number(*n),
        LiteralValue::Boolean(b) => SolverLiteral::Boolean(*b),
        LiteralValue::BigInt(s) => SolverLiteral::BigInt(s.clone()),
    }
}

fn lower_mapped_modifier(m: MappedModifier) -> MappedModifierKind {
    match m {
        MappedModifier::Add => MappedModifierKind::Add,
        MappedModifier::Remove => MappedModifierKind::Remove,
        MappedModifier::None => MappedModifierKind::Unchanged,
    }
}

fn lower_object(arena: &mut QueryArena, obj: &ObjectExpr) -> NodeId {
    let mut properties = Vec::new();
    let mut index_signatures = Vec::new();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();

    for member in &obj.properties {
        match member {
            ObjectMember::Property(prop) => {
                properties.push(PropertyNode {
                    name: prop.name.clone(),
                    ty: lower_type_expr(arena, &prop.ty),
                    optional: prop.optional,
                    readonly: prop.readonly,
                    is_method: false,
                });
            }
            ObjectMember::IndexSignature(idx) => {
                index_signatures.push(IndexSignatureNode {
                    key_type: lower_type_expr(arena, &idx.key_type),
                    value_type: lower_type_expr(arena, &idx.value_type),
                    readonly: idx.readonly,
                });
            }
            ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                let sig = lower_call_signature(arena, func);
                if matches!(member, ObjectMember::ConstructSignature(_)) {
                    construct_signatures.push(sig);
                } else {
                    call_signatures.push(sig);
                }
            }
            ObjectMember::Method(method) => {
                properties.push(PropertyNode {
                    name: method.name.clone(),
                    ty: lower_function(arena, &method.function),
                    optional: method.optional,
                    readonly: false,
                    is_method: true,
                });
            }
        }
    }

    arena.object(ObjectNode {
        properties,
        index_signatures,
        call_signatures,
        construct_signatures,
    })
}

fn lower_call_signature(arena: &mut QueryArena, func: &FunctionExpr) -> CallSignatureNode {
    let params: Vec<ParamNode> = func
        .parameters
        .iter()
        .map(|p| ParamNode {
            name: p.name.clone(),
            ty: lower_type_expr(arena, &p.ty),
            optional: p.optional,
            rest: p.rest,
        })
        .collect();
    let ret = func
        .return_type
        .as_ref()
        .map(|r| lower_type_expr(arena, r))
        .unwrap_or_else(|| arena.primitive(PrimitiveKind::Void));
    let type_params = func
        .type_parameters
        .iter()
        .map(|tp| lower_type_param(arena, tp))
        .collect();
    CallSignatureNode {
        type_parameters: type_params,
        parameters: params,
        return_type: ret,
    }
}

fn lower_function(arena: &mut QueryArena, func: &FunctionExpr) -> NodeId {
    let sig = lower_call_signature(arena, func);
    arena.function(FunctionNode {
        signatures: vec![sig],
    })
}

fn lower_type_param(arena: &mut QueryArena, param: &TypeParam) -> TypeParamNode {
    TypeParamNode {
        name: param.name.clone(),
        constraint: param.constraint.as_ref().map(|c| lower_type_expr(arena, c)),
        default: param.default.as_ref().map(|d| lower_type_expr(arena, d)),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::analysis::type_expr::{
        FunctionParam, IndexSignature, ObjectProperty, TupleElement, ValueRef,
    };

    #[test]
    fn lower_primitive_types() {
        let mut arena = QueryArena::new();

        let id = lower_type_expr(&mut arena, &TypeExpr::Primitive(PrimitiveName::String));
        assert!(matches!(
            arena.get(id),
            Node::Primitive(PrimitiveKind::String)
        ));

        let id = lower_type_expr(&mut arena, &TypeExpr::Primitive(PrimitiveName::Never));
        assert!(matches!(
            arena.get(id),
            Node::Primitive(PrimitiveKind::Never)
        ));
    }

    #[test]
    fn lower_literal_types() {
        let mut arena = QueryArena::new();

        let id = lower_type_expr(&mut arena, &TypeExpr::string_literal("hello"));
        assert!(matches!(
            arena.get(id),
            Node::Literal(SolverLiteral::String(s)) if s == "hello"
        ));

        let id = lower_type_expr(&mut arena, &TypeExpr::number_literal(42.0));
        assert!(matches!(
            arena.get(id),
            Node::Literal(SolverLiteral::Number(n)) if *n == 42.0
        ));
    }

    #[test]
    fn lower_union() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Union(Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ]));
        let id = lower_type_expr(&mut arena, &expr);
        assert!(matches!(arena.get(id), Node::Union(members) if members.len() == 2));
    }

    #[test]
    fn lower_object_with_properties_and_index() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "x".into(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: true,
                }),
                ObjectMember::IndexSignature(IndexSignature {
                    key_name: "key".into(),
                    key_type: TypeExpr::Primitive(PrimitiveName::String),
                    value_type: TypeExpr::Primitive(PrimitiveName::Number),
                    readonly: false,
                }),
            ],
        }));
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::Object(obj) => {
                assert_eq!(obj.properties.len(), 1);
                assert_eq!(obj.properties[0].name, "x");
                assert!(obj.properties[0].readonly);
                assert_eq!(obj.index_signatures.len(), 1);
            }
            _ => panic!("expected Object node"),
        }
    }

    #[test]
    fn lower_function_type() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Function(Arc::new(FunctionExpr {
            parameters: vec![FunctionParam {
                name: Some("x".into()),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                rest: false,
            }],
            return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
            type_parameters: vec![],
        }));
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::Function(func) => {
                assert_eq!(func.signatures.len(), 1);
                assert_eq!(func.signatures[0].parameters.len(), 1);
            }
            _ => panic!("expected Function node"),
        }
    }

    #[test]
    fn lower_ref_with_args() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Ref {
            name: Arc::from("Partial"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
        };
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::Ref {
                name,
                type_arguments,
            } => {
                assert_eq!(name, "Partial");
                assert_eq!(type_arguments.len(), 1);
            }
            _ => panic!("expected Ref node"),
        }
    }

    #[test]
    fn lower_conditional() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            true_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Boolean)),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::Conditional { distributive, .. } => {
                assert!(!distributive); // check is a primitive, not a type param
            }
            _ => panic!("expected Conditional node"),
        }
    }

    #[test]
    fn lower_conditional_distributive() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            })),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Boolean)),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::Conditional { distributive, .. } => {
                assert!(distributive); // check is a type parameter → distributive
            }
            _ => panic!("expected Conditional node"),
        }
    }

    #[test]
    fn lower_keyof() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Primitive(PrimitiveName::Object)));
        let id = lower_type_expr(&mut arena, &expr);
        assert!(matches!(arena.get(id), Node::KeyOf(_)));
    }

    #[test]
    fn lower_indexed_access() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Primitive(PrimitiveName::Object)),
            index: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        };
        let id = lower_type_expr(&mut arena, &expr);
        assert!(matches!(arena.get(id), Node::IndexedAccess { .. }));
    }

    #[test]
    fn lower_mapped_type() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: MappedModifier::Add,
            readonly: MappedModifier::None,
            name_type: None,
        };
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::Mapped {
                parameter,
                optional,
                readonly,
                ..
            } => {
                assert_eq!(parameter, "K");
                assert_eq!(*optional, MappedModifierKind::Add);
                assert_eq!(*readonly, MappedModifierKind::Unchanged);
            }
            _ => panic!("expected Mapped node"),
        }
    }

    #[test]
    fn lower_tuple() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Tuple {
            elements: Arc::from(vec![
                TupleElement {
                    label: Some("first".into()),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                },
                TupleElement {
                    label: None,
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: true,
                    rest: false,
                },
            ]),
            readonly: true,
        };
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::Tuple { elements, readonly } => {
                assert_eq!(elements.len(), 2);
                assert!(readonly);
                assert_eq!(elements[0].label.as_deref(), Some("first"));
                assert!(elements[1].optional);
            }
            _ => panic!("expected Tuple node"),
        }
    }

    #[test]
    fn lower_parenthesized_is_transparent() {
        let mut arena = QueryArena::new();
        let inner = TypeExpr::Primitive(PrimitiveName::String);
        let expr = TypeExpr::Parenthesized(Arc::new(inner.clone()));

        let id_inner = lower_type_expr(&mut arena, &inner);
        let id_paren = lower_type_expr(&mut arena, &expr);

        // Both should produce string primitives (parenthesized is transparent)
        assert!(matches!(
            arena.get(id_inner),
            Node::Primitive(PrimitiveKind::String)
        ));
        assert!(matches!(
            arena.get(id_paren),
            Node::Primitive(PrimitiveKind::String)
        ));
    }

    #[test]
    fn lower_typeof() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::TypeOf(ValueRef {
            path: vec!["ns".into(), "foo".into()],
        });
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::TypeOf { path } => {
                assert_eq!(path, &["ns", "foo"]);
            }
            _ => panic!("expected TypeOf node"),
        }
    }

    #[test]
    fn lower_unknown_becomes_error() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Unknown {
            raw: "some complex syntax".into(),
        };
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::Error { description } => {
                assert_eq!(description, "some complex syntax");
            }
            _ => panic!("expected Error node"),
        }
    }

    #[test]
    fn lower_array() {
        let mut arena = QueryArena::new();
        let expr = TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: true,
        };
        let id = lower_type_expr(&mut arena, &expr);
        match arena.get(id) {
            Node::Array { readonly, .. } => assert!(readonly),
            _ => panic!("expected Array node"),
        }
    }
}
