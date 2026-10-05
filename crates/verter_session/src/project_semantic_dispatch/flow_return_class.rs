//! Class-expression evaluation: the flow evaluator's composition of
//! TypeScript's class rules over a lowered class body
//! ([`verter_session_query::flow::slice::SliceClass`]), measured on 7.0.2 — and
//! the late-bound computed-key rule a class side shares with an object
//! literal (an open-typed key feeds its side's implicit index signature).

use std::sync::Arc;

use super::{
    signature_answer_is_frame_shadowed, FlowBinderEnv, FlowDemandDriver, FlowEvaluator,
    FlowProductSubject, Positional,
};
use crate::semantic_query::{
    AuthoredPropertyKey, PrimitiveKind, SemanticNodeData, SemanticNodeId, SurfaceMember,
};
use verter_session_query::flow::slice::{
    SliceClass, SliceClassMemberValue, SliceExpr, SliceObjectKey,
};

/// The instance a class expression's members run against while the class
/// evaluates them: its polymorphic `this` binder, and the instance members
/// evaluated so far, which a member body reads through `this`.
pub(super) struct ClassReceiver {
    /// The class's polymorphic `this`.
    pub(super) binder: SemanticNodeId,
    /// The instance members evaluated so far, in evaluation order.
    pub(super) members: std::cell::RefCell<Vec<SurfaceMember>>,
    /// Every statically named instance member the class declares.
    pub(super) declared: rustc_hash::FxHashSet<Arc<str>>,
    /// The base instance, when the class extends one.
    pub(super) base: Option<SemanticNodeId>,
    /// Set when a member body read a declared member not evaluated yet.
    pub(super) forward_read: std::cell::Cell<bool>,
}

/// A class expression's evaluation in progress, stepped by
/// [`FlowEvaluator::class_eval_step`]: its environment and base, its
/// receiver, the member order, and the child it waits on.
pub(super) struct ClassEvalFrame<'e> {
    class: &'e SliceClass,
    class_env: Option<Arc<FlowBinderEnv>>,
    base: Option<ClassBase>,
    receiver: Option<std::rc::Rc<ClassReceiver>>,
    /// The frame's receiver before the class's, given back when the class
    /// completes.
    enclosing_receiver: Option<Option<std::rc::Rc<ClassReceiver>>>,
    order: Vec<usize>,
    next: usize,
    again: Vec<usize>,
    again_next: usize,
    values: Vec<Option<SemanticNodeId>>,
    awaiting: ClassAwait<'e>,
}

/// The child a [`ClassEvalFrame`] waits on.
enum ClassAwait<'e> {
    Start,
    Nothing,
    Heritage {
        holds_before: usize,
    },
    /// Member `index`'s value, the frame's degradation before it, and
    /// whether it is the second evaluation of a function that read ahead.
    Member {
        index: usize,
        degradation: Option<crate::semantic_query::FlowReturnDegradation>,
        again: bool,
        value: MemberAwait<'e>,
    },
}

/// What a member's awaited child is.
enum MemberAwait<'e> {
    Initializer {
        holds_before: usize,
        declared_reads: bool,
        widen: bool,
        expr: &'e SliceExpr,
    },
    Method,
    Getter,
}

/// What a class expression's evaluation needs next.
pub(super) enum ClassStep<'e> {
    /// This expression's value.
    Descend(&'e SliceExpr),
    /// This member function's signature.
    Nested(super::nested::NestedDemand),
    /// Nothing: the class's value.
    Done(Positional<SemanticNodeId>),
}

/// Whether a member's value is a function evaluated after the others.
fn is_class_function(value: &SliceClassMemberValue) -> bool {
    matches!(
        value,
        SliceClassMemberValue::Method(_) | SliceClassMemberValue::Getter(_)
    )
}

/// What a class expression's `extends` value provides to the class.
struct ClassBase {
    /// The parameters of each accepted base construct signature, in order —
    /// a constructor-less class inherits one construct signature per entry.
    constructor_params: Vec<Arc<[crate::semantic_query::FunctionParam]>>,
    /// The base instance type: the first accepted construct signature's
    /// result, when it is a valid base type. A union result is not
    /// (`isValidBaseType`): the class then inherits no instance member,
    /// though its default construct signatures still follow the base's.
    instance: Option<SemanticNodeId>,
    /// The base constructor's static members.
    static_members: Vec<SurfaceMember>,
    /// The base constructor's type variable, when the class extends one.
    type_variable: Option<SemanticNodeId>,
    /// The accessibility of the declaration behind the base's first
    /// accepted construct signature — what a constructor-less class's own
    /// construct signatures carry.
    constructor_visibility: Option<verter_type_expr::MemberVisibility>,
}

/// The key kinds a computed member whose key is not a single literal or
/// unique symbol contributes an implicit index signature for — the
/// checker's `getIndexInfosOfIndexSymbol` over a class's late-bound
/// members.
#[derive(Default)]
pub(super) struct ComputedIndexKinds {
    string: Option<bool>,
    number: Option<bool>,
    symbol: Option<bool>,
    /// The values of the computed members that contribute, with the kind
    /// their key named.
    values: Vec<(ComputedKeyKind, SemanticNodeId)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ComputedKeyKind {
    String,
    Number,
    Symbol,
}

impl ComputedIndexKinds {
    /// Record one contribution; the implied signature stays `readonly`
    /// only while every contributor of its kind is.
    pub(super) fn record(&mut self, kind: ComputedKeyKind, readonly: bool, value: SemanticNodeId) {
        let slot = match kind {
            ComputedKeyKind::String => &mut self.string,
            ComputedKeyKind::Number => &mut self.number,
            ComputedKeyKind::Symbol => &mut self.symbol,
        };
        *slot = Some(slot.unwrap_or(true) && readonly);
        self.values.push((kind, value));
    }

    /// Whether any late-bound member was recorded.
    pub(super) fn any(&self) -> bool {
        !self.values.is_empty()
    }
}

/// One member surface under composition — a class's instance or static
/// side, or an object literal — with its late-bound members.
#[derive(Default)]
pub(super) struct MemberSide {
    pub(super) members: Vec<SurfaceMember>,
    pub(super) index_signatures: Vec<crate::semantic_query::IndexSignature>,
    pub(super) computed: ComputedIndexKinds,
}

impl<'d, 'b, D: FlowDemandDriver> FlowEvaluator<'d, 'b, D> {
    /// Evaluate a class EXPRESSION to its value: the class's constructor
    /// type, composed by TypeScript's class rules (measured on 7.0.2).
    ///
    /// - The INSTANCE type is the class's own identity
    ///   ([`SemanticNodeData::ClassExpressionInstance`]) — a reference whose
    ///   type arguments are the outer and own type parameters where the
    ///   class is authored — over its instance surface: the base instance
    ///   intersected with the own members (heritage first, so an own member
    ///   shadows an inherited one). The base instance is the result of the
    ///   FIRST base construct signature that accepts the `extends` type
    ///   arguments (`resolveBaseTypesOfClass`).
    /// - The CONSTRUCTOR type carries the construct signatures — the
    ///   declared constructor's visible signatures, else one per accepted
    ///   base construct signature with its parameters
    ///   (`getDefaultConstructSignatures`), else `new () =>` — each generic
    ///   over the class's own type parameters and returning the instance,
    ///   over the static members (own ones shadowing the base
    ///   constructor's).
    /// - A class that extends a TYPE VARIABLE (`class extends Base` over
    ///   `Base: S`) is the constructor type intersected with that variable
    ///   (`getBaseTypeVariableOfClass`) — the mixin form, whose construct
    ///   signatures the intersection rules compose.
    /// - A member's type is its annotation, its initializer's value (the
    ///   frame's bindings read at their DECLARED types, widened unless
    ///   `readonly`), or its nested function value's signature (a getter's
    ///   return). A computed key that settles to a literal or unique symbol
    ///   names its member; any other contributes to the side's implicit
    ///   index signature, whose value is every applicable member's type.
    ///
    /// A base the class cannot be composed over (an unevaluable `extends`
    /// value, `any`, a base with no accepted construct signature) is the
    /// unmodelled position; a member whose type is not modelled keeps its
    /// key over the typed marker.
    pub(super) fn eval_class_value(&mut self, class: &SliceClass) -> Positional<SemanticNodeId> {
        // [`Self::eval_expr`] steps a class from its own stack; here the same
        // steps run with each child evaluated in place.
        let mut frame = self.class_eval_frame(class);
        let mut delivered = None;
        loop {
            match self.class_eval_step(&mut frame, delivered.take()) {
                ClassStep::Done(value) => return value,
                ClassStep::Descend(child) => delivered = Some(self.eval_expr(child)),
                ClassStep::Nested(demand) => {
                    delivered = Some(Positional::Value(self.eval_nested_demand(demand)))
                }
            }
        }
    }

    /// A class expression's evaluation, begun: the class's OWN type
    /// parameters bind throughout its body, composed over the frame's
    /// environment.
    pub(super) fn class_eval_frame<'e>(&mut self, class: &'e SliceClass) -> ClassEvalFrame<'e> {
        let class_env = (!class.type_parameters.is_empty()).then(|| {
            Arc::new(self.dispatch.flow_binder_env(
                self.canonical,
                self.owner,
                &class.type_parameters,
                Some(self.binder_env),
                Some(class.offset),
            ))
        });
        ClassEvalFrame {
            class,
            class_env,
            base: None,
            receiver: None,
            enclosing_receiver: None,
            order: Vec::new(),
            next: 0,
            again: Vec::new(),
            again_next: 0,
            values: vec![None; class.members.len()],
            awaiting: ClassAwait::Start,
        }
    }

    /// Step a class expression's evaluation until it needs its `extends`
    /// value, a member initializer's value or a member function's signature
    /// (whose body evaluates from the stack of evaluators), or is done, with
    /// the child it asked for last `delivered`.
    ///
    /// Instance members run against the class's receiver: the non-function
    /// members first, then the functions, so a member body reads a sibling
    /// through `this`; a function that read a declared sibling not evaluated
    /// yet is evaluated again once every other member has been.
    pub(super) fn class_eval_step<'e>(
        &mut self,
        frame: &mut ClassEvalFrame<'e>,
        delivered: Option<Positional<SemanticNodeId>>,
    ) -> ClassStep<'e> {
        let class = frame.class;
        match std::mem::replace(&mut frame.awaiting, ClassAwait::Nothing) {
            ClassAwait::Start => match &class.heritage {
                // A hold inside the `extends` value is not this class's
                // value: the class cannot be composed over a provisional base.
                Some(heritage) => {
                    frame.awaiting = ClassAwait::Heritage {
                        holds_before: self.holds.len(),
                    };
                    return ClassStep::Descend(&heritage.base);
                }
                None => self.class_begin_members(frame),
            },
            ClassAwait::Heritage { holds_before } => {
                let heritage = class.heritage.as_ref().expect("the class's heritage");
                self.holds.truncate(holds_before);
                let Some(Positional::Value(constructor)) = delivered else {
                    return ClassStep::Done(Positional::Unmodeled);
                };
                let frame_env = self.binder_env;
                let env: &FlowBinderEnv = frame.class_env.as_deref().unwrap_or(frame_env);
                let mut type_arguments = Vec::with_capacity(heritage.type_arguments.len());
                for argument in heritage.type_arguments.iter() {
                    if signature_answer_is_frame_shadowed(self.dispatch, env, argument) {
                        return ClassStep::Done(Positional::Unmodeled);
                    }
                    type_arguments.push(self.lower_type_in(env, argument.ty()));
                }
                match self.class_base(constructor, &type_arguments) {
                    Some(base) => frame.base = Some(base),
                    None => return ClassStep::Done(Positional::Unmodeled),
                }
                self.class_begin_members(frame);
            }
            ClassAwait::Member {
                index,
                degradation,
                again,
                value,
            } => {
                let outcome = delivered.expect("the member's outcome");
                let node = match value {
                    MemberAwait::Initializer {
                        holds_before,
                        declared_reads,
                        widen,
                        expr,
                    } => {
                        let node = self.settle_composite_part(outcome, holds_before);
                        self.declared_reads = declared_reads;
                        if widen {
                            self.widen_value_position_read(expr, node)
                        } else {
                            node
                        }
                    }
                    MemberAwait::Method => match outcome {
                        Positional::Value(signature) => signature,
                        Positional::Hold | Positional::Unmodeled => self.unmodeled_position(),
                    },
                    MemberAwait::Getter => {
                        let return_type = match outcome {
                            Positional::Value(signature) => {
                                match self.dispatch.graph().node_data(signature).as_deref() {
                                    Some(SemanticNodeData::Signature { return_type, .. }) => {
                                        Some(*return_type)
                                    }
                                    _ => None,
                                }
                            }
                            Positional::Hold | Positional::Unmodeled => None,
                        };
                        match return_type {
                            Some(return_type) => return_type,
                            None => self.unmodeled_position(),
                        }
                    }
                };
                self.class_member_valued(frame, index, degradation, again, node);
            }
            ClassAwait::Nothing => unreachable!("an outcome delivered to no request"),
        }
        self.class_next_member(frame)
    }

    /// Open the class's receiver and order its members.
    fn class_begin_members(&mut self, frame: &mut ClassEvalFrame<'_>) {
        let class = frame.class;
        let receiver = std::rc::Rc::new(ClassReceiver {
            binder: self
                .dispatch
                .this_binder(self.canonical, self.owner, &class.name, None),
            members: std::cell::RefCell::new(Vec::new()),
            declared: class
                .members
                .iter()
                .filter(|member| !member.is_static)
                .filter_map(|member| match &member.key {
                    SliceObjectKey::Static(name) => Some(Arc::clone(name)),
                    SliceObjectKey::Computed { .. } => None,
                })
                .collect(),
            base: frame.base.as_ref().and_then(|base| base.instance),
            forward_read: std::cell::Cell::new(false),
        });
        frame.enclosing_receiver = Some(self.receiver.replace(receiver.clone()));
        frame.receiver = Some(receiver);
        frame.order = (0..class.members.len())
            .filter(|index| !is_class_function(&class.members[*index].value))
            .chain(
                (0..class.members.len())
                    .filter(|index| is_class_function(&class.members[*index].value)),
            )
            .collect();
    }

    /// Record one member's evaluated value, or queue a function that read a
    /// declared sibling not evaluated yet to evaluate again.
    fn class_member_valued(
        &mut self,
        frame: &mut ClassEvalFrame<'_>,
        index: usize,
        degradation: Option<crate::semantic_query::FlowReturnDegradation>,
        again: bool,
        value: SemanticNodeId,
    ) {
        let member = &frame.class.members[index];
        let receiver = frame.receiver.clone().expect("the class's receiver");
        if !again && receiver.forward_read.get() && is_class_function(&member.value) {
            self.degradation = degradation;
            frame.again.push(index);
            return;
        }
        self.record_receiver_member(&receiver, member, value);
        frame.values[index] = Some(value);
    }

    /// Evaluate the class's next member, or finish the class.
    fn class_next_member<'e>(&mut self, frame: &mut ClassEvalFrame<'e>) -> ClassStep<'e> {
        let class = frame.class;
        loop {
            let (index, again) = if let Some(&index) = frame.order.get(frame.next) {
                frame.next += 1;
                if let Some(receiver) = frame.receiver.as_ref() {
                    receiver.forward_read.set(false);
                }
                (index, false)
            } else if let Some(&index) = frame.again.get(frame.again_next) {
                frame.again_next += 1;
                (index, true)
            } else {
                self.receiver = frame
                    .enclosing_receiver
                    .take()
                    .expect("the class's enclosing receiver");
                return ClassStep::Done(self.finish_class_value(frame));
            };
            let degradation = self.degradation;
            let frame_env = self.binder_env;
            let value = match &class.members[index].value {
                SliceClassMemberValue::Declared(ty) => {
                    let env: &FlowBinderEnv = frame.class_env.as_deref().unwrap_or(frame_env);
                    self.lower_gated_in(env, ty)
                }
                SliceClassMemberValue::Initializer { value, widen } => {
                    frame.awaiting = ClassAwait::Member {
                        index,
                        degradation,
                        again,
                        value: MemberAwait::Initializer {
                            holds_before: self.holds.len(),
                            declared_reads: std::mem::replace(&mut self.declared_reads, true),
                            widen: *widen,
                            expr: value,
                        },
                    };
                    return ClassStep::Descend(value);
                }
                SliceClassMemberValue::Method(function)
                | SliceClassMemberValue::Getter(function) => {
                    let getter =
                        matches!(class.members[index].value, SliceClassMemberValue::Getter(_));
                    // A member's nested function value, evaluated under the
                    // class's binder environment.
                    match super::nested::NestedDemand::of(function, None) {
                        Some((mut demand, gap)) => {
                            if let Some(gap) = gap {
                                self.record_degradation(
                                    crate::semantic_query::FlowReturnDegradation::FlowGap(gap),
                                );
                            }
                            demand.outer_env = frame.class_env.clone();
                            frame.awaiting = ClassAwait::Member {
                                index,
                                degradation,
                                again,
                                value: if getter {
                                    MemberAwait::Getter
                                } else {
                                    MemberAwait::Method
                                },
                            };
                            return ClassStep::Nested(demand);
                        }
                        None => self.unmodeled_position(),
                    }
                }
                SliceClassMemberValue::Unmodeled => self.unmodeled_position(),
            };
            self.class_member_valued(frame, index, degradation, again, value);
        }
    }

    /// A class whose members all evaluated: its keys, sides, identity,
    /// construct signatures and constructor type composed.
    fn finish_class_value(&mut self, frame: &mut ClassEvalFrame<'_>) -> Positional<SemanticNodeId> {
        let class = frame.class;
        let class_env = frame.class_env.take();
        let base = frame.base.take();
        let values: Vec<SemanticNodeId> = std::mem::take(&mut frame.values)
            .into_iter()
            .map(|value| value.expect("every member evaluated"))
            .collect();
        let graph = self.dispatch.graph();
        let frame_env = self.binder_env;
        let env: &FlowBinderEnv = class_env.as_deref().unwrap_or(frame_env);
        let mut instance_side = MemberSide::default();
        let mut static_side = MemberSide::default();
        for (member, value) in class.members.iter().zip(values) {
            let side = if member.is_static {
                &mut static_side
            } else {
                &mut instance_side
            };
            let key = match &member.key {
                SliceObjectKey::Static(name) => AuthoredPropertyKey::string(name.as_ref()),
                SliceObjectKey::Computed { value: key, .. } => {
                    match self.eval_class_member_key(key) {
                        MemberKey::Named(key) => key,
                        MemberKey::Index(kind) => {
                            side.computed.record(kind, member.readonly, value);
                            continue;
                        }
                        // A key whose type names no property kind (the
                        // checker's TS2464) declares nothing.
                        MemberKey::None => continue,
                        MemberKey::Unmodeled => {
                            self.unmodeled_position();
                            continue;
                        }
                    }
                }
            };
            side.members.push(SurfaceMember {
                key,
                value,
                optional: member.optional,
                readonly: member.readonly,
                method_kind: member.method_kind,
                has_implementation_body: member.method_kind.is_some(),
                visibility: member.visibility,
                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                spans: member.spans,
                declaration_origin: Some(Arc::from(self.canonical)),
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::default(),
                merge_role: crate::semantic_query::MergeRoleStamp::default(),
            });
        }
        for signature in class.index_signatures.iter() {
            let key_type = self.lower_gated_in(env, &signature.key);
            let value_type = self.lower_gated_in(env, &signature.value);
            let side = if signature.is_static {
                &mut static_side
            } else {
                &mut instance_side
            };
            side.index_signatures
                .push(crate::semantic_query::IndexSignature {
                    key_type,
                    value_type,
                    readonly: signature.readonly,
                    spans: verter_type_expr::IndexSignatureSpans::default(),
                    declaration_origin: Some(Arc::from(self.canonical)),
                });
        }
        self.add_computed_index_signatures(&mut instance_side);
        let own_instance = graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                has_index_signature: !instance_side.index_signatures.is_empty(),
                members: Arc::from(instance_side.members.into_boxed_slice()),
                call_signatures: Arc::from(Vec::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
                index_signatures: Arc::from(instance_side.index_signatures.into_boxed_slice()),
                keyspace: None,
            },
        ));
        let surface = match base.as_ref().and_then(|base| base.instance) {
            Some(base_instance) => self
                .dispatch
                .intern_normalized_union_or_intersection(&[base_instance, own_instance], false),
            None => own_instance,
        };
        // The reference where the class is authored: every outer clause's
        // arguments are its own parameters, and so are the class's own. A
        // clause whose parameters this frame cannot see is dropped from the
        // identity rather than given arguments it does not have.
        let mut outer_clauses = Vec::with_capacity(class.outer_clauses.len());
        let mut type_arguments = Vec::new();
        for clause in class.outer_clauses.iter() {
            let arguments: Option<Vec<SemanticNodeId>> = clause
                .parameters
                .iter()
                .map(|name| env.env.get(name.as_ref()).copied())
                .collect();
            if let Some(arguments) = arguments {
                type_arguments.extend(arguments);
                outer_clauses.push(clause.clone());
            }
        }
        let own_type_parameters: Vec<crate::semantic_query::TypeParamDecl> = class_env
            .as_ref()
            .map(|class_env| class_env.type_param_decls.clone())
            .unwrap_or_default();
        type_arguments.extend(own_type_parameters.iter().map(|decl| decl.param));
        let type_arguments: Arc<[SemanticNodeId]> = Arc::from(type_arguments.into_boxed_slice());
        let identity = crate::semantic_query::ClassExpressionIdentity {
            canonical_id: Arc::from(self.canonical),
            owner: self.owner,
            offset: class.offset,
            name: Arc::clone(&class.name),
            outer_clauses: Arc::from(outer_clauses.into_boxed_slice()),
            own_arity: own_type_parameters.len() as u32,
            constructor_visibility: class
                .constructor_visibility
                .or_else(|| base.as_ref().and_then(|base| base.constructor_visibility)),
            prototype: None,
            object_literal: false,
        };
        // The prototype: the class as authored — every argument its own
        // parameter — with each parameter erased to `any`.
        let authored = graph.intern_node_with_scope(
            SemanticNodeData::ClassExpressionInstance {
                identity: Arc::new(identity.clone()),
                type_arguments: Arc::clone(&type_arguments),
                surface,
            },
            self.binder_env.scope.clone(),
        );
        let any = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
        let prototype = type_arguments
            .iter()
            .fold(authored, |prototype, parameter| {
                self.dispatch
                    .substitute_semantic_type_param(prototype, *parameter, any)
            });
        let instance = graph.intern_node_with_scope(
            SemanticNodeData::ClassExpressionInstance {
                identity: Arc::new(crate::semantic_query::ClassExpressionIdentity {
                    prototype: Some(prototype),
                    ..identity
                }),
                type_arguments: Arc::clone(&type_arguments),
                surface,
            },
            self.binder_env.scope.clone(),
        );
        let construct = |params: Arc<[crate::semantic_query::FunctionParam]>| {
            graph.intern_node(SemanticNodeData::Signature {
                kind: crate::semantic_query::SignatureKind::Construct,
                params,
                return_type: instance,
                type_parameters: Arc::from(own_type_parameters.clone().into_boxed_slice()),
                occurrence: None,
                return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(instance),
                signature_span: None,
                return_type_span: None,
                // A constructor declares no type predicate.
                predicate: None,
                is_abstract: false,
            })
        };
        let construct_signatures: Vec<SemanticNodeId> = match (&class.constructors, &base) {
            (Some(signatures), _) => {
                let mut out = Vec::with_capacity(signatures.len());
                for parameters in signatures.iter() {
                    let mut params = Vec::with_capacity(parameters.len());
                    for parameter in parameters.iter() {
                        if signature_answer_is_frame_shadowed(self.dispatch, env, &parameter.ty) {
                            return Positional::Unmodeled;
                        }
                        params.push(crate::semantic_query::FunctionParam::synthetic(
                            parameter.name.clone(),
                            self.lower_type_in(env, parameter.ty.ty()),
                            parameter.optional,
                            parameter.rest,
                        ));
                    }
                    out.push(construct(Arc::from(params.into_boxed_slice())));
                }
                out
            }
            (None, Some(base)) => base
                .constructor_params
                .iter()
                .map(|params| construct(Arc::clone(params)))
                .collect(),
            (None, None) => vec![construct(Arc::from(Vec::new().into_boxed_slice()))],
        };
        // The constructor's `prototype` property (`getTypeOfPrototypeProperty`):
        // the prototype computed above. It replaces a static the class
        // declares under that name and the base constructor's own.
        static_side
            .members
            .retain(|member| member.key.as_string() != Some("prototype"));
        static_side.members.insert(
            0,
            crate::project_semantic_dispatch::build::class_prototype_member(
                prototype,
                Some(Arc::from(self.canonical)),
            ),
        );
        if let Some(base) = &base {
            for inherited in base.static_members.iter() {
                if !static_side
                    .members
                    .iter()
                    .any(|own| own.key == inherited.key)
                {
                    static_side.members.push(inherited.clone());
                }
            }
        }
        // The static side's implicit string index also covers the
        // constructor's `prototype`, one of its members.
        self.add_computed_index_signatures(&mut static_side);
        let constructor = graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                has_index_signature: !static_side.index_signatures.is_empty(),
                members: Arc::from(static_side.members.into_boxed_slice()),
                call_signatures: Arc::from(Vec::new().into_boxed_slice()),
                construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                index_signatures: Arc::from(static_side.index_signatures.into_boxed_slice()),
                keyspace: None,
            },
        ));
        Positional::Value(match base.and_then(|base| base.type_variable) {
            Some(type_variable) => self
                .dispatch
                .intern_normalized_union_or_intersection(&[constructor, type_variable], false),
            None => constructor,
        })
    }

    /// Record an evaluated, statically named instance member on the
    /// class's receiver.
    fn record_receiver_member(
        &self,
        receiver: &ClassReceiver,
        member: &verter_session_query::flow::slice::SliceClassMember,
        value: SemanticNodeId,
    ) {
        let SliceObjectKey::Static(name) = &member.key else {
            return;
        };
        if member.is_static {
            return;
        }
        receiver.members.borrow_mut().push(SurfaceMember {
            key: AuthoredPropertyKey::string(name.as_ref()),
            value,
            optional: member.optional,
            readonly: member.readonly,
            method_kind: member.method_kind,
            has_implementation_body: member.method_kind.is_some(),
            visibility: member.visibility,
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            spans: member.spans,
            declaration_origin: Some(Arc::from(self.canonical)),
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::default(),
            merge_role: crate::semantic_query::MergeRoleStamp::default(),
        });
    }

    /// Lower one gated member-position type under `env`: the typed marker
    /// when the frame shadows a name the answer reads.
    fn lower_gated_in(
        &mut self,
        env: &FlowBinderEnv,
        gated: &verter_session_query::flow::slice::GatedType,
    ) -> SemanticNodeId {
        if signature_answer_is_frame_shadowed(self.dispatch, env, gated) {
            return self.unmodeled_position();
        }
        self.lower_type_in(env, gated.ty())
    }

    /// Lower one body-position `TypeExpr` under `env`.
    /// `this` in a class declaration's member: the class's polymorphic
    /// `this` (a binder over the class instance `C<T…>`) in an instance
    /// member, the constructor `typeof C` in a static one.
    pub(super) fn eval_this(
        &mut self,
        this: &verter_session_query::flow::slice::SliceThis,
    ) -> Positional<SemanticNodeId> {
        match this {
            verter_session_query::flow::slice::SliceThis::Instance {
                class,
                type_parameters,
            } => {
                let reference = verter_type_expr::TypeExpr::Ref {
                    name: Arc::clone(class),
                    type_arguments: type_parameters
                        .iter()
                        .map(|name| verter_type_expr::TypeExpr::named(name.as_ref()))
                        .collect(),
                };
                let instance = self.lower_type_in(self.binder_env, &reference);
                Positional::Value(self.dispatch.this_binder(
                    self.canonical,
                    self.owner,
                    class,
                    Some(instance),
                ))
            }
            verter_session_query::flow::slice::SliceThis::Untyped => Positional::Value(
                self.dispatch
                    .graph()
                    .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
            ),
            verter_session_query::flow::slice::SliceThis::Receiver => match &self.receiver {
                Some(receiver) => Positional::Value(receiver.binder),
                None => {
                    self.record_degradation(
                        crate::semantic_query::FlowReturnDegradation::UnmodeledPosition,
                    );
                    Positional::Unmodeled
                }
            },
            verter_session_query::flow::slice::SliceThis::Static { class: value, .. }
            | verter_session_query::flow::slice::SliceThis::Value { value, .. } => {
                // The deferred `typeof C` / `typeof value` carrier: the
                // value is read where a consumer demands it, never while its
                // own member's return is still being evaluated.
                let mut path = value.split('.');
                let Some(root) = path.next() else {
                    return Positional::Unmodeled;
                };
                let value_root = crate::semantic_query::ValueRootKey {
                    scope: crate::semantic_query::ScopeId {
                        canonical_id: Arc::from(self.canonical),
                        owner: self.owner,
                        local_scope: None,
                        binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                            self.owner,
                        ),
                    },
                    name: Arc::from(root),
                };
                let path: Arc<[Arc<str>]> = path.map(Arc::from).collect();
                Positional::Value(self.dispatch.graph().intern_node_with_scope(
                    SemanticNodeData::new_typeof(value_root, path, Arc::from([])),
                    self.binder_env.scope.clone(),
                ))
            }
        }
    }

    /// Where member `name` of `receiver` reads from when `receiver` is a
    /// class's polymorphic `this`: the class's own member position (or the
    /// base's), never the whole class body — whose lowering would re-enter
    /// the member body this read is part of.
    pub(super) fn this_member_source(
        &self,
        receiver: SemanticNodeId,
        name: &str,
    ) -> Option<SemanticNodeId> {
        let graph = self.dispatch.graph();
        if let Some(bound) = self
            .receiver
            .as_ref()
            .filter(|bound| bound.binder == receiver)
        {
            // A class expression's own member reads the value its class
            // evaluated for it; one not evaluated yet is a forward read the
            // class retries; an inherited one reads off the base instance,
            // keeping the polymorphic `this` the reading receiver binds.
            let key = AuthoredPropertyKey::string(name);
            let own: Vec<crate::semantic_query::SurfaceEntry> = bound
                .members
                .borrow()
                .iter()
                .filter(|member| member.key == key)
                .cloned()
                .map(crate::semantic_query::SurfaceEntry::Member)
                .collect();
            if !own.is_empty() {
                return Some(graph.intern_node(SemanticNodeData::Object(
                    crate::semantic_query::SurfaceView::from_entries(own, None, false),
                )));
            }
            if bound.declared.contains(name) {
                bound.forward_read.set(true);
                return None;
            }
            return bound.base;
        }
        let data = graph.node_data(receiver)?;
        let SemanticNodeData::TypeParam {
            decl,
            param_index,
            constraint: Some(instance),
            ..
        } = data.as_ref()
        else {
            return None;
        };
        if *param_index != crate::project_semantic_dispatch::substitute::THIS_BINDER_INDEX {
            return None;
        }
        let args: Vec<SemanticNodeId> = match graph.node_data(*instance).as_deref() {
            Some(SemanticNodeData::InstantiationRef { args, .. }) => args.to_vec(),
            _ => Vec::new(),
        };
        self.dispatch.class_member_source(
            &decl.canonical_id,
            decl.owner,
            &decl.decl_name,
            &args,
            name,
        )
    }

    /// Where member `name` of `receiver` reads from when `receiver` is a
    /// reference to a class declaration's instance (`H` or `G<string>`):
    /// the class's own member position (or the base's) — never the whole
    /// class body, whose lowering would re-enter a member body reading its
    /// own class through a value of the class's type.
    pub(super) fn class_reference_member_source(
        &self,
        receiver: SemanticNodeId,
        name: &str,
    ) -> Option<SemanticNodeId> {
        let data = self.dispatch.graph().node_data(receiver)?;
        let (identity, args): (&crate::semantic_query::DeclIdentity, &[SemanticNodeId]) =
            match data.as_ref() {
                SemanticNodeData::DeclRef { identity } => (identity, &[]),
                SemanticNodeData::InstantiationRef { base, args } => (base, args),
                _ => return None,
            };
        self.dispatch.class_member_source(
            &identity.canonical_id,
            identity.owner,
            &identity.decl_name,
            args,
            name,
        )
    }

    fn lower_type_in(
        &self,
        env: &FlowBinderEnv,
        ty: &verter_type_expr::TypeExpr,
    ) -> SemanticNodeId {
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        self.dispatch.shallow_lower_type_expr_with_context(
            ty,
            &env.env,
            &env.scope,
            &env.name_resolution,
            env.scope_payload.as_ref(),
            &env.shadowing,
            &mut substitutions,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )
    }

    /// A read under [`Self::declared_reads`]: the binding's annotation when
    /// it has one, `any` for an auto-typed local (the checker's declared
    /// type there is its auto type, read as `any`: `let y; y = 1; fail();
    /// return y` is `any`), else its reaching value — never a narrow.
    pub(super) fn declared_local_read(
        &mut self,
        binding: &FlowProductSubject,
    ) -> Option<SemanticNodeId> {
        if let Some(declared) = self.local_declared(binding) {
            return Some(declared);
        }
        if self
            .auto_typed_locals
            .contains(&self.canonical_runtime_subject(binding))
        {
            return Some(
                self.dispatch
                    .graph()
                    .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
            );
        }
        self.read_local(binding)
    }

    /// What a computed class member key names.
    fn eval_class_member_key(&mut self, key: &SliceExpr) -> MemberKey {
        // A hold inside a KEY is not this class's value: drop it and read
        // the outcome.
        let holds_before = self.holds.len();
        let outcome = self.eval_expr(key);
        self.holds.truncate(holds_before);
        let Positional::Value(node) = outcome else {
            return MemberKey::Unmodeled;
        };
        let graph = self.dispatch.graph();
        // An enum member names the member its value names.
        let node = match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::EnumLiteral(literal)) => literal.base,
            _ => node,
        };
        match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String(value))) => {
                return MemberKey::Named(AuthoredPropertyKey::string(value.as_str()));
            }
            Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::Number(value))) => {
                return MemberKey::Named(AuthoredPropertyKey::from_known(
                    crate::semantic_query::PropertyKey::from_js_number(*value),
                ));
            }
            Some(SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_)) => {
                if let Some(identity) = self.dispatch.unique_symbol_identity_for_typeof_node(node) {
                    return MemberKey::Named(AuthoredPropertyKey::UniqueSymbol(identity));
                }
            }
            _ => {}
        }
        self.late_bound_key(node)
    }

    /// What a computed key whose value names no single property names: a
    /// late-bound member of the index-signature kind its type is
    /// assignable to (`getIndexInfosOfIndexSymbol`: number, then symbol,
    /// then the rest of `string | number | symbol`), or nothing for a type
    /// that is no property-key type.
    pub(super) fn late_bound_key(&mut self, node: SemanticNodeId) -> MemberKey {
        let graph = self.dispatch.graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let symbol = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Symbol));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let property_key = self.union(&[string, number, symbol]);
        match self.assignable(node, property_key) {
            Some(true) => {}
            Some(false) => return MemberKey::None,
            None => return MemberKey::Unmodeled,
        }
        match (self.assignable(node, number), self.assignable(node, symbol)) {
            (Some(true), _) => MemberKey::Index(ComputedKeyKind::Number),
            (Some(false), Some(true)) => MemberKey::Index(ComputedKeyKind::Symbol),
            (Some(false), Some(false)) => MemberKey::Index(ComputedKeyKind::String),
            _ => MemberKey::Unmodeled,
        }
    }

    /// Add one side's implicit index signatures: for each key kind a
    /// late-bound member named, unless the side declares a signature of
    /// that key already, the union of every applicable member's type (the
    /// checker's `getObjectLiteralIndexInfo`) — a string index reads every
    /// string- and number-named member, a number index every numeric-named
    /// one, a symbol index every unique-symbol-named one.
    pub(super) fn add_computed_index_signatures(&mut self, side: &mut MemberSide) {
        let graph = self.dispatch.graph();
        let kinds = [
            (
                ComputedKeyKind::String,
                side.computed.string,
                PrimitiveKind::String,
            ),
            (
                ComputedKeyKind::Number,
                side.computed.number,
                PrimitiveKind::Number,
            ),
            (
                ComputedKeyKind::Symbol,
                side.computed.symbol,
                PrimitiveKind::Symbol,
            ),
        ];
        for (kind, readonly, primitive) in kinds {
            let Some(readonly) = readonly else {
                continue;
            };
            let key_type = graph.intern_node(SemanticNodeData::Primitive(primitive));
            let declared = side.index_signatures.iter().any(|signature| {
                matches!(
                    graph.node_data(signature.key_type).as_deref(),
                    Some(SemanticNodeData::Primitive(declared)) if *declared == primitive
                )
            });
            if declared {
                continue;
            }
            let applies = |candidate: ComputedKeyKind| match kind {
                ComputedKeyKind::String => candidate != ComputedKeyKind::Symbol,
                ComputedKeyKind::Number => candidate == ComputedKeyKind::Number,
                ComputedKeyKind::Symbol => candidate == ComputedKeyKind::Symbol,
            };
            let mut values: Vec<SemanticNodeId> = side
                .computed
                .values
                .iter()
                .filter(|(candidate, _)| applies(*candidate))
                .map(|(_, value)| *value)
                .collect();
            for member in side.members.iter() {
                if applies(member_key_kind(&member.key)) {
                    values.push(member.value);
                }
            }
            let value_type = self.union(&values);
            side.index_signatures
                .push(crate::semantic_query::IndexSignature {
                    key_type,
                    value_type,
                    readonly,
                    spans: verter_type_expr::IndexSignatureSpans::default(),
                    declaration_origin: Some(Arc::from(self.canonical)),
                });
        }
    }

    /// The base a class expression's `extends` value provides, `None` when
    /// the class cannot be composed over it (see [`Self::eval_class_value`]).
    fn class_base(
        &mut self,
        constructor: SemanticNodeId,
        type_arguments: &[SemanticNodeId],
    ) -> Option<ClassBase> {
        let graph = self.dispatch.graph();
        let settled = self.dispatch.resolve_signature_source_carrier(
            constructor,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        );
        if matches!(
            graph.node_data(settled).as_deref(),
            Some(SemanticNodeData::Primitive(_) | SemanticNodeData::Opaque(_))
        ) {
            return None;
        }
        let signatures = match self
            .dispatch
            .shared_signature_nodes(settled, crate::semantic_query::SignatureKind::Construct)
        {
            super::super::signature_discovery::SharedSignatureNodes::Nodes(signatures) => {
                signatures
            }
            super::super::signature_discovery::SharedSignatureNodes::Incomplete(_) => return None,
        };
        // The base construct signatures that accept the `extends` type
        // arguments, instantiated with them
        // (`getInstantiatedConstructorsForTypeArguments`): a generic one
        // takes the arguments (its defaults filling the rest), a
        // non-generic one only an argument-less `extends`.
        let mut constructors = Vec::with_capacity(signatures.len());
        for signature in signatures {
            let generic = match graph.node_data(signature).as_deref() {
                Some(SemanticNodeData::Signature {
                    type_parameters, ..
                }) => !type_parameters.is_empty(),
                _ => return None,
            };
            let accepted = if generic {
                self.dispatch
                    .instantiate_call_candidate(signature, type_arguments)
            } else {
                type_arguments.is_empty().then_some(signature)
            };
            if let Some(accepted) = accepted {
                constructors.push(accepted);
            }
        }
        let mut constructor_params = Vec::with_capacity(constructors.len());
        let mut instance = None;
        for signature in &constructors {
            let Some(SemanticNodeData::Signature {
                params,
                return_type,
                ..
            }) = graph.node_data(*signature).as_deref().cloned()
            else {
                return None;
            };
            instance.get_or_insert(return_type);
            constructor_params.push(params);
        }
        let instance = instance?;
        // A union base constructor's signature returns the union of its
        // members' results reduced by SUBTYPE (the checker's union-signature
        // return), and a result that stays a union is not a valid base type
        // (`isValidBaseType`): the class then inherits no instance member.
        let instance = match graph.node_data(instance).as_deref() {
            Some(arms @ SemanticNodeData::Union(_)) => {
                let arms = arms.composite_members().expect("union arm").to_vec();
                let kept: Vec<SemanticNodeId> = arms
                    .iter()
                    .copied()
                    .filter(|arm| {
                        !arms.iter().any(|other| {
                            other != arm
                                && self.assignable(*arm, *other) == Some(true)
                                && self.assignable(*other, *arm) != Some(true)
                        })
                    })
                    .collect();
                match kept.as_slice() {
                    [single] => Some(*single),
                    _ => None,
                }
            }
            _ => Some(instance),
        };
        // The base constructor's own type variable, when it is one (or an
        // intersection carrying one).
        let mut node = constructor;
        let mut visited = rustc_hash::FxHashSet::default();
        let type_variable = loop {
            if !visited.insert(node) {
                break None;
            }
            match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::Alias(target)) => node = *target,
                Some(SemanticNodeData::TypeParam { .. }) => break Some(node),
                Some(SemanticNodeData::Intersection(members)) => {
                    break members.iter().copied().find(|member| {
                        matches!(
                            graph.node_data(*member).as_deref(),
                            Some(SemanticNodeData::TypeParam { .. })
                        )
                    })
                }
                _ => break None,
            }
        };
        // The static members the class inherits: a type-variable base
        // contributes its statics through the intersection with it, an
        // object base through its surface, an intersection base through
        // every arm's (intersection rules: a shared key intersects its
        // values); a union base constructor contributes none.
        let static_members = match (type_variable, graph.node_data(settled).as_deref()) {
            (Some(_), _) => Vec::new(),
            (None, Some(SemanticNodeData::Object(view))) => view.positive_members().to_vec(),
            (None, Some(arms @ SemanticNodeData::Intersection(_))) => {
                let per_arm: Vec<Vec<SurfaceMember>> = arms
                    .composite_members()
                    .expect("intersection arm")
                    .iter()
                    .map(|arm| {
                        let arm = self.dispatch.resolve_signature_source_carrier(
                            *arm,
                            crate::semantic_query::ProjectionReductionContext::structural_transit(),
                        );
                        match graph.node_data(arm).as_deref() {
                            Some(SemanticNodeData::Object(view)) => {
                                view.positive_members().to_vec()
                            }
                            _ => Vec::new(),
                        }
                    })
                    .collect();
                self.dispatch.presence_intersection_for_flow(&per_arm)
            }
            (None, Some(SemanticNodeData::Union(_))) => Vec::new(),
            (None, _) => return None,
        };
        let constructor_visibility = constructors
            .first()
            .and_then(|signature| self.dispatch.construct_signature_visibility(*signature));
        Some(ClassBase {
            constructor_params,
            instance,
            static_members,
            type_variable,
            constructor_visibility,
        })
    }
}

/// What one computed member key names.
pub(super) enum MemberKey {
    /// A literal or unique-symbol key: one named member.
    Named(AuthoredPropertyKey),
    /// A late-bound key: a contribution to the side's implicit index
    /// signature of this kind.
    Index(ComputedKeyKind),
    /// A key whose type is no property-key type: no member at all.
    None,
    /// A key this evaluation could not settle.
    Unmodeled,
}

/// The index-signature kind a NAMED member's key falls under: a unique
/// symbol, a numeric-literal name (`isNumericLiteralName`: `1`, `"1.5"`),
/// or a string.
fn member_key_kind(key: &AuthoredPropertyKey) -> ComputedKeyKind {
    match key {
        AuthoredPropertyKey::UniqueSymbol(_) => ComputedKeyKind::Symbol,
        AuthoredPropertyKey::Number(_) => ComputedKeyKind::Number,
        AuthoredPropertyKey::String(name) => {
            let numeric = name.parse::<f64>().is_ok_and(|number| {
                match crate::semantic_query::PropertyKey::from_js_number(number) {
                    crate::semantic_query::PropertyKey::Number(canonical) => {
                        canonical.to_string() == name.as_ref()
                    }
                    crate::semantic_query::PropertyKey::String(canonical) => {
                        canonical.as_ref() == name.as_ref()
                    }
                    crate::semantic_query::PropertyKey::UniqueSymbol(_) => false,
                }
            });
            if numeric {
                ComputedKeyKind::Number
            } else {
                ComputedKeyKind::String
            }
        }
        AuthoredPropertyKey::Computed(_) => ComputedKeyKind::String,
    }
}
