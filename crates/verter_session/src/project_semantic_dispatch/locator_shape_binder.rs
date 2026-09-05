//! Locator-shape binder construction — the shared type-parameter binder-frame
//! constructor and the `LowerLocator` producer body.
//!
//! Extracted from `locator_shape.rs` as a continuation `impl
//! ProjectSemanticDispatch` block (same module tree, sibling file). These two
//! methods are the binder-frame minting engine (`build_type_param_binder_frame`)
//! and the session-private locator-shape provider (`build_lower_locator`) the
//! `SemanticQueryKey::LowerLocator` memo drives.

use super::*;

impl<'a> ProjectSemanticDispatch<'a> {
    /// The ONE shared binder-frame constructor for declared type-parameter
    /// lists — decl headers (typed-IR bounds lowered inline), function /
    /// constructor signature generics, free authored `TypeParameter`
    /// occurrences, and the fact-side re-derivation from `NarrowTypeParam`
    /// facts (bounds lowered through the memoized `LowerLocator` query) ALL
    /// route through it, so every derivation interns IDENTICAL binder ids.
    ///
    /// TS-exact lexical visibility, two-phase:
    ///
    /// 1. **Predeclare every sibling** as a bound-free `TypeParam` node —
    ///    the sibling NAME set is complete before any bound lowers, so a
    ///    bound's sibling reference (backward, forward, or self) always
    ///    finds the frame entry and can never fall through to an outer
    ///    same-named declaration.
    /// 2. **Lower bounds per position** — the discipline splits by identity
    ///    mode:
    ///    - `DeclHeader` / `Signature` (embedded-bound modes): the bound of
    ///      the parameter at ordinal `N` lowers under a frame where
    ///      parameters declared BEFORE `N` are usable through their final
    ///      (bound-carrying) binders and the parameter itself plus later
    ///      siblings are — for a CONSTRAINT — usable predeclared shells (TS
    ///      constraints may reference later siblings and self, F-bounded
    ///      forms included), or — for a DEFAULT — shadow-forbidden entries
    ///      (TS rejects default forward / self references; such a reference
    ///      lowers to the fail-closed `Opaque(Miss)`, never an outer
    ///      capture). Constraints stay graph EDGES on the binder nodes —
    ///      nothing here evaluates them, so a mutual
    ///      `<T extends U, U extends T>` just creates the edges (the shell
    ///      break makes the node data acyclic by construction) and
    ///      resolution-time cycle handling stays where it lives.
    ///    - `FunctionSignature` (the bound-free mode): the predeclared node
    ///      IS the final binder — bound content never enters the binder's
    ///      interned identity, so a CONSTRAINT lowers under the COMPLETE
    ///      binder map (a self or forward sibling reference binds the one
    ///      true binder and an instantiation substitution reaches it),
    ///      while a DEFAULT keeps the prior-sibling-only frame with self /
    ///      later shadow-forbidden (TS2744). The lowered bounds ride the
    ///      returned [`BuiltTypeParamBinder`] metadata (and from there the
    ///      signature's `TypeParamDecl` list) only.
    ///
    /// `visibility` selects the RETURNED frame: `Body` demands every final
    /// binder (the whole-declaration frame); a bound demand returns exactly
    /// that bound's per-position frame. Bound BODIES enter through the
    /// caller's `lower_bound` strategy (`(ordinal, position, frame-ctx) →
    /// node`; `None` = absent bound, or a fact-side lowering miss whose
    /// binder-identity divergence degrades substitution to a no-op while
    /// the read-boundary fold taints warm admission), so the shape-inline
    /// and fact-query derivations share one minting engine.
    ///
    /// Binder identity is deterministic (content-addressed interning over
    /// the same scope + identity mode + ordinal — plus the bound NODES in
    /// the embedded-bound modes only), so the substitution step that
    /// applies `Instantiate` args to a fetched shape re-derives the SAME
    /// binder ids from the prepared declaration's parameter facts.
    pub(in crate::project_semantic_dispatch) fn build_type_param_binder_frame(
        &self,
        base: &LocatorShapeCtx<'_>,
        identity: BinderIdentityMode<'_>,
        specs: &[TypeParamBinderSpec],
        visibility: TypeParamVisibility,
        mut lower_bound: impl FnMut(
            u32,
            TypeParamBoundPosition,
            &LocatorShapeCtx<'_>,
        ) -> Option<SemanticNodeId>,
    ) -> (LocatorBinderFrame, Vec<BuiltTypeParamBinder>) {
        let graph = self.graph();
        let scope = base.scope;
        // A FUNCTION-signature clause keeps bound content OUT of the binder
        // identity: the predeclared node IS the final binder.
        let bound_free = matches!(identity, BinderIdentityMode::FunctionSignature { .. });
        let mint_identity = |name: &Arc<str>, index: usize| match &identity {
            // A decl-header binder's identity carries the OWNING symbol
            // name plus the parameter's declared ordinal — a DISTINCT
            // identity from the display-name-keyed shells a nested function
            // type's own `<T>` binders intern, so applying an argument to
            // the declaration's `T` can never rewrite a shadowing inner `T`.
            BinderIdentityMode::DeclHeader { owner_symbol } => (
                DeclIdentity::from_scope(scope, Arc::clone(owner_symbol)),
                index as u16,
            ),
            BinderIdentityMode::Signature => (DeclIdentity::from_scope(scope, Arc::clone(name)), 0),
            // A function-clause binder's identity = the declaring anchor
            // entity + the clause ordinal. The composed `decl_name` is an
            // IDENTITY MINT (the `\u{1}` separator cannot appear in an
            // authored identifier); nothing ever parses it back.
            BinderIdentityMode::FunctionSignature { anchor_symbol } => (
                DeclIdentity::from_scope(
                    scope,
                    match anchor_symbol {
                        Some(anchor) => Arc::from(format!("{anchor}\u{1}{name}").as_str()),
                        None => Arc::clone(name),
                    },
                ),
                index as u16,
            ),
        };

        // Phase 1: predeclare every sibling as a bound-free binder node. In
        // the embedded-bound modes these are the SHELLS the per-position
        // frames expose for self / forward constraint references; in the
        // bound-free function-signature mode they ARE the final binders.
        let shells: Vec<(Arc<str>, SemanticNodeId)> = specs
            .iter()
            .enumerate()
            .map(|(index, spec)| {
                let (decl, param_index) = mint_identity(&spec.name, index);
                let shell = graph.intern_node_with_scope(
                    SemanticNodeData::TypeParam {
                        decl,
                        param_index,
                        constraint: None,
                        default: None,
                        display_name: Arc::clone(&spec.name),
                    },
                    scope.clone(),
                );
                (Arc::clone(&spec.name), shell)
            })
            .collect();

        // The frame the bound at `ordinal` lowers under (also the frame a
        // bound-position demand returns).
        //
        // Embedded-bound modes: prior finals usable; self + later siblings
        // constraint-usable shells / default-forbidden shadows.
        //
        // Bound-free mode: a CONSTRAINT lowers under the COMPLETE binder
        // map (prior, self, and later references all bind the one true
        // binder, so an instantiation substitution reaches them); a DEFAULT
        // still sees prior binders only, with self / later shadow-forbidden
        // (TS2744).
        let bound_frame =
            |finals: &[BuiltTypeParamBinder], ordinal: usize, position: TypeParamBoundPosition| {
                let mut frame = LocatorBinderFrame::default();
                if bound_free {
                    for (position_index, (name, binder)) in shells.iter().enumerate() {
                        match position {
                            TypeParamBoundPosition::Constraint => {
                                frame.bind(Arc::clone(name), *binder);
                            }
                            TypeParamBoundPosition::Default => {
                                if position_index < ordinal {
                                    frame.bind(Arc::clone(name), *binder);
                                } else {
                                    frame.bind_shadow_only(Arc::clone(name));
                                }
                            }
                        }
                    }
                    return frame;
                }
                for built in finals {
                    frame.bind(Arc::clone(&built.name), built.binder);
                }
                for (name, shell) in shells.iter().skip(ordinal) {
                    match position {
                        TypeParamBoundPosition::Constraint => frame.bind(Arc::clone(name), *shell),
                        TypeParamBoundPosition::Default => frame.bind_shadow_only(Arc::clone(name)),
                    }
                }
                frame
            };

        // Phase 2: left-to-right bound lowering. Embedded-bound modes mint
        // the final (bound-carrying) binders here; the bound-free mode
        // keeps the predeclared binder and records the lowered bounds as
        // metadata only. A `Body` demand needs every binder; a bound demand
        // needs exactly the entries declared before its ordinal.
        let limit = match visibility {
            TypeParamVisibility::Body => specs.len(),
            TypeParamVisibility::Constraint { ordinal }
            | TypeParamVisibility::Default { ordinal } => (ordinal as usize).min(specs.len()),
        };
        let mut finals: Vec<BuiltTypeParamBinder> = Vec::with_capacity(limit);
        for (index, spec) in specs.iter().take(limit).enumerate() {
            let mut lower_in = |finals: &[BuiltTypeParamBinder],
                                position: TypeParamBoundPosition| {
                let frame = bound_frame(finals, index, position);
                let mut frames: Vec<LocatorBinderFrame> = base.binders.to_vec();
                frames.push(frame);
                let ctx =
                    LocatorShapeCtx::new(scope, &frames, base.name_resolution, base.scope_payload)
                        .with_optional_infer_source(base.infer_source);
                lower_bound(index as u32, position, &ctx)
            };
            let constraint = if spec.has_constraint {
                lower_in(&finals, TypeParamBoundPosition::Constraint)
            } else {
                None
            };
            let default = if spec.has_default {
                lower_in(&finals, TypeParamBoundPosition::Default)
            } else {
                None
            };
            let binder = if bound_free {
                shells[index].1
            } else {
                let (decl, param_index) = mint_identity(&spec.name, index);
                graph.intern_node_with_scope(
                    SemanticNodeData::TypeParam {
                        decl,
                        param_index,
                        constraint,
                        default,
                        display_name: Arc::clone(&spec.name),
                    },
                    scope.clone(),
                )
            };
            finals.push(BuiltTypeParamBinder {
                name: Arc::clone(&spec.name),
                binder,
                constraint,
                default,
            });
        }

        let frame = match visibility {
            TypeParamVisibility::Body => {
                let mut frame = LocatorBinderFrame::default();
                if bound_free {
                    for (name, binder) in &shells {
                        frame.bind(Arc::clone(name), *binder);
                    }
                } else {
                    for built in &finals {
                        frame.bind(Arc::clone(&built.name), built.binder);
                    }
                }
                frame
            }
            TypeParamVisibility::Constraint { ordinal } => bound_frame(
                &finals,
                ordinal as usize,
                TypeParamBoundPosition::Constraint,
            ),
            TypeParamVisibility::Default { ordinal } => {
                bound_frame(&finals, ordinal as usize, TypeParamBoundPosition::Default)
            }
        };
        (frame, finals)
    }

    /// Cold build for [`SemanticQueryKey::LowerLocator`] — the SESSION
    /// phase of the two-phase worker-purity split.
    ///
    /// The live `whole_hash` is re-sourced through
    /// `ensure_indexed_ready_serve` (exactly as `build_instantiate` does)
    /// and recorded on the read-set via the observed self-root — never
    /// carried in the key (R6). The WORKER phase
    /// ([`crate::decl_body_memo::DeclBodyMemo::deref_locator_body`]) derefs
    /// the locator through the artifact's retained snapshot (lease-only)
    /// and returns owned typed IR; this build graph-lowers that IR into
    /// ROLE-FREE locator-shape nodes with the decl's type parameters bound
    /// as `TypeParam` shells. The `IndexedReady` Arc — and through it the
    /// memo's `SnapshotLease` — is held THROUGH the graph-lowering call
    /// (releasing it before the deref would force a reparse).
    pub(in crate::project_semantic_dispatch) fn build_lower_locator(
        &self,
        key: &LocatorLoweringKey,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let canonical = Arc::clone(&key.slot().defining_canonical);
        let Some(serve) = self.ctx.ensure_indexed_ready_serve(canonical.as_ref()) else {
            // The owning file is unknown to the live view — the value
            // cannot be self-rooted; refuse warm admission.
            let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                (QueryResult::Error(QueryError::Miss), empty_signature()).into();
            output.cache_suppress = true;
            return output;
        };
        // Keeps the ShallowFileState → DeclBodyMemo → SnapshotLease alive
        // through the graph lowering below.
        let indexed = serve.indexed;
        let observed_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> =
            vec![(Arc::clone(&canonical), indexed.whole_hash)];

        let derefed = match indexed
            .shallow_state
            .decl_bodies()
            .deref_locator_body(key.locator())
        {
            Ok(derefed) => derefed,
            // Every deref failure is a typed fail-closed non-result. A GENUINE
            // deref miss (`UnknownSymbol`, `PathUnresolved`, an unrouted
            // payload, …) is a real, cacheable resolution result — the
            // `QueryResult::Error(Miss)` is never warm-published at this
            // `LowerLocator` level (errors never promote to a warm entry), and
            // the enclosing `Instantiate` may soundly cache the resulting
            // `Opaque(Miss)`. A `LeaseMiss` is a TRANSIENT ReturnOnly (a broken
            // lease pin): the enclosing query must NOT warm-publish the derived
            // `Opaque(Miss)` as a false body — set `cache_suppress` so the
            // universal read-boundary fold (`lower_locator`'s `execute_read`)
            // taints the enclosing `LowerLocator` / `Instantiate` build, and a
            // later demand under a live lease recovers.
            Err(deref_error) => {
                let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
                    QueryResult::Error(QueryError::Miss),
                    self.project_generation_signature(),
                )
                    .into();
                if matches!(
                    deref_error,
                    crate::decl_body_memo::LocatorBodyDerefError::LeaseMiss
                ) {
                    output.cache_suppress = true;
                }
                return output.with_observed_self_roots(observed_self_roots);
            }
        };

        let anchor = match key.locator() {
            AuthoredBodyLocator::DeclBody(slot) => &slot.anchor,
            AuthoredBodyLocator::AugmentationBody(augmentation) => &augmentation.anchor,
            AuthoredBodyLocator::JsdocTypedefBody(typedef) => &typedef.anchor,
            AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
        };
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&canonical),
            owner: anchor.owner,
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };
        let anchor_symbol = match key.locator() {
            AuthoredBodyLocator::DeclBody(slot) => Arc::clone(&slot.anchor.symbol),
            AuthoredBodyLocator::AugmentationBody(aug) => Arc::clone(&aug.anchor.symbol),
            AuthoredBodyLocator::JsdocTypedefBody(typedef) => Arc::clone(&typedef.anchor.symbol),
            AuthoredBodyLocator::MacroPayload(payload) => Arc::clone(&payload.anchor.symbol),
        };

        // Anchor-scope IDENTITY-resolution inputs — the SAME sources the
        // reducing path threads to its reference-head resolution (the
        // anchor declaration's import / namespace-sibling-aware
        // `name_resolution` map plus the bundle's declaration-scope
        // payload), so the cached `DeclRef` / `InstantiationRef` identities
        // match the reducing path's under declaration-local / namespace /
        // import shadowing. Absence (no prepared declaration for the
        // anchor) degrades to the payload-aware in-scope resolver.
        let bundle = self.ctx.prepared_decl_bundle(canonical.as_ref());
        let scope_payload = bundle
            .as_ref()
            .map(|bundle| DeclarationScopePayload::from_bundle(bundle, anchor.owner));
        let prepared_anchor: Option<AnchorPreparedDecl> = match key.locator() {
            AuthoredBodyLocator::DeclBody(slot) => match slot.anchor.space {
                verter_type_expr::locators::LocatorSymbolSpace::Type => bundle
                    .as_ref()
                    .and_then(|bundle| {
                        match bundle.prepared_type_decls.get_in_for_projection(
                            anchor.owner,
                            anchor_symbol.as_ref(),
                        ) {
                            crate::resolver_core::prepared_decl::PreparedTypeDeclResolution::Complete(
                                prepared,
                            )
                            | crate::resolver_core::prepared_decl::PreparedTypeDeclResolution::AuthoredPartial {
                                declaration: prepared,
                                ..
                            } => Some(AnchorPreparedDecl::Type(prepared)),
                            crate::resolver_core::prepared_decl::PreparedTypeDeclResolution::Missing => {
                                None
                            }
                            crate::resolver_core::prepared_decl::PreparedTypeDeclResolution::Failed {
                                failure,
                                ..
                            } => {
                                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                                    crate::resolver_core::resolver_context::NonCacheableReadReason::PreparationFailure,
                                );
                                tracing::error!(
                                    ?failure,
                                    canonical_id = canonical.as_ref(),
                                    owner = ?anchor.owner,
                                    symbol = anchor_symbol.as_ref(),
                                    "locator anchor preparation failed without an authored carrier"
                                );
                                None
                            }
                        }
                    }),
                verter_type_expr::locators::LocatorSymbolSpace::Value => self
                    .ctx
                    .prepared_value_decl_return_only(
                        canonical.as_ref(),
                        anchor.owner,
                        anchor_symbol.as_ref(),
                    )
                    .map(AnchorPreparedDecl::Value),
                verter_type_expr::locators::LocatorSymbolSpace::Namespace => None,
            },
            AuthoredBodyLocator::AugmentationBody(aug) => {
                use verter_semantic::analysis::type_eval::AugmentationScopeKind;
                let scope_kind = match &aug.scope {
                    verter_type_expr::locators::AuthoredAugmentationScope::Global => {
                        AugmentationScopeKind::Global
                    }
                    verter_type_expr::locators::AuthoredAugmentationScope::Module { specifier } => {
                        AugmentationScopeKind::Module(specifier.as_ref().to_string())
                    }
                };
                bundle
                    .as_ref()
                    .and_then(|bundle| {
                        bundle
                            .prepare_augmentation_type_decl_in(
                                &scope_kind,
                                anchor.owner,
                                anchor_symbol.as_ref(),
                            )
                            .ok()
                            .flatten()
                    })
                    .map(|prepared| AnchorPreparedDecl::Augmentation(Box::new(prepared)))
            }
            // A JSDoc typedef declares NO header type parameters (its deref
            // returns `type_parameters: Vec::new()`) and its comment-derived
            // payload is re-parsed by the dedicated lease-only re-derivation,
            // independent of any prepared declaration — so no binder-frame
            // prepared decl exists for the anchor. Mirrors the MacroPayload
            // arm below.
            AuthoredBodyLocator::JsdocTypedefBody(_) => None,
            AuthoredBodyLocator::MacroPayload(_) => None,
        };
        let name_resolution = prepared_anchor
            .as_ref()
            .map(AnchorPreparedDecl::name_resolution);

        // The deref returns the FULL sibling parameter list plus the
        // position-exact TS visibility; the shared constructor rebuilds the
        // frame the derefed shape lowers under (a whole body sees every
        // final binder; a bound position sees its per-position frame).
        let header_params = &derefed.type_parameters;
        let specs: Vec<TypeParamBinderSpec> = header_params
            .iter()
            .map(|param| TypeParamBinderSpec {
                name: Arc::from(param.name.as_str()),
                has_constraint: param.constraint.is_some(),
                has_default: param.default.is_some(),
            })
            .collect();
        let base = LocatorShapeCtx::new(&scope, &[], name_resolution, scope_payload.as_ref())
            .with_infer_source(key.locator());
        let (frame, _binders) = self.build_type_param_binder_frame(
            &base,
            BinderIdentityMode::DeclHeader {
                owner_symbol: &anchor_symbol,
            },
            &specs,
            derefed.visibility,
            |ordinal, position, bound_ctx| {
                let param = header_params.get(ordinal as usize)?;
                let bound = match position {
                    TypeParamBoundPosition::Constraint => param.constraint.as_deref(),
                    TypeParamBoundPosition::Default => param.default.as_deref(),
                }?;
                let bound_source = locator_with_header_bound(key.locator(), ordinal, position);
                let bound_ctx = (*bound_ctx).with_infer_source(&bound_source);
                Some(self.lower_type_expr_for_locator_shape(bound, &bound_ctx))
            },
        );
        let frames = [frame];
        let shape_ctx =
            LocatorShapeCtx::new(&scope, &frames, name_resolution, scope_payload.as_ref())
                .with_infer_source(key.locator());

        let node = match derefed.shape {
            DerefedBodyShape::Single(expr) => match derefed.lexical_root {
                Some(root) => {
                    let ancestor = self.lower_type_expr_for_locator_shape(&root.expr, &shape_ctx);
                    self.navigate_lowered_locator(ancestor, &root.path)
                        .unwrap_or_else(|| self.opaque(QueryError::Miss))
                }
                None => self.lower_type_expr_for_locator_shape(&expr, &shape_ctx),
            },
            // A whole merged decl body lowers each contributor and interns
            // the DISTINCT MergedDecl carrier — never a bare Intersection
            // (the peer-merge reducer needs the contributor structure).
            DerefedBodyShape::Merged(contributors) => {
                let ids: Vec<SemanticNodeId> = contributors
                    .iter()
                    .enumerate()
                    .map(|(ordinal, contributor)| {
                        let source = locator_with_path_step(
                            key.locator(),
                            verter_type_expr::locators::TypeBodyPathStep::MergedContributor {
                                ordinal: u32::try_from(ordinal)
                                    .expect("merged contributor ordinal exceeds u32"),
                            },
                        );
                        let contributor_ctx = shape_ctx.with_infer_source(&source);
                        self.lower_type_expr_for_locator_shape(contributor, &contributor_ctx)
                    })
                    .collect();
                self.graph().intern_node_with_scope(
                    SemanticNodeData::MergedDecl {
                        contributors: Arc::from(ids.into_boxed_slice()),
                    },
                    scope.clone(),
                )
            }
        };

        // Occurrence patch: when the ROOT located body IS a signature and
        // the key's locator maps to an exact authored position (a value
        // overload-group member, an object/interface member, or the whole
        // declaration body), stamp the occurrence the nested lowering left
        // unset. Nested function nodes keep `occurrence: None` — only the
        // root position is occurrence-grade here.
        let node = self.patch_root_signature_occurrence(node, key.locator());

        let output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
            .into();
        output.with_observed_self_roots(observed_self_roots)
    }

    fn navigate_lowered_locator(
        &self,
        node: SemanticNodeId,
        path: &[verter_type_expr::locators::TypeBodyPathStep],
    ) -> Option<SemanticNodeId> {
        enum Position {
            Node(SemanticNodeId),
            Member(crate::semantic_query::SurfaceMember),
            IndexSignature(crate::semantic_query::IndexSignature),
        }

        use verter_type_expr::locators::TypeBodyPathStep as Step;
        let mut position = Position::Node(node);
        for step in path {
            if let Position::IndexSignature(signature) = &position {
                position = match step {
                    Step::IndexSignatureKey => Position::Node(signature.key_type),
                    Step::IndexSignatureValue => Position::Node(signature.value_type),
                    _ => return None,
                };
                continue;
            }
            if let Position::Member(member) = &position {
                position = match step {
                    Step::MemberValue => Position::Node(member.value),
                    Step::MemberKey => match &member.key {
                        crate::semantic_query::AuthoredPropertyKey::Computed(key) => {
                            Position::Node(*key)
                        }
                        _ => return None,
                    },
                    _ => Position::Node(member.value),
                };
                if matches!(step, Step::MemberValue | Step::MemberKey) {
                    continue;
                }
            }
            let node = match &position {
                Position::Node(node) => *node,
                Position::Member(member) => member.value,
                Position::IndexSignature(_) => unreachable!("handled above"),
            };
            // Alias wrappers are transparent at every navigation step —
            // the lowered-graph counterpart of the pre-lowering
            // `unwrap_parenthesized`: a reference the ancestor lowering
            // resolved onto an already-interned `Alias` carrier still
            // exposes its structural child.
            // bounded-loop: at most ALIAS_UNWRAP_BUDGET alias hops before
            // the conservative typed miss, so a malformed alias cycle
            // cannot hang the navigator.
            const ALIAS_UNWRAP_BUDGET: usize = 64;
            let mut budget = ALIAS_UNWRAP_BUDGET;
            let mut node = node;
            let mut data = self.graph().node_data(node)?;
            while let SemanticNodeData::Alias(child) = data.as_ref() {
                if budget == 0 {
                    return None;
                }
                budget -= 1;
                node = *child;
                data = self.graph().node_data(node)?;
            }
            let data = data.as_ref();
            // The match is over the closed STEP vocabulary with no
            // wildcard: a new `TypeBodyPathStep` variant is a compile
            // error here, forcing its author to classify it for the
            // lowered-graph navigator (and the pre-lowering
            // `navigate_expr` it must agree with) instead of silently
            // degrading to a miss. A shape/ordinal mismatch inside an
            // arm is still the typed `None` miss.
            position = match step {
                Step::MergedContributor { ordinal } => match data {
                    SemanticNodeData::MergedDecl { contributors } => {
                        Position::Node(*contributors.get(*ordinal as usize)?)
                    }
                    _ => return None,
                },
                Step::IntersectionArm { ordinal } => match data {
                    SemanticNodeData::Intersection(arms) => {
                        Position::Node(*arms.get(*ordinal as usize)?)
                    }
                    _ => return None,
                },
                Step::UnionArm { ordinal } => match data {
                    SemanticNodeData::Union(arms) => Position::Node(*arms.get(*ordinal as usize)?),
                    _ => return None,
                },
                // A lazy generic application keeps its arguments in its own
                // `args` field rather than the opaque-carrier accessor, so
                // it needs its own arm: without it a `Foo<Arg>` written in a
                // GENERIC declaration's body (which lowers its ancestor and
                // walks the graph) would miss the argument the pre-lowering
                // walk over `TypeExpr::Ref { type_arguments }` finds in a
                // non-generic declaration's body.
                Step::TypeArgument { ordinal } => match data {
                    SemanticNodeData::InstantiationRef { args, .. } => {
                        Position::Node(*args.get(*ordinal as usize)?)
                    }
                    _ => Position::Node(*data.carrier_type_args().get(*ordinal as usize)?),
                },
                Step::Member { ordinal } => match data {
                    SemanticNodeData::Object(surface) => {
                        match surface.entries.get(*ordinal as usize)? {
                            crate::semantic_query::SurfaceEntry::Member(member) => {
                                Position::Member(member.clone())
                            }
                            crate::semantic_query::SurfaceEntry::CallSignature(signature)
                            | crate::semantic_query::SurfaceEntry::ConstructSignature(signature) => {
                                Position::Node(*signature)
                            }
                            crate::semantic_query::SurfaceEntry::IndexSignature(signature) => {
                                Position::IndexSignature(signature.clone())
                            }
                        }
                    }
                    _ => return None,
                },
                // `MemberKey` / `MemberValue` are consumed by the
                // selected-member position above; at a NODE position there
                // is no member axis to descend, so the typed miss mirrors
                // the pre-lowering navigator's refusal.
                Step::MemberKey | Step::MemberValue => return None,
                // Likewise `IndexSignatureKey` / `IndexSignatureValue`
                // apply only to a selected index-signature entry; at any
                // other position they are the typed miss.
                Step::IndexSignatureKey | Step::IndexSignatureValue => return None,
                // A `TypeParamBound` step is valid only as the FIRST path
                // step (served from the decl header before navigation
                // begins); reaching the lowered graph means it appeared
                // mid-path — the same misplaced-step refusal the
                // pre-lowering navigator spells
                // `TypeParamBoundStepMisplaced`.
                Step::TypeParamBound { .. } => return None,
                Step::FunctionParam { ordinal } => match data {
                    SemanticNodeData::Signature { params, .. } => {
                        Position::Node(params.get(*ordinal as usize)?.ty)
                    }
                    _ => return None,
                },
                Step::FunctionReturn => match data {
                    SemanticNodeData::Signature { return_type, .. } => Position::Node(*return_type),
                    _ => return None,
                },
                // A group-level `ValueSignature` step is consumed by the
                // value-parts deref BEFORE binder-crossing detection, so a
                // root path can never legitimately carry one; reaching the
                // lowered graph with it is the typed miss.
                Step::ValueSignature { .. } => return None,
                Step::MappedSource => match data {
                    SemanticNodeData::Mapped { source, .. } => Position::Node(*source),
                    _ => return None,
                },
                Step::MappedValue => match data {
                    SemanticNodeData::Mapped { mapper, .. } => Position::Node(mapper.value_expr),
                    _ => return None,
                },
                Step::MappedNameType => match data {
                    SemanticNodeData::Mapped { mapper, .. } => Position::Node(mapper.name_remap?),
                    _ => return None,
                },
                Step::ConditionalCheck => match data {
                    SemanticNodeData::Conditional { check, .. } => Position::Node(*check),
                    _ => return None,
                },
                Step::ConditionalExtends => match data {
                    SemanticNodeData::Conditional { extends, .. } => Position::Node(*extends),
                    _ => return None,
                },
                Step::ConditionalTrue => match data {
                    SemanticNodeData::Conditional {
                        true_branch_ref, ..
                    } => Position::Node(*true_branch_ref),
                    _ => return None,
                },
                Step::ConditionalFalse => match data {
                    SemanticNodeData::Conditional {
                        false_branch_ref, ..
                    } => Position::Node(*false_branch_ref),
                    _ => return None,
                },
                Step::IndexedAccessObject => match data {
                    SemanticNodeData::IndexedAccess { object, .. } => Position::Node(*object),
                    _ => return None,
                },
                Step::IndexedAccessIndex => match data {
                    SemanticNodeData::IndexedAccess { index, .. } => {
                        Position::Node(self.index_key_node(index)?)
                    }
                    _ => return None,
                },
                Step::TupleElement { ordinal } => match data {
                    SemanticNodeData::Tuple { elements, .. } => {
                        Position::Node(elements.get(*ordinal as usize)?.value)
                    }
                    _ => return None,
                },
            };
        }
        match position {
            Position::Node(node) => Some(node),
            Position::Member(member) => Some(member.value),
            Position::IndexSignature(_) => None,
        }
    }

    /// The indexed-access key as a NODE.
    ///
    /// A computed key already is one. The folded literal forms (`T["a"]`,
    /// `T[0]`) are stored as authored key DATA rather than as a child
    /// node, so addressing that authored position interns the literal type
    /// the key denotes — the same type the pre-lowering walk selects
    /// directly from `TypeExpr::IndexedAccess { index }`. A unique-symbol
    /// key denotes no literal type node, so it stays a typed miss.
    fn index_key_node(&self, index: &IndexKey) -> Option<SemanticNodeId> {
        match index {
            IndexKey::Computed(node) => Some(*node),
            IndexKey::String(name) => Some(self.graph().intern_node(SemanticNodeData::Literal(
                verter_type_expr::LiteralValue::String(name.to_string()),
            ))),
            IndexKey::Number(value) => Some(self.graph().intern_node(SemanticNodeData::Literal(
                verter_type_expr::LiteralValue::Number(value.get() as f64),
            ))),
            IndexKey::UniqueSymbol(_) => None,
        }
    }

    /// Stamp the root signature's occurrence from the lowering key's
    /// locator. No-op when the root is not a signature, the occurrence is
    /// already set, or the locator path does not name an exact callable
    /// position.
    fn patch_root_signature_occurrence(
        &self,
        node: SemanticNodeId,
        locator: &AuthoredBodyLocator,
    ) -> SemanticNodeId {
        let Some(occurrence) = signature_occurrence_for_locator(locator) else {
            return node;
        };
        let Some(data) = self.graph().node_data(node) else {
            return node;
        };
        let mut new_data = (*data).clone();
        let SemanticNodeData::Signature {
            occurrence: slot, ..
        } = &mut new_data
        else {
            return node;
        };
        if slot.is_some() {
            return node;
        }
        *slot = Some(occurrence);
        self.graph().intern_preserving_scope(node, new_data)
    }
}

/// The occurrence identity of the callable position a locator names, when
/// the path maps EXACTLY: a value overload-group member
/// (`[ValueSignature { ordinal }]` — the group ordinal is the
/// contributor/overload ordinal), one object/interface member
/// (`[Member { ordinal }]`), or the whole declaration body (`[]`). Any
/// deeper or unmapped shape has no occurrence-grade identity here.
fn signature_occurrence_for_locator(
    locator: &AuthoredBodyLocator,
) -> Option<crate::semantic_query::SignatureNodeOccurrence> {
    let slot = match locator {
        AuthoredBodyLocator::DeclBody(slot) => slot,
        _ => return None,
    };
    let (function_part, overload_ordinal, signature_ordinal) = match slot.path.as_ref() {
        [verter_type_expr::locators::TypeBodyPathStep::ValueSignature { ordinal }] => (
            verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            *ordinal,
            *ordinal,
        ),
        [verter_type_expr::locators::TypeBodyPathStep::Member { ordinal }] => (
            verter_type_expr::facts::FunctionPartIdentity::Member {
                member_path: Arc::from([*ordinal]),
            },
            0,
            0,
        ),
        [] => (
            verter_type_expr::facts::FunctionPartIdentity::Other { ordinal: 0 },
            0,
            0,
        ),
        _ => return None,
    };
    Some(crate::semantic_query::SignatureNodeOccurrence {
        function: verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: slot.anchor.clone(),
            function_part,
            overload_ordinal,
        },
        signature_ordinal,
    })
}

fn locator_with_header_bound(
    locator: &AuthoredBodyLocator,
    ordinal: u32,
    position: TypeParamBoundPosition,
) -> AuthoredBodyLocator {
    locator_with_path_step(
        locator,
        verter_type_expr::locators::TypeBodyPathStep::TypeParamBound { ordinal, position },
    )
}

fn locator_with_path_step(
    locator: &AuthoredBodyLocator,
    step: verter_type_expr::locators::TypeBodyPathStep,
) -> AuthoredBodyLocator {
    let append = |path: &Arc<[verter_type_expr::locators::TypeBodyPathStep]>| {
        let mut next = Vec::with_capacity(path.len() + 1);
        next.extend_from_slice(path);
        next.push(step);
        Arc::from(next.into_boxed_slice())
    };
    match locator {
        AuthoredBodyLocator::DeclBody(slot) => {
            AuthoredBodyLocator::DeclBody(verter_type_expr::locators::TypeBodySlot {
                anchor: slot.anchor.clone(),
                path: append(&slot.path),
            })
        }
        AuthoredBodyLocator::AugmentationBody(body) => AuthoredBodyLocator::AugmentationBody(
            verter_type_expr::locators::AugmentationBodyLocator {
                anchor: body.anchor.clone(),
                scope: body.scope.clone(),
                path: append(&body.path),
            },
        ),
        AuthoredBodyLocator::JsdocTypedefBody(body) => AuthoredBodyLocator::JsdocTypedefBody(
            verter_type_expr::locators::JsdocTypedefBodyLocator {
                anchor: body.anchor.clone(),
                path: append(&body.path),
            },
        ),
        AuthoredBodyLocator::MacroPayload(payload) => {
            AuthoredBodyLocator::MacroPayload(payload.clone())
        }
    }
}

/// Compile-enforced completeness tripwire for the closed
/// `TypeBodyPathStep` vocabulary the two independent navigators share:
/// [`ProjectSemanticDispatch::navigate_lowered_locator`] here
/// (post-lowering `SemanticNodeData`) and `navigate_expr`
/// (`decl_body_memo::locator_deref`, pre-lowering `TypeExpr`). Both fail
/// closed (`None` / `PathUnresolved`) on an unhandled step, so a forgotten
/// variant degrades to a typed miss rather than silently wrong data.
///
/// This match has NO wildcard arm, so adding a new [`TypeBodyPathStep`]
/// variant is a compile error HERE — the prompt to add handling to both
/// navigators, never a name-keyed source scanner. It asserts vocabulary
/// COMPLETENESS only; it cannot observe what either navigator selects.
/// That behavioral agreement is proven separately by forcing the same
/// authored position through both routes — a body nested under a generic
/// callable's return takes the lowered-graph navigator, the same body as a
/// whole declaration takes the pre-lowering one — in
/// `both_locator_navigators_select_the_same_authored_position`.
#[cfg(test)]
mod step_vocabulary_completeness {
    use verter_type_expr::locators::{TypeBodyPathStep, TypeParamBoundPosition};

    fn assert_every_step_variant_is_named(step: TypeBodyPathStep) {
        match step {
            TypeBodyPathStep::MergedContributor { .. }
            | TypeBodyPathStep::IntersectionArm { .. }
            | TypeBodyPathStep::TypeArgument { .. }
            | TypeBodyPathStep::Member { .. }
            | TypeBodyPathStep::MemberKey
            | TypeBodyPathStep::MemberValue
            | TypeBodyPathStep::TypeParamBound { .. }
            | TypeBodyPathStep::FunctionParam { .. }
            | TypeBodyPathStep::FunctionReturn
            | TypeBodyPathStep::ValueSignature { .. }
            | TypeBodyPathStep::MappedSource
            | TypeBodyPathStep::MappedValue
            | TypeBodyPathStep::MappedNameType
            | TypeBodyPathStep::ConditionalCheck
            | TypeBodyPathStep::ConditionalExtends
            | TypeBodyPathStep::ConditionalTrue
            | TypeBodyPathStep::ConditionalFalse
            | TypeBodyPathStep::UnionArm { .. }
            | TypeBodyPathStep::IndexedAccessObject
            | TypeBodyPathStep::IndexedAccessIndex
            | TypeBodyPathStep::IndexSignatureKey
            | TypeBodyPathStep::IndexSignatureValue
            | TypeBodyPathStep::TupleElement { .. } => {}
        }
    }

    #[test]
    fn step_vocabulary_is_exhaustively_named() {
        // Constructing one instance is enough to type-check the exhaustive
        // match above; the real assertion is the compile-time exhaustiveness
        // itself (no wildcard arm), not this runtime call.
        assert_every_step_variant_is_named(TypeBodyPathStep::TypeParamBound {
            ordinal: 0,
            position: TypeParamBoundPosition::Constraint,
        });
    }
}
