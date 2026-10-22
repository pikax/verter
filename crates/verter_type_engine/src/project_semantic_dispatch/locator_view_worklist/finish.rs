use super::*;

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    pub(super) fn plan_bare_reference_projection(
        &self,
        data: &SemanticNodeData,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        deferred_interior: bool,
    ) -> ReferenceProjectionPlan {
        let (name, bare_scope) = data.bare_ref_head().expect("BareRef head");
        let name = Arc::<str>::from(name);
        let type_args: Arc<[SemanticNodeId]> =
            Arc::from(data.carrier_type_args().to_vec().into_boxed_slice());
        // Members retain back-edges; an alias root or conditional operand
        // demands the sibling's type through the same contribution folder.
        if let Some((specifier, decl_name)) = self.ambient_namespace_type_target(bare_scope, &name)
        {
            let continuation = if deferred_interior {
                CarrierArgsContinuation::Intern {
                    head: crate::project_semantic_dispatch::carrier::RefHeadResolution::Unresolved,
                    name,
                    scope: bare_scope.clone(),
                }
            } else {
                CarrierArgsContinuation::ExternalModule {
                    specifier,
                    decl_name,
                    name,
                    scope: bare_scope.clone(),
                    context,
                }
            };
            let argument_context = self.reference_argument_context(&continuation, context);
            return ReferenceProjectionPlan::NeedsArgs {
                continuation,
                args: type_args,
                argument_context,
            };
        }
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
        suspend: bool,
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
        let plan = self.plan_import_type_head(
            &resolver_context,
            owner_canonical.as_ref(),
            specifier,
            &qualifier,
            typeof_query,
            type_args.len(),
        );
        let argument_context = match &plan {
            CarrierResolutionPlan::NeedsArgs(continuation) => {
                self.reference_argument_context(continuation, context)
            }
            CarrierResolutionPlan::Ready(_) => context,
        };
        match plan {
            CarrierResolutionPlan::NeedsArgs(CarrierArgsContinuation::ReadTypeof { key }) => {
                let read = Box::new(TypeofRead {
                    key: *key,
                    type_args,
                    argument_context,
                });
                if suspend {
                    ReferenceProjectionPlan::Typeof(read)
                } else {
                    self.read_typeof_in_place(*read)
                }
            }
            CarrierResolutionPlan::Ready(value) => ReferenceProjectionPlan::Ready(value.node),
            CarrierResolutionPlan::NeedsArgs(continuation) => ReferenceProjectionPlan::NeedsArgs {
                continuation,
                args: type_args,
                argument_context,
            },
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
                inputs,
                ancestors,
            );
        }
        if matches!(data, SemanticNodeData::IndexedAccess { .. }) {
            return self.finish_indexed_projection(
                node,
                context,
                data,
                memo,
                self.in_deferred_interior(ancestors),
            );
        }
        ProjectionFinish::Node(self.finish_projection_structure(
            node,
            context,
            data,
            inputs,
            substitutions,
            memo,
        ))
    }

    /// The slot `child`, `parent`'s child at `index`, projects in. An
    /// application a tuple spreads, under a carrier mode that splices it, is
    /// [`ProjectionSlot::Spread`]: the tuple's own positions and length are
    /// the application's, so it is instantiated there as the checker
    /// instantiates it, while the same application stays a carrier anywhere
    /// else. Its two readings are memoized apart, so neither depends on
    /// which occurrence the projection meets first.
    pub(super) fn child_projection_slot(
        &self,
        parent: &SemanticNodeData,
        index: usize,
        child: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> ProjectionSlot {
        let spread = matches!(
            context.mode,
            ProjectionMode::Navigate | ProjectionMode::Shallow
        ) && matches!(
            parent,
            SemanticNodeData::Tuple { elements, .. }
                if elements
                    .get(index)
                    .is_some_and(|element| element.rest && element.value == child)
        ) && matches!(
            self.graph().node_data(child).as_deref(),
            Some(SemanticNodeData::InstantiationRef { .. })
        );
        if spread {
            ProjectionSlot::Spread
        } else {
            ProjectionSlot::Plain
        }
    }

    /// The slot the node being finished projects in, read off the frame of
    /// the composite enclosing it, whose cursor has just passed it.
    pub(super) fn finishing_projection_slot(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        ancestors: &[ProjectionFrame],
    ) -> ProjectionSlot {
        match ancestors.last() {
            Some(ProjectionFrame::CompositeResume {
                data, next_child, ..
            }) => next_child
                .checked_sub(1)
                .map_or(ProjectionSlot::Plain, |index| {
                    self.child_projection_slot(data.as_ref(), index, node, context)
                }),
            _ => ProjectionSlot::Plain,
        }
    }

    /// Select through the shared indexed-access query at navigation altitude,
    /// then let this projection's worklist resolve the selected value at the
    /// caller's demand. A namespace's lazy TypeOf members can then suspend.
    fn finish_indexed_projection(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: &SemanticNodeData,
        memo: &ViewMemo,
        deferred_position: bool,
    ) -> ProjectionFinish {
        let SemanticNodeData::IndexedAccess { object, index } = data else {
            unreachable!("indexed projection carries an indexed access")
        };
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
                || !matches!(
                    self.graph().node_data(object_id).as_deref(),
                    Some(SemanticNodeData::Object(_))
                ));
        if should_defer {
            ProjectionFinish::Node(self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::IndexedAccess {
                    object: object_id,
                    index,
                },
            ))
        } else {
            match self.execute_type_node(SemanticQueryKey::IndexedAccess {
                base: object_id,
                index,
                mode: ProjectionMode::Navigate,
            }) {
                QueryResult::Value(SemanticQueryOutput { value, .. }) if value != node => {
                    ProjectionFinish::Project(value)
                }
                QueryResult::Value(_) => ProjectionFinish::Node(node),
                _ => ProjectionFinish::Node(self.opaque(QueryError::Miss)),
            }
        }
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
        inputs: &LocatorViewInputs<'_>,
        ancestors: &[ProjectionFrame],
    ) -> ProjectionFinish {
        let graph = self.graph();
        let defers_type_arguments = self.defers_type_arguments(base);
        let argument_context = Self::argument_context_for(defers_type_arguments, context);
        let projected_args: Arc<[SemanticNodeId]> = Arc::from(
            args.iter()
                .map(|argument| {
                    let operand_context =
                        self.argument_operand_context(*argument, argument_context);
                    self.canonical_argument_carrier(self.answer_argument(
                        projected(memo, *argument, operand_context),
                        context,
                        argument_context,
                        operand_context,
                        inputs,
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
        // The carrier modes keep an application a carrier, except one a
        // tuple spreads over closed arguments, which is instantiated to
        // splice (`[...Deep<[0, 0]>, 1]`).
        let keeps_carrier = matches!(
            context.mode,
            ProjectionMode::Navigate | ProjectionMode::Skeleton | ProjectionMode::Shallow
        ) && !(self.finishing_projection_slot(node, context, ancestors)
            == ProjectionSlot::Spread
            && !projected_args.iter().any(|argument| {
                super::super::raise::builtin_lowering_argument_is_open(self, *argument)
            }));
        if keeps_carrier {
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

    /// What a closed `keyof` or indexed access nested in a published
    /// `Expanded` answer reads. The checker reduces an operator over closed
    /// operands when it builds the type, so the answer holds what the
    /// operator reads in an object, tuple, signature, type argument or union
    /// as its root does: an access reads its member (a member naming a
    /// declaration is that declaration, as the checker prints it), and a
    /// `keyof` is the literal key set the union it stands in reduces over.
    ///
    /// `None` keeps the operator: another altitude (the carrier is the
    /// demand-driven shape there), an operand naming a type parameter or an
    /// `infer` binder (the operator is generic), or a read that stopped
    /// (its partiality folds into the build) or reads nothing.
    ///
    /// The projection of an operator itself is always its carrier, wherever
    /// it is written, so one projected node serves every place that shares
    /// it; the place that holds it reduces it — a composite for the
    /// accesses among its children ([`Self::answer_member`]), the answer's
    /// own union or intersection for its `keyof` and access arms
    /// ([`Self::reduce_answer_constituents`]).
    fn reduce_closed_answer_operator(
        &self,
        operator: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<ClosedOperatorRead> {
        if context.mode != ProjectionMode::Expanded || !may_reduce_operator(context) {
            return None;
        }
        let graph = self.graph();
        if graph.binder_dependencies_for(operator).may_mention_binder() {
            return None;
        }
        let read = match graph.node_data(operator).as_deref() {
            Some(SemanticNodeData::KeyOf { base }) => {
                ClosedOperatorRead::Evaluated(self.key_set_of(*base)?)
            }
            Some(SemanticNodeData::IndexedAccess { .. }) => {
                match self.named_indexed_access(operator, context) {
                    Some(named) => ClosedOperatorRead::Named(
                        named.into_complete_active_query_build_node(self)?,
                    ),
                    None => ClosedOperatorRead::Evaluated(
                        self.evaluate_deferred_semantic_node_with_context(operator, context)
                            .into_complete_active_query_build_node(self)?,
                    ),
                }
            }
            _ => return None,
        };
        let node = read.node();
        // A `keyof` that settles to a `keyof` read nothing; an access may
        // read a member written `keyof P`, which is what it denotes.
        let reads_nothing = match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Opaque(_)) => true,
            Some(SemanticNodeData::KeyOf { .. }) => matches!(
                graph.node_data(operator).as_deref(),
                Some(SemanticNodeData::KeyOf { .. })
            ),
            _ => false,
        };
        (node != operator && !reads_nothing).then_some(read)
    }

    /// The answer a closed operator held at `context` reads, as `then`
    /// takes it. A member an access evaluated to is projected as the
    /// answer projects it, so the closed operators inside it read as they
    /// do written directly (`{ b: Y['c'] }` read through `X['a']` answers
    /// `{ b: 1 }`); a declaration the access names stays that name.
    ///
    /// An operator whose read reaches the same operator again (`X['a']`
    /// over `type X = { a: { b: X['a'] } }`) keeps it as written there:
    /// the re-entry reads the dispatcher's in-progress record, never a
    /// depth limit.
    ///
    /// One operator is read once per demand ([`AnswerReads`]), however
    /// many places of the answer hold it. Reads nest on the native stack, so
    /// one more read past the connected demand's nesting bound is refused:
    /// the operator stays as written and the demand is a partial.
    fn read_answer_operator(
        &self,
        operator: SemanticNodeId,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
        then: impl FnOnce(SemanticNodeId) -> SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        if self.carrier_normalizing_holds(operator) {
            return None;
        }
        let open = self.answer_reads.borrow().open;
        if let Some(reasons) = self.connected_demand().nesting_trip(open) {
            self.fold_local_partial_completeness(reasons);
            return None;
        }
        let build = self.build_local_taint.borrow().len();
        let key = (operator, context, inputs.scope.clone());
        let frame = self.answer_reads.borrow_mut().open(build);
        let memoized = self.answer_reads.borrow().memoized(&key, build);
        let read = memoized.unwrap_or_else(|| {
            let cycle_hits = self.answer_reads.borrow().cycle_hits;
            let read = match self.reduce_closed_answer_operator(operator, context) {
                None => None,
                Some(ClosedOperatorRead::Named(node)) => Some(node),
                Some(ClosedOperatorRead::Evaluated(node)) => {
                    self.carrier_normalizing.borrow_mut().push(operator);
                    let projected = self.with_closed_read_inputs(inputs.scope, |read_inputs| {
                        self.project_operator_read(node, context, read_inputs)
                    });
                    self.carrier_normalizing.borrow_mut().pop();
                    projected
                }
            };
            self.answer_reads
                .borrow_mut()
                .settle(key, read, cycle_hits, build);
            read
        });
        let answer = read.map(|read| {
            self.carrier_normalizing.borrow_mut().push(operator);
            let answer = then(read);
            self.carrier_normalizing.borrow_mut().pop();
            answer
        });
        self.answer_reads.borrow_mut().close(frame);
        answer
    }

    /// Whether `node` is already being normalized or read on this
    /// dispatcher's call chain: the in-progress record every carrier,
    /// composite and answer read consults before it enters a node. A hit
    /// makes what the chain computes depend on where it was entered from,
    /// so it is counted ([`AnswerReads`]).
    pub(in crate::project_semantic_dispatch) fn carrier_normalizing_holds(
        &self,
        node: SemanticNodeId,
    ) -> bool {
        let holds = self.carrier_normalizing.borrow().contains(&node);
        if holds {
            self.answer_reads.borrow_mut().record_cycle_hit();
        }
        holds
    }

    /// `read` projected at `context` as a root of its own, in place. A
    /// projection that stopped keeps the operator and folds its partiality
    /// into the build.
    fn project_operator_read(
        &self,
        read: SemanticNodeId,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
    ) -> Option<SemanticNodeId> {
        #[cfg(any(test, feature = "test-support"))]
        ANSWER_MEMBER_READS_FOR_TESTS.with(|reads| reads.set(reads.get() + 1));
        // The operator is closed, so what it reads binds nothing of the
        // body being projected.
        let mut substitutions = Vec::new();
        let mut memo = ViewMemo::default();
        let mut seam = ProjectionSeam::inline();
        let outcome = match self.begin_projection(
            read,
            context,
            inputs,
            &mut substitutions,
            &mut memo,
            &mut seam,
        ) {
            Ok(mut run) => match self.drain_projection(
                &mut run,
                inputs,
                &mut substitutions,
                &mut memo,
                &mut seam,
                None,
            ) {
                ProjectionPoll::Done(outcome) => outcome,
                ProjectionPoll::Need(_) => {
                    unreachable!("an inline projection evaluates its instantiations in place")
                }
            },
            Err(outcome) => outcome,
        };
        match outcome.completeness {
            ResultCompleteness::Complete => Some(outcome.node),
            ResultCompleteness::Partial(reasons) => {
                self.fold_local_partial_completeness(reasons);
                None
            }
        }
    }

    /// The published `Expanded` answer a demand reads through its root
    /// access, answered in `scope`'s file: `read`, the member the access
    /// reads, projected as the answer projects a body written in place, so
    /// the closed operators inside it read as they do there (`X['a']` over
    /// `type X = { a: { b: Y['c'] } }` answers `{ b: 1 }`). A projection
    /// that stopped answers `read` and folds its partiality into the
    /// request.
    pub fn published_answer_read(
        &self,
        read: SemanticNodeId,
        scope: &NodeScopeId,
    ) -> SemanticNodeId {
        let _operation = self.enter_operation(false);
        let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
        self.with_closed_read_inputs(scope, |inputs| {
            match self.project_operator_read(read, context, inputs) {
                Some(projected) => self.reduce_answer_constituents(projected, context, inputs),
                None => read,
            }
        })
    }

    /// The inputs a member read through a closed operator projects with,
    /// answered in `scope`'s file. The member names nothing of the body
    /// holding the operator — its references are already resolved — so no
    /// type-parameter binding, script-setup generic, shadowing or authored
    /// resolution debt of that body reaches it, and the read depends on
    /// `scope` alone, which is what [`AnswerReads`] keys it by.
    fn with_closed_read_inputs<R>(
        &self,
        scope: &NodeScopeId,
        read: impl FnOnce(&LocatorViewInputs<'_>) -> R,
    ) -> R {
        let env = rustc_hash::FxHashMap::default();
        let name_resolution = rustc_hash::FxHashMap::default();
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::empty();
        read(&LocatorViewInputs {
            env: &env,
            scope,
            name_resolution: &name_resolution,
            scope_payload: None,
            shadowing: &shadowing,
            authored_resolution_debt: None,
            self_value: None,
        })
    }

    /// A projected child as the composite holding it keeps it: a closed
    /// indexed access reads its member, and reads on while that member is a
    /// closed access; anything else, a `keyof` included (an object, tuple,
    /// signature or type argument prints it as `keyof P`), stays itself.
    pub(super) fn answer_member(
        &self,
        child: SemanticNodeId,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
    ) -> SemanticNodeId {
        if context.mode != ProjectionMode::Expanded || !may_reduce_operator(context) {
            return child;
        }
        match self.graph().node_data(child).as_deref() {
            Some(SemanticNodeData::IndexedAccess { .. }) => self
                .read_answer_operator(child, context, inputs, |read| {
                    self.answer_member(read, context, inputs)
                })
                .unwrap_or(child),
            _ => child,
        }
    }

    /// A projected type argument as the application holding it keeps it,
    /// read at `read_context`. An application that defers its arguments
    /// projects them at navigation altitude, so the closed accesses they
    /// hold anywhere inside — an object's member, a tuple or array element,
    /// a signature's parameter or return, a union or intersection arm, the
    /// argument of a nested deferring application — are still carriers; the
    /// checker builds the reference over what they read
    /// (`Promise<{ x: V["a"] }>` answers `Promise<{ x: 1 }>`). A name the
    /// argument holds stays that name, and an alias's application keeps the
    /// identity its body substitutes.
    pub(super) fn answer_argument(
        &self,
        projected: SemanticNodeId,
        holder: ProjectionReductionContext,
        argument_context: ProjectionReductionContext,
        operand_context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
    ) -> SemanticNodeId {
        if may_reduce_operator(argument_context) {
            return self.answer_member(projected, operand_context, inputs);
        }
        if holder.mode != ProjectionMode::Expanded || !may_reduce_operator(holder) {
            return projected;
        }
        // Post-order over the argument's composite parts, each part read
        // once however many places of the argument share it. A part met
        // again while its own parts are pending keeps itself there.
        let graph = self.graph();
        let mut answered = rustc_hash::FxHashMap::<SemanticNodeId, SemanticNodeId>::default();
        let mut entered = rustc_hash::FxHashSet::<SemanticNodeId>::default();
        let mut stack = vec![(projected, false)];
        while let Some((node, parts_answered)) = stack.pop() {
            if answered.contains_key(&node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                answered.insert(node, node);
                continue;
            };
            if !parts_answered {
                if !entered.insert(node) {
                    continue;
                }
                let mut pending = Vec::new();
                self.for_each_argument_part(&data, |part| {
                    if !answered.contains_key(&part) && !entered.contains(&part) {
                        pending.push(part);
                    }
                });
                if !pending.is_empty() {
                    stack.push((node, true));
                    stack.extend(pending.into_iter().map(|part| (part, false)));
                    continue;
                }
            }
            let answer = self.rebuild_argument_part(node, &data, &answered, holder, inputs);
            answered.insert(node, answer);
        }
        answered.get(&projected).copied().unwrap_or(projected)
    }

    /// The parts of a type argument whose closed accesses the holder reads:
    /// the children an answer composite reads at its own altitude
    /// ([`Self::finish_projection_structure`]), and the arguments of a
    /// nested application that defers them.
    fn for_each_argument_part(
        &self,
        data: &SemanticNodeData,
        mut visit: impl FnMut(SemanticNodeId),
    ) {
        match data {
            SemanticNodeData::Union(arms) => arms.iter().copied().for_each(visit),
            SemanticNodeData::Intersection(arms) => arms.iter().copied().for_each(visit),
            SemanticNodeData::Array { element, .. } => visit(*element),
            SemanticNodeData::Tuple { elements, .. } => {
                elements.iter().for_each(|element| visit(element.value));
            }
            SemanticNodeData::Object(view) => {
                for entry in view.entries.iter() {
                    match entry {
                        crate::semantic_query::SurfaceEntry::Member(member) => visit(member.value),
                        crate::semantic_query::SurfaceEntry::CallSignature(signature)
                        | crate::semantic_query::SurfaceEntry::ConstructSignature(signature) => {
                            visit(*signature);
                        }
                        crate::semantic_query::SurfaceEntry::IndexSignature(signature) => {
                            visit(signature.key_type);
                            visit(signature.value_type);
                        }
                    }
                }
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                ..
            } => {
                params.iter().for_each(|parameter| visit(parameter.ty));
                visit(*return_type);
            }
            SemanticNodeData::InstantiationRef { base, args }
                if self.defers_type_arguments(base) =>
            {
                args.iter().copied().for_each(visit);
            }
            _ => {}
        }
    }

    /// `node` rebuilt over its parts as [`Self::answer_argument`] answered
    /// them, or, for a closed access, the member it reads.
    fn rebuild_argument_part(
        &self,
        node: SemanticNodeId,
        data: &SemanticNodeData,
        answered: &rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let answer = |part: SemanticNodeId| answered.get(&part).copied().unwrap_or(part);
        let mut changed = false;
        self.for_each_argument_part(data, |part| changed |= answer(part) != part);
        if !changed {
            return match data {
                SemanticNodeData::IndexedAccess { .. } => self.answer_member(node, context, inputs),
                _ => node,
            };
        }
        match data {
            SemanticNodeData::Union(arms) => {
                let ids = arms.iter().map(|arm| answer(*arm)).collect();
                let nullability = self.answer_nullability(context, inputs);
                self.rebuild_projected_union(node, arms.origin_category(), ids, nullability)
            }
            SemanticNodeData::Intersection(arms) => {
                let ids = arms.iter().map(|arm| answer(*arm)).collect();
                self.rebuild_projected_intersection(node, arms.origin_category(), ids)
            }
            SemanticNodeData::Array { element, readonly } => graph.intern_preserving_scope(
                node,
                SemanticNodeData::Array {
                    element: answer(*element),
                    readonly: *readonly,
                },
            ),
            SemanticNodeData::Tuple { elements, readonly } => {
                let elements: Vec<_> = elements
                    .iter()
                    .map(|element| TupleElement {
                        value: answer(element.value),
                        ..element.copy()
                    })
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Tuple {
                        elements: crate::semantic_query::PayloadArray::from(
                            elements.into_boxed_slice(),
                        ),
                        readonly: *readonly,
                    },
                )
            }
            SemanticNodeData::Object(view) => {
                use crate::semantic_query::SurfaceEntry;
                let entries = view
                    .entries
                    .iter()
                    .map(|entry| match entry {
                        SurfaceEntry::Member(member) => SurfaceEntry::Member(SurfaceMember {
                            value: answer(member.value),
                            ..member.clone()
                        }),
                        SurfaceEntry::CallSignature(signature) => {
                            SurfaceEntry::CallSignature(answer(*signature))
                        }
                        SurfaceEntry::ConstructSignature(signature) => {
                            SurfaceEntry::ConstructSignature(answer(*signature))
                        }
                        SurfaceEntry::IndexSignature(signature) => {
                            SurfaceEntry::IndexSignature(IndexSignature {
                                key_type: answer(signature.key_type),
                                value_type: answer(signature.value_type),
                                ..signature.clone()
                            })
                        }
                    })
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Object(SurfaceView::from_entries(
                        entries,
                        view.keyspace,
                        view.has_known_index_signature(),
                    )),
                )
            }
            SemanticNodeData::Signature { .. } => {
                let mut rebuilt = data.clone();
                if let SemanticNodeData::Signature {
                    params,
                    return_type,
                    return_carrier,
                    ..
                } = &mut rebuilt
                {
                    let answered_params: Vec<_> = params
                        .iter()
                        .map(|parameter| FunctionParam {
                            ty: answer(parameter.ty),
                            ..parameter.copy()
                        })
                        .collect();
                    *params = crate::semantic_query::PayloadArray::from(
                        answered_params.into_boxed_slice(),
                    );
                    *return_type = answer(*return_type);
                    if let crate::semantic_query::SignatureReturnCarrier::Declared(declared) =
                        return_carrier
                    {
                        *declared = *return_type;
                    }
                }
                graph.intern_preserving_scope(node, rebuilt)
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let args: Vec<_> = args
                    .iter()
                    .map(|argument| self.canonical_argument_carrier(answer(*argument)))
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::InstantiationRef {
                        base: base.clone(),
                        args: crate::semantic_query::PayloadArray::from(args.into_boxed_slice()),
                    },
                )
            }
            _ => node,
        }
    }

    /// A projection root's answer once its own union or intersection reads
    /// its closed operator arms: an arm's `keyof` settles to its keys, so
    /// the answer reduces over them, and an arm's access reads its member,
    /// through every union or intersection the root is made of. The root
    /// itself, an operator or not, stays for its demand to read (typeinfo's
    /// `materialize_through_aliases` bridge, which projects what a root
    /// access reads through [`Self::published_answer_read`]), and an arm
    /// inside any other composite keeps the carrier that composite prints.
    ///
    /// This runs on the projection's result, never on a memoized node: a
    /// node the projection shares with an object, tuple or operand keeps
    /// the carrier there.
    pub(in crate::project_semantic_dispatch) fn reduce_answer_constituents(
        &self,
        root: SemanticNodeId,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
    ) -> SemanticNodeId {
        if context.mode != ProjectionMode::Expanded || !may_reduce_operator(context) {
            return root;
        }
        match self.graph().node_data(root).as_deref() {
            Some(SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                self.reduce_answer_constituent(root, context, inputs)
            }
            _ => root,
        }
    }

    /// One arm of the answer's own intersection, projected as a root of
    /// its own, as [`Self::reduce_answer_constituents`] reads the arms of
    /// the answer's union or intersection.
    pub(in crate::project_semantic_dispatch) fn reduce_answer_arm(
        &self,
        arm: SemanticNodeId,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
    ) -> SemanticNodeId {
        if context.mode != ProjectionMode::Expanded || !may_reduce_operator(context) {
            return arm;
        }
        self.reduce_answer_constituent(arm, context, inputs)
    }

    /// An arm of the answer's own union or intersection, its closed
    /// operators read: what an operator arm reads is itself an arm (a
    /// member written `keyof P` settles to its keys, a member union's arms
    /// join the answer's). Unions and intersections nest only as deep as
    /// they are written.
    fn reduce_answer_constituent(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let data = graph.node_data(node);
        match data.as_deref() {
            Some(SemanticNodeData::KeyOf { .. } | SemanticNodeData::IndexedAccess { .. }) => self
                .read_answer_operator(node, context, inputs, |read| {
                    self.reduce_answer_constituent(read, context, inputs)
                })
                .unwrap_or(node),
            Some(SemanticNodeData::Union(arms)) => {
                let ids: Vec<_> = arms
                    .iter()
                    .map(|arm| self.reduce_answer_constituent(*arm, context, inputs))
                    .collect();
                if ids.as_slice() == &***arms {
                    return node;
                }
                let nullability = self.answer_nullability(context, inputs);
                self.rebuild_projected_union(node, arms.origin_category(), ids, nullability)
            }
            Some(SemanticNodeData::Intersection(arms)) => {
                let ids: Vec<_> = arms
                    .iter()
                    .map(|arm| self.reduce_answer_constituent(*arm, context, inputs))
                    .collect();
                if ids.as_slice() == &***arms {
                    return node;
                }
                self.rebuild_answer_intersection(node, arms.origin_category(), ids)
            }
            _ => node,
        }
    }

    /// The null algebra a projected union uses: a published expanded answer
    /// belongs to its declaration's project and uses that project's; the
    /// shared structural carrier remains policy-neutral (strict). The
    /// canonical builder includes the policy in node identity.
    fn answer_nullability(
        &self,
        context: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
    ) -> verter_session_query::flow::policy::NullabilityPolicy {
        use verter_session_query::flow::policy::NullabilityPolicy;

        if context.mode == ProjectionMode::Expanded && may_reduce_operator(context) {
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
        }
    }

    /// A union rebuilt over its projected arms.
    fn rebuild_projected_union(
        &self,
        node: SemanticNodeId,
        category: crate::semantic_query::composite::CompositeOriginCategory,
        ids: Vec<SemanticNodeId>,
        nullability: verter_session_query::flow::policy::NullabilityPolicy,
    ) -> SemanticNodeId {
        let graph = self.graph();
        if ids.is_empty() {
            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
        } else if ids.len() == 1 {
            ids[0]
        } else if self.composite_rebuild_re_decides(category, &ids, true) {
            // The projection of a canonical/authored composite is a
            // DERIVED result — route it through the canonical authority
            // (the authored shell stays recoverable via `node`); a derived
            // multi-arm composite interns Global.
            self.intern_normalized_union(&ids, nullability)
        } else {
            // Order- and scope-preserving projection rebuild for the
            // carrier categories whose arm order is semantics.
            graph.intern_preserving_scope(
                node,
                SemanticNodeData::Union(
                    crate::semantic_query::composite::CompositeList::preserving_rebuild(Arc::from(
                        ids.into_boxed_slice(),
                    )),
                ),
            )
        }
    }

    /// An intersection of the answer's own, rebuilt over arms it reduced:
    /// the union it denotes when an arm is a union, as the checker
    /// distributes it (`getIntersectionType`), else the intersection.
    pub(in crate::project_semantic_dispatch) fn rebuild_answer_intersection(
        &self,
        node: SemanticNodeId,
        category: crate::semantic_query::composite::CompositeOriginCategory,
        ids: Vec<SemanticNodeId>,
    ) -> SemanticNodeId {
        use crate::semantic_query::composite::CompositeOriginCategory as C;

        // A heritage body or an overload carrier keeps its arms in order.
        let ordered = matches!(
            category,
            C::OrderedCarrier | C::OverloadGroup | C::MergedOverloadGroup | C::Heritage
        );
        if !ordered {
            if let Some(distributed) = self.distributed_intersection(&ids) {
                return distributed;
            }
        }
        self.rebuild_projected_intersection(node, category, ids)
    }

    /// An intersection rebuilt over its projected arms.
    fn rebuild_projected_intersection(
        &self,
        node: SemanticNodeId,
        category: crate::semantic_query::composite::CompositeOriginCategory,
        ids: Vec<SemanticNodeId>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        if ids.is_empty() {
            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
        } else if ids.len() == 1 {
            ids[0]
        } else if self.composite_rebuild_re_decides(category, &ids, false) {
            // Derived projection result (and provably order-safe: no
            // rebuilt arm may contribute call signatures) — canonical
            // authority, Global intern.
            self.intern_normalized_union_or_intersection(&ids, false)
        } else {
            // Order- and scope-preserving projection rebuild: a heritage
            // body or an ordered overload carrier (or a possibly-callable
            // rebuilt arm set) keeps its verbatim order, and a heritage
            // body stays one.
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

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn finish_projection_structure(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        data: &SemanticNodeData,
        inputs: &LocatorViewInputs<'_>,
        _substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &ViewMemo,
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
                let ids: Vec<_> = arms
                    .iter()
                    .map(|arm| self.answer_member(projected(memo, *arm, context), context, inputs))
                    .collect();
                let nullability = self.answer_nullability(context, inputs);
                let erases_nullish = !nullability.is_strict()
                    && ids.iter().any(|arm| {
                        matches!(
                            graph.node_data(*arm).as_deref(),
                            Some(SemanticNodeData::Primitive(
                                PrimitiveKind::Null | PrimitiveKind::Undefined
                            ))
                        )
                    });
                if ids.as_slice() == &***arms
                    && (!erases_nullish
                        || !self.composite_rebuild_re_decides(arms.origin_category(), &ids, true))
                {
                    // Identity projection: nothing to rebuild and nothing to
                    // re-decide — the original carrier (its category, order
                    // and interned identity, including alias origin) stays
                    // exactly itself when no nullable arm must be erased.
                    return node;
                }
                self.rebuild_projected_union(node, arms.origin_category(), ids, nullability)
            }
            SemanticNodeData::Intersection(arms) => {
                let ids: Vec<_> = arms
                    .iter()
                    .map(|arm| self.answer_member(projected(memo, *arm, context), context, inputs))
                    .collect();
                if ids.as_slice() == &***arms {
                    // Identity projection — see the Union arm.
                    return node;
                }
                self.rebuild_projected_intersection(node, arms.origin_category(), ids)
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
                let new_element = self.answer_member(projected(memo, *element, context), context, inputs);
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
                    .enumerate()
                    .map(|(index, element)| TupleElement {
                        label: element.label.clone(),
                        value: self.answer_member(
                            projected_in(
                                memo,
                                element.value,
                                context,
                                self.child_projection_slot(data, index, element.value, context),
                            ),
                            context,
                            inputs,
                        ),
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
                // A literal hole folds into the template's text, as the
                // checker's template construction folds it.
                let (quasis, expressions) = match self.folded_literal_holes(quasis, &expressions) {
                    Some((folded, holes)) => {
                        (verter_type_expr::OwnedSlice::from(folded.as_slice()), holes)
                    }
                    None => ((*quasis).clone(), expressions),
                };
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::TemplateLiteral {
                        quasis,
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
                        value: self.answer_member(
                            projected(memo, member.value, member_context),
                            member_context, inputs
                        ),
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
                        key_type: self.answer_member(projected(memo, signature.key_type, context), context, inputs),
                        value_type: self.answer_member(projected(memo, signature.value_type, context), context, inputs),
                        readonly: signature.readonly,
                        key_name: signature.key_name.clone(),
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
                        ty: self.answer_member(projected(memo, parameter.ty, context), context, inputs),
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
                            .map(|value| self.answer_member(projected(memo, value, context), context, inputs)),
                        default: parameter
                            .default
                            .map(|value| self.answer_member(projected(memo, value, context), context, inputs)),
                        original: parameter.original.clone(),
                        is_const: parameter.is_const,
                    })
                    .collect();
                let projected_return =
                    self.answer_member(projected(memo, *return_type, context), context, inputs);
                let signature = graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Signature {
                        kind: *kind,
                        params: crate::semantic_query::PayloadArray::from(params.into_boxed_slice()),
                        return_type: projected_return,
                        type_parameters: crate::semantic_query::PayloadArray::from(type_parameters.into_boxed_slice()),
                        // Projection preserves the occurrence; a declared
                        // carrier retargets the projected return.
                        occurrence: occurrence.clone(),
                        return_carrier: match return_carrier {
                            crate::semantic_query::SignatureReturnCarrier::Declared(_) => {
                                crate::semantic_query::SignatureReturnCarrier::Declared(projected_return)
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
                    match self.execute_type_node(SemanticQueryKey::KeyOf {
                        base: base_id,
                        context,
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                        _ => self.opaque(QueryError::Miss),
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
            SemanticNodeData::IndexedAccess { .. } => {
                unreachable!("an indexed access finishes through its selection plan")
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

/// The closed operators one demand's answer has read, so each is read once
/// however many places hold it.
///
/// A read is reused only where it would be recomputed identically: in the
/// build that read it (whose taint, partiality and facts it already
/// carries), and only when nothing it computed met the in-progress record,
/// since a read that stopped at a cycle answers by where it was entered
/// from. The record lives while a read is open and is dropped when the
/// outermost read closes.
#[derive(Default)]
pub(in crate::project_semantic_dispatch) struct AnswerReads {
    /// Reads open on the native stack.
    open: usize,
    /// The build-local taint depth the outermost open read runs in.
    build: usize,
    /// In-progress-record hits on the dispatcher so far, wrapping.
    cycle_hits: u64,
    read: rustc_hash::FxHashMap<AnswerReadKey, Option<SemanticNodeId>>,
}

type AnswerReadKey = (SemanticNodeId, ProjectionReductionContext, NodeScopeId);

/// One open read's place in [`AnswerReads`].
#[must_use = "an opened read is closed"]
struct AnswerReadFrame(());

impl AnswerReads {
    fn record_cycle_hit(&mut self) {
        self.cycle_hits = self.cycle_hits.wrapping_add(1);
    }

    fn open(&mut self, build: usize) -> AnswerReadFrame {
        if self.open == 0 {
            self.build = build;
        }
        self.open += 1;
        AnswerReadFrame(())
    }

    fn close(&mut self, AnswerReadFrame(()): AnswerReadFrame) {
        self.open -= 1;
        if self.open == 0 {
            self.read.clear();
        }
    }

    /// What `key` read earlier in this demand's build, if it did.
    fn memoized(&self, key: &AnswerReadKey, build: usize) -> Option<Option<SemanticNodeId>> {
        if build == self.build {
            self.read.get(key).copied()
        } else {
            None
        }
    }

    /// Keep what `key` read, unless the read met the in-progress record
    /// after `cycle_hits` or ran in another build.
    fn settle(
        &mut self,
        key: AnswerReadKey,
        read: Option<SemanticNodeId>,
        cycle_hits: u64,
        build: usize,
    ) {
        if self.cycle_hits == cycle_hits && build == self.build {
            self.read.insert(key, read);
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Member reads projected on this thread.
    static ANSWER_MEMBER_READS_FOR_TESTS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// Run `run` and count the member reads closed operators in its answers
/// projected on this thread. Test-only.
#[cfg(any(test, feature = "test-support"))]
pub fn count_answer_member_reads_for_tests<R>(run: impl FnOnce() -> R) -> (R, usize) {
    let before = ANSWER_MEMBER_READS_FOR_TESTS.with(std::cell::Cell::get);
    let value = run();
    let after = ANSWER_MEMBER_READS_FOR_TESTS.with(std::cell::Cell::get);
    (value, after - before)
}

/// What a closed answer operator read: the declaration an access names,
/// kept as the name the checker prints, or a node it evaluated to.
enum ClosedOperatorRead {
    Named(SemanticNodeId),
    Evaluated(SemanticNodeId),
}

impl ClosedOperatorRead {
    fn node(&self) -> SemanticNodeId {
        match self {
            Self::Named(node) | Self::Evaluated(node) => *node,
        }
    }
}
