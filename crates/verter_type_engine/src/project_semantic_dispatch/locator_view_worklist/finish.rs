use super::*;

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    pub(super) fn plan_bare_reference_projection(
        &self,
        data: &SemanticNodeData,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
    ) -> ReferenceProjectionPlan {
        let (name, _bare_scope) = data.bare_ref_head().expect("BareRef head");
        let name = Arc::<str>::from(name);
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
            CarrierResolutionPlan::Ready(value) => ReferenceProjectionPlan::Ready(value.node),
            CarrierResolutionPlan::NeedsArgs(continuation) => {
                let argument_context = self.reference_argument_context(&continuation, context);
                ReferenceProjectionPlan::NeedsArgs {
                    continuation,
                    args: type_args,
                    argument_context,
                }
            }
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
        let qualifier: Vec<verter_type_expr::OwnedStr> = qualifier.to_vec();
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
            CarrierResolutionPlan::Ready(value) => ReferenceProjectionPlan::Ready(value.node),
            CarrierResolutionPlan::NeedsArgs(continuation) => {
                let argument_context = self.reference_argument_context(&continuation, context);
                ReferenceProjectionPlan::NeedsArgs {
                    continuation,
                    args: type_args,
                    argument_context,
                }
            }
        }
    }

    /// How `node`'s projection finishes once its children are projected: a
    /// rebuilt or reduced node, or — for an application that evaluates — the
    /// instantiation whose node it is.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_projection_node(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: &SemanticNodeData,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &ViewMemo,
        ancestors: &[ProjectionFrame],
    ) -> ProjectionFinish {
        if let SemanticNodeData::InstantiationRef { base, args } = data {
            return self.finish_application_projection(
                node,
                context,
                base,
                args.as_arc(),
                memo,
                ancestors,
            );
        }
        ProjectionFinish::Node(self.finish_projection_structure(
            node,
            context,
            data,
            inputs,
            substitutions,
            memo,
            self.in_deferred_interior(ancestors),
            self.answer_position(ancestors),
        ))
    }

    /// An application over its projected arguments: the application itself
    /// where the view keeps it a carrier, its recursive back-edge, or the
    /// instantiation it evaluates to.
    fn finish_application_projection(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        base: &crate::semantic_query::DeclIdentity,
        args: &Arc<[SemanticNodeId]>,
        memo: &ViewMemo,
        ancestors: &[ProjectionFrame],
    ) -> ProjectionFinish {
        let graph = self.graph();
        let defers_type_arguments = self.defers_type_arguments(base);
        let argument_context = Self::argument_context_for(defers_type_arguments, context);
        let projected_args: Arc<[SemanticNodeId]> = Arc::from(
            args.iter()
                .map(|argument| {
                    self.canonical_argument_carrier(projected(
                        memo,
                        *argument,
                        self.argument_operand_context(*argument, argument_context),
                    ))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let projected_args = if may_reduce_operator(context) {
            self.evaluate_carrier_arguments_deferring(
                defers_type_arguments,
                &projected_args,
                context,
            )
            .into_active_query_build_arguments(self)
        } else {
            projected_args
        };
        let rebuild = |projected_args: Arc<[SemanticNodeId]>| {
            if projected_args.as_ref() == args.as_ref() {
                node
            } else {
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::InstantiationRef {
                        base: base.clone(),
                        args: crate::semantic_query::PayloadArray::from(projected_args),
                    },
                )
            }
        };
        // An infer declaration is a pattern binder. Preserve its helper
        // application until the relation owner demands the pattern's shape.
        if projected_args.iter().any(|&arg| {
            self.graph()
                .binder_dependencies_for(arg)
                .may_declare_infer()
        }) {
            return ProjectionFinish::Node(rebuild(projected_args));
        }
        if base.canonical_id.as_ref() == "__builtin__" {
            if self.is_promise_global_name(base.decl_name.as_ref()) {
                return ProjectionFinish::Node(rebuild(projected_args));
            }
            let build_carrier = (context.demand == ReductionDemand::StructuralTransit
                && (context.mode != ProjectionMode::Skeleton
                    || context.merge_role() == MemberMergeRole::Heritage))
                || context.mode == ProjectionMode::Shallow
                || (super::super::raise::is_l1_object_filter_utility(base.decl_name.as_ref())
                    && (context.mode == ProjectionMode::Navigate
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
                return ProjectionFinish::Node(rebuild(projected_args));
            }
            return ProjectionFinish::Instantiate(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    self.type_slot_for(
                        base.canonical_id.clone(),
                        base.owner,
                        base.decl_name.clone(),
                    ),
                    projected_args,
                    self.instantiate_context_for(&base.canonical_id, context),
                ),
            ));
        }
        if matches!(
            context.mode,
            ProjectionMode::Navigate | ProjectionMode::Skeleton | ProjectionMode::Shallow
        ) {
            ProjectionFinish::Node(rebuild(projected_args))
        } else if self.is_instantiate_active(
            base.canonical_id.as_ref(),
            base.owner,
            base.decl_name.as_ref(),
        ) && self.in_deferred_position(ancestors)
        {
            // An application of the declaration an enclosing
            // `build_instantiate` frame is still materialising, in a
            // position the checker defers, is its recursive back-edge,
            // recording the instantiation it stands for — the same rule as
            // a 0-arg `DeclRef`. In a position the checker instantiates
            // eagerly it is instantiated below.
            ProjectionFinish::Node(self.recursive_ref_sentinel(base, projected_args))
        } else {
            ProjectionFinish::Instantiate(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    self.type_slot_for(
                        base.canonical_id.clone(),
                        base.owner,
                        base.decl_name.clone(),
                    ),
                    projected_args,
                    self.instantiate_context_for(&base.canonical_id, context),
                ),
            ))
        }
    }

    /// The type a closed `keyof` or indexed access nested in a published
    /// `Expanded` answer denotes. The checker reduces an operator over
    /// closed operands when it builds the type, so the answer holds what the
    /// operator reads in an object, tuple, signature, type argument or union
    /// as its root does: an access reads its member (a member naming a
    /// declaration is that declaration, as the checker prints it), and a
    /// `keyof` is the literal key set the union it stands in reduces over.
    ///
    /// `None` keeps the operator: another altitude (the carrier is the
    /// demand-driven shape there), an operand naming a type parameter or an
    /// `infer` binder (the operator is generic), or a read that stopped
    /// (its partiality folds into the build) or reads nothing.
    fn reduce_closed_answer_operator(
        &self,
        operator: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        if context.mode != ProjectionMode::Expanded || !may_reduce_operator(context) {
            return None;
        }
        let graph = self.graph();
        if graph.binder_dependencies_for(operator).may_mention_binder() {
            return None;
        }
        let reduced = match graph.node_data(operator).as_deref() {
            Some(SemanticNodeData::KeyOf { base }) => self.key_set_of(*base),
            Some(SemanticNodeData::IndexedAccess { .. }) => {
                match self.named_indexed_access(operator, context) {
                    Some(named) => named.into_complete_active_query_build_node(self),
                    None => self
                        .evaluate_deferred_semantic_node_with_context(operator, context)
                        .into_complete_active_query_build_node(self),
                }
            }
            _ => None,
        }?;
        (reduced != operator
            && !matches!(
                graph.node_data(reduced).as_deref(),
                Some(SemanticNodeData::Opaque(_) | SemanticNodeData::KeyOf { .. })
            ))
        .then_some(reduced)
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn finish_projection_structure(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: &SemanticNodeData,
        inputs: &LocatorViewInputs<'_>,
        _substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &ViewMemo,
        deferred_position: bool,
        position: AnswerPosition,
    ) -> SemanticNodeId {
        let graph = self.graph();
        match data {
            // Rebuilt over the PROJECTED operands so a later substitution can
            // still reduce the operation through its owning family.
            SemanticNodeData::IntrinsicApplication { op, args } => {
                let projected_args: Vec<SemanticNodeId> = args
                    .iter()
                    .map(|argument| projected(memo, *argument, context))
                    .collect();
                graph.intern_node(
                    SemanticNodeData::intrinsic_application(
                        *op,
                        Arc::from(projected_args.into_boxed_slice()),
                    )
                    .expect("rebuilt one-for-one from a well-formed application"),
                )
            }
            SemanticNodeData::Alias(target) => projected(memo, *target, context),
            // The same class expression over its projected type arguments
            // and instance surface.
            SemanticNodeData::ClassExpressionInstance {
                identity,
                type_arguments,
                surface,
            } => {
                let projected_arguments: Vec<SemanticNodeId> = type_arguments
                    .iter()
                    .map(|argument| projected(memo, *argument, context))
                    .collect();
                let projected_surface = projected(memo, *surface, context);
                if projected_surface == *surface && projected_arguments[..] == type_arguments[..] {
                    node
                } else {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::ClassExpressionInstance {
                            identity: Arc::clone(identity),
                            type_arguments: crate::semantic_query::PayloadArray::from(projected_arguments.into_boxed_slice()),
                            surface: projected_surface,
                        },
                    )
                }
            }
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
                            display_name: (*display_name).clone(),
                        },
                    )
                }
            }
            SemanticNodeData::Union(arms) => {
                use verter_session_query::flow::policy::NullabilityPolicy;

                let category = arms.origin_category();
                let ids: Vec<_> = arms
                    .iter()
                    .map(|arm| projected(memo, *arm, context))
                    .collect();
                // A published expanded answer belongs to this declaration's
                // project. Its union uses that project's null algebra; the
                // shared structural carrier remains policy-neutral/strict.
                // The canonical builder includes the policy in node identity.
                let nullability = if context.mode == ProjectionMode::Expanded
                    && may_reduce_operator(context)
                {
                    inputs
                        .scope
                        .canonical_file()
                        .map_or(NullabilityPolicy::Strict, |canonical| {
                            NullabilityPolicy::from_strict_null_checks(
                                self.ctx
                                    .semantic_compiler_options_for(&canonical)
                                    .strict_null_checks,
                            )
                        })
                } else {
                    NullabilityPolicy::Strict
                };
                let re_decides = self.composite_rebuild_re_decides(category, &ids, true);
                let erases_nullish = !nullability.is_strict()
                    && ids.iter().any(|arm| {
                        matches!(
                            graph.node_data(*arm).as_deref(),
                            Some(SemanticNodeData::Primitive(
                                PrimitiveKind::Null | PrimitiveKind::Undefined
                            ))
                        )
                    });
                if ids.as_slice() == &***arms && (!erases_nullish || !re_decides) {
                    // Identity projection: nothing to rebuild and nothing to
                    // re-decide — the original carrier (its category, order
                    // and interned identity, including alias origin) stays
                    // exactly itself when no nullable arm must be erased.
                    return node;
                }
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else if re_decides {
                    // The projection of a canonical/authored composite is a
                    // DERIVED result — route it through the canonical
                    // authority (the authored shell stays recoverable via
                    // `node`); a derived multi-arm composite interns Global.
                    self.intern_normalized_union(&ids, nullability)
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
                    // Order- and scope-preserving projection rebuild: a
                    // heritage body or an ordered overload carrier (or a
                    // possibly-callable rebuilt arm set) keeps its verbatim
                    // order, and a heritage body stays one.
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Intersection(
                            crate::semantic_query::composite::CompositeList::rebuilt_from(
                                category,
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
                        contributors: crate::semantic_query::PayloadArray::from(ids.into_boxed_slice()),
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
                                elements: crate::semantic_query::PayloadArray::from(normalized.into_boxed_slice()),
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
                        quasis: (*quasis).clone(),
                        expressions: crate::semantic_query::PayloadArray::from(expressions.into_boxed_slice()),
                    },
                )
            }
            SemanticNodeData::Object(view) => {
                let member_context = context.into_structural_provenance();
                let members: Vec<_> = view
                    .positive_members()
                    .iter()
                    .map(|member| SurfaceMember {
                        key: match &member.key {
                            // A key that already names a nominal identity is
                            // answered without projecting it at all: the
                            // projection (and its memo writes) is computed
                            // only on the fallback path.
                            verter_type_expr::AuthoredPropertyKey::Computed(computed) => self
                                .unique_symbol_identity_for_typeof_node(*computed)
                                .map(verter_type_expr::AuthoredPropertyKey::UniqueSymbol)
                                .unwrap_or_else(|| {
                                    let projected = projected(memo, *computed, member_context);
                                    self.unique_symbol_identity_for_typeof_node(projected)
                                        .map(verter_type_expr::AuthoredPropertyKey::UniqueSymbol)
                                        .unwrap_or(verter_type_expr::AuthoredPropertyKey::Computed(
                                            projected,
                                        ))
                                }),
                            verter_type_expr::AuthoredPropertyKey::String(value) => {
                                verter_type_expr::AuthoredPropertyKey::String(value.clone())
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
                predicate,
                is_abstract,
            } => {
                let params: Vec<_> = params
                    .iter()
                    .map(|parameter| FunctionParam {
                        name: parameter.name.clone(),
                        ty: projected(memo, parameter.ty, context),
                        optional: parameter.optional,
                        rest: parameter.rest,
                        span: parameter.span,
                        declared_literal: parameter.declared_literal,
                    })
                    .collect();
                let type_parameters: Vec<_> = type_parameters
                    .iter()
                    .map(|parameter| TypeParamDecl {
                        name: parameter.name.clone(),
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
                let signature = graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Signature {
                        kind: *kind,
                        params: crate::semantic_query::PayloadArray::from(params.into_boxed_slice()),
                        return_type: projected(memo, *return_type, context),
                        type_parameters: crate::semantic_query::PayloadArray::from(type_parameters.into_boxed_slice()),
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
                            crate::semantic_query::SignatureReturnCarrier::DeferredFlow(_)
                    | crate::semantic_query::SignatureReturnCarrier::EvaluatedFlow(_) => return_carrier.clone(),
                        },
                        signature_span: *signature_span,
                        return_type_span: *return_type_span,
                        predicate: predicate
                            .map(|predicate| predicate.map_type(|ty| projected(memo, ty, context))),
                        is_abstract: *is_abstract,
                    },
                );
                if context.mode == crate::semantic_query::ProjectionMode::Expanded {
                    self.materialize_returned_callable(signature)
                } else {
                    signature
                }
            }
            SemanticNodeData::KeyOf { base } => {
                let base_id = projected(
                    memo,
                    *base,
                    context.into_structural_transit_with_mode(ProjectionMode::Navigate),
                );
                if may_reduce_operator(context) {
                    let keys = match self.execute_type_node(SemanticQueryKey::KeyOf {
                        base: base_id,
                        context,
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                        _ => return self.opaque(QueryError::Miss),
                    };
                    // An arm's keys are constituents of the union or
                    // intersection the answer holds: it reduces over P's
                    // keys, as the checker's union flattens them. Elsewhere
                    // the carrier is the `keyof P` the checker prints.
                    if position == AnswerPosition::Constituent {
                        self.reduce_closed_answer_operator(keys, context)
                            .unwrap_or(keys)
                    } else {
                        keys
                    }
                } else {
                    match graph.node_data(base_id).as_deref() {
                        None => self.opaque(QueryError::Miss),
                        _ if super::super::raise::node_is_unknown_materializing_failure(self, base_id) => self.opaque(QueryError::Miss),
                        _ if base_id == *base => node,
                        _ => graph.intern_preserving_scope(
                            node,
                            SemanticNodeData::KeyOf { base: base_id },
                        ),
                    }
                }
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                let object_context = self.indexed_access_operand_context(*object, context);
                let object_id = projected(memo, *object, object_context);
                let index = match index {
                    IndexKey::String(value) => IndexKey::String(value.clone()),
                    IndexKey::Number(value) => IndexKey::Number(*value),
                    IndexKey::UniqueSymbol(identity) => IndexKey::UniqueSymbol(identity.clone()),
                    // A key that already names a nominal identity is
                    // answered without projecting it at all; the projection
                    // is computed only on the fallback path.
                    IndexKey::Computed(value) => self
                        .unique_symbol_identity_for_typeof_node(*value)
                        .map(IndexKey::UniqueSymbol)
                        .unwrap_or_else(|| {
                            let projected = projected(memo, *value, context);
                            self.unique_symbol_identity_for_typeof_node(projected)
                                .map(IndexKey::UniqueSymbol)
                                .unwrap_or_else(|| self.normalized_index_key_node(projected))
                        }),
                };
                let proven_empty = matches!(&index, IndexKey::Computed(key) if self.known_key_space_domain(*key).is_some_and(|domain| domain.is_empty()));
                let should_defer = !proven_empty
                    && (!may_reduce_operator(context)
                        || context.mode == ProjectionMode::Skeleton
                        || deferred_position
                        || !matches!(graph.node_data(object_id).as_deref(), Some(SemanticNodeData::Object(_))));
                if should_defer {
                    let access = graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::IndexedAccess {
                            object: object_id,
                            index,
                        },
                    );
                    // The checker never keeps an access over closed operands:
                    // it reads the member when it builds the type, wherever
                    // the answer holds the access. The projection's own value
                    // is its demand's to read; an operand, its operator's.
                    match position {
                        AnswerPosition::Constituent | AnswerPosition::Interior => self
                            .reduce_closed_answer_operator(access, context)
                            .unwrap_or(access),
                        AnswerPosition::Root | AnswerPosition::Operand => access,
                    }
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
            SemanticNodeData::InstantiationRef { .. } => {
                unreachable!("an application finishes through its instantiation plan")
            }
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::RawFallback { .. }
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::SyntheticBinding { .. }
            | SemanticNodeData::DeferredCallable(_)
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::Mapped { .. }
            | SemanticNodeData::TypeOf(_)
            // The nominal terminal self-resolves; it reaches projection
            // finish only as an already-complete leaf.
            | SemanticNodeData::TypeOfNominal(_)
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_) => {
                unreachable!("leaf or staged node reached projection finish")
            }
        }
    }
}
