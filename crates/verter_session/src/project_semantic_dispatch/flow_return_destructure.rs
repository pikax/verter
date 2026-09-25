//! Destructuring evaluation: the flow evaluator's binding of a
//! [`SlicePattern`] from its parent value — the checker's
//! `getTypeForBindingElement` / `getRestType` / `sliceTupleType` rules,
//! measured on 7.0.2.

use std::sync::Arc;

use super::{widen_values_within, FlowEvaluator, FlowProductSubject, Positional};
use crate::flow_slice_content::{
    SliceArrayElement, SliceBindingKind, SliceExpr, SlicePattern, SlicePatternElement,
    SlicePatternKey,
};
use crate::semantic_query::{
    LiteralValue, PrimitiveKind, SemanticNodeData, SemanticNodeId, SemanticQueryApi,
};
use verter_semantic::analysis::flow::SkeletonBindingId;

/// One element's value: the node, and the FRESH literal values in it.
struct ElementValue {
    node: Option<SemanticNodeId>,
    fresh: Vec<SemanticNodeId>,
}

impl ElementValue {
    fn of(node: Option<SemanticNodeId>) -> Self {
        Self {
            node,
            fresh: Vec::new(),
        }
    }
}

impl FlowEvaluator<'_, '_> {
    /// [`crate::flow_slice_content::SliceStatement::Destructure`]: the
    /// parent value is the declarator's annotation when it has one (the
    /// initializer still runs), else its initializer's value — an array
    /// literal under an array pattern read POSITIONALLY, each element its
    /// own widened value (the checker types the literal as a tuple in the
    /// pattern's context) — and every element binds from it.
    pub(super) fn eval_destructure(
        &mut self,
        pattern: &SlicePattern,
        kind: SliceBindingKind,
        init: Option<&SliceExpr>,
        declared: Option<&crate::flow_slice_content::GatedType>,
        annotated: bool,
        correlated: bool,
        source: Option<&crate::flow_slice_content::SliceNarrowSubject>,
    ) {
        self.prescan_statement_value_writes(init);
        if let Some(declared) = declared {
            if let Some(init) = init {
                let holds_before = self.holds.len();
                let _ = self.eval_expr(init);
                self.holds.truncate(holds_before);
            }
            let parent = if declared
                .shadowed()
                .iter()
                .any(|name| self.owner_scope_answers_name(name))
            {
                None
            } else {
                Some(self.lower_body_type(declared.ty()))
            };
            self.bind_pattern(pattern, ElementValue::of(parent), kind, true);
            self.register_destructured_aliases(pattern, parent, correlated, source);
            return;
        }
        let Some(init) = init else {
            self.bind_pattern(pattern, ElementValue::of(None), kind, false);
            self.register_destructured_aliases(pattern, None, correlated, source);
            return;
        };
        if annotated {
            let holds_before = self.holds.len();
            let outcome = self.eval_expr(init);
            self.holds.truncate(holds_before);
            let parent = match outcome {
                Positional::Value(node) => Some(node),
                Positional::Hold | Positional::Unmodeled => None,
            };
            self.bind_pattern(pattern, ElementValue::of(parent), kind, true);
            self.register_destructured_aliases(pattern, parent, correlated, source);
            return;
        }
        if let (
            SlicePattern::Array { elements, rest },
            SliceExpr::Array {
                elements: values,
                const_asserted: false,
            },
        ) = (pattern, init)
        {
            if rest.is_none()
                && values
                    .iter()
                    .all(|value| matches!(value, SliceArrayElement::Value { .. }))
            {
                let mut positional: Vec<Option<SemanticNodeId>> = Vec::with_capacity(values.len());
                for value in values.iter() {
                    let SliceArrayElement::Value { value, .. } = value else {
                        unreachable!("every element was checked to be a value");
                    };
                    let holds_before = self.holds.len();
                    let outcome = self.eval_expr(value);
                    self.holds.truncate(holds_before);
                    positional.push(match outcome {
                        Positional::Value(node) => Some(node),
                        Positional::Hold | Positional::Unmodeled => None,
                    });
                }
                let undefined = self.primitive_node(PrimitiveKind::Undefined);
                for (index, element) in elements.iter().enumerate() {
                    let Some(element) = element else {
                        continue;
                    };
                    // A position past the literal's end reads `undefined`
                    // (TS2493 — the checker's recovery type).
                    let node = match positional.get(index) {
                        Some(node) => *node,
                        None => Some(undefined),
                    };
                    self.bind_pattern_element(element, ElementValue::of(node), kind, false);
                }
                return;
            }
        }
        let holds_before = self.holds.len();
        let outcome = self.eval_expr(init);
        self.holds.truncate(holds_before);
        let parent = match outcome {
            Positional::Value(node) => Some(node),
            Positional::Hold | Positional::Unmodeled => None,
        };
        self.bind_pattern(pattern, ElementValue::of(parent), kind, false);
        self.register_destructured_aliases(pattern, parent, correlated, source);
    }

    /// [`crate::flow_slice_content::SliceStatement::DestructureAssign`]: the
    /// right-hand side's value is written through each target in source
    /// order, every element read off it as a declarator's is.
    pub(super) fn eval_destructure_assign(
        &mut self,
        pattern: &SlicePattern,
        value: &SliceExpr,
        definition: verter_semantic::analysis::flow::SkeletonExprSiteId,
    ) {
        self.prescan_statement_value_writes(Some(value));
        let holds_before = self.holds.len();
        let outcome = self.eval_expr(value);
        self.holds.truncate(holds_before);
        let parent = match outcome {
            Positional::Value(node) => Some(node),
            Positional::Hold | Positional::Unmodeled => None,
        };
        let enclosing = self.pattern_write_definition.replace(definition);
        self.bind_pattern(
            pattern,
            ElementValue::of(parent),
            SliceBindingKind::Let,
            false,
        );
        self.pattern_write_definition = enclosing;
    }

    /// Bind a loop element's pattern from the iterated element type.
    pub(super) fn bind_loop_pattern(
        &mut self,
        pattern: &SlicePattern,
        element: SemanticNodeId,
        kind: SliceBindingKind,
    ) {
        self.bind_pattern(pattern, ElementValue::of(Some(element)), kind, false);
        // A `const` loop pattern correlates its elements.
        if kind == SliceBindingKind::Const {
            self.register_destructured_aliases(pattern, Some(element), true, None);
        }
    }

    /// Bind one pattern from its parent value. A parent this frame could
    /// not type binds every element to the typed unmodelled marker, which
    /// degrades only a read of it.
    fn bind_pattern(
        &mut self,
        pattern: &SlicePattern,
        parent: ElementValue,
        kind: SliceBindingKind,
        annotated: bool,
    ) {
        match pattern {
            SlicePattern::Binding { binding, .. } => {
                self.bind_pattern_binding(*binding, parent, kind)
            }
            SlicePattern::Target { target, .. } => {
                let Some(definition) = self.pattern_write_definition else {
                    return;
                };
                match parent.node {
                    Some(node) => {
                        self.apply_write(target, node, false, definition, false, false);
                    }
                    None => {
                        let marker = super::super::flow_return_callee::unmodeled_position_marker(
                            self.dispatch,
                        );
                        self.apply_write(target, marker, true, definition, false, false);
                    }
                }
            }
            SlicePattern::Object { properties, rest } => {
                let mut keys: Vec<Option<Arc<str>>> = Vec::with_capacity(properties.len());
                for (key, element) in properties.iter() {
                    let name = self.pattern_key_name(key);
                    let member = match (parent.node, name.as_ref()) {
                        (Some(parent), Some(name)) => self.pattern_property(parent, name),
                        _ => None,
                    };
                    keys.push(name);
                    self.bind_pattern_element(element, ElementValue::of(member), kind, annotated);
                }
                if let Some((rest, _)) = rest {
                    let value = match (parent.node, keys.into_iter().collect::<Option<Vec<_>>>()) {
                        (Some(parent), Some(keys)) => self.object_rest_type(parent, &keys),
                        _ => None,
                    };
                    self.bind_pattern_binding(*rest, ElementValue::of(value), kind);
                }
            }
            SlicePattern::Array { elements, rest } => {
                for (index, element) in elements.iter().enumerate() {
                    let Some(element) = element else {
                        continue;
                    };
                    let member = parent
                        .node
                        .and_then(|parent| self.pattern_position(parent, index));
                    self.bind_pattern_element(element, ElementValue::of(member), kind, annotated);
                }
                if let Some((rest, _)) = rest {
                    let value = parent
                        .node
                        .and_then(|parent| self.array_rest_type(parent, elements.len()));
                    self.bind_pattern_binding(*rest, ElementValue::of(value), kind);
                }
            }
        }
    }

    /// One element: its default replaces the member's `undefined` — beside
    /// the member's other constituents (subtype-reduced), or alone under an
    /// annotation, where the annotation's member type is the element's.
    fn bind_pattern_element(
        &mut self,
        element: &SlicePatternElement,
        member: ElementValue,
        kind: SliceBindingKind,
        annotated: bool,
    ) {
        let value = match (&element.default, member.node) {
            (Some(default), member_node) => {
                let holds_before = self.holds.len();
                let outcome = self.eval_expr(default);
                self.holds.truncate(holds_before);
                match (member_node, outcome) {
                    (Some(member_node), Positional::Value(default_node)) => {
                        let defined = self.remove_undefined_arms(member_node);
                        if annotated {
                            ElementValue::of(Some(defined))
                        } else {
                            let node = self
                                .reduced_union(vec![
                                    super::ReductionArm::plain(defined),
                                    super::ReductionArm::plain(default_node),
                                ])
                                .0;
                            let fresh = if element.default_fresh {
                                vec![default_node]
                            } else {
                                Vec::new()
                            };
                            ElementValue {
                                node: Some(node),
                                fresh,
                            }
                        }
                    }
                    _ => ElementValue::of(None),
                }
            }
            (None, _) => member,
        };
        self.bind_pattern(&element.pattern, value, kind, annotated);
    }

    /// Bind one pattern identifier: a `const` keeps the fresh literals it
    /// holds as its widening membership, a `let` / `var` widens them at the
    /// declaration.
    fn bind_pattern_binding(
        &mut self,
        binding: SkeletonBindingId,
        value: ElementValue,
        kind: SliceBindingKind,
    ) {
        let subject = FlowProductSubject::Local(binding);
        // A binding outside the demand's selected slots has no product: no
        // read of it is evaluated.
        if !self.products.contains_subject(&subject) {
            return;
        }
        if kind != SliceBindingKind::Var {
            self.record_scope_shadow(&subject);
        }
        self.set_declared_local(&subject, kind, None);
        let Some(node) = value.node else {
            let marker = super::super::flow_return_callee::unmodeled_position_marker(self.dispatch);
            self.bind_local(&subject, kind, marker, None, true);
            return;
        };
        let fresh: Vec<SemanticNodeId> = value
            .fresh
            .into_iter()
            .filter(|fresh| self.top_level_literal_nodes(node).contains(fresh))
            .collect();
        match kind {
            SliceBindingKind::Const => {
                let membership = (!fresh.is_empty()).then(|| {
                    super::WideningMembership::Partial(Arc::from(fresh.into_boxed_slice()))
                });
                self.bind_local(&subject, kind, node, membership, false);
            }
            SliceBindingKind::Let | SliceBindingKind::Var => {
                let node = widen_values_within(self.dispatch, node, &fresh, self.nullability);
                self.bind_local(&subject, kind, node, None, false);
            }
        }
    }

    /// The property a pattern key names: its static name, or a computed
    /// key's string or numeric literal type. `None` for a computed key of
    /// any other type.
    fn pattern_key_name(&mut self, key: &SlicePatternKey) -> Option<Arc<str>> {
        let SlicePatternKey::Computed(key) = key else {
            let SlicePatternKey::Named(name) = key else {
                unreachable!("a key is named or computed");
            };
            return Some(Arc::clone(name));
        };
        let holds_before = self.holds.len();
        let outcome = self.eval_expr(key);
        self.holds.truncate(holds_before);
        let Positional::Value(node) = outcome else {
            return None;
        };
        match self.dispatch.graph().node_data(node).as_deref()? {
            SemanticNodeData::Literal(LiteralValue::String(value)) => {
                Some(Arc::from(value.as_str()))
            }
            SemanticNodeData::Literal(LiteralValue::Number(value)) => Some(Arc::from(
                crate::semantic_query::index_key::js_number_to_string(*value).as_str(),
            )),
            _ => None,
        }
    }

    fn primitive_node(&self, kind: PrimitiveKind) -> SemanticNodeId {
        self.dispatch
            .graph()
            .intern_node(SemanticNodeData::Primitive(kind))
    }

    fn is_any(&self, node: SemanticNodeId) -> bool {
        matches!(
            self.dispatch.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
        )
    }

    /// `getNonUndefinedType`: the members other than `undefined`.
    fn remove_undefined_arms(&self, node: SemanticNodeId) -> SemanticNodeId {
        let graph = self.dispatch.graph();
        let arms = self
            .dispatch
            .union_arms_of(node)
            .map_or_else(|| vec![node], |arms| arms.to_vec());
        let kept: Vec<SemanticNodeId> = arms
            .iter()
            .copied()
            .filter(|arm| {
                !matches!(
                    graph.node_data(*arm).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined))
                )
            })
            .collect();
        match kept.as_slice() {
            _ if kept.len() == arms.len() => node,
            [] => self.primitive_node(PrimitiveKind::Never),
            [single] => *single,
            _ => self.union(&kept),
        }
    }

    /// An object pattern property's type: the parent's member through the
    /// shared member-read projection (a declared-optional member includes
    /// `undefined`); `any` of `any`.
    pub(super) fn pattern_property(
        &mut self,
        parent: SemanticNodeId,
        key: &Arc<str>,
    ) -> Option<SemanticNodeId> {
        if self.is_any(parent) {
            return Some(parent);
        }
        let member = self.project_segments_navigate(parent, std::slice::from_ref(key))?;
        (!matches!(
            self.dispatch.graph().node_data(member).as_deref(),
            Some(SemanticNodeData::Opaque(_))
        ))
        .then_some(member)
    }

    /// An array pattern position's type: a tuple's element, an array's or
    /// a string's element type — the checker's indexed access by the
    /// position's numeric literal type; `any` of `any`.
    pub(super) fn pattern_position(
        &mut self,
        parent: SemanticNodeId,
        index: usize,
    ) -> Option<SemanticNodeId> {
        if self.is_any(parent) {
            return Some(parent);
        }
        let parent = self.dispatch.resolved_reduction_view(parent);
        let arms = self
            .dispatch
            .union_arms_of(parent)
            .map_or_else(|| vec![parent], |arms| arms.to_vec());
        let graph = self.dispatch.graph();
        let mut values = Vec::with_capacity(arms.len());
        for arm in arms {
            let arm = self.dispatch.resolved_reduction_view(arm);
            let data = graph.node_data(arm);
            let value = match data.as_deref()? {
                SemanticNodeData::Primitive(PrimitiveKind::String)
                | SemanticNodeData::Literal(LiteralValue::String(_)) => {
                    self.primitive_node(PrimitiveKind::String)
                }
                SemanticNodeData::Array { element, .. } => *element,
                SemanticNodeData::Tuple { .. } => {
                    drop(data);
                    let position = graph.intern_node(SemanticNodeData::Literal(
                        LiteralValue::Number(index as f64),
                    ));
                    let value = self.indexed_access(arm, position)?;
                    if matches!(
                        graph.node_data(value).as_deref(),
                        Some(SemanticNodeData::Opaque(_))
                    ) {
                        return None;
                    }
                    value
                }
                _ => return None,
            };
            values.push(value);
        }
        Some(match values.as_slice() {
            [single] => *single,
            _ => self.union(&values),
        })
    }

    /// `getRestType` of an object pattern: the parent's members other than
    /// the pattern's keys, each spread (not `readonly`), a union parent
    /// distributed. `None` for a parent whose members this frame cannot
    /// read off its node.
    fn object_rest_type(
        &mut self,
        parent: SemanticNodeId,
        keys: &[Arc<str>],
    ) -> Option<SemanticNodeId> {
        if self.is_any(parent) {
            return Some(parent);
        }
        let parent = self.dispatch.resolved_reduction_view(parent);
        if let Some(arms) = self.dispatch.union_arms_of(parent) {
            let arms = arms.to_vec();
            let mut rests = Vec::with_capacity(arms.len());
            for arm in arms {
                rests.push(self.object_rest_type(arm, keys)?);
            }
            return Some(self.union(&rests));
        }
        let graph = self.dispatch.graph();
        let data = graph.node_data(parent);
        let Some(SemanticNodeData::Object(surface)) = data.as_deref() else {
            return None;
        };
        let members: Vec<crate::semantic_query::SurfaceMember> = surface
            .positive_members()
            .iter()
            .filter(|member| {
                !keys.iter().any(|key| {
                    member.key.as_known().is_some_and(|known| {
                        known.element_access_collides(&verter_type_expr::PropertyKey::from(
                            key.as_ref(),
                        ))
                    })
                })
            })
            .map(|member| crate::semantic_query::SurfaceMember {
                readonly: false,
                ..member.clone()
            })
            .collect();
        let surface = surface
            .clone()
            .with_positive_members(Arc::from(members.into_boxed_slice()));
        drop(data);
        Some(graph.intern_node(SemanticNodeData::Object(surface)))
    }

    /// The rest of an array pattern after `consumed` positions: the tuple
    /// of a fixed tuple's remaining elements, an array itself (a mutable
    /// array of a readonly one's element), `string[]` of a string; `any`
    /// of `any`.
    fn array_rest_type(
        &mut self,
        parent: SemanticNodeId,
        consumed: usize,
    ) -> Option<SemanticNodeId> {
        if self.is_any(parent) {
            return Some(parent);
        }
        let graph = self.dispatch.graph();
        let data = graph.node_data(parent);
        match data.as_deref()? {
            SemanticNodeData::Array { element, .. } => {
                let element = *element;
                drop(data);
                Some(graph.intern_node(SemanticNodeData::Array {
                    element,
                    readonly: false,
                }))
            }
            SemanticNodeData::Primitive(PrimitiveKind::String) => {
                drop(data);
                let string = self.primitive_node(PrimitiveKind::String);
                Some(graph.intern_node(SemanticNodeData::Array {
                    element: string,
                    readonly: false,
                }))
            }
            SemanticNodeData::Tuple { elements, .. } => {
                let remaining: Vec<crate::semantic_query::TupleElement> =
                    elements.iter().skip(consumed).cloned().collect();
                drop(data);
                // An optional element read out of its tuple carries its
                // missing-element `undefined` (`sliceTupleType` reads the
                // tuple's element types, optionality included).
                let undefined = self.primitive_node(PrimitiveKind::Undefined);
                let remaining: Vec<crate::semantic_query::TupleElement> = remaining
                    .into_iter()
                    .map(|element| {
                        if element.optional && self.nullability.is_strict() {
                            crate::semantic_query::TupleElement {
                                value: self.union(&[element.value, undefined]),
                                ..element
                            }
                        } else {
                            element
                        }
                    })
                    .collect();
                Some(graph.intern_node(SemanticNodeData::Tuple {
                    elements: Arc::from(remaining.into_boxed_slice()),
                    readonly: false,
                }))
            }
            _ => None,
        }
    }
}

impl FlowEvaluator<'_, '_> {
    /// The type a `for…of` over `source` iterates when `source` is not an
    /// array, a tuple or a string — the checker's
    /// `getIterationTypesOfIterable` slow path through the ITERATOR
    /// PROTOCOL: the member keyed by the `Symbol.iterator` unique symbol
    /// (resolved where the GLOBAL `SymbolConstructor` is declared, never
    /// through this frame's lexical `Symbol`), its lone call signature's
    /// return — the iterator
    /// — that iterator's `next()` return, and the `value` of every
    /// constituent of that result whose `done` admits `false`. `any` of
    /// `any`. `None` when a step is absent or undecidable here (an
    /// overloaded or generic signature, an unresolved lib).
    pub(super) fn iterated_type(&mut self, source: SemanticNodeId) -> Option<SemanticNodeId> {
        if self.is_any(source) {
            return Some(source);
        }
        // The key is `Symbol.iterator`'s own declared type, read where the
        // global `SymbolConstructor` is declared — the unique symbol the
        // lib's `[Symbol.iterator]` members are keyed by.
        let constructor =
            self.dispatch
                .global_named_type(self.canonical, self.owner, "SymbolConstructor")?;
        let (scope, owner) = match self.dispatch.graph().node_data(constructor).as_deref() {
            Some(SemanticNodeData::DeclRef { identity }) => {
                (Arc::clone(&identity.canonical_id), identity.owner)
            }
            _ => (Arc::from(self.canonical), self.owner),
        };
        let key = self.dispatch.lower_type_expr_in_owner_scope_with_context(
            scope.as_ref(),
            owner,
            &verter_type_expr::TypeExpr::TypeOf(verter_type_expr::ValueRef {
                path: vec!["Symbol".to_owned(), "iterator".to_owned()],
                type_args: Vec::new(),
            }),
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )?;
        let identity = self.dispatch.unique_symbol_identity_for_typeof_node(key)?;
        let method = self.project_key(
            source,
            crate::semantic_query::PropertyKey::UniqueSymbol(identity),
        )?;
        let iterator = self.lone_call_return(method)?;
        let next = self.present_member(iterator, &[Arc::from("next")])?;
        let result = self.lone_call_return(next)?;
        let result = self.dispatch.resolved_reduction_view(result);
        let arms = self
            .dispatch
            .union_arms_of(result)
            .map_or_else(|| vec![result], |arms| arms.to_vec());
        let graph = self.dispatch.graph();
        let false_node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(false)));
        let mut values = Vec::new();
        for arm in arms {
            let arm = self.dispatch.resolved_reduction_view(arm);
            // A constituent with no `done` is a yield result (`done`
            // defaults to `false`).
            let yields = match self.present_member(arm, &[Arc::from("done")]) {
                Some(done) => match self.dispatch.execute_relate_pair(false_node, done) {
                    super::super::dispatch_txn::RelationStep::Assignable { .. } => true,
                    super::super::dispatch_txn::RelationStep::NotAssignable => false,
                    _ => return None,
                },
                None => true,
            };
            if yields {
                values.push(self.present_member(arm, &[Arc::from("value")])?);
            }
        }
        Some(match values.as_slice() {
            [] => self.primitive_node(PrimitiveKind::Never),
            [single] => *single,
            _ => self.union(&values),
        })
    }

    /// A named member of `base`; a projection miss is `None`.
    fn present_member(
        &mut self,
        base: SemanticNodeId,
        path: &[Arc<str>],
    ) -> Option<SemanticNodeId> {
        let node = self.project_member_path(base, path)?;
        (!matches!(
            self.dispatch.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Opaque(_))
        ))
        .then_some(node)
    }

    /// The return type of a member's lone, non-generic call signature
    /// (`SignaturesOfType`).
    fn lone_call_return(&mut self, callee: SemanticNodeId) -> Option<SemanticNodeId> {
        let (calls, _) = self.dispatch.shared_signature_buckets(callee).ok()?;
        let [signature] = calls.as_slice() else {
            return None;
        };
        match self.dispatch.graph().node_data(*signature).as_deref()? {
            SemanticNodeData::Signature {
                return_type,
                type_parameters,
                ..
            } if type_parameters.is_empty() => Some(*return_type),
            _ => None,
        }
    }

    /// The member of `base` keyed by `key`, through the one shared
    /// `ProjectPath { Navigate }` walk; a projection miss is `None`.
    fn project_key(
        &mut self,
        base: SemanticNodeId,
        key: crate::semantic_query::PropertyKey,
    ) -> Option<SemanticNodeId> {
        let node = match self.dispatch.execute_type_node(
            crate::semantic_query::SemanticQueryKey::ProjectPath {
                base,
                path: Arc::from(
                    vec![crate::semantic_query::PathSegment::Member(key)].into_boxed_slice(),
                ),
                context: crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
            },
        ) {
            crate::semantic_query::QueryResult::Value(output) => output.value,
            _ => return None,
        };
        (!matches!(
            self.dispatch.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Opaque(_))
        ))
        .then_some(node)
    }
}
