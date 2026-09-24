//! Class-expression lowering: the content half of the flow lane's class
//! composition ([`SliceExpr::Class`]).
//!
//! A class expression is a VALUE of the frame it is authored in. Its
//! `extends` value, computed keys and static initializers evaluate in that
//! frame; its instance initializers read the frame's lexical scope when an
//! instance is constructed; its methods and accessors are nested function
//! values the function index serves under the frame. Every member lowers
//! to the carrier the evaluator composes TypeScript's class rules over
//! (measured on 7.0.2): an authored type, an initializer evaluated in the
//! frame, or a nested function value whose body-derived return the flow
//! lane infers.

use std::sync::Arc;

use oxc_ast::ast::{
    BindingPattern, ClassElement, Expression, FormalParameters, MethodDefinitionKind, PropertyKey,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use verter_type_expr::{PrimitiveName, TypeExpr};
use verter_type_expr_oxc::{lower_return_annotation, lower_ts_type};

use super::{
    expr_is_widening_nullish, infer_declaration_expression_type,
    leaf_answer_is_fabricated_at_a_call_position, unwrap_parenthesized, DefiningFrameGate,
    ExprMode, FunctionNode, GatedType, LeafCallScanner, Lowerer, SliceClass, SliceClassHeritage,
    SliceClassIndexSignature, SliceClassMember, SliceClassMemberValue, SliceClassParam, SliceExpr,
    SliceObjectKey, SliceTypeParam, TopLevelLiteralPolicy,
};

/// The name the checker prints for a class expression nobody names.
const ANONYMOUS_CLASS: &str = "(Anonymous class)";
/// The name the checker prints for a function expression nobody names.
const ANONYMOUS_FUNCTION: &str = "(Anonymous function)";

/// One method or accessor group of a class body, keyed by its authored
/// spelling and staticness, collected before its visible type is decided.
struct MemberGroup<'a> {
    /// The member's position in the lowered member list.
    slot: usize,
    is_static: bool,
    /// The authored key spelling — what groups an overload set and an
    /// accessor pair.
    spelling: &'a str,
    kind: GroupKind<'a>,
}

enum GroupKind<'a> {
    /// A method: its bodiless overload signatures and its implementation.
    Method {
        overloads: Vec<&'a oxc_ast::ast::MethodDefinition<'a>>,
        implementation: Option<&'a oxc_ast::ast::MethodDefinition<'a>>,
    },
    /// An accessor pair.
    Accessor {
        getter: Option<&'a oxc_ast::ast::MethodDefinition<'a>>,
        setter: Option<&'a oxc_ast::ast::MethodDefinition<'a>>,
    },
}

impl<'s> Lowerer<'s> {
    /// Lower a value a name is assigned to: a class expression takes the
    /// name (`const C = class {}`, `{ C: class {} }`, `C = class {}` are
    /// the checker's `C`); any other value lowers as it always does.
    pub(super) fn lower_assigned_value(
        &mut self,
        expr: &Expression<'_>,
        name: &str,
        mode: ExprMode,
    ) -> SliceExpr {
        match expr {
            Expression::ClassExpression(class) => self.lower_class_expression(class, Some(name)),
            _ => self.lower_expr(expr, mode),
        }
    }

    /// Lower a class EXPRESSION to its value carrier ([`SliceExpr::Class`]).
    ///
    /// The `extends` value lowers as a flow value of this frame (a
    /// parameter, a local, a call, a conditional), and so does a computed
    /// key. A property initializer lowers as a flow value too — the
    /// evaluator reads the frame's bindings at their DECLARED types for it,
    /// the checker's own flow-container rule for a property. A method or
    /// accessor with a body is a nested function value, whose body-derived
    /// return the flow lane infers exactly as it infers an object-literal
    /// method's. Overload signatures, index signatures, `accessor`
    /// properties, the class's own type parameters and a non-public
    /// constructor all lower to what the checker composes from them.
    ///
    /// `assigned_name` is the name the class expression is assigned to,
    /// which names it.
    pub(super) fn lower_class_expression(
        &mut self,
        class: &oxc_ast::ast::Class<'_>,
        assigned_name: Option<&str>,
    ) -> SliceExpr {
        // The class's OWN type parameters bind throughout its body; the
        // evaluator composes their binder environment over the frame's.
        let own_binders: Vec<Arc<str>> = class
            .type_parameters
            .as_deref()
            .map(|clause| {
                clause
                    .params
                    .iter()
                    .map(|param| Arc::from(param.name.name.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        let type_parameters: Vec<SliceTypeParam> = class
            .type_parameters
            .as_deref()
            .map(|clause| {
                clause
                    .params
                    .iter()
                    .map(|param| SliceTypeParam {
                        name: Arc::from(param.name.name.as_str()),
                        constraint: param.constraint.as_ref().map(|constraint| {
                            self.gate(
                                lower_ts_type(constraint, self.source),
                                constraint.span(),
                                &own_binders,
                            )
                        }),
                        default: param.default.as_ref().map(|default| {
                            self.gate(
                                lower_ts_type(default, self.source),
                                default.span(),
                                &own_binders,
                            )
                        }),
                    })
                    .collect()
            })
            .unwrap_or_default();
        // A class's positions lower ungated by the demand selection, like a
        // nested function value's body: the flow skeleton records them as the
        // class site's own footprint, never as sites of their own, so the
        // class value as a whole is what the selection names.
        let selection = self.selection.take();
        let heritage = class.super_class.as_ref().map(|base| SliceClassHeritage {
            base: Box::new(self.lower_expr(
                unwrap_parenthesized(base),
                ExprMode::BindingInit {
                    preserve_literal: true,
                },
            )),
            type_arguments: class
                .super_type_arguments
                .as_ref()
                .map(|arguments| {
                    arguments
                        .params
                        .iter()
                        .map(|argument| {
                            self.gate(
                                lower_ts_type(argument, self.source),
                                argument.span(),
                                &own_binders,
                            )
                        })
                        .collect()
                })
                .unwrap_or_else(|| Arc::from(Vec::new().into_boxed_slice())),
        });
        let mut members: Vec<Option<SliceClassMember>> = Vec::with_capacity(class.body.body.len());
        let mut groups: Vec<MemberGroup<'_>> = Vec::new();
        let mut index_signatures = Vec::new();
        let mut constructor_overloads: Vec<Arc<[SliceClassParam]>> = Vec::new();
        let mut constructor_implementation: Option<Arc<[SliceClassParam]>> = None;
        for element in &class.body.body {
            match element {
                // A static block runs at class evaluation but declares no
                // member.
                ClassElement::StaticBlock(_) => {}
                ClassElement::TSIndexSignature(signature) => {
                    let Some(parameter) = signature.parameters.first() else {
                        continue;
                    };
                    let key = &parameter.type_annotation.type_annotation;
                    let value = &signature.type_annotation.type_annotation;
                    index_signatures.push(SliceClassIndexSignature {
                        is_static: signature.r#static,
                        key: self.gate(lower_ts_type(key, self.source), key.span(), &own_binders),
                        value: self.gate(
                            lower_ts_type(value, self.source),
                            value.span(),
                            &own_binders,
                        ),
                        readonly: signature.readonly,
                    });
                }
                ClassElement::PropertyDefinition(property) => {
                    // A `#private` brand is not a type-level member.
                    if matches!(property.key, PropertyKey::PrivateIdentifier(_)) {
                        continue;
                    }
                    let Some(key) = self.class_member_key(&property.key, property.computed) else {
                        continue;
                    };
                    let value = match (&property.type_annotation, &property.value) {
                        (Some(annotation), _) => SliceClassMemberValue::Declared(self.gate(
                            lower_ts_type(&annotation.type_annotation, self.source),
                            annotation.span,
                            &own_binders,
                        )),
                        (None, Some(initializer)) => {
                            self.lower_class_initializer(initializer, property.readonly)
                        }
                        (None, None) => SliceClassMemberValue::Declared(self.gate(
                            TypeExpr::Primitive(PrimitiveName::Any),
                            property.span,
                            &[],
                        )),
                    };
                    members.push(Some(SliceClassMember {
                        key,
                        is_static: property.r#static,
                        optional: property.optional,
                        readonly: property.readonly,
                        visibility: class_member_visibility(property.accessibility),
                        method_kind: None,
                        spans: verter_type_expr::MemberSpans {
                            declaration: Some(property.span.into()),
                            name: Some(property.key.span().into()),
                            type_annotation: property
                                .type_annotation
                                .as_ref()
                                .map(|annotation| annotation.type_annotation.span().into()),
                        },
                        value,
                    }));
                }
                // An `accessor` property is a get / set pair over one
                // storage slot: to the type system, a property of its
                // annotation, else its initializer's widened type.
                ClassElement::AccessorProperty(property) => {
                    if matches!(property.key, PropertyKey::PrivateIdentifier(_)) {
                        continue;
                    }
                    let Some(key) = self.class_member_key(&property.key, property.computed) else {
                        continue;
                    };
                    let value = match (&property.type_annotation, &property.value) {
                        (Some(annotation), _) => SliceClassMemberValue::Declared(self.gate(
                            lower_ts_type(&annotation.type_annotation, self.source),
                            annotation.span,
                            &own_binders,
                        )),
                        (None, Some(initializer)) => {
                            self.lower_class_initializer(initializer, false)
                        }
                        (None, None) => SliceClassMemberValue::Declared(self.gate(
                            TypeExpr::Primitive(PrimitiveName::Any),
                            property.span,
                            &[],
                        )),
                    };
                    members.push(Some(SliceClassMember {
                        key,
                        is_static: property.r#static,
                        optional: false,
                        readonly: false,
                        visibility: class_member_visibility(property.accessibility),
                        method_kind: None,
                        spans: verter_type_expr::MemberSpans {
                            declaration: Some(property.span.into()),
                            name: Some(property.key.span().into()),
                            type_annotation: property
                                .type_annotation
                                .as_ref()
                                .map(|annotation| annotation.type_annotation.span().into()),
                        },
                        value,
                    }));
                }
                ClassElement::MethodDefinition(method) => {
                    if matches!(method.key, PropertyKey::PrivateIdentifier(_)) {
                        continue;
                    }
                    // The constructor's accessibility is not part of the
                    // constructor type's shape (declaration emit prints a
                    // `private constructor(a)` class's `new (a)`), so every
                    // constructor lowers alike.
                    if method.kind == MethodDefinitionKind::Constructor {
                        let parameters = self.lower_class_constructor(
                            &method.value.params,
                            &own_binders,
                            method.value.body.is_some(),
                            &mut members,
                        );
                        match (method.value.body.is_some(), parameters) {
                            (true, Some(parameters)) => {
                                constructor_implementation = Some(parameters)
                            }
                            (false, Some(parameters)) => constructor_overloads.push(parameters),
                            // A parameter whose type cannot be read without
                            // running code: the constructor has no shape.
                            (_, None) => {
                                self.selection = selection;
                                return SliceExpr::Gap(
                                    crate::semantic_query::FlowGap::UnmodeledExpression,
                                );
                            }
                        }
                        continue;
                    }
                    let spelling = &self.source
                        [method.key.span().start as usize..method.key.span().end as usize];
                    let accessor = matches!(
                        method.kind,
                        MethodDefinitionKind::Get | MethodDefinitionKind::Set
                    );
                    let existing = groups.iter_mut().find(|group| {
                        group.is_static == method.r#static
                            && group.spelling == spelling
                            && matches!(group.kind, GroupKind::Accessor { .. }) == accessor
                    });
                    let group = match existing {
                        Some(group) => group,
                        None => {
                            members.push(None);
                            groups.push(MemberGroup {
                                slot: members.len() - 1,
                                is_static: method.r#static,
                                spelling,
                                kind: if accessor {
                                    GroupKind::Accessor {
                                        getter: None,
                                        setter: None,
                                    }
                                } else {
                                    GroupKind::Method {
                                        overloads: Vec::new(),
                                        implementation: None,
                                    }
                                },
                            });
                            groups.last_mut().expect("just pushed")
                        }
                    };
                    match &mut group.kind {
                        GroupKind::Method {
                            overloads,
                            implementation,
                        } => {
                            if method.value.body.is_some() {
                                *implementation = Some(method);
                            } else {
                                overloads.push(method);
                            }
                        }
                        GroupKind::Accessor { getter, setter } => {
                            if method.kind == MethodDefinitionKind::Get {
                                *getter = Some(method);
                            } else {
                                *setter = Some(method);
                            }
                        }
                    }
                }
            }
        }
        // Each method and accessor group lands at its first declaration's
        // position: an overloaded method as one member per VISIBLE
        // signature (the overloads, never the implementation behind them).
        let mut resolved: Vec<Vec<SliceClassMember>> = Vec::with_capacity(groups.len());
        for group in &groups {
            resolved.push(self.lower_class_member_group(group, &own_binders));
        }
        let mut lowered = Vec::with_capacity(members.len());
        let mut groups_by_slot = groups.iter().zip(resolved).peekable();
        for (slot, member) in members.into_iter().enumerate() {
            match member {
                Some(member) => lowered.push(member),
                None => {
                    if let Some((_, group_members)) =
                        groups_by_slot.next_if(|(group, _)| group.slot == slot)
                    {
                        lowered.extend(group_members);
                    }
                }
            }
        }
        self.selection = selection;
        // The class-evaluation-time positions (decorators, static blocks,
        // static initializers) RUN here, in this frame: their calls take
        // the same certification a leaf-folded class takes, and an
        // unprovable one flags the enclosing statement's typed gap. The
        // `extends` value lowered above as a value of its own, which
        // answers for its own effects.
        let mut scanner = LeafCallScanner::default();
        scanner.visit_class_after_heritage(class);
        self.drain_leaf_call_scanner(scanner);
        let name: Arc<str> = match (&class.id, assigned_name) {
            (Some(id), _) => Arc::from(id.name.as_str()),
            (None, Some(assigned)) => Arc::from(assigned),
            (None, None) => Arc::from(ANONYMOUS_CLASS),
        };
        SliceExpr::Class(Arc::new(SliceClass {
            offset: class.span.start,
            name,
            outer_clauses: self.class_outer_clauses(),
            type_parameters: Arc::from(type_parameters.into_boxed_slice()),
            heritage,
            constructors: match (constructor_overloads.is_empty(), constructor_implementation) {
                (false, _) => Some(Arc::from(constructor_overloads.into_boxed_slice())),
                (true, Some(implementation)) => Some(Arc::from(vec![implementation])),
                (true, None) => None,
            },
            members: Arc::from(lowered.into_boxed_slice()),
            index_signatures: Arc::from(index_signatures.into_boxed_slice()),
        }))
    }

    /// The members one method or accessor group contributes.
    ///
    /// An accessor pair is ONE property whose type is the getter's return
    /// annotation, else the setter's parameter annotation, else the
    /// getter's body-derived return (the checker's `getTypeOfAccessors`
    /// order, measured on 7.0.2), `readonly` when no setter pairs with the
    /// getter. A method is its overload signatures when it declares any,
    /// else its implementation: a nested function value whose signature
    /// the flow lane composes, a body-derived return included.
    fn lower_class_member_group(
        &mut self,
        group: &MemberGroup<'_>,
        own_binders: &[Arc<str>],
    ) -> Vec<SliceClassMember> {
        match &group.kind {
            GroupKind::Accessor { getter, setter } => {
                let Some(first) = getter.or(*setter) else {
                    return Vec::new();
                };
                let Some(key) = self.class_member_key(&first.key, first.computed) else {
                    return Vec::new();
                };
                let getter_annotation = getter.and_then(|getter| getter.value.return_type.as_ref());
                let setter_annotation = setter.and_then(|setter| {
                    setter
                        .value
                        .params
                        .items
                        .first()
                        .and_then(|parameter| parameter.type_annotation.as_ref())
                });
                let value = match (getter_annotation, setter_annotation, getter) {
                    (Some(annotation), _, _) | (None, Some(annotation), _) => {
                        SliceClassMemberValue::Declared(self.gate(
                            lower_ts_type(&annotation.type_annotation, self.source),
                            annotation.span,
                            own_binders,
                        ))
                    }
                    (None, None, Some(getter)) if getter.value.body.is_some() => {
                        match self.lower_nested_function(&FunctionNode::Function(&getter.value)) {
                            SliceExpr::UnmodeledBinding => SliceClassMemberValue::Unmodeled,
                            function => SliceClassMemberValue::Getter(Box::new(function)),
                        }
                    }
                    _ => SliceClassMemberValue::Declared(self.gate(
                        TypeExpr::Primitive(PrimitiveName::Any),
                        first.span,
                        &[],
                    )),
                };
                vec![SliceClassMember {
                    key,
                    is_static: group.is_static,
                    optional: first.optional,
                    readonly: setter.is_none(),
                    visibility: class_member_visibility(first.accessibility),
                    method_kind: None,
                    spans: verter_type_expr::MemberSpans {
                        declaration: Some(first.span.into()),
                        name: Some(first.key.span().into()),
                        type_annotation: None,
                    },
                    value,
                }]
            }
            GroupKind::Method {
                overloads,
                implementation,
            } => {
                let visible: Vec<&oxc_ast::ast::MethodDefinition<'_>> = if overloads.is_empty() {
                    implementation.iter().copied().collect()
                } else {
                    overloads.clone()
                };
                let mut out = Vec::with_capacity(visible.len());
                for method in visible {
                    let Some(key) = self.class_member_key(&method.key, method.computed) else {
                        continue;
                    };
                    let value = if method.value.body.is_some() {
                        match self.lower_nested_function(&FunctionNode::Function(&method.value)) {
                            SliceExpr::UnmodeledBinding => {
                                match self.lower_class_signature(&method.value, own_binders) {
                                    Some(signature) => SliceClassMemberValue::Declared(signature),
                                    None => SliceClassMemberValue::Unmodeled,
                                }
                            }
                            function => SliceClassMemberValue::Method(Box::new(function)),
                        }
                    } else {
                        match self.lower_class_signature(&method.value, own_binders) {
                            Some(signature) => SliceClassMemberValue::Declared(signature),
                            None => SliceClassMemberValue::Unmodeled,
                        }
                    };
                    out.push(SliceClassMember {
                        key,
                        is_static: group.is_static,
                        optional: method.optional,
                        readonly: false,
                        visibility: class_member_visibility(method.accessibility),
                        method_kind: Some(verter_type_expr::ObjectMethodKind::Method),
                        spans: verter_type_expr::MemberSpans {
                            declaration: Some(method.span.into()),
                            name: Some(method.key.span().into()),
                            type_annotation: None,
                        },
                        value,
                    });
                }
                out
            }
        }
    }

    /// A class member's key: a static name, else the computed key's
    /// value as a flow value of this frame (a numeric-literal key's name
    /// is its NUMBER's, which only the value knows). `None` for a key form
    /// that names no member (a `#private` brand).
    fn class_member_key(
        &mut self,
        key: &PropertyKey<'_>,
        computed: bool,
    ) -> Option<SliceObjectKey> {
        match key {
            PropertyKey::StaticIdentifier(id) if !computed => {
                Some(SliceObjectKey::Static(Arc::from(id.name.as_str())))
            }
            PropertyKey::StringLiteral(literal) => {
                Some(SliceObjectKey::Static(Arc::from(literal.value.as_str())))
            }
            PropertyKey::PrivateIdentifier(_) => None,
            _ => {
                let expression = key.as_expression()?;
                Some(SliceObjectKey::Computed {
                    value: Box::new(self.lower_expr(
                        expression,
                        ExprMode::BindingInit {
                            preserve_literal: true,
                        },
                    )),
                    authored: verter_type_expr_oxc::lower_property_key(key, self.source),
                })
            }
        }
    }

    /// A property initializer's value: the initializer as a flow value of
    /// this frame, widened unless the property is `readonly`. With
    /// `strictNullChecks` off a bare `null` / `undefined` initializer is the
    /// widening nullable type, which the property's widening turns into
    /// `any`.
    fn lower_class_initializer(
        &mut self,
        initializer: &Expression<'_>,
        readonly: bool,
    ) -> SliceClassMemberValue {
        if !self.nullability.is_strict() && expr_is_widening_nullish(initializer) {
            return SliceClassMemberValue::Declared(self.gate(
                TypeExpr::Primitive(PrimitiveName::Any),
                initializer.span(),
                &[],
            ));
        }
        SliceClassMemberValue::Initializer {
            value: Box::new(self.lower_expr(
                initializer,
                ExprMode::BindingInit {
                    preserve_literal: readonly,
                },
            )),
            widen: !readonly,
        }
    }

    /// A class expression's declared constructor parameters. A parameter
    /// property (`constructor(public a: string)`) of the IMPLEMENTATION
    /// also declares an instance member of the parameter's type. `None`
    /// when a parameter's type cannot be read without running code.
    fn lower_class_constructor(
        &self,
        params: &FormalParameters<'_>,
        own_binders: &[Arc<str>],
        implementation: bool,
        members: &mut Vec<Option<SliceClassMember>>,
    ) -> Option<Arc<[SliceClassParam]>> {
        let lowered = self.lower_class_signature_params(params)?;
        let mut out = Vec::with_capacity(lowered.len());
        for (index, (name, ty, optional, rest, span)) in lowered.into_iter().enumerate() {
            let ty = self.gate(ty, span, own_binders);
            if let (true, Some(parameter)) = (implementation, params.items.get(index)) {
                let is_property =
                    parameter.accessibility.is_some() || parameter.readonly || parameter.r#override;
                if let (true, Some(key)) = (is_property, name.as_ref()) {
                    members.push(Some(SliceClassMember {
                        key: SliceObjectKey::Static(Arc::clone(key)),
                        is_static: false,
                        optional: parameter.optional,
                        readonly: parameter.readonly,
                        visibility: class_member_visibility(parameter.accessibility),
                        method_kind: None,
                        spans: verter_type_expr::MemberSpans {
                            declaration: Some(parameter.span.into()),
                            name: Some(parameter.pattern.span().into()),
                            type_annotation: parameter
                                .type_annotation
                                .as_ref()
                                .map(|annotation| annotation.type_annotation.span().into()),
                        },
                        value: SliceClassMemberValue::Declared(ty.clone()),
                    }));
                }
            }
            out.push(SliceClassParam {
                name,
                ty,
                optional,
                rest,
            });
        }
        Some(Arc::from(out.into_boxed_slice()))
    }

    /// A method signature composed from its annotations — an overload
    /// signature, or an implementation the function index does not serve:
    /// its own type parameters, its parameters, and its return annotation
    /// (`any` when it has none). `None` for a `this` parameter or a
    /// parameter whose type cannot be read without running code.
    fn lower_class_signature(
        &self,
        function: &oxc_ast::ast::Function<'_>,
        own_binders: &[Arc<str>],
    ) -> Option<GatedType> {
        if function.this_param.is_some() {
            return None;
        }
        let type_parameters: Vec<verter_type_expr::TypeParam> = function
            .type_parameters
            .as_deref()
            .map(|clause| {
                clause
                    .params
                    .iter()
                    .map(|param| verter_type_expr::TypeParam {
                        name: param.name.name.to_string(),
                        constraint: param
                            .constraint
                            .as_ref()
                            .map(|constraint| Arc::new(lower_ts_type(constraint, self.source))),
                        default: param
                            .default
                            .as_ref()
                            .map(|default| Arc::new(lower_ts_type(default, self.source))),
                        is_const: param.r#const,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (return_type, predicate) = match function.return_type.as_ref() {
            Some(annotation) => lower_return_annotation(&annotation.type_annotation, self.source),
            None => (TypeExpr::Primitive(PrimitiveName::Any), None),
        };
        let parameters = self
            .lower_class_signature_params(&function.params)?
            .into_iter()
            .map(|(name, ty, optional, rest, _)| {
                verter_type_expr::FunctionParam::synthetic(
                    name.map(|name| name.to_string()),
                    ty,
                    optional,
                    rest,
                )
            })
            .collect();
        let mut signature = verter_type_expr::FunctionExpr::with_spans(
            parameters,
            Some(Arc::new(return_type)),
            type_parameters,
            verter_type_expr::FunctionSpans {
                signature: Some(function.span.into()),
                return_type: function
                    .return_type
                    .as_ref()
                    .map(|annotation| annotation.type_annotation.span().into()),
            },
        );
        signature.predicate = predicate;
        // The method's own clause binds inside its signature, over the
        // class's.
        let mut binders = own_binders.to_vec();
        binders.extend(
            signature
                .type_parameters
                .iter()
                .map(|param| Arc::from(param.name.as_str())),
        );
        Some(self.gate(
            TypeExpr::Function(Arc::new(signature)),
            function.span,
            &binders,
        ))
    }

    /// The parameters of a class member's signature: each one's annotation,
    /// else its default initializer's widened type, else `any` (an array of
    /// `any` for a rest parameter). `None` when a default initializer's
    /// type composes over a call.
    #[allow(clippy::type_complexity)]
    fn lower_class_signature_params(
        &self,
        params: &FormalParameters<'_>,
    ) -> Option<Vec<(Option<Arc<str>>, TypeExpr, bool, bool, oxc_span::Span)>> {
        let name_of = |pattern: &BindingPattern<'_>| match pattern {
            BindingPattern::BindingIdentifier(id) => Some(Arc::<str>::from(id.name.as_str())),
            _ => None,
        };
        let mut out = Vec::with_capacity(params.items.len() + usize::from(params.rest.is_some()));
        for param in &params.items {
            let ty = match (&param.type_annotation, &param.initializer) {
                (Some(annotation), _) => lower_ts_type(&annotation.type_annotation, self.source),
                (None, Some(initializer)) => {
                    let ty = infer_declaration_expression_type(
                        initializer,
                        self.source,
                        TopLevelLiteralPolicy::Widen,
                    )
                    .ok()?;
                    if leaf_answer_is_fabricated_at_a_call_position(&ty, initializer) {
                        return None;
                    }
                    ty
                }
                (None, None) => TypeExpr::Primitive(PrimitiveName::Any),
            };
            out.push((
                name_of(&param.pattern),
                ty,
                param.optional || param.initializer.is_some(),
                false,
                param.span,
            ));
        }
        if let Some(rest) = &params.rest {
            let ty = match &rest.type_annotation {
                Some(annotation) => lower_ts_type(&annotation.type_annotation, self.source),
                None => TypeExpr::Array {
                    element: Arc::new(TypeExpr::Primitive(PrimitiveName::Any)),
                    readonly: false,
                },
            };
            out.push((name_of(&rest.rest.argument), ty, false, true, rest.span));
        }
        Some(out)
    }

    /// The type-parameter clauses enclosing a class expression authored in
    /// this frame, outermost first, each named the way the checker prints
    /// its declaration when it qualifies a reference to the class.
    ///
    /// Every frame from the served function inward contributes: a class
    /// member's class clause (`GHolder`), then the frame's own clause
    /// (`outer`, `Holder.make`, `arrow`, `inner`).
    fn class_outer_clauses(&self) -> Arc<[crate::semantic_query::ClassExpressionClause]> {
        let mut frames: Vec<&DefiningFrameGate> = vec![&self.frame_gate];
        let mut current = self.frame_gate.outer.enclosing.as_deref();
        while let Some(frame) = current {
            frames.push(&frame.gate);
            current = frame.gate.outer.enclosing.as_deref();
        }
        frames.reverse();
        let mut clauses = Vec::new();
        for gate in frames {
            if gate.type_parameters.is_empty() && gate.enclosing_type_parameters.is_empty() {
                continue;
            }
            let (container, class) = self
                .program
                .body
                .get(self.contributor as usize)
                .and_then(|statement| ClauseContainerFinder::find(statement, gate.anchor))
                .unwrap_or_else(|| (Arc::from(ANONYMOUS_FUNCTION), None));
            if !gate.enclosing_type_parameters.is_empty() {
                clauses.push(crate::semantic_query::ClassExpressionClause {
                    container: class.unwrap_or_else(|| Arc::from(ANONYMOUS_CLASS)),
                    parameters: Arc::clone(&gate.enclosing_type_parameters),
                });
            }
            if !gate.type_parameters.is_empty() {
                clauses.push(crate::semantic_query::ClassExpressionClause {
                    container,
                    parameters: Arc::clone(&gate.type_parameters),
                });
            }
        }
        Arc::from(clauses.into_boxed_slice())
    }
}

impl<'a> LeafCallScanner<'a> {
    /// [`Visit::visit_class`] without the `extends` value — the class
    /// lowering evaluates that one as a value of its own.
    fn visit_class_after_heritage(&mut self, it: &oxc_ast::ast::Class<'a>) {
        self.class_nesting += 1;
        self.visit_decorators(&it.decorators);
        self.nested_frame_nesting += 1;
        if let Some(id) = &it.id {
            self.visit_binding_identifier(id);
        }
        if let Some(type_parameters) = &it.type_parameters {
            self.visit_ts_type_parameter_declaration(type_parameters);
        }
        if let Some(super_type_arguments) = &it.super_type_arguments {
            self.visit_ts_type_parameter_instantiation(super_type_arguments);
        }
        self.visit_ts_class_implements_list(&it.implements);
        self.visit_class_body(&it.body);
        self.nested_frame_nesting -= 1;
        self.class_nesting -= 1;
    }
}

/// A class member's declared accessibility.
fn class_member_visibility(
    accessibility: Option<oxc_ast::ast::TSAccessibility>,
) -> verter_type_expr::MemberVisibility {
    match accessibility {
        Some(oxc_ast::ast::TSAccessibility::Private) => verter_type_expr::MemberVisibility::Private,
        Some(oxc_ast::ast::TSAccessibility::Protected) => {
            verter_type_expr::MemberVisibility::Protected
        }
        Some(oxc_ast::ast::TSAccessibility::Public) | None => {
            verter_type_expr::MemberVisibility::Public
        }
    }
}

/// A property key's static name, when it has one.
fn static_key_name(key: &PropertyKey<'_>, computed: bool) -> Option<Arc<str>> {
    match key {
        PropertyKey::StaticIdentifier(id) if !computed => Some(Arc::from(id.name.as_str())),
        PropertyKey::StringLiteral(literal) => Some(Arc::from(literal.value.as_str())),
        _ => None,
    }
}

/// The name the checker prints for the declaration owning one function's
/// type-parameter clause (`getNameOfSymbolAsWritten` over the symbol's
/// container chain, measured on 7.0.2), found by the function's start
/// offset in its contributing statement.
///
/// A function declaration is its own name (a namespace member too: `N.make`
/// prints `make`); a function or arrow expression its own name, else the
/// name it is assigned to (`const arrow = <T>() => …` is `arrow`, `{ f: <T>()
/// => … }` is `f`), else `(Anonymous function)`; a class method
/// `Class.method`; an object-literal method `o.m` when a variable holds the
/// literal, else `m`. A class member also names its class — the
/// declaration of the class clause its body sees.
#[derive(Default)]
struct ClauseContainerFinder {
    target: u32,
    /// Names assigned to values, by the value's span; `true` when a
    /// variable declarator assigned it.
    assigned: Vec<(oxc_span::Span, Arc<str>, bool)>,
    /// The enclosing classes' printed names, innermost last.
    classes: Vec<Arc<str>>,
    /// The enclosing object literals' holding variables, innermost last.
    objects: Vec<Option<Arc<str>>>,
    found: Option<(Arc<str>, Option<Arc<str>>)>,
}

impl ClauseContainerFinder {
    /// The container name of the function starting at `target` in
    /// `statement`, and its class's name when it is a class member.
    fn find(
        statement: &oxc_ast::ast::Statement<'_>,
        target: u32,
    ) -> Option<(Arc<str>, Option<Arc<str>>)> {
        let mut finder = Self {
            target,
            ..Self::default()
        };
        finder.visit_statement(statement);
        finder.found
    }

    fn assigned_to(&self, span: oxc_span::Span, declarator_only: bool) -> Option<Arc<str>> {
        self.assigned
            .iter()
            .rev()
            .find(|(value, _, declarator)| *value == span && (*declarator || !declarator_only))
            .map(|(_, name, _)| Arc::clone(name))
    }

    fn found_function(&mut self, start: u32, own: Option<&str>, span: oxc_span::Span) {
        if start != self.target || self.found.is_some() {
            return;
        }
        let name = own
            .map(Arc::from)
            .or_else(|| self.assigned_to(span, false))
            .unwrap_or_else(|| Arc::from(ANONYMOUS_FUNCTION));
        self.found = Some((name, None));
    }
}

impl<'a> Visit<'a> for ClauseContainerFinder {
    fn visit_variable_declarator(&mut self, it: &oxc_ast::ast::VariableDeclarator<'a>) {
        if let (BindingPattern::BindingIdentifier(id), Some(init)) = (&it.id, &it.init) {
            self.assigned
                .push((init.span(), Arc::from(id.name.as_str()), true));
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_object_property(&mut self, it: &oxc_ast::ast::ObjectProperty<'a>) {
        let key = static_key_name(&it.key, it.computed);
        if it.method || it.kind != oxc_ast::ast::PropertyKind::Init {
            if let (Some(key), Expression::FunctionExpression(function)) = (&key, &it.value) {
                if function.span.start == self.target && self.found.is_none() {
                    let container = match self.objects.last().cloned().flatten() {
                        Some(object) => Arc::from(format!("{object}.{key}")),
                        None => Arc::clone(key),
                    };
                    self.found = Some((container, None));
                }
            }
        } else if let Some(key) = key {
            self.assigned.push((it.value.span(), key, false));
        }
        walk::walk_object_property(self, it);
    }

    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        let name = match &it.left {
            oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                Some(identifier.name.as_str())
            }
            oxc_ast::ast::AssignmentTarget::StaticMemberExpression(member) => {
                Some(member.property.name.as_str())
            }
            _ => None,
        };
        if let Some(name) = name {
            self.assigned
                .push((it.right.span(), Arc::from(name), false));
        }
        walk::walk_assignment_expression(self, it);
    }

    fn visit_object_expression(&mut self, it: &oxc_ast::ast::ObjectExpression<'a>) {
        let holder = self.assigned_to(it.span, true);
        self.objects.push(holder);
        walk::walk_object_expression(self, it);
        self.objects.pop();
    }

    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'a>) {
        let name = it
            .id
            .as_ref()
            .map(|id| Arc::from(id.name.as_str()))
            .or_else(|| self.assigned_to(it.span, false))
            .unwrap_or_else(|| Arc::from(ANONYMOUS_CLASS));
        self.classes.push(name);
        walk::walk_class(self, it);
        self.classes.pop();
    }

    fn visit_method_definition(&mut self, it: &oxc_ast::ast::MethodDefinition<'a>) {
        if it.value.span.start == self.target && self.found.is_none() {
            let class = self
                .classes
                .last()
                .cloned()
                .unwrap_or_else(|| Arc::from(ANONYMOUS_CLASS));
            let method = static_key_name(&it.key, it.computed)
                .unwrap_or_else(|| Arc::from(ANONYMOUS_FUNCTION));
            self.found = Some((Arc::from(format!("{class}.{method}")), Some(class)));
        }
        walk::walk_method_definition(self, it);
    }

    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        // An anonymous function DECLARATION is a default export's.
        let own = match (&it.id, it.r#type) {
            (Some(id), _) => Some(id.name.as_str()),
            (None, oxc_ast::ast::FunctionType::FunctionDeclaration) => Some("default"),
            (None, _) => None,
        };
        self.found_function(it.span.start, own, it.span);
        walk::walk_function(self, it, flags);
    }

    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.found_function(it.span.start, None, it.span);
        walk::walk_arrow_function_expression(self, it);
    }
}
