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
    lower_type_expr_in_scope(arena, expr, None)
}

pub fn lower_type_expr_in_scope(
    arena: &mut QueryArena,
    expr: &TypeExpr,
    scope_canonical_id: Option<&str>,
) -> NodeId {
    match expr {
        TypeExpr::Primitive(prim) => arena.primitive(lower_primitive(*prim)),

        TypeExpr::Literal(lit) => arena.literal(lower_literal(lit)),

        TypeExpr::Union(members) => {
            let ids: Vec<NodeId> = members
                .iter()
                .map(|m| lower_type_expr_in_scope(arena, m, scope_canonical_id))
                .collect();
            arena.union(ids)
        }

        TypeExpr::Intersection(members) => {
            let ids: Vec<NodeId> = members
                .iter()
                .map(|m| lower_type_expr_in_scope(arena, m, scope_canonical_id))
                .collect();
            arena.intersection(ids)
        }

        TypeExpr::Array { element, readonly } => {
            let el = lower_type_expr_in_scope(arena, element, scope_canonical_id);
            arena.array(el, *readonly)
        }

        TypeExpr::Tuple { elements, readonly } => {
            let els: Vec<TupleNodeElement> = elements
                .iter()
                .map(|el| TupleNodeElement {
                    label: el.label.clone(),
                    ty: lower_type_expr_in_scope(arena, &el.ty, scope_canonical_id),
                    optional: el.optional,
                    rest: el.rest,
                })
                .collect();
            arena.alloc(Node::Tuple {
                elements: els,
                readonly: *readonly,
            })
        }

        TypeExpr::Object(obj) => lower_object(arena, obj, scope_canonical_id),

        TypeExpr::Function(func) => lower_function(arena, func, scope_canonical_id),

        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            let args: Vec<NodeId> = type_arguments
                .iter()
                .map(|a| lower_type_expr_in_scope(arena, a, scope_canonical_id))
                .collect();
            arena.scoped_type_ref(name.as_ref(), args, scope_canonical_id.map(str::to_string))
        }

        TypeExpr::TypeParameter(param) => {
            let constraint = param
                .constraint
                .as_ref()
                .map(|c| lower_type_expr_in_scope(arena, c, scope_canonical_id));
            let default = param
                .default
                .as_ref()
                .map(|d| lower_type_expr_in_scope(arena, d, scope_canonical_id));
            arena.alloc(Node::TypeParam {
                name: param.name.clone(),
                constraint,
                default,
            })
        }

        TypeExpr::KeyOf(operand) => {
            let op = lower_type_expr_in_scope(arena, operand, scope_canonical_id);
            arena.key_of(op)
        }

        TypeExpr::TypeOf(value_ref) => arena.alloc(Node::TypeOf {
            path: value_ref.path.clone(),
        }),

        TypeExpr::IndexedAccess { object, index } => {
            let obj = lower_type_expr_in_scope(arena, object, scope_canonical_id);
            let idx = lower_type_expr_in_scope(arena, index, scope_canonical_id);
            arena.indexed_access(obj, idx)
        }

        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_id = lower_type_expr_in_scope(arena, check, scope_canonical_id);
            let extends_id = lower_type_expr_in_scope(arena, extends, scope_canonical_id);
            let true_id = lower_type_expr_in_scope(arena, true_type, scope_canonical_id);
            let false_id = lower_type_expr_in_scope(arena, false_type, scope_canonical_id);
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
            let src = lower_type_expr_in_scope(arena, source, scope_canonical_id);
            let val = lower_type_expr_in_scope(arena, value, scope_canonical_id);
            let nt = name_type
                .as_ref()
                .map(|n| lower_type_expr_in_scope(arena, n, scope_canonical_id));
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
                .map(|e| lower_type_expr_in_scope(arena, e, scope_canonical_id))
                .collect();
            arena.alloc(Node::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: exprs,
            })
        }

        TypeExpr::Infer { name } => arena.alloc(Node::Infer { name: name.clone() }),

        TypeExpr::Rest(inner) => {
            let id = lower_type_expr_in_scope(arena, inner, scope_canonical_id);
            arena.alloc(Node::Rest(id))
        }

        TypeExpr::Parenthesized(inner) => {
            lower_type_expr_in_scope(arena, inner, scope_canonical_id)
        }

        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => {
            use super::arena::{ConditionalBranch, ConditionalFrameSnapshot};
            let args: Vec<NodeId> = type_arguments
                .iter()
                .map(|a| lower_type_expr_in_scope(arena, a, scope_canonical_id))
                .collect();
            let ctx: Vec<ConditionalFrameSnapshot> = conditional_context
                .iter()
                .map(|f| ConditionalFrameSnapshot {
                    branch: match f.branch {
                        crate::analysis::type_expr::RecursiveConditionalBranch::True => {
                            ConditionalBranch::True
                        }
                        crate::analysis::type_expr::RecursiveConditionalBranch::False => {
                            ConditionalBranch::False
                        }
                    },
                    decided: f.decided,
                    check: lower_type_expr_in_scope(arena, &f.check, scope_canonical_id),
                    extends: lower_type_expr_in_scope(arena, &f.extends, scope_canonical_id),
                })
                .collect();
            arena.alloc(Node::RecursiveRef {
                symbol_name: name.to_string(),
                type_arguments: args,
                conditional_context: ctx,
            })
        }

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

fn lower_object(
    arena: &mut QueryArena,
    obj: &ObjectExpr,
    scope_canonical_id: Option<&str>,
) -> NodeId {
    let mut properties = Vec::new();
    let mut index_signatures = Vec::new();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();

    for member in &obj.properties {
        match member {
            ObjectMember::Property(prop) => {
                properties.push(PropertyNode {
                    name: prop.name.clone(),
                    ty: lower_type_expr_in_scope(arena, &prop.ty, scope_canonical_id),
                    optional: prop.optional,
                    readonly: prop.readonly,
                    is_method: false,
                });
            }
            ObjectMember::IndexSignature(idx) => {
                index_signatures.push(IndexSignatureNode {
                    key_type: lower_type_expr_in_scope(arena, &idx.key_type, scope_canonical_id),
                    value_type: lower_type_expr_in_scope(
                        arena,
                        &idx.value_type,
                        scope_canonical_id,
                    ),
                    readonly: idx.readonly,
                });
            }
            ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                let sig = lower_call_signature(arena, func, scope_canonical_id);
                if matches!(member, ObjectMember::ConstructSignature(_)) {
                    construct_signatures.push(sig);
                } else {
                    call_signatures.push(sig);
                }
            }
            ObjectMember::Method(method) => {
                properties.push(PropertyNode {
                    name: method.name.clone(),
                    ty: lower_function(arena, &method.function, scope_canonical_id),
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

fn lower_call_signature(
    arena: &mut QueryArena,
    func: &FunctionExpr,
    scope_canonical_id: Option<&str>,
) -> CallSignatureNode {
    let params: Vec<ParamNode> = func
        .parameters
        .iter()
        .map(|p| ParamNode {
            name: p.name.clone(),
            ty: lower_type_expr_in_scope(arena, &p.ty, scope_canonical_id),
            optional: p.optional,
            rest: p.rest,
        })
        .collect();
    let ret = func
        .return_type
        .as_ref()
        .map(|r| lower_type_expr_in_scope(arena, r, scope_canonical_id))
        .unwrap_or_else(|| arena.primitive(PrimitiveKind::Void));
    let type_params = func
        .type_parameters
        .iter()
        .map(|tp| lower_type_param(arena, tp, scope_canonical_id))
        .collect();
    CallSignatureNode {
        type_parameters: type_params,
        parameters: params,
        return_type: ret,
    }
}

fn lower_function(
    arena: &mut QueryArena,
    func: &FunctionExpr,
    scope_canonical_id: Option<&str>,
) -> NodeId {
    let sig = lower_call_signature(arena, func, scope_canonical_id);
    arena.function(FunctionNode {
        signatures: vec![sig],
    })
}

fn lower_type_param(
    arena: &mut QueryArena,
    param: &TypeParam,
    scope_canonical_id: Option<&str>,
) -> TypeParamNode {
    TypeParamNode {
        name: param.name.clone(),
        constraint: param
            .constraint
            .as_ref()
            .map(|c| lower_type_expr_in_scope(arena, c, scope_canonical_id)),
        default: param
            .default
            .as_ref()
            .map(|d| lower_type_expr_in_scope(arena, d, scope_canonical_id)),
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
                scope_canonical_id,
            } => {
                assert_eq!(name, "Partial");
                assert_eq!(type_arguments.len(), 1);
                assert_eq!(scope_canonical_id, &None);
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
