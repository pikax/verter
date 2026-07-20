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
    /// 1. **Predeclare every sibling** as a bound-free `TypeParam` SHELL —
    ///    the sibling NAME set is complete before any bound lowers, so a
    ///    bound's sibling reference (backward, forward, or self) always
    ///    finds the frame entry and can never fall through to an outer
    ///    same-named declaration.
    /// 2. **Lower bounds per position**: the bound of the parameter at
    ///    ordinal `N` lowers under a frame where parameters declared BEFORE
    ///    `N` are usable through their final (bound-carrying) binders and
    ///    the parameter itself plus later siblings are — for a CONSTRAINT —
    ///    usable shells (TS constraints may reference later siblings and
    ///    self, F-bounded forms included), or — for a DEFAULT —
    ///    shadow-forbidden entries (TS rejects default forward / self
    ///    references; such a reference lowers to the fail-closed
    ///    `Opaque(Miss)`, never an outer capture). Constraints stay graph
    ///    EDGES on the binder nodes — nothing here evaluates them, so a
    ///    mutual `<T extends U, U extends T>` just creates the edges (the
    ///    shell break makes the node data acyclic by construction) and
    ///    resolution-time cycle handling stays where it lives.
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
    /// the same scope + identity mode + ordinal + bound NODES), so the
    /// substitution step that applies `Instantiate` args to a fetched shape
    /// re-derives the SAME binder ids from the prepared declaration's
    /// parameter facts.
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
        };

        // Phase 1: predeclare every sibling as a bound-free shell.
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
        // bound-position demand returns): prior finals usable; self + later
        // siblings constraint-usable shells / default-forbidden shadows.
        let bound_frame =
            |finals: &[BuiltTypeParamBinder], ordinal: usize, position: TypeParamBoundPosition| {
                let mut frame = LocatorBinderFrame::default();
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

        // Phase 2: left-to-right final-binder minting. A `Body` demand
        // needs every final binder; a bound demand needs exactly the finals
        // declared before its ordinal.
        let limit = match visibility {
            TypeParamVisibility::Body => specs.len(),
            TypeParamVisibility::Constraint { ordinal }
            | TypeParamVisibility::Default { ordinal } => (ordinal as usize).min(specs.len()),
        };
        let mut finals: Vec<BuiltTypeParamBinder> = Vec::with_capacity(limit);
        for (index, spec) in specs.iter().take(limit).enumerate() {
            let mut lower_in = |position: TypeParamBoundPosition| {
                let frame = bound_frame(&finals, index, position);
                let mut frames: Vec<LocatorBinderFrame> = base.binders.to_vec();
                frames.push(frame);
                let ctx =
                    LocatorShapeCtx::new(scope, &frames, base.name_resolution, base.scope_payload);
                lower_bound(index as u32, position, &ctx)
            };
            let constraint = if spec.has_constraint {
                lower_in(TypeParamBoundPosition::Constraint)
            } else {
                None
            };
            let default = if spec.has_default {
                lower_in(TypeParamBoundPosition::Default)
            } else {
                None
            };
            let (decl, param_index) = mint_identity(&spec.name, index);
            let binder = graph.intern_node_with_scope(
                SemanticNodeData::TypeParam {
                    decl,
                    param_index,
                    constraint,
                    default,
                    display_name: Arc::clone(&spec.name),
                },
                scope.clone(),
            );
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
                for built in &finals {
                    frame.bind(Arc::clone(&built.name), built.binder);
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
                    .prepared_value_decl(canonical.as_ref(), anchor.owner, anchor_symbol.as_ref())
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
        let base = LocatorShapeCtx::new(&scope, &[], name_resolution, scope_payload.as_ref());
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
                Some(self.lower_type_expr_for_locator_shape(bound, bound_ctx))
            },
        );
        let frames = [frame];
        let shape_ctx =
            LocatorShapeCtx::new(&scope, &frames, name_resolution, scope_payload.as_ref());

        let node = match derefed.shape {
            DerefedBodyShape::Single(expr) => {
                self.lower_type_expr_for_locator_shape(&expr, &shape_ctx)
            }
            // A whole merged decl body lowers each contributor and interns
            // the DISTINCT MergedDecl carrier — never a bare Intersection
            // (the peer-merge reducer needs the contributor structure).
            DerefedBodyShape::Merged(contributors) => {
                let ids: Vec<SemanticNodeId> = contributors
                    .iter()
                    .map(|contributor| {
                        self.lower_type_expr_for_locator_shape(contributor, &shape_ctx)
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

        let output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
            .into();
        output.with_observed_self_roots(observed_self_roots)
    }
}
