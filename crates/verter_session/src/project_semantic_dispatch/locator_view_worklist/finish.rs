use super::*;

impl<'a> ProjectSemanticDispatch<'a> {
    pub(super) fn plan_bare_reference_projection(
        &self,
        data: &SemanticNodeData,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
    ) -> ReferenceProjectionPlan {
        let (name, _bare_scope) = data.bare_ref_head().expect("BareRef head");
        let name = Arc::clone(name);
        let type_args: Arc<[SemanticNodeId]> =
            Arc::from(data.carrier_type_args().to_vec().into_boxed_slice());
        if type_args.is_empty() {
            if let Some(binding) = inputs
                .scope_payload
                .and_then(|payload| payload.scope_type_bindings().get(name.as_ref()))
            {
                let mut nested: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
                let constraint = binding.constraint.as_ref().map(|constraint| {
                    self.shallow_lower_type_expr_with_context(
                        constraint,
                        inputs.env,
                        inputs.scope,
                        inputs.name_resolution,
                        inputs.scope_payload,
                        inputs.shadowing,
                        &mut nested,
                        context,
                    )
                });
                let default = binding.default.as_ref().map(|default| {
                    self.shallow_lower_type_expr_with_context(
                        default,
                        inputs.env,
                        inputs.scope,
                        inputs.name_resolution,
                        inputs.scope_payload,
                        inputs.shadowing,
                        &mut nested,
                        context,
                    )
                });
                substitutions.extend(nested);
                let decl = match inputs.scope {
                    NodeScopeId::Global => DeclIdentity {
                        canonical_id: Arc::from(""),
                        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        whole_hash: crate::semantic_query::HashValue::default(),
                        decl_name: Arc::from("<script-setup>"),
                    },
                    NodeScopeId::File {
                        canonical_id,
                        owner,
                        whole_hash,
                        ..
                    } => DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        owner: *owner,
                        whole_hash: *whole_hash,
                        decl_name: Arc::from("<script-setup>"),
                    },
                };
                return ReferenceProjectionPlan::Ready(self.graph().intern_node_with_scope(
                    SemanticNodeData::TypeParam {
                        decl,
                        param_index: binding.ordinal,
                        constraint,
                        default,
                        display_name: Arc::clone(&binding.name),
                    },
                    inputs.scope.clone(),
                ));
            }
        }
        let resolver_context = CarrierResolverContext::new(
            inputs.env,
            inputs.scope,
            inputs.name_resolution,
            inputs.scope_payload,
            inputs.shadowing,
            context,
        )
        .with_authored_resolution_debt(inputs.authored_resolution_debt);
        match self.plan_bare_ref_head(&resolver_context, &name, type_args.len()) {
            CarrierResolutionPlan::Ready(value) => ReferenceProjectionPlan::Ready(value),
            CarrierResolutionPlan::NeedsArgs(continuation) => ReferenceProjectionPlan::NeedsArgs {
                continuation,
                args: type_args,
                argument_context: context.into_structural_provenance(),
            },
        }
    }

    pub(super) fn plan_import_reference_projection(
        &self,
        data: &SemanticNodeData,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
    ) -> ReferenceProjectionPlan {
        let (specifier, qualifier, typeof_query) =
            data.import_type_head().expect("ImportType head");
        let qualifier: Vec<Arc<str>> = qualifier.iter().map(Arc::clone).collect();
        let type_args: Arc<[SemanticNodeId]> =
            Arc::from(data.carrier_type_args().to_vec().into_boxed_slice());
        let NodeScopeId::File {
            canonical_id: owner_canonical,
            ..
        } = inputs.scope
        else {
            return ReferenceProjectionPlan::Ready(self.opaque(QueryError::Miss));
        };
        let resolver_context = CarrierResolverContext::new(
            inputs.env,
            inputs.scope,
            inputs.name_resolution,
            inputs.scope_payload,
            inputs.shadowing,
            context,
        )
        .with_authored_resolution_debt(inputs.authored_resolution_debt);
        match self.plan_import_type_head(
            &resolver_context,
            owner_canonical.as_ref(),
            specifier,
            &qualifier,
            typeof_query,
            type_args.len(),
        ) {
            CarrierResolutionPlan::Ready(value) => ReferenceProjectionPlan::Ready(value),
            CarrierResolutionPlan::NeedsArgs(continuation) => ReferenceProjectionPlan::NeedsArgs {
                continuation,
                args: type_args,
                argument_context: context,
            },
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(super) fn finish_projection_node(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: &SemanticNodeData,
        _inputs: &LocatorViewInputs<'_>,
        _substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &ViewMemo,
    ) -> SemanticNodeId {
        let graph = self.graph();
        match data {
            SemanticNodeData::Alias(target) => projected(memo, *target, context),
            SemanticNodeData::TypeParam {
                decl,
                param_index,
                constraint,
                default,
                display_name,
            } => {
                let new_constraint = constraint.map(|value| projected(memo, value, context));
                let new_default = default.map(|value| projected(memo, value, context));
                if new_constraint == *constraint && new_default == *default {
                    node
                } else {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::TypeParam {
                            decl: decl.clone(),
                            param_index: *param_index,
                            constraint: new_constraint,
                            default: new_default,
                            display_name: Arc::clone(display_name),
                        },
                    )
                }
            }
            SemanticNodeData::Union(arms) => {
                let ids: Vec<_> = arms
                    .iter()
                    .map(|arm| projected(memo, *arm, context))
                    .collect();
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Union(Arc::from(ids.into_boxed_slice())),
                    )
                }
            }
            SemanticNodeData::Intersection(arms) => {
                let ids: Vec<_> = arms
                    .iter()
                    .map(|arm| projected(memo, *arm, context))
                    .collect();
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Intersection(Arc::from(ids.into_boxed_slice())),
                    )
                }
            }
            SemanticNodeData::MergedDecl { contributors } => {
                let ids: Vec<_> = contributors
                    .iter()
                    .map(|contributor| projected(memo, *contributor, context))
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::MergedDecl {
                        contributors: Arc::from(ids.into_boxed_slice()),
                    },
                )
            }
            SemanticNodeData::Array { element, readonly } => {
                let new_element = projected(memo, *element, context);
                if new_element == *element {
                    node
                } else {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Array {
                            element: new_element,
                            readonly: *readonly,
                        },
                    )
                }
            }
            SemanticNodeData::Tuple { elements, readonly } => {
                let projected_elements: Vec<_> = elements
                    .iter()
                    .map(|element| TupleElement {
                        label: element.label.clone(),
                        value: projected(memo, element.value, context),
                        optional: element.optional,
                        rest: element.rest,
                    })
                    .collect();
                match self.normalize_tuple_spread(&projected_elements, *readonly) {
                    super::super::build::NormalizedTupleShape::Array(array_node) => array_node,
                    super::super::build::NormalizedTupleShape::Tuple(normalized) => graph
                        .intern_preserving_scope(
                            node,
                            SemanticNodeData::Tuple {
                                elements: Arc::from(normalized.into_boxed_slice()),
                                readonly: *readonly,
                            },
                        ),
                }
            }
            SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            } => {
                let expressions: Vec<_> = expressions
                    .iter()
                    .map(|expression| projected(memo, *expression, context))
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::TemplateLiteral {
                        quasis: Arc::clone(quasis),
                        expressions: Arc::from(expressions.into_boxed_slice()),
                    },
                )
            }
            SemanticNodeData::Object(view) => {
                let member_context = context.into_structural_provenance();
                let members: Vec<_> = view
                    .members
                    .iter()
                    .map(|member| SurfaceMember {
                        name: Arc::clone(&member.name),
                        value: projected(memo, member.value, member_context),
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                        visibility: member.visibility,
                        spans: member.spans,
                        declaration_origin: member.declaration_origin.clone(),
                        declared_in_macro_type_arg: context.own_body_stamp(),
                        merge_role: context.role_stamp(),
                    })
                    .collect();
                let call_signatures: Vec<_> = view
                    .call_signatures
                    .iter()
                    .map(|signature| projected(memo, *signature, context))
                    .collect();
                let construct_signatures: Vec<_> = view
                    .construct_signatures
                    .iter()
                    .map(|signature| projected(memo, *signature, context))
                    .collect();
                let index_signatures: Vec<_> = view
                    .index_signatures
                    .iter()
                    .map(|signature| IndexSignature {
                        key_type: projected(memo, signature.key_type, context),
                        value_type: projected(memo, signature.value_type, context),
                        readonly: signature.readonly,
                        spans: signature.spans,
                        declaration_origin: signature.declaration_origin.clone(),
                    })
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Object(SurfaceView {
                        members: Arc::from(members.into_boxed_slice()),
                        call_signatures: Arc::from(call_signatures.into_boxed_slice()),
                        construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                        index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                        keyspace: view
                            .keyspace
                            .map(|keyspace| projected(memo, keyspace, context)),
                        has_index_signature: view.has_index_signature,
                    }),
                )
            }
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
                signature_span,
                return_type_span,
            } => {
                let params: Vec<_> = params
                    .iter()
                    .map(|parameter| FunctionParam {
                        name: parameter.name.clone(),
                        ty: projected(memo, parameter.ty, context),
                        optional: parameter.optional,
                        rest: parameter.rest,
                        span: parameter.span,
                    })
                    .collect();
                let type_parameters: Vec<_> = type_parameters
                    .iter()
                    .map(|parameter| TypeParamDecl {
                        name: Arc::clone(&parameter.name),
                        constraint: parameter
                            .constraint
                            .map(|value| projected(memo, value, context)),
                        default: parameter
                            .default
                            .map(|value| projected(memo, value, context)),
                    })
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Function {
                        params: Arc::from(params.into_boxed_slice()),
                        return_type: projected(memo, *return_type, context),
                        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
                        signature_span: *signature_span,
                        return_type_span: *return_type_span,
                    },
                )
            }
            SemanticNodeData::ConstructorType { signature } => projected(memo, *signature, context),
            SemanticNodeData::KeyOf { base } => {
                let base_id = projected(memo, *base, context);
                if may_reduce_operator(context) {
                    match self.execute_type_node(SemanticQueryKey::KeyOf {
                        base: base_id,
                        context,
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                        _ => self.opaque(QueryError::Miss),
                    }
                } else {
                    match graph.node_data(base_id).as_deref() {
                        Some(SemanticNodeData::Opaque(_)) | None => self.opaque(QueryError::Miss),
                        _ if base_id == *base => node,
                        _ => graph.intern_preserving_scope(
                            node,
                            SemanticNodeData::KeyOf { base: base_id },
                        ),
                    }
                }
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                let object_context = if matches!(
                    graph.node_data(*object).as_deref(),
                    Some(SemanticNodeData::IndexedAccess { .. })
                ) {
                    context.with_mode(ProjectionMode::Navigate)
                } else {
                    context
                };
                let object_id = projected(memo, *object, object_context);
                let index = match index {
                    IndexKey::String(value) => IndexKey::String(Arc::clone(value)),
                    IndexKey::Number(value) => IndexKey::Number(*value),
                    IndexKey::TypeNode(value) => {
                        self.normalized_index_key_node(projected(memo, *value, context))
                    }
                };
                let should_defer = matches!(index, IndexKey::TypeNode(_))
                    || !matches!(
                        graph.node_data(object_id).as_deref(),
                        Some(SemanticNodeData::Object(_))
                    );
                if should_defer {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::IndexedAccess {
                            object: object_id,
                            index,
                        },
                    )
                } else {
                    match self.execute_type_node(SemanticQueryKey::IndexedAccess {
                        base: object_id,
                        index,
                        mode: context.mode,
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                        _ => self.opaque(QueryError::Miss),
                    }
                }
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let argument_context = context.into_structural_provenance();
                let projected_args: Arc<[SemanticNodeId]> = Arc::from(
                    args.iter()
                        .map(|argument| projected(memo, *argument, argument_context))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let rebuild = |projected_args: Arc<[SemanticNodeId]>| {
                    if projected_args.as_ref() == args.as_ref() {
                        node
                    } else {
                        graph.intern_preserving_scope(
                            node,
                            SemanticNodeData::InstantiationRef {
                                base: base.clone(),
                                args: projected_args,
                            },
                        )
                    }
                };
                if base.canonical_id.as_ref() == "__builtin__" {
                    if self.is_promise_global_name(base.decl_name.as_ref()) {
                        return rebuild(projected_args);
                    }
                    let build_carrier = (context.demand == ReductionDemand::StructuralTransit
                        && (context.mode != ProjectionMode::Skeleton
                            || context.merge_role() == MemberMergeRole::Heritage))
                        || context.mode == ProjectionMode::Shallow
                        || (super::super::raise::is_l1_object_filter_utility(
                            base.decl_name.as_ref(),
                        ) && (context.mode == ProjectionMode::Navigate
                            || super::super::raise::utility_enumeration_domain_is_open_or_unknown(
                                self,
                                base,
                                &projected_args,
                            )))
                        || (matches!(
                            context.mode,
                            ProjectionMode::Navigate | ProjectionMode::Skeleton
                        ) && projected_args.iter().any(|argument| {
                            super::super::raise::builtin_lowering_argument_is_open(self, *argument)
                        }));
                    if build_carrier {
                        return rebuild(projected_args);
                    }
                    return match self.execute_type_node(SemanticQueryKey::Instantiate(
                        crate::semantic_query::InstantiateKey::new(
                            self.type_slot_for(
                                Arc::clone(&base.canonical_id),
                                base.owner,
                                Arc::clone(&base.decl_name),
                            ),
                            projected_args,
                            self.instantiate_context_for(&base.canonical_id, context),
                        ),
                    )) {
                        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                        _ => self.opaque(QueryError::Miss),
                    };
                }
                if matches!(
                    context.mode,
                    ProjectionMode::Navigate | ProjectionMode::Skeleton | ProjectionMode::Shallow
                ) {
                    rebuild(projected_args)
                } else {
                    match self.execute_type_node(SemanticQueryKey::Instantiate(
                        crate::semantic_query::InstantiateKey::new(
                            self.type_slot_for(
                                Arc::clone(&base.canonical_id),
                                base.owner,
                                Arc::clone(&base.decl_name),
                            ),
                            projected_args,
                            self.instantiate_context_for(&base.canonical_id, context),
                        ),
                    )) {
                        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                        _ => self.opaque(QueryError::Miss),
                    }
                }
            }
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::RawFallback { .. }
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::SyntheticBinding { .. }
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::Mapped { .. }
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_) => {
                unreachable!("leaf or staged node reached projection finish")
            }
        }
    }
}
