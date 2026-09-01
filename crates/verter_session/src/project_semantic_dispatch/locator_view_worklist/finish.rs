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
                // The ONE shared script-setup `TypeParam` construction: the
                // binding carries the content-free (name, ordinal) facts;
                // the helper re-borrows the clause lease-only from the
                // pinned artifact and lowers the selected parameter's
                // bounds. A missing / stale re-borrow is its typed miss
                // (Opaque + cache suppression), never a bound-free binder.
                let node = self.lower_script_setup_type_param_binding(
                    binding,
                    inputs.env,
                    inputs.scope,
                    inputs.name_resolution,
                    inputs.scope_payload,
                    inputs.shadowing,
                    substitutions,
                    context,
                );
                return ReferenceProjectionPlan::Ready(node);
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
                let category = arms.origin_category();
                let ids: Vec<_> = arms
                    .iter()
                    .map(|arm| projected(memo, *arm, context))
                    .collect();
                if ids.as_slice() == &***arms {
                    // Identity projection: nothing to rebuild and nothing to
                    // re-decide — the original carrier (its category, order
                    // and interned identity) stays exactly itself.
                    return node;
                }
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else if self.composite_rebuild_re_decides(category, &ids, true) {
                    // The projection of a canonical/authored composite is a
                    // DERIVED result — route it through the canonical
                    // authority (the authored shell stays recoverable via
                    // `node`); a derived multi-arm composite interns Global.
                    self.intern_normalized_union_or_intersection(&ids, true)
                } else {
                    // Order- and scope-preserving projection rebuild for the
                    // carrier categories whose arm order is semantics.
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Union(
                            crate::semantic_query::composite::CompositeList::preserving_rebuild(
                                Arc::from(ids.into_boxed_slice()),
                            ),
                        ),
                    )
                }
            }
            SemanticNodeData::Intersection(arms) => {
                let category = arms.origin_category();
                let ids: Vec<_> = arms
                    .iter()
                    .map(|arm| projected(memo, *arm, context))
                    .collect();
                if ids.as_slice() == &***arms {
                    // Identity projection — see the Union arm.
                    return node;
                }
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else if self.composite_rebuild_re_decides(category, &ids, false) {
                    // Derived projection result (and provably order-safe:
                    // no rebuilt arm may contribute call signatures) —
                    // canonical authority, Global intern.
                    self.intern_normalized_union_or_intersection(&ids, false)
                } else {
                    // Order- and scope-preserving projection rebuild: an
                    // ordered heritage/overload carrier (or a possibly-
                    // callable rebuilt arm set) keeps its verbatim order.
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Intersection(
                            crate::semantic_query::composite::CompositeList::preserving_rebuild(
                                Arc::from(ids.into_boxed_slice()),
                            ),
                        ),
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
                let members: Vec<_> =
                    view.positive_members()
                        .iter()
                        .map(|member| SurfaceMember {
                            key: match &member.key {
                                verter_type_expr::AuthoredPropertyKey::Computed(computed) => {
                                    self.unique_symbol_identity_for_typeof_node(*computed)
                                        .map(verter_type_expr::AuthoredPropertyKey::UniqueSymbol)
                                        .unwrap_or_else(|| {
                                            verter_type_expr::AuthoredPropertyKey::Computed(
                                                projected(memo, *computed, member_context),
                                            )
                                        })
                                }
                                verter_type_expr::AuthoredPropertyKey::String(value) => {
                                    verter_type_expr::AuthoredPropertyKey::String(Arc::clone(value))
                                }
                                verter_type_expr::AuthoredPropertyKey::Number(value) => {
                                    verter_type_expr::AuthoredPropertyKey::Number(*value)
                                }
                                verter_type_expr::AuthoredPropertyKey::UniqueSymbol(identity) => {
                                    verter_type_expr::AuthoredPropertyKey::UniqueSymbol(
                                        identity.clone(),
                                    )
                                }
                            },
                            value: projected(memo, member.value, member_context),
                            optional: member.optional,
                            readonly: member.readonly,
                            method_kind: member.method_kind,
                            has_implementation_body: member.has_implementation_body,
                            visibility: member.visibility,
                            // Projection preserves the member's excess-property
                            // provenance verbatim (structure-preserving rewrite).
                            excess_origin: member.excess_origin,
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
                let mut members = members.into_iter();
                let mut calls = call_signatures.into_iter();
                let mut constructs = construct_signatures.into_iter();
                let mut indexes = index_signatures.into_iter();
                let entries = view
                    .entries
                    .iter()
                    .map(|entry| match entry {
                        crate::semantic_query::SurfaceEntry::Member(_) => {
                            crate::semantic_query::SurfaceEntry::Member(
                                members.next().expect("derived member index matches stream"),
                            )
                        }
                        crate::semantic_query::SurfaceEntry::CallSignature(_) => {
                            crate::semantic_query::SurfaceEntry::CallSignature(
                                calls.next().expect("derived call index matches stream"),
                            )
                        }
                        crate::semantic_query::SurfaceEntry::ConstructSignature(_) => {
                            crate::semantic_query::SurfaceEntry::ConstructSignature(
                                constructs
                                    .next()
                                    .expect("derived construct index matches stream"),
                            )
                        }
                        crate::semantic_query::SurfaceEntry::IndexSignature(_) => {
                            crate::semantic_query::SurfaceEntry::IndexSignature(
                                indexes.next().expect("derived index matches stream"),
                            )
                        }
                    })
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Object(SurfaceView::from_entries(
                        entries,
                        view.keyspace
                            .map(|keyspace| projected(memo, keyspace, context)),
                        view.has_known_index_signature(),
                    )),
                )
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                let projected = program.map_child_nodes(|child| projected(memo, child, context));
                if projected == *program {
                    node
                } else {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::ObjectSpreadProgram(projected),
                    )
                }
            }
            SemanticNodeData::Signature {
                kind,
                params,
                return_type,
                type_parameters,
                occurrence,
                return_carrier,
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
                        param: projected(memo, parameter.param, context),
                        constraint: parameter
                            .constraint
                            .map(|value| projected(memo, value, context)),
                        default: parameter
                            .default
                            .map(|value| projected(memo, value, context)),
                        is_const: parameter.is_const,
                    })
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Signature {
                        kind: *kind,
                        params: Arc::from(params.into_boxed_slice()),
                        return_type: projected(memo, *return_type, context),
                        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
                        // Projection preserves the occurrence; a declared
                        // carrier retargets the projected return.
                        occurrence: occurrence.clone(),
                        return_carrier: match return_carrier {
                            crate::semantic_query::SignatureReturnCarrier::Declared(_) => {
                                crate::semantic_query::SignatureReturnCarrier::Declared(projected(
                                    memo,
                                    *return_type,
                                    context,
                                ))
                            }
                            crate::semantic_query::SignatureReturnCarrier::Function(source) => {
                                crate::semantic_query::SignatureReturnCarrier::Function(
                                    source.clone(),
                                )
                            }
                        },
                        signature_span: *signature_span,
                        return_type_span: *return_type_span,
                    },
                )
            }
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
                    IndexKey::UniqueSymbol(identity) => IndexKey::UniqueSymbol(identity.clone()),
                    IndexKey::Computed(value) => self
                        .unique_symbol_identity_for_typeof_node(*value)
                        .map(IndexKey::UniqueSymbol)
                        .unwrap_or_else(|| {
                            self.normalized_index_key_node(projected(memo, *value, context))
                        }),
                };
                let should_defer = matches!(index, IndexKey::Computed(_))
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
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::SyntheticBinding { .. }
            | SemanticNodeData::DeferredCallable(_)
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
