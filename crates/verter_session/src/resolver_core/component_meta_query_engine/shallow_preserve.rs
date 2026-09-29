//! Shallow-preservation fast paths for the macro-field expander, decided in
//! NODE DOMAIN on `ComponentMetaQueryEngine<'a>`.
//!
//! The classifier methods decide how shallowly a macro field's value
//! publishes (without crossing the import boundary) off the shared graph
//! carriers: the field's structural mirror member / authored member position
//! raises ONCE through the shared dispatch bridge, and the imported-route /
//! alias-unwrap / parent-parameter predicates walk `SemanticNodeData`
//! directly. They read the engine's scope-routing caches via `&mut self` and
//! have no engine-state references beyond what the parent module declares.
//!
//! Visibility: the fast-path classifier trio
//! (`field_needs_parent_projection` / `macro_field_value_node` /
//! `try_fast_shallow_field_expr`) is `pub(crate)` for the eval_env macro
//! expander; the node predicates stay private and are visible inside the
//! `component_meta_query_engine` folder via the parent-private locality rule
//! (Rust child modules see parent privates).

use super::helpers::is_package_canonical;
use super::{ComponentMetaQueryEngine, FastShallowFieldExpr, FastShallowFieldExprExactness};
use rustc_hash::FxHashSet;

impl<'a> ComponentMetaQueryEngine<'a> {
    /// Field-level fast path predicate (node-domain).
    ///
    /// Returns `true` when the macro field at `output_path` MUST take the
    /// slow parent-projection path; returns `false` when the fast path
    /// applies and the closure can short-circuit to publishing the field's
    /// authored source without dispatching the macro's parent shell.
    ///
    /// The decision is "the field's authored value does not transitively
    /// reference any name in the parent shell's prepared `type_parameters`",
    /// decided in NODE DOMAIN off the shared graph carriers:
    ///
    /// - a non-reference parent shell (inline literal, compound shape) keeps
    ///   the slow path (type-argument substitution matters there);
    /// - an unresolvable / unprepared shell keeps the slow path (defensive);
    /// - an EMPTY prepared type-parameter list is always the fast path;
    /// - a non-empty list checks the field's authored member body node (the
    ///   member value position raised through the one shared dispatch, where
    ///   the declaration's own parameters are bound as `TypeParam` shells)
    ///   for a `TypeParam` shell naming one of the parent's parameters. A
    ///   member body that cannot be raised keeps the slow path.
    pub(crate) fn field_needs_parent_projection(
        &mut self,
        scope_canonical_id: &str,
        macro_index: usize,
        output_path: &[verter_semantic::analysis::type_eval_build::PathSegment],
    ) -> bool {
        use verter_semantic::analysis::type_eval_build::PathSegment as MacroPathSegment;

        let Some(product) = crate::structural_carrier_producer::macro_type_arg_hot_ref(
            self.ctx,
            scope_canonical_id,
            macro_index,
        ) else {
            return true;
        };
        let Some(data) =
            crate::project_semantic_dispatch::node_data_for(self.ctx, product.hot.node())
        else {
            return true;
        };
        let Some(scope_owner) = self
            .ctx
            .project_type_store()
            .semantic_graph()
            .node_scope(product.hot.node())
            .and_then(|scope| scope.top_level_owner())
        else {
            return true;
        };
        // Anonymous / compound carrier - keep the slow path (parity with the
        // former "carrier is not a Ref" rule; parens are structurally
        // transparent in the mirror).
        let Some((name, _)) = data.bare_ref_head() else {
            return true;
        };
        let name = std::sync::Arc::clone(name);
        let Some(root_identity) =
            self.root_identity_in_scope(scope_canonical_id, scope_owner, name.as_ref())
        else {
            return true;
        };
        let Some(prepared) = self.prepared_type_decl(
            &root_identity.canonical_id,
            root_identity.owner,
            &root_identity.symbol_name,
        ) else {
            return true;
        };
        // Empty parameter list - no parent-param references possible. The
        // fast path always applies.
        if prepared.type_parameters.is_empty() {
            return false;
        }
        // Non-empty parameters: prove the FIELD's authored member body does
        // not reference them, off the member value node raised through the
        // one dispatch (declaration parameters are bound as `TypeParam`
        // shells there). Only a single-member path has a directly
        // addressable member slot; deeper paths keep the slow path.
        let [MacroPathSegment::Member(field_name)] = output_path else {
            return true;
        };
        let field_key = verter_type_expr::facts::FactPropertyKey::identifier(field_name.as_ref());
        let Some(member) = prepared.member_index.get(&field_key) else {
            return true;
        };
        let param_names: FxHashSet<&str> = prepared
            .type_parameters
            .iter()
            .map(|param| param.name.as_str())
            .collect();
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
        let Some(member_node) = dispatch
            .raise_authored_locator_to_hot(
                &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(member.ty.clone()),
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
            )
            .at_optional_boundary()
        else {
            return true;
        };
        node_references_type_param_names(self.ctx, member_node.node(), &param_names)
    }

    /// Node-domain fast-path classifier for one macro field: decide whether
    /// the field's value can publish SHALLOW (its authored / resolved source)
    /// without dispatching the macro's parent-shell projection.
    ///
    /// `field_value` is the field's value node — the structural mirror member
    /// for an inline-literal shell, or the member value position raised
    /// through the one dispatch for a named shell (see
    /// [`Self::macro_field_value_node`]). The routes mirror the former
    /// `TypeExpr`-shape classifier one-for-one, decided off
    /// [`crate::semantic_query::SemanticNodeData`] carriers:
    ///
    /// 1. a direct imported utility route anywhere under the field value
    ///    (a builtin utility applied over an imported argument) stays the
    ///    SYMBOLIC authored carrier;
    /// 2. an imported generic reference root stays the SYMBOLIC authored
    ///    carrier;
    /// 3. a single-member import path (`ImportedRoot['member']`): a
    ///    package-backed root stays symbolic; a workspace-owned root
    ///    materialises ONLY that member's authored position (path-precise) —
    ///    unless the member references the root's type parameters (slow
    ///    path);
    /// 4. a bare workspace-owned no-param alias publishes the alias
    ///    DECLARATION's authored body as the source, classified through the
    ///    shared [`crate::meta_resolve::exactness::classify_node`];
    /// 5. an imported generic route reached through containers / local alias
    ///    hops stays the SYMBOLIC authored carrier.
    pub(crate) fn try_fast_shallow_field_expr(
        &mut self,
        scope_canonical_id: &str,
        payload: &verter_type_expr::locators::MacroPayloadLocator,
        field_value: crate::semantic_query::HotTypeRef,
    ) -> Option<FastShallowFieldExpr> {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;
        let scope_owner = payload.anchor.owner;

        let authored_source = || {
            verter_type_expr::facts::SemanticTypeSource::Authored(
                verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(payload.clone()),
            )
        };
        let symbolic_preserve = |hot: crate::semantic_query::HotTypeRef| {
            Some(FastShallowFieldExpr {
                hot,
                semantic_source: authored_source(),
                exactness: FastShallowFieldExprExactness::Symbolic,
            })
        };

        // (1) Direct imported utility route anywhere under the field value.
        if self.node_contains_imported_utility_route(
            scope_canonical_id,
            scope_owner,
            field_value.node(),
        ) {
            return symbolic_preserve(field_value);
        }

        let root_data =
            crate::project_semantic_dispatch::node_data_for(self.ctx, field_value.node())?;

        // (2) Imported generic reference root.
        if let Some((name, _)) = root_data.bare_ref_head() {
            if !root_data.carrier_type_args().is_empty()
                && self.bare_ref_origin_in_scope(scope_canonical_id, scope_owner, name.as_ref())
                    == BareRefOrigin::Imported
            {
                let name = std::sync::Arc::clone(name);
                let _ =
                    self.root_identity_in_scope(scope_canonical_id, scope_owner, name.as_ref())?;
                return symbolic_preserve(field_value);
            }
        }
        if let crate::semantic_query::SemanticNodeData::InstantiationRef { base, .. } =
            root_data.as_ref()
        {
            // A pre-resolved generic application whose base lives outside the
            // owner scope is the imported-generic class.
            if base.canonical_id.as_ref() != scope_canonical_id
                && verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                    base.decl_name.as_ref(),
                )
                .is_none()
            {
                return symbolic_preserve(field_value);
            }
        }

        // (3) Single-member import path: `ImportedRoot['member']`.
        if let crate::semantic_query::SemanticNodeData::IndexedAccess {
            object,
            index: crate::semantic_query::IndexKey::String(member_name),
        } = root_data.as_ref()
        {
            let object_data = crate::project_semantic_dispatch::node_data_for(self.ctx, *object);
            let root_name = object_data.as_deref().and_then(|data| {
                data.bare_ref_head().and_then(|(name, _)| {
                    data.carrier_type_args()
                        .is_empty()
                        .then(|| std::sync::Arc::clone(name))
                })
            });
            if let Some(root_name) = root_name {
                if self.bare_ref_origin_in_scope(
                    scope_canonical_id,
                    scope_owner,
                    root_name.as_ref(),
                ) == BareRefOrigin::Imported
                {
                    let root_identity = self.root_identity_in_scope(
                        scope_canonical_id,
                        scope_owner,
                        root_name.as_ref(),
                    )?;
                    if is_package_canonical(self.ctx, &root_identity.canonical_id) {
                        return symbolic_preserve(field_value);
                    }
                    let member_name = std::sync::Arc::clone(member_name);
                    let prepared = self.prepared_type_decl(
                        &root_identity.canonical_id,
                        root_identity.owner,
                        &root_identity.symbol_name,
                    )?;
                    let member_key =
                        verter_type_expr::facts::FactPropertyKey::identifier(member_name.as_ref());
                    let member = prepared.member_index.get(&member_key)?.clone();
                    let param_names: FxHashSet<&str> = prepared
                        .type_parameters
                        .iter()
                        .map(|param| param.name.as_str())
                        .collect();
                    let dispatch =
                        crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
                    let member_node = dispatch
                        .raise_authored_locator_to_hot(
                            &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
                                member.ty.clone(),
                            ),
                            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                                crate::semantic_query::ProjectionMode::Navigate,
                            ),
                        )
                        .at_optional_boundary()?;
                    if node_references_type_param_names(self.ctx, member_node.node(), &param_names)
                    {
                        return None;
                    }
                    let exactness = match crate::meta_resolve::exactness::classify_node(
                        &dispatch,
                        member_node.node(),
                    ) {
                        verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete => {
                            FastShallowFieldExprExactness::Concrete
                        }
                        _ => FastShallowFieldExprExactness::Symbolic,
                    };
                    return Some(FastShallowFieldExpr {
                        hot: member_node,
                        semantic_source: verter_type_expr::facts::SemanticTypeSource::Authored(
                            verter_type_expr::locators::AuthoredBodyLocator::DeclBody(member.ty),
                        ),
                        exactness,
                    });
                }
            }
        }

        // (4) Bare workspace-owned no-param alias: publish the alias
        // DECLARATION's authored body as the source (the one engine lowers it
        // on demand), classified through the shared exactness predicate.
        if let Some((name, _)) = root_data.bare_ref_head() {
            if root_data.carrier_type_args().is_empty()
                && matches!(
                    self.bare_ref_origin_in_scope(scope_canonical_id, scope_owner, name.as_ref()),
                    BareRefOrigin::Imported | BareRefOrigin::Local
                )
            {
                let name = std::sync::Arc::clone(name);
                if let Some(root_identity) =
                    self.root_identity_in_scope(scope_canonical_id, scope_owner, name.as_ref())
                {
                    if !is_package_canonical(self.ctx, &root_identity.canonical_id) {
                        if let Some(prepared) = self.prepared_type_decl(
                            &root_identity.canonical_id,
                            root_identity.owner,
                            &root_identity.symbol_name,
                        ) {
                            if prepared.type_parameters.is_empty() {
                                let body_slot = prepared.body_facts.body_slot.clone();
                                let dispatch =
                                    crate::project_semantic_dispatch::ProjectSemanticDispatch::new(
                                        self.ctx,
                                    );
                                if let Some(body_node) = dispatch.raise_authored_locator_to_hot(
                                    &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
                                        body_slot.clone(),
                                    ),
                                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                                        crate::semantic_query::ProjectionMode::Navigate,
                                    ),
                                ).at_optional_boundary() {
                                    let exactness =
                                        match crate::meta_resolve::exactness::classify_node(
                                            &dispatch,
                                            body_node.node(),
                                        ) {
                                            verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete => {
                                                FastShallowFieldExprExactness::Concrete
                                            }
                                            _ => FastShallowFieldExprExactness::Symbolic,
                                        };
                                    return Some(FastShallowFieldExpr {
                                        hot: body_node,
                                        semantic_source:
                                            verter_type_expr::facts::SemanticTypeSource::Authored(
                                                verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
                                                    body_slot,
                                                ),
                                            ),
                                        exactness,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // (5) Imported generic route through containers / local alias hops.
        self.node_has_imported_generic_route(scope_canonical_id, scope_owner, field_value.node())
            .then(|| FastShallowFieldExpr {
                hot: field_value,
                semantic_source: authored_source(),
                exactness: FastShallowFieldExprExactness::Symbolic,
            })
    }

    /// Resolve the value NODE for the macro field at `output_path` —
    /// the fast-path classification subject:
    ///
    /// - an inline-literal shell serves the field's structural mirror member
    ///   (a pure carrier-data read of the interned macro type-argument
    ///   graph);
    /// - a named workspace-owned shell raises the field's authored member
    ///   value position through the one dispatch (the memoized
    ///   `LowerLocator` query).
    ///
    /// `None` = no directly addressable field value (compound shells,
    /// package-backed roots, deep paths) — the caller skips the fast paths.
    pub(crate) fn macro_field_value_node(
        &mut self,
        scope_canonical_id: &str,
        macro_index: usize,
        output_path: &[verter_semantic::analysis::type_eval_build::PathSegment],
    ) -> Option<crate::semantic_query::HotTypeRef> {
        use verter_semantic::analysis::type_eval_build::PathSegment as MacroPathSegment;

        let [MacroPathSegment::Member(field_name)] = output_path else {
            return None;
        };
        let product = crate::structural_carrier_producer::macro_type_arg_hot_ref(
            self.ctx,
            scope_canonical_id,
            macro_index,
        )?;
        let scope_owner = self
            .ctx
            .project_type_store()
            .semantic_graph()
            .node_scope(product.hot.node())?
            .top_level_owner()?;
        let data = crate::project_semantic_dispatch::node_data_for(self.ctx, product.hot.node())?;
        if let crate::semantic_query::SemanticNodeData::Object(surface) = data.as_ref() {
            return match surface.project_string_key(field_name.as_ref()) {
                crate::semantic_query::SurfaceKeyProjection::Exact(member) => {
                    Some(crate::semantic_query::HotTypeRef::new(member.value))
                }
                crate::semantic_query::SurfaceKeyProjection::AbsentProven => None,
            };
        }
        let (name, _) = data.bare_ref_head()?;
        if !data.carrier_type_args().is_empty() {
            return None;
        }
        let name = std::sync::Arc::clone(name);
        let root_identity =
            self.root_identity_in_scope(scope_canonical_id, scope_owner, name.as_ref())?;
        if is_package_canonical(self.ctx, &root_identity.canonical_id) {
            return None;
        }
        let prepared = self.prepared_type_decl(
            &root_identity.canonical_id,
            root_identity.owner,
            &root_identity.symbol_name,
        )?;
        let field_key = verter_type_expr::facts::FactPropertyKey::identifier(field_name.as_ref());
        let member = prepared.member_index.get(&field_key)?;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
        dispatch
            .raise_authored_locator_to_hot(
                &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(member.ty.clone()),
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
            )
            .at_optional_boundary()
    }

    /// Whether any node under `node` is a builtin utility application over an
    /// imported argument — the node-domain mirror of the former
    /// `contains_direct_imported_utility_route` `TypeExpr` walk. Every node is
    /// read once per question ([`UtilityQuestion`]), whatever the nesting.
    fn node_contains_imported_utility_route(
        &mut self,
        scope_canonical_id: &str,
        scope_owner: verter_type_expr::TopLevelOwnerId,
        node: crate::semantic_query::SemanticNodeId,
    ) -> bool {
        use crate::graph_walk::Reach;
        use crate::semantic_query::SemanticNodeData;
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;

        crate::graph_walk::reaches(UtilityQuestion::Route(node), |question| match question {
            UtilityQuestion::Route(node) => {
                let Some(data) = crate::project_semantic_dispatch::node_data_for(self.ctx, node)
                else {
                    return Reach::Parts(Vec::new());
                };
                Reach::Parts(match data.as_ref() {
                    data_ref if data_ref.bare_ref_head().is_some() => {
                        let args = data_ref.carrier_type_args();
                        let utility = data_ref.bare_ref_head().is_some_and(|(name, _)| {
                            verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                                name.as_ref(),
                            )
                            .is_some()
                        });
                        utility_arguments(utility, args)
                    }
                    SemanticNodeData::InstantiationRef { base, args } => {
                        let utility =
                            verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                                base.decl_name.as_ref(),
                            )
                            .is_some();
                        utility_arguments(utility, args)
                    }
                    composite
                    @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                        routes(composite.composite_members().expect("composite arm").iter())
                    }
                    SemanticNodeData::MergedDecl {
                        contributors: members,
                    } => routes(members.iter()),
                    SemanticNodeData::Array { element, .. } => routes([element]),
                    SemanticNodeData::Tuple { elements, .. } => {
                        routes(elements.iter().map(|element| &element.value))
                    }
                    SemanticNodeData::Object(surface) => {
                        routes(
                            surface
                                .positive_members()
                                .iter()
                                .map(|member| &member.value)
                                .chain(surface.index_signatures.iter().flat_map(|signature| {
                                    [&signature.key_type, &signature.value_type]
                                }))
                                .chain(surface.call_signatures.iter())
                                .chain(surface.construct_signatures.iter()),
                        )
                    }
                    SemanticNodeData::Signature {
                        params,
                        return_type,
                        predicate,
                        ..
                    } => params
                        .iter()
                        .map(|param| param.ty)
                        .chain(std::iter::once(*return_type))
                        .chain(predicate.and_then(|predicate| predicate.ty))
                        .map(UtilityQuestion::Route)
                        .collect(),
                    SemanticNodeData::Alias(inner) => routes([inner]),
                    _ => Vec::new(),
                })
            }
            // One utility ARGUMENT node is imported-routed: an imported bare
            // reference, an imported `typeof` root, an indexed access over an
            // imported root, a foreign resolved reference, or another imported
            // utility route.
            UtilityQuestion::Argument(node) => {
                let Some(data) = crate::project_semantic_dispatch::node_data_for(self.ctx, node)
                else {
                    return Reach::Parts(Vec::new());
                };
                if let Some((name, _)) = data.bare_ref_head() {
                    let name = std::sync::Arc::clone(name);
                    if data.carrier_type_args().is_empty()
                        && self.bare_ref_origin_in_scope(
                            scope_canonical_id,
                            scope_owner,
                            name.as_ref(),
                        ) == BareRefOrigin::Imported
                    {
                        return Reach::Hit;
                    }
                    return Reach::Parts(vec![UtilityQuestion::Route(node)]);
                }
                if let Some((value_root, _)) = data.typeof_head() {
                    return if self.bare_ref_origin_in_scope(
                        scope_canonical_id,
                        scope_owner,
                        value_root.name.as_ref(),
                    ) == BareRefOrigin::Imported
                    {
                        Reach::Hit
                    } else {
                        Reach::Parts(Vec::new())
                    };
                }
                let foreign = |canonical_id: &str| {
                    if canonical_id != scope_canonical_id {
                        Reach::Hit
                    } else {
                        Reach::Parts(Vec::new())
                    }
                };
                match data.as_ref() {
                    SemanticNodeData::IndexedAccess { object, .. } => {
                        Reach::Parts(vec![UtilityQuestion::Argument(*object)])
                    }
                    SemanticNodeData::DeclRef { identity } => {
                        foreign(identity.canonical_id.as_ref())
                    }
                    SemanticNodeData::InstantiationRef { base, .. } => {
                        foreign(base.canonical_id.as_ref())
                    }
                    _ => Reach::Parts(vec![UtilityQuestion::Route(node)]),
                }
            }
        })
    }

    /// Whether the field value reaches an IMPORTED GENERIC reference through
    /// containers or workspace-local alias hops — the node-domain mirror of
    /// the former `fast_symbolic_imported_generic_route` walk. Local alias
    /// hops raise the alias declaration's authored body through the one
    /// dispatch (the memoized `LowerLocator` query) and read it in the
    /// declaration's scope. Every node is read once per scope, whatever the
    /// nesting, so an alias that reaches itself ends the walk.
    fn node_has_imported_generic_route(
        &mut self,
        scope_canonical_id: &str,
        scope_owner: verter_type_expr::TopLevelOwnerId,
        node: crate::semantic_query::SemanticNodeId,
    ) -> bool {
        use crate::graph_walk::Reach;
        use crate::semantic_query::SemanticNodeData;
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;

        // The scopes the walk reads nodes in: the field's, then each local
        // alias declaration's it hops into.
        let mut scopes: Vec<(std::sync::Arc<str>, verter_type_expr::TopLevelOwnerId)> =
            vec![(std::sync::Arc::from(scope_canonical_id), scope_owner)];
        crate::graph_walk::reaches((node, 0usize), |(node, scope)| {
            let (scope_canonical_id, scope_owner) = scopes[scope].clone();
            let in_scope = |nodes: &mut dyn Iterator<
                Item = crate::semantic_query::SemanticNodeId,
            >| { nodes.map(|node| (node, scope)).collect::<Vec<_>>() };
            let Some(data) = crate::project_semantic_dispatch::node_data_for(self.ctx, node) else {
                return Reach::Parts(Vec::new());
            };
            if let Some((name, _)) = data.bare_ref_head() {
                let name = std::sync::Arc::clone(name);
                let args_empty = data.carrier_type_args().is_empty();
                drop(data);
                return match self.bare_ref_origin_in_scope(
                    scope_canonical_id.as_ref(),
                    scope_owner,
                    name.as_ref(),
                ) {
                    BareRefOrigin::Imported if !args_empty => Reach::Hit,
                    BareRefOrigin::Local if args_empty => {
                        let Some(root_identity) = self.root_identity_in_scope(
                            scope_canonical_id.as_ref(),
                            scope_owner,
                            name.as_ref(),
                        ) else {
                            return Reach::Parts(Vec::new());
                        };
                        let body = self
                            .prepared_type_decl(
                                &root_identity.canonical_id,
                                root_identity.owner,
                                &root_identity.symbol_name,
                            )
                            .map(|prepared| prepared.body_facts.body_slot.clone())
                            .and_then(|body_slot| {
                                let dispatch =
                                    crate::project_semantic_dispatch::ProjectSemanticDispatch::new(
                                        self.ctx,
                                    );
                                dispatch.raise_authored_locator_to_hot(
                                    &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
                                        body_slot,
                                    ),
                                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                                        crate::semantic_query::ProjectionMode::Navigate,
                                    ),
                                ).at_optional_boundary()
                            });
                        let Some(body) = body else {
                            return Reach::Parts(Vec::new());
                        };
                        let declaration_scope = (
                            std::sync::Arc::clone(&root_identity.canonical_id),
                            root_identity.owner,
                        );
                        let declaration_scope =
                            match scopes.iter().position(|known| *known == declaration_scope) {
                                Some(index) => index,
                                None => {
                                    scopes.push(declaration_scope);
                                    scopes.len() - 1
                                }
                            };
                        Reach::Parts(vec![(body.node(), declaration_scope)])
                    }
                    _ => Reach::Parts(Vec::new()),
                };
            }
            Reach::Parts(match data.as_ref() {
                SemanticNodeData::InstantiationRef { base, args } => {
                    if base.canonical_id.as_ref() != scope_canonical_id.as_ref()
                        && verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                            base.decl_name.as_ref(),
                        )
                        .is_none()
                    {
                        return Reach::Hit;
                    }
                    in_scope(&mut args.iter().copied())
                }
                SemanticNodeData::IndexedAccess { object, .. } => vec![(*object, scope)],
                SemanticNodeData::Array { element, .. } => vec![(*element, scope)],
                SemanticNodeData::KeyOf { base } => vec![(*base, scope)],
                SemanticNodeData::Alias(inner) => vec![(*inner, scope)],
                SemanticNodeData::Tuple { elements, .. } => {
                    in_scope(&mut elements.iter().map(|element| element.value))
                }
                composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                    in_scope(
                        &mut composite
                            .composite_members()
                            .expect("composite arm")
                            .iter()
                            .copied(),
                    )
                }
                _ => Vec::new(),
            })
        })
    }

    fn bare_ref_origin_in_scope(
        &mut self,
        scope_canonical_id: &str,
        scope_owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> verter_semantic::analysis::type_solver::host::BareRefOrigin {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;
        if let Some(state) = self.ctx.shallow_file_state(scope_canonical_id) {
            if state.import_target_in(scope_owner, name).is_some() {
                return BareRefOrigin::Imported;
            }
            if state.has_type_symbol_in(scope_owner, name)
                || state.has_value_symbol_in(scope_owner, name)
            {
                return BareRefOrigin::Local;
            }
        }
        let payload = self.scope_payload_for_scope(scope_canonical_id, scope_owner);
        if let Some(payload) = payload.as_deref() {
            if scope_owner == verter_type_expr::TopLevelOwnerId::ordinary_file()
                && payload.import_bindings().contains_key(name)
            {
                return BareRefOrigin::Imported;
            }
            if (matches!(
                scope_owner.kind(),
                verter_type_expr::TopLevelOwnerKind::Instance
            ) && payload.scope_type_bindings().contains_key(name))
                || (scope_owner == verter_type_expr::TopLevelOwnerId::ordinary_file()
                    && (payload.scope_type_names().contains(name)
                        || payload.scope_value_names().contains(name)))
            {
                return BareRefOrigin::Local;
            }
        }
        BareRefOrigin::Unknown
    }

    fn root_identity_in_scope(
        &mut self,
        scope_canonical_id: &str,
        scope_owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity> {
        let payload = self.scope_payload_for_scope(scope_canonical_id, scope_owner);
        crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self.ctx,
            scope_canonical_id,
            scope_owner,
            payload.as_deref(),
            name,
        )
    }

    // Leak-close-3 — Q7 deletion (Claude architecture).
    //
    // The trio `deep_resolve_slot_function_refs` /
    // `deep_resolve_type_refs` / `deep_resolve_fn_refs` previously
    // walked Object members / Function params / Array elements /
    // Union / Intersection arms and dispatched every `TypeExpr::Ref`
    // through the expr-domain surface bridge at `(Expanded,
    // Expanded, Published)`. That per-Ref Expanded recursion was the
    // ChatMessages `outputSchema|execute` audit-footprint leak —
    // walking `UIMessage<M,D,U>` slot-bindings expanded `UITools<…>`
    // into per-tool Object surfaces and emitted ProjectMember edges
    // for `outputSchema` / `execute`.
    //
    // Q7 deletes the trio entirely. Macro-shape publication keeps
    // its top-level Class A Expanded projection so unresolved-import
    // diagnostics still surface, but slot Function param types and
    // Object property bodies stay as Ref carriers. Consumers re-
    // resolve the carrier on demand:
    //
    //   * `compute_bindings_via_graph` (graph-native, slot_binding_graph.rs):
    //     already Shallow throughout. Independent of the published shape.
    //   * `slot_bindings_from_type_expr` (verter_semantic/component_meta.rs):
    //     walks `func.parameters.first().ty`. When the param is a Ref
    //     carrier the recursive enumerator hits the `_ => {}` arm and
    //     emits no parser-path bindings; `evaluated_slot_bindings` from
    //     `compute_bindings_via_graph` fills the row instead.
    //   * Final-result cache, projector surface: re-resolve on demand
    //     via the universal-caching dispatch substrate.
    //
    // Verified discriminating by:
    //   - `block_6i_leak_closure` audit-invariant test
    //   - `imported_mapped_slots_reach_resolved_evaluated_types`
    //     (locked-down): graph-native bindings still produce `plan`
    //     and `planId`.
    //   - `slot_binding_imported_props_with_any_index_signature_stays_symbolic`
    //     (locked-down): IndexedAccess symbolic preservation unaffected.
    //   - `project_emits_unresolved_import_publishes_diagnostic`
    //     (locked-down): Class A Expanded still surfaces unresolved-
    //     import diagnostics; only the post-pass is removed.
}

/// Whether any node under `node` is a `TypeParam` shell naming one of
/// `param_names` — the node-domain mirror of the former
/// `field_references_type_params` `TypeExpr` walk (declaration parameters are
/// bound as `TypeParam` shells by the locator-shape lowering, so a name hit
/// IS a parent-parameter reference). Every node is read once, whatever the
/// nesting; purely carrier-data-driven.
fn node_references_type_param_names(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
    param_names: &FxHashSet<&str>,
) -> bool {
    use crate::graph_walk::Reach;
    use crate::semantic_query::SemanticNodeData;

    if param_names.is_empty() {
        return false;
    }
    crate::graph_walk::reaches(node, |node| {
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
            return Reach::Parts(Vec::new());
        };
        Reach::Parts(match data.as_ref() {
            SemanticNodeData::TypeParam {
                display_name,
                constraint,
                default,
                ..
            } => {
                if param_names.contains(display_name.as_ref()) {
                    return Reach::Hit;
                }
                constraint.iter().chain(default.iter()).copied().collect()
            }
            SemanticNodeData::Alias(inner) => vec![*inner],
            SemanticNodeData::Array { element, .. } => vec![*element],
            SemanticNodeData::KeyOf { base } => vec![*base],
            SemanticNodeData::IndexedAccess { object, index } => {
                let mut parts = vec![*object];
                if let crate::semantic_query::IndexKey::Computed(inner) = index {
                    parts.push(*inner);
                }
                parts
            }
            SemanticNodeData::Tuple { elements, .. } => {
                elements.iter().map(|element| element.value).collect()
            }
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                composite
                    .composite_members()
                    .expect("composite arm")
                    .iter()
                    .copied()
                    .collect()
            }
            SemanticNodeData::MergedDecl {
                contributors: members,
            } => members.to_vec(),
            SemanticNodeData::Object(surface) => surface
                .positive_members()
                .iter()
                .map(|member| member.value)
                .chain(
                    surface
                        .index_signatures
                        .iter()
                        .flat_map(|signature| [signature.key_type, signature.value_type]),
                )
                .chain(surface.call_signatures.iter().copied())
                .chain(surface.construct_signatures.iter().copied())
                .collect(),
            SemanticNodeData::Signature {
                params,
                return_type,
                predicate,
                ..
            } => params
                .iter()
                .map(|param| param.ty)
                .chain(std::iter::once(*return_type))
                .chain(predicate.and_then(|predicate| predicate.ty))
                .collect(),
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                pending,
                ..
            } => pending
                .iter()
                .flat_map(|frame| frame.argument_nodes())
                .chain([*check, *extends, *true_branch_ref, *false_branch_ref])
                .collect(),
            // A mapped type's binder shadows same-named outer parameters
            // inside its value; the conservative answer keeps the SLOW path
            // for a mapped shape referencing anything (parity with the
            // shadow-aware walk's caution).
            SemanticNodeData::Mapped { .. } => return Reach::Hit,
            d if d.bare_ref_head().is_some() || d.typeof_head().is_some() => {
                d.carrier_type_args().to_vec()
            }
            _ => Vec::new(),
        })
    })
}

/// The two questions the imported-utility scan
/// ([`ComponentMetaQueryEngine::node_contains_imported_utility_route`]) asks
/// of a node.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum UtilityQuestion {
    /// Does a builtin utility applied to an imported argument occur under
    /// the node?
    Route(crate::semantic_query::SemanticNodeId),
    /// Is the node, as a utility's argument, imported-routed?
    Argument(crate::semantic_query::SemanticNodeId),
}

/// A type application's parts for the imported-utility scan: under a
/// builtin utility, each argument asked as an argument; and every argument
/// scanned for a route.
fn utility_arguments(
    utility: bool,
    args: &[crate::semantic_query::SemanticNodeId],
) -> Vec<UtilityQuestion> {
    let mut parts = Vec::with_capacity(args.len() * 2);
    if utility {
        parts.extend(args.iter().copied().map(UtilityQuestion::Argument));
    }
    parts.extend(args.iter().copied().map(UtilityQuestion::Route));
    parts
}

/// `nodes` scanned for an imported-utility route.
fn routes<'n>(
    nodes: impl IntoIterator<Item = &'n crate::semantic_query::SemanticNodeId>,
) -> Vec<UtilityQuestion> {
    nodes
        .into_iter()
        .copied()
        .map(UtilityQuestion::Route)
        .collect()
}

#[cfg(test)]
#[path = "shallow_preserve_depth_tests.rs"]
mod depth_tests;
