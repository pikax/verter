//! `host_manage::intrinsic_projection` — project-scoped HTML intrinsic
//! attribute / element projection pipeline.
//!
//! Domain E. Materialises `JSX.IntrinsicElements`
//! and `HTMLAttributes` shapes against the project-resolved Vue / JSX
//! companions, projects per-tag member sets, and merges fallback
//! attributes. Public surface remains rooted at `crate::host_manage::*`;
//! this file contributes a private `impl VerterHost { … }` block that
//! continues the parent shell's impl chain.

use std::sync::Arc;

use crate::VerterHost;

impl VerterHost {
    pub(super) fn project_intrinsic_cache_anchor(&self, canonical_id: &str) -> (String, u64) {
        let ws = self.ws();
        let generation = ws.content_generation();
        let anchor = ws
            .owner_for_file(canonical_id)
            .map(|owner| {
                format!(
                    "{}|{}",
                    owner.project_root,
                    owner.tsconfig_path.unwrap_or_default()
                )
            })
            .unwrap_or_else(|| format!("host:{}", self.instance_id));
        (anchor, generation)
    }

    pub(super) fn project_intrinsic_members_for_tag(
        &self,
        owner_canonical_id: &str,
        tag: &str,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>> {
        let vue_canonical = self.resolve_project_intrinsic_canonical(owner_canonical_id, "vue")?;
        let jsx_canonical =
            self.resolve_project_intrinsic_canonical(owner_canonical_id, "vue/jsx")?;

        // Ensure module facts are materialized for the resolved canonicals.
        let _ = self.ensure_indexed_ready(&vue_canonical);
        let _ = self.ensure_indexed_ready(&jsx_canonical);

        let fallback_members = self
            .expand_project_intrinsic_shape_for_canonical(&vue_canonical, "HTMLAttributes", ctx)
            .map(Self::owned_intrinsic_members_from_shape);

        let tag_members =
            self.expand_project_intrinsic_tag_members_for_canonical(&jsx_canonical, tag, ctx);

        match (
            tag_members.filter(|members| !members.is_empty()),
            fallback_members.filter(|members| !members.is_empty()),
        ) {
            (Some(tag_members), Some(fallback_members)) => {
                Some(Self::merge_intrinsic_members(tag_members, fallback_members))
            }
            (Some(tag_members), None) => Some(tag_members),
            (None, Some(fallback_members)) => Some(fallback_members),
            (None, None) => None,
        }
    }

    fn resolve_project_intrinsic_canonical(
        &self,
        owner_canonical_id: &str,
        specifier: &str,
    ) -> Option<String> {
        let ws = self.ws();
        let owner = ws.owner_for_file(owner_canonical_id)?;
        let resolved = ws.resolve_import_for_project(
            &owner,
            specifier,
            verter_workspace::ResolutionContext {
                phase: verter_workspace::ResolvePhase::ProviderGraph,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
            },
        )?;
        let _ = self.ensure_indexed_ready(&resolved.source_id);
        Some(resolved.source_id)
    }

    fn expand_project_intrinsic_shape_for_canonical(
        &self,
        canonical_id: &str,
        type_name: &str,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        // JSX intrinsics resolve through the
        // bridge helper so the §5.14.1 pre-flight gate sees zero
        // external engine-method callers. The bridge body retains
        // the engine call through the migration window per §5.13a.2;
        // the prepared-decl fallback for re-exported /
        // namespace-qualified globals (e.g. `JSX.IntrinsicElements`)
        // is engine-internal until 5l atomic engine retirement.
        // The engine binds to the supplied request-bound `ctx` so
        // cache validators inside the engine inherit the overlay-aware
        // view.
        let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
        let expanded = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
            &mut engine,
            canonical_id,
            type_name,
        )
        .or_else(|| {
            let expr = verter_type_expr::TypeExpr::named(type_name);
            // Route the dispatch helper through the
            // request-bound `ctx` rather than `self: &VerterHost`.
            // Passing `self` here coerced into the bare-host
            // `<&VerterHost as ResolverContext>` impl, which panics
            // under `cfg(not(any(test, debug_assertions)))` (release)
            // once `project_expr_class_a_via_dispatch` reaches
            // `ctx.prepared_decl_bundle(...)` deeper in the call
            // graph.
            crate::meta_resolve::project_expr_class_a_via_dispatch(ctx, canonical_id, &expr)
        })?;
        let mut shape =
            verter_semantic::analysis::type_expand::type_expr_to_object_shape(&expanded);
        Self::materialize_project_intrinsic_shape_members(&mut shape, &mut engine, canonical_id);
        Some(shape)
    }

    fn expand_project_intrinsic_tag_members_for_canonical(
        &self,
        canonical_id: &str,
        tag: &str,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>> {
        let intrinsics_shape = self.expand_project_intrinsic_shape_for_canonical(
            canonical_id,
            "JSX.IntrinsicElements",
            ctx,
        )?;
        let tag_type = intrinsics_shape
            .properties
            .into_iter()
            .find(|property| property.name == tag)
            .map(|property| property.ty)?;
        // Resolve NativeElements scope for tag body expansion.
        let tag_scope_canonical = self
            .resolve_local_import_symbol_target(canonical_id, "NativeElements")
            .map(|(resolved_id, _)| resolved_id)
            .filter(|resolved_id| resolved_id != canonical_id);
        let scope = tag_scope_canonical.as_deref().unwrap_or(canonical_id);
        let _ = self.ensure_indexed_ready(scope);
        // Class A path via
        // the shared dispatch helper. The intrinsic-member
        // materialiser still uses the engine for its own bundle-level
        // scope cache.
        //
        // Route through the request-bound `ctx` rather
        // than `self: &VerterHost`. Same rationale as line 109 above:
        // the bare-host coercion panics under
        // `cfg(not(any(test, debug_assertions)))` deeper in the
        // dispatch.
        let expanded =
            crate::meta_resolve::project_expr_class_a_via_dispatch(ctx, scope, &tag_type)
                .unwrap_or_else(|| tag_type.clone());
        // The engine binds to the supplied request-bound `ctx`.
        let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
        let mut tag_shape =
            verter_semantic::analysis::type_expand::type_expr_to_object_shape(&expanded);
        Self::materialize_project_intrinsic_shape_members(&mut tag_shape, &mut engine, scope);
        Some(Self::owned_intrinsic_members_from_shape(tag_shape))
    }

    fn solve_project_intrinsic_member_type(
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
        scope_canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
    ) -> verter_type_expr::TypeExpr {
        // Class A via shared
        // dispatch helper. The engine here is still kept on the
        // calling intrinsic member-surface materialiser path.
        crate::meta_resolve::project_expr_class_a_via_dispatch(
            engine.ctx(),
            scope_canonical_id,
            expr,
        )
        .unwrap_or_else(|| expr.clone())
    }

    fn materialize_project_intrinsic_member_surface_expr(
        expr: &verter_type_expr::TypeExpr,
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
        scope_canonical_id: &str,
        nested_surface: bool,
    ) -> verter_type_expr::TypeExpr {
        use verter_type_expr::{ObjectMember, TypeExpr};

        if nested_surface {
            let solved =
                Self::solve_project_intrinsic_member_type(engine, scope_canonical_id, expr);
            if solved != *expr {
                return Self::materialize_project_intrinsic_member_surface_expr(
                    &solved,
                    engine,
                    scope_canonical_id,
                    true,
                );
            }
        }

        match expr {
            TypeExpr::Function(function) => {
                let mut function = function.as_ref().clone();
                for param in &mut function.parameters {
                    param.ty = Self::materialize_project_intrinsic_member_surface_expr(
                        &param.ty,
                        engine,
                        scope_canonical_id,
                        true,
                    );
                }
                if let Some(return_type) = function.return_type.as_mut() {
                    let materialized = Self::materialize_project_intrinsic_member_surface_expr(
                        return_type,
                        engine,
                        scope_canonical_id,
                        true,
                    );
                    *return_type = Arc::new(materialized);
                }
                TypeExpr::Function(Arc::new(function))
            }
            TypeExpr::Object(object) => {
                let mut object = object.as_ref().clone();
                for member in &mut object.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            if nested_surface
                                || matches!(
                                    &property.ty,
                                    TypeExpr::Function(_) | TypeExpr::Object(_),
                                )
                            {
                                property.ty =
                                    Self::materialize_project_intrinsic_member_surface_expr(
                                        &property.ty,
                                        engine,
                                        scope_canonical_id,
                                        true,
                                    );
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            signature.key_type =
                                Self::materialize_project_intrinsic_member_surface_expr(
                                    &signature.key_type,
                                    engine,
                                    scope_canonical_id,
                                    true,
                                );
                            signature.value_type =
                                Self::materialize_project_intrinsic_member_surface_expr(
                                    &signature.value_type,
                                    engine,
                                    scope_canonical_id,
                                    true,
                                );
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for param in &mut function.parameters {
                                param.ty = Self::materialize_project_intrinsic_member_surface_expr(
                                    &param.ty,
                                    engine,
                                    scope_canonical_id,
                                    true,
                                );
                            }
                            if let Some(return_type) = function.return_type.as_mut() {
                                let materialized =
                                    Self::materialize_project_intrinsic_member_surface_expr(
                                        return_type,
                                        engine,
                                        scope_canonical_id,
                                        true,
                                    );
                                *return_type = Arc::new(materialized);
                            }
                        }
                        ObjectMember::Method(method) => {
                            for param in &mut method.function.parameters {
                                param.ty = Self::materialize_project_intrinsic_member_surface_expr(
                                    &param.ty,
                                    engine,
                                    scope_canonical_id,
                                    true,
                                );
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                let materialized =
                                    Self::materialize_project_intrinsic_member_surface_expr(
                                        return_type,
                                        engine,
                                        scope_canonical_id,
                                        true,
                                    );
                                *return_type = Arc::new(materialized);
                            }
                        }
                    }
                }
                TypeExpr::Object(Arc::new(object))
            }
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: Arc::new(Self::materialize_project_intrinsic_member_surface_expr(
                    element,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                )),
                readonly: *readonly,
            },
            TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                elements: Arc::from(
                    elements
                        .iter()
                        .map(|element| verter_type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: Self::materialize_project_intrinsic_member_surface_expr(
                                &element.ty,
                                engine,
                                scope_canonical_id,
                                nested_surface,
                            ),
                            optional: element.optional,
                            rest: element.rest,
                        })
                        .collect::<Vec<_>>(),
                ),
                readonly: *readonly,
            },
            TypeExpr::Union(types) => TypeExpr::Union(Arc::from(
                types
                    .iter()
                    .map(|ty| {
                        Self::materialize_project_intrinsic_member_surface_expr(
                            ty,
                            engine,
                            scope_canonical_id,
                            nested_surface,
                        )
                    })
                    .collect::<Vec<_>>(),
            )),
            TypeExpr::Intersection(types) => TypeExpr::Intersection(Arc::from(
                types
                    .iter()
                    .map(|ty| {
                        Self::materialize_project_intrinsic_member_surface_expr(
                            ty,
                            engine,
                            scope_canonical_id,
                            nested_surface,
                        )
                    })
                    .collect::<Vec<_>>(),
            )),
            TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
                Self::materialize_project_intrinsic_member_surface_expr(
                    inner,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                ),
            )),
            TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(
                Self::materialize_project_intrinsic_member_surface_expr(
                    inner,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                ),
            )),
            TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(
                Self::materialize_project_intrinsic_member_surface_expr(
                    inner,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                ),
            )),
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => TypeExpr::Conditional {
                check: Arc::new(Self::materialize_project_intrinsic_member_surface_expr(
                    check,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                )),
                extends: Arc::new(Self::materialize_project_intrinsic_member_surface_expr(
                    extends,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                )),
                true_type: Arc::new(Self::materialize_project_intrinsic_member_surface_expr(
                    true_type,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                )),
                false_type: Arc::new(Self::materialize_project_intrinsic_member_surface_expr(
                    false_type,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                )),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                optional,
                readonly,
                name_type,
                value,
            } => TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: Arc::new(Self::materialize_project_intrinsic_member_surface_expr(
                    source,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                )),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_deref().map(|name_type| {
                    Arc::new(Self::materialize_project_intrinsic_member_surface_expr(
                        name_type,
                        engine,
                        scope_canonical_id,
                        nested_surface,
                    ))
                }),
                value: Arc::new(Self::materialize_project_intrinsic_member_surface_expr(
                    value,
                    engine,
                    scope_canonical_id,
                    nested_surface,
                )),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: Arc::from(
                    expressions
                        .iter()
                        .map(|expr| {
                            Self::materialize_project_intrinsic_member_surface_expr(
                                expr,
                                engine,
                                scope_canonical_id,
                                nested_surface,
                            )
                        })
                        .collect::<Vec<_>>(),
                ),
            },
            _ => expr.clone(),
        }
    }

    fn materialize_project_intrinsic_shape_members(
        shape: &mut verter_semantic::analysis::type_expand::ExpandedObjectShape,
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
        scope_canonical_id: &str,
    ) {
        for property in &mut shape.properties {
            property.ty = Self::materialize_project_intrinsic_member_surface_expr(
                &property.ty,
                engine,
                scope_canonical_id,
                false,
            );
        }
        for signature in &mut shape.index_signatures {
            signature.key_type = Self::materialize_project_intrinsic_member_surface_expr(
                &signature.key_type,
                engine,
                scope_canonical_id,
                true,
            );
            signature.value_type = Self::materialize_project_intrinsic_member_surface_expr(
                &signature.value_type,
                engine,
                scope_canonical_id,
                true,
            );
        }
        for signature in &mut shape.call_signatures {
            for parameter in &mut signature.parameters {
                parameter.ty = Self::materialize_project_intrinsic_member_surface_expr(
                    &parameter.ty,
                    engine,
                    scope_canonical_id,
                    true,
                );
            }
            signature.return_type = Self::materialize_project_intrinsic_member_surface_expr(
                &signature.return_type,
                engine,
                scope_canonical_id,
                true,
            );
        }
    }

    fn owned_intrinsic_members_from_shape(
        shape: verter_semantic::analysis::type_expand::ExpandedObjectShape,
    ) -> Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember> {
        let mut members = rustc_hash::FxHashMap::default();
        for property in shape.properties {
            if let Some(event_name) =
                verter_semantic::analysis::html_intrinsics::on_prop_to_event_name(
                    property.name.as_str(),
                )
            {
                members.entry(format!("listener:{event_name}")).or_insert(
                    verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember {
                        name: event_name,
                        kind: verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener,
                        type_expr: property.ty,
                    },
                );
                continue;
            }

            if !verter_semantic::analysis::html_intrinsics::should_expose_intrinsic_member(
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr,
                property.name.as_str(),
            ) {
                continue;
            }

            members.entry(format!("attr:{}", property.name)).or_insert(
                verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember {
                    name: property.name,
                    kind: verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr,
                    type_expr: property.ty,
                },
            );
        }

        let mut members: Vec<_> = members.into_values().collect();
        members.sort_by(|left, right| {
            let left_rank = match left.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            let right_rank = match right.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            left_rank
                .cmp(&right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        members
    }

    fn merge_intrinsic_members(
        primary: Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>,
        fallback: Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>,
    ) -> Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember> {
        let mut members = rustc_hash::FxHashMap::default();
        for member in fallback {
            members.insert(
                format!(
                    "{}:{}",
                    match member.kind {
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => "attr",
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                            "listener"
                        }
                    },
                    member.name
                ),
                member,
            );
        }
        for member in primary {
            members.insert(
                format!(
                    "{}:{}",
                    match member.kind {
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => "attr",
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                            "listener"
                        }
                    },
                    member.name
                ),
                member,
            );
        }

        let mut members: Vec<_> = members.into_values().collect();
        members.sort_by(|left, right| {
            let left_rank = match left.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            let right_rank = match right.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            left_rank
                .cmp(&right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        members
    }
}
