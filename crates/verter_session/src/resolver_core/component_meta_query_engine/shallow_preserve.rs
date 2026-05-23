//! Shallow preservation, imported-route fast paths, and deep slot/type
//! ref resolution methods on `ComponentMetaQueryEngine<'a>`.
//!
//! Private inherent methods that classify how shallowly an
//! imported / package-backed type expression should be preserved
//! (without crossing the import boundary), and resolve transitive
//! references inside slot / function bodies during deep walks.
//!
//! The methods cross-call each other (`should_preserve_*`,
//! `deep_resolve_*`) and read the engine's caches via `&self` /
//! `&mut self`. They have no engine-state references beyond what the
//! parent module already declares.
//!
//! Visibility:
//! - `pub fn should_preserve_shallow_field_expr` (re-exported via
//!   `mod.rs`'s engine surface; used by external callers in
//!   `meta_resolve.rs`).
//! - `pub fn deep_resolve_slot_function_refs` (used internally; kept
//!   `pub` to match prior signature).
//! - All other methods stay private (no visibility qualifier) and are
//!   visible inside the `component_meta_query_engine` folder via the
//!   parent-private locality rule (Rust child modules see parent
//!   privates).

use rustc_hash::FxHashSet;
use verter_type_expr::TypeExpr;

use super::helpers::{is_package_canonical, strip_parens_expr, type_expr_references_type_params};
use super::{ComponentMetaQueryEngine, FastShallowFieldExpr, FastShallowFieldExprExactness};

impl<'a> ComponentMetaQueryEngine<'a> {
    /// Field-level fast path predicate (Issue #3).
    ///
    /// Returns `true` when the macro field expression `parsed` MUST
    /// take the slow parent-projection path; returns `false` when the
    /// fast path applies and the closure can short-circuit to
    /// `ExpansionResult::exact_concrete(parsed)` without dispatching
    /// the macro's parent shell.
    ///
    /// The decision is "the field's parsed `TypeExpr` does not
    /// transitively reference any name in the parent shell's prepared
    /// `type_parameters`" — modulo shadowing introduced by mapped
    /// types and function-type parameter lists. The shadow-aware
    /// walk is delegated to
    /// [`verter_semantic::analysis::type_expr_refs::field_references_type_params`].
    ///
    /// Returns `true` (slow path) on any of:
    /// - `macro_type_arg` does not resolve to a `Ref` after stripping
    ///   parens (anonymous parent shell — no type params to compare,
    ///   but the slow path remains the safe default for compound
    ///   shapes like `Pick<X, K>` whose type-argument substitution
    ///   matters);
    /// - the parent shell's name does not resolve to a known root
    ///   identity in the owner scope (defensive fallback);
    /// - the prepared type decl is missing (cache miss → slow path);
    /// - the prepared decl has a non-empty type-parameter list AND
    ///   the field expression references at least one of those names.
    ///
    /// Returns `false` (fast path) when the prepared decl exists and
    /// has either:
    /// - an empty type-parameter list (no params to reference); or
    /// - a non-empty list whose names are all absent from the field
    ///   expression (modulo shadowing).
    pub(crate) fn field_needs_parent_projection(
        &mut self,
        scope_canonical_id: &str,
        parsed: &TypeExpr,
        macro_type_arg: &TypeExpr,
    ) -> bool {
        // Strip outer parens to expose the carrier shape.
        let carrier = strip_parens_expr(macro_type_arg);
        let TypeExpr::Ref { name, .. } = carrier else {
            // Anonymous / compound carrier — keep the slow path.
            return true;
        };
        let Some(root_identity) = self.root_identity_in_scope(scope_canonical_id, name.as_ref())
        else {
            return true;
        };
        let Some(prepared) =
            self.prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)
        else {
            return true;
        };
        // Empty parameter list — no parent-param references possible.
        // The fast path always applies.
        if prepared.type_parameters.is_empty() {
            return false;
        }
        // Predicate-correct walk with shadowing semantics.
        verter_semantic::analysis::type_expr_refs::field_references_type_params(
            parsed,
            &prepared.type_parameters,
        )
    }

    /// Symbolic-preservation predicate for the define-props member rescue
    /// path (replacement for
    /// `TypeQueryEngine::should_preserve_shallow_field_expr`).
    ///
    /// Returns `true` when `expr` references a package-backed imported
    /// type surface that the component-meta pipeline should keep in
    /// symbolic form (as a bare `Ref` / `IndexedAccess`) instead of
    /// materialising through dispatch. Routes through
    /// `bare_name_resolve::resolve_bare_name_in_scope` +
    /// `ctx.prepared_type_decl` — no `SessionSolverHost`/`TypeSolverHost`
    /// dependency.
    pub fn should_preserve_shallow_field_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        // Cycle-detection set keyed on the borrowed address of each
        // `TypeExpr` rather than a deep value clone. `TypeExpr` is an
        // enum whose `clone()` recursively duplicates the entire
        // subtree; storing each visited expression as a value made
        // every recursion node's insert O(subtree-size) and combined
        // with the recursion turned the predicate into O(N^2) on tree
        // size. ChatMessage's `UIMessage<TMetadata, TDataParts, TTools>`
        // surface produces deep enough trees that the quadratic blew
        // up to >1 minute per field. Address-based dedup is O(1) per
        // node and preserves the original cycle-break contract: the
        // recursion only re-enters a node if the SAME borrow (same
        // address) is reached, which is what value-equality was
        // approximating. A different `TypeExpr` value at a different
        // address still recurses normally — there is no cross-call
        // reuse expectation since each call originates from a unique
        // root.
        let mut active_exprs = rustc_hash::FxHashSet::<usize>::default();
        let mut active_refs = rustc_hash::FxHashSet::<String>::default();
        self.should_preserve_shallow_field_expr_inner(
            scope_canonical_id,
            expr,
            &mut active_exprs,
            &mut active_refs,
        )
    }

    fn should_preserve_shallow_field_expr_inner(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
        active_exprs: &mut rustc_hash::FxHashSet<usize>,
        active_refs: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        use verter_type_expr::ObjectMember;

        let expr_addr = expr as *const TypeExpr as usize;
        if !active_exprs.insert(expr_addr) {
            return false;
        }
        let preserve = if self.should_preserve_imported_bare_ref(scope_canonical_id, expr)
            || self.should_preserve_imported_member_path(scope_canonical_id, expr)
            || self.should_preserve_imported_utility_route(scope_canonical_id, expr)
            || self.should_preserve_package_member_path(scope_canonical_id, expr)
        {
            true
        } else {
            match expr {
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
                    members.iter().any(|member| {
                        self.should_preserve_shallow_field_expr_inner(
                            scope_canonical_id,
                            member,
                            active_exprs,
                            active_refs,
                        )
                    })
                }
                TypeExpr::Array { element, .. }
                | TypeExpr::KeyOf(element)
                | TypeExpr::Rest(element)
                | TypeExpr::Parenthesized(element) => self
                    .should_preserve_shallow_field_expr_inner(
                        scope_canonical_id,
                        element,
                        active_exprs,
                        active_refs,
                    ),
                TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                    self.should_preserve_shallow_field_expr_inner(
                        scope_canonical_id,
                        &element.ty,
                        active_exprs,
                        active_refs,
                    )
                }),
                TypeExpr::Object(object) => {
                    object.properties.iter().any(|member| match member {
                        ObjectMember::Property(property) => self
                            .should_preserve_shallow_field_expr_inner(
                                scope_canonical_id,
                                &property.ty,
                                active_exprs,
                                active_refs,
                            ),
                        ObjectMember::IndexSignature(signature) => {
                            self.should_preserve_shallow_field_expr_inner(
                                scope_canonical_id,
                                &signature.key_type,
                                active_exprs,
                                active_refs,
                            ) || self.should_preserve_shallow_field_expr_inner(
                                scope_canonical_id,
                                &signature.value_type,
                                active_exprs,
                                active_refs,
                            )
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            function.parameters.iter().any(|parameter| {
                                self.should_preserve_shallow_field_expr_inner(
                                    scope_canonical_id,
                                    &parameter.ty,
                                    active_exprs,
                                    active_refs,
                                )
                            }) || function.return_type.as_deref().is_some_and(|return_type| {
                                self.should_preserve_shallow_field_expr_inner(
                                    scope_canonical_id,
                                    return_type,
                                    active_exprs,
                                    active_refs,
                                )
                            })
                        }
                        ObjectMember::Method(method) => {
                            method.function.parameters.iter().any(|parameter| {
                                self.should_preserve_shallow_field_expr_inner(
                                    scope_canonical_id,
                                    &parameter.ty,
                                    active_exprs,
                                    active_refs,
                                )
                            }) || method.function.return_type.as_deref().is_some_and(
                                |return_type| {
                                    self.should_preserve_shallow_field_expr_inner(
                                        scope_canonical_id,
                                        return_type,
                                        active_exprs,
                                        active_refs,
                                    )
                                },
                            )
                        }
                    })
                }
                TypeExpr::Function(function) => {
                    function.parameters.iter().any(|parameter| {
                        self.should_preserve_shallow_field_expr_inner(
                            scope_canonical_id,
                            &parameter.ty,
                            active_exprs,
                            active_refs,
                        )
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        self.should_preserve_shallow_field_expr_inner(
                            scope_canonical_id,
                            return_type,
                            active_exprs,
                            active_refs,
                        )
                    })
                }
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => {
                    let utility_with_args = !type_arguments.is_empty()
                        && verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(name.as_ref()).is_some();
                    if utility_with_args
                        && type_arguments.iter().any(|argument| {
                            self.should_preserve_shallow_field_expr_inner(
                                scope_canonical_id,
                                argument,
                                active_exprs,
                                active_refs,
                            )
                        })
                    {
                        true
                    } else {
                        self.should_preserve_transitive_ref(
                            scope_canonical_id,
                            name.as_ref(),
                            active_exprs,
                            active_refs,
                        )
                    }
                }
                TypeExpr::IndexedAccess { object, index } => {
                    self.should_preserve_shallow_field_expr_inner(
                        scope_canonical_id,
                        object,
                        active_exprs,
                        active_refs,
                    ) || self.should_preserve_shallow_field_expr_inner(
                        scope_canonical_id,
                        index,
                        active_exprs,
                        active_refs,
                    )
                }
                _ => false,
            }
        };
        active_exprs.remove(&expr_addr);
        preserve
    }

    fn should_preserve_imported_bare_ref(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        let stripped = strip_parens_expr(expr);
        let TypeExpr::Ref {
            name,
            type_arguments,
        } = stripped
        else {
            return false;
        };
        if !type_arguments.is_empty() {
            return false;
        }
        if self.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref())
            != verter_semantic::analysis::type_solver::host::BareRefOrigin::Imported
        {
            return false;
        }
        let Some(root_identity) = self.root_identity_in_scope(scope_canonical_id, name.as_ref())
        else {
            return false;
        };
        let prepared = self
            .ctx
            .prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name);
        // Issue #11 / delegate the symbolic-vs-materialize
        // decision to the shared helper. The helper consumes
        // `WorkspaceRead::is_workspace_owned` / `is_package_backed`
        // (NOT path-substring `node_modules` checks), so package-backed
        // refs (including pnpm-symlink + workspace-package-inside-
        // node_modules edge cases) classify correctly.
        let policy_ctx = crate::component_meta_resolution_policy::policy_helpers::PolicyContext {
            is_workspace_owned: &|canonical| self.ctx.workspace_is_workspace_owned(canonical),
            is_package_backed: &|canonical| self.ctx.workspace_is_package_backed(canonical),
            route_preservation_context: false,
            cycle_active_for_target: false,
            shallow_preserve_list_entry: false,
        };
        if crate::component_meta_resolution_policy::policy_helpers::imported_ref_must_materialize_canonically(
            &root_identity.canonical_id,
            prepared.as_deref(),
            &policy_ctx,
        ) {
            return false;
        }
        // Helper said "may preserve symbolic". Apply the legacy
        // post-helper checks: package-backed refs (helper short-
        // circuits, but the legacy site's `is_package_canonical`
        // covered cases the helper rejects when it has no prepared
        // body) and direct-member shapes for non-workspace-owned
        // targets.
        if self
            .ctx
            .workspace_is_package_backed(&root_identity.canonical_id)
        {
            return true;
        }
        let Some(prepared) = prepared else {
            return false;
        };
        !prepared.member_index.is_empty()
            || matches!(
                prepared.projection_class,
                verter_semantic::analysis::type_solver::prepared::PreparedProjectionClass::DirectMembers
            )
            || matches!(
                prepared.kind,
                verter_semantic::analysis::type_eval::TypeDeclKind::Class
            )
    }

    fn should_preserve_imported_member_path(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        fn root_import_name(expr: &TypeExpr) -> Option<&str> {
            match strip_parens_expr(expr) {
                TypeExpr::IndexedAccess { object, .. } => root_import_name(object),
                TypeExpr::Ref { name, .. } => Some(name.as_ref()),
                _ => None,
            }
        }

        let stripped = strip_parens_expr(expr);
        let TypeExpr::IndexedAccess { object, .. } = stripped else {
            return false;
        };
        let Some(name) = root_import_name(object) else {
            return false;
        };
        if self.bare_ref_origin_in_scope(scope_canonical_id, name)
            != verter_semantic::analysis::type_solver::host::BareRefOrigin::Imported
        {
            return false;
        }
        let Some(root_identity) = self.root_identity_in_scope(scope_canonical_id, name) else {
            return false;
        };
        is_package_canonical(self.ctx, &root_identity.canonical_id)
    }

    fn should_preserve_imported_utility_route(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        let stripped = strip_parens_expr(expr);
        let TypeExpr::Ref {
            name,
            type_arguments,
        } = stripped
        else {
            return false;
        };
        if type_arguments.is_empty()
            || verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                name.as_ref(),
            )
            .is_none()
        {
            return false;
        }
        type_arguments.iter().any(|argument| {
            self.should_preserve_imported_bare_ref(scope_canonical_id, argument)
                || self.should_preserve_imported_utility_route(scope_canonical_id, argument)
                || self.should_preserve_package_member_path(scope_canonical_id, argument)
                || matches!(
                    strip_parens_expr(argument),
                    TypeExpr::TypeOf(value_ref)
                        if value_ref.path.first().is_some_and(|root| {
                            self.bare_ref_origin_in_scope(scope_canonical_id, root)
                                == verter_semantic::analysis::type_solver::host::BareRefOrigin::Imported
                        })
                )
        })
    }

    fn should_preserve_package_member_path(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        fn root_import_name(expr: &TypeExpr) -> Option<&str> {
            match strip_parens_expr(expr) {
                TypeExpr::IndexedAccess { object, .. } => root_import_name(object),
                TypeExpr::Ref { name, .. } => Some(name.as_ref()),
                _ => None,
            }
        }

        let Some(name) = root_import_name(expr) else {
            return false;
        };
        if self.bare_ref_origin_in_scope(scope_canonical_id, name)
            != verter_semantic::analysis::type_solver::host::BareRefOrigin::Imported
        {
            return false;
        }
        let Some(root_identity) = self.root_identity_in_scope(scope_canonical_id, name) else {
            return false;
        };
        is_package_canonical(self.ctx, &root_identity.canonical_id)
    }

    fn should_preserve_transitive_ref(
        &mut self,
        scope_canonical_id: &str,
        name: &str,
        active_exprs: &mut rustc_hash::FxHashSet<usize>,
        active_refs: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        let Some(root_identity) = self.root_identity_in_scope(scope_canonical_id, name) else {
            return false;
        };
        let cache_key = format!(
            "{}::{}",
            root_identity.canonical_id, root_identity.symbol_name
        );
        if is_package_canonical(self.ctx, &root_identity.canonical_id) {
            return true;
        }
        if !active_refs.insert(cache_key.clone()) {
            return false;
        }
        let result = self
            .ctx
            .prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)
            .is_some_and(|prepared| {
                if matches!(prepared.body, TypeExpr::TypeParameter(_)) {
                    true
                } else {
                    self.should_preserve_shallow_field_expr_inner(
                        root_identity.canonical_id.as_str(),
                        &prepared.body,
                        active_exprs,
                        active_refs,
                    )
                }
            });
        active_refs.remove(&cache_key);
        result
    }

    pub(crate) fn try_fast_shallow_field_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<FastShallowFieldExpr> {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;

        fn single_member_import_root(expr: &TypeExpr) -> Option<(&str, &str)> {
            let TypeExpr::IndexedAccess { object, index } = strip_parens_expr(expr) else {
                return None;
            };
            let TypeExpr::Ref {
                name,
                type_arguments,
            } = strip_parens_expr(object)
            else {
                return None;
            };
            if !type_arguments.is_empty() {
                return None;
            }
            let TypeExpr::Literal(verter_type_expr::LiteralValue::String(member_name)) =
                strip_parens_expr(index)
            else {
                return None;
            };
            Some((name.as_ref(), member_name.as_str()))
        }

        fn fast_symbolic_imported_generic_route(
            engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            expr: &TypeExpr,
            active_locals: &mut FxHashSet<String>,
        ) -> bool {
            match strip_parens_expr(expr) {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => match engine.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref()) {
                    BareRefOrigin::Imported => !type_arguments.is_empty(),
                    BareRefOrigin::Local if type_arguments.is_empty() => {
                        let Some(root_identity) =
                            engine.root_identity_in_scope(scope_canonical_id, name.as_ref())
                        else {
                            return false;
                        };
                        let active_key = format!(
                            "{}::{}",
                            root_identity.canonical_id, root_identity.symbol_name
                        );
                        if !active_locals.insert(active_key.clone()) {
                            return false;
                        }
                        let preserve = engine
                            .prepared_type_decl(
                                &root_identity.canonical_id,
                                &root_identity.symbol_name,
                            )
                            .is_some_and(|prepared| {
                                fast_symbolic_imported_generic_route(
                                    engine,
                                    root_identity.canonical_id.as_str(),
                                    &prepared.body,
                                    active_locals,
                                )
                            });
                        active_locals.remove(&active_key);
                        preserve
                    }
                    _ => false,
                },
                TypeExpr::IndexedAccess { object, .. }
                | TypeExpr::Array {
                    element: object, ..
                }
                | TypeExpr::KeyOf(object)
                | TypeExpr::Rest(object)
                | TypeExpr::Parenthesized(object) => fast_symbolic_imported_generic_route(
                    engine,
                    scope_canonical_id,
                    object,
                    active_locals,
                ),
                TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                    fast_symbolic_imported_generic_route(
                        engine,
                        scope_canonical_id,
                        &element.ty,
                        active_locals,
                    )
                }),
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
                    members.iter().any(|member| {
                        fast_symbolic_imported_generic_route(
                            engine,
                            scope_canonical_id,
                            member,
                            active_locals,
                        )
                    })
                }
                _ => false,
            }
        }

        fn collapse_same_file_imported_alias_chain(
            engine: &mut ComponentMetaQueryEngine<'_>,
            canonical_id: &str,
            expr: &TypeExpr,
        ) -> TypeExpr {
            let mut current = expr.clone();
            let mut visited = FxHashSet::<String>::default();

            loop {
                let TypeExpr::Ref {
                    name,
                    type_arguments,
                } = strip_parens_expr(&current)
                else {
                    return current;
                };
                if !type_arguments.is_empty() || !visited.insert(name.to_string()) {
                    return current;
                }
                let Some(root_identity) =
                    engine.root_identity_in_scope(canonical_id, name.as_ref())
                else {
                    return current;
                };
                if root_identity.canonical_id != canonical_id {
                    return current;
                }
                let Some(prepared) = engine
                    .prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)
                else {
                    return current;
                };
                current = prepared.body.clone();
            }
        }

        fn imported_value_route_arg(
            engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            expr: &TypeExpr,
        ) -> bool {
            match strip_parens_expr(expr) {
                TypeExpr::TypeOf(verter_type_expr::ValueRef { path }) => {
                    path.first().is_some_and(|root| {
                        engine.bare_ref_origin_in_scope(scope_canonical_id, root)
                            == BareRefOrigin::Imported
                    })
                }
                TypeExpr::Parenthesized(inner) => {
                    imported_value_route_arg(engine, scope_canonical_id, inner)
                }
                _ => false,
            }
        }

        fn contains_direct_imported_utility_route(
            engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            expr: &TypeExpr,
        ) -> bool {
            fn imported_route_arg(
                engine: &mut ComponentMetaQueryEngine<'_>,
                scope_canonical_id: &str,
                expr: &TypeExpr,
            ) -> bool {
                match strip_parens_expr(expr) {
                    TypeExpr::Ref {
                        name,
                        type_arguments,
                    } => {
                        (type_arguments.is_empty()
                            && engine.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref())
                                == BareRefOrigin::Imported)
                            || imported_value_route_arg(engine, scope_canonical_id, expr)
                            || contains_direct_imported_utility_route(
                                engine,
                                scope_canonical_id,
                                expr,
                            )
                    }
                    TypeExpr::IndexedAccess { object, .. } => {
                        imported_route_arg(engine, scope_canonical_id, object)
                    }
                    TypeExpr::TypeOf(_) => {
                        imported_value_route_arg(engine, scope_canonical_id, expr)
                    }
                    TypeExpr::Parenthesized(inner) => {
                        imported_route_arg(engine, scope_canonical_id, inner)
                    }
                    _ => contains_direct_imported_utility_route(engine, scope_canonical_id, expr),
                }
            }

            match strip_parens_expr(expr) {
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => members.iter().any(
                    |member| {
                        contains_direct_imported_utility_route(engine, scope_canonical_id, member)
                    },
                ),
                TypeExpr::Array { element, .. }
                | TypeExpr::Rest(element)
                | TypeExpr::Parenthesized(element) => {
                    contains_direct_imported_utility_route(engine, scope_canonical_id, element)
                }
                TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                    contains_direct_imported_utility_route(
                        engine,
                        scope_canonical_id,
                        &element.ty,
                    )
                }),
                TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                    verter_type_expr::ObjectMember::Property(property) => {
                        contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            &property.ty,
                        )
                    }
                    verter_type_expr::ObjectMember::Method(method) => {
                        method.function.parameters.iter().any(|parameter| {
                            contains_direct_imported_utility_route(
                                engine,
                                scope_canonical_id,
                                &parameter.ty,
                            )
                        }) || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| {
                                contains_direct_imported_utility_route(
                                    engine,
                                    scope_canonical_id,
                                    return_type,
                                )
                            })
                    }
                    verter_type_expr::ObjectMember::CallSignature(function)
                    | verter_type_expr::ObjectMember::ConstructSignature(
                        function,
                    ) => function.parameters.iter().any(|parameter| {
                        contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            &parameter.ty,
                        )
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            return_type,
                        )
                    }),
                    verter_type_expr::ObjectMember::IndexSignature(index) => {
                        contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            &index.key_type,
                        ) || contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            &index.value_type,
                        )
                    }
                }),
                TypeExpr::Function(function) => function.parameters.iter().any(|parameter| {
                    contains_direct_imported_utility_route(
                        engine,
                        scope_canonical_id,
                        &parameter.ty,
                    )
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    contains_direct_imported_utility_route(engine, scope_canonical_id, return_type)
                }),
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } if !type_arguments.is_empty()
                    && verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                        name.as_ref(),
                    )
                    .is_some() =>
                {
                    type_arguments.iter().any(|argument| {
                        imported_route_arg(engine, scope_canonical_id, argument)
                    })
                }
                _ => false,
            }
        }

        if contains_direct_imported_utility_route(self, scope_canonical_id, expr) {
            return Some(FastShallowFieldExpr {
                expr: expr.clone(),
                exactness: FastShallowFieldExprExactness::Symbolic,
            });
        }

        // Note: a prior `fast_symbolic_imported_bare_ref_route` branch lived
        // here and short-circuited bare imported Refs whose name ended with
        // "Props". That predicate was a nominal heuristic — the Typed-IR-Only
        // Resolver Rule (CLAUDE.md §3.4) bans suffix-based role classification.
        // The standard projector path below handles bare imported Refs
        // correctly without the shortcut.

        if let TypeExpr::Ref {
            name,
            type_arguments,
        } = strip_parens_expr(expr)
        {
            if !type_arguments.is_empty()
                && self.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref())
                    == BareRefOrigin::Imported
            {
                let _ = self.root_identity_in_scope(scope_canonical_id, name.as_ref())?;
                return Some(FastShallowFieldExpr {
                    expr: expr.clone(),
                    exactness: FastShallowFieldExprExactness::Symbolic,
                });
            }
        }

        if let Some((root_name, member_name)) = single_member_import_root(expr) {
            if self.bare_ref_origin_in_scope(scope_canonical_id, root_name)
                == BareRefOrigin::Imported
            {
                let root_identity = self.root_identity_in_scope(scope_canonical_id, root_name)?;
                if is_package_canonical(self.ctx, &root_identity.canonical_id) {
                    return Some(FastShallowFieldExpr {
                        expr: expr.clone(),
                        exactness: FastShallowFieldExprExactness::Symbolic,
                    });
                }
                let prepared = self
                    .prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)?;
                let member = prepared.member(member_name)?;
                if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
                    return None;
                }
                let collapsed = collapse_same_file_imported_alias_chain(
                    self,
                    &root_identity.canonical_id,
                    &member.ty,
                );
                return Some(FastShallowFieldExpr {
                    expr: collapsed,
                    exactness: FastShallowFieldExprExactness::Concrete,
                });
            }
        }

        if let Some(expanded) = self.try_fast_expand_shallow_alias_body(scope_canonical_id, expr) {
            // Classify the unwrapped alias body via the shared
            // [`crate::meta_resolve::exactness::classify_type_expr`]
            // predicate. `type MyStr = string` then publishes
            // `Concrete` instead of `Symbolic`; nested or open shapes
            // (Ref, IndexedAccess, KeyOf, Conditional, TypeParameter,
            // Mapped, …) stay symbolic. The slot-binding synthesis
            // path uses the graph-native sibling
            // [`crate::meta_resolve::exactness::classify_node`] so
            // both surfaces share identical alias-unwrap + closed-object
            // semantics.
            let exactness = match crate::meta_resolve::exactness::classify_type_expr(&expanded) {
                verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete => {
                    FastShallowFieldExprExactness::Concrete
                }
                _ => FastShallowFieldExprExactness::Symbolic,
            };
            return Some(FastShallowFieldExpr {
                expr: expanded,
                exactness,
            });
        }

        let mut active_locals = FxHashSet::default();
        fast_symbolic_imported_generic_route(self, scope_canonical_id, expr, &mut active_locals)
            .then(|| FastShallowFieldExpr {
                expr: expr.clone(),
                exactness: FastShallowFieldExprExactness::Symbolic,
            })
    }

    fn try_fast_expand_shallow_alias_body(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;

        let TypeExpr::Ref {
            name,
            type_arguments,
        } = strip_parens_expr(expr)
        else {
            return None;
        };
        if !type_arguments.is_empty() {
            return None;
        }
        if !matches!(
            self.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref()),
            BareRefOrigin::Imported | BareRefOrigin::Local
        ) {
            return None;
        }
        let root_identity = self.root_identity_in_scope(scope_canonical_id, name.as_ref())?;
        if is_package_canonical(self.ctx, &root_identity.canonical_id) {
            return None;
        }
        let prepared =
            self.prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)?;
        if !prepared.type_parameters.is_empty() {
            return None;
        }
        let mut active_aliases = FxHashSet::default();
        let expanded = self.rewrite_fast_shallow_alias_body(
            root_identity.canonical_id.as_str(),
            &prepared.body,
            &mut active_aliases,
        )?;
        (expanded != *expr).then_some(expanded)
    }

    fn rewrite_fast_shallow_alias_body(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
        active_aliases: &mut FxHashSet<String>,
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;

        match expr {
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Unknown { .. }
            | TypeExpr::TypeParameter(_) => Some(expr.clone()),
            TypeExpr::Parenthesized(inner) => Some(TypeExpr::Parenthesized(std::sync::Arc::new(
                self.rewrite_fast_shallow_alias_body(scope_canonical_id, inner, active_aliases)?,
            ))),
            TypeExpr::KeyOf(inner) => Some(TypeExpr::KeyOf(std::sync::Arc::new(
                self.rewrite_fast_shallow_alias_body(scope_canonical_id, inner, active_aliases)?,
            ))),
            TypeExpr::Rest(inner) => Some(TypeExpr::Rest(std::sync::Arc::new(
                self.rewrite_fast_shallow_alias_body(scope_canonical_id, inner, active_aliases)?,
            ))),
            TypeExpr::Array { element, readonly } => Some(TypeExpr::Array {
                element: std::sync::Arc::new(self.rewrite_fast_shallow_alias_body(
                    scope_canonical_id,
                    element,
                    active_aliases,
                )?),
                readonly: *readonly,
            }),
            TypeExpr::Tuple { elements, readonly } => {
                let elements = elements
                    .iter()
                    .map(|element| {
                        Some(verter_type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: self.rewrite_fast_shallow_alias_body(
                                scope_canonical_id,
                                &element.ty,
                                active_aliases,
                            )?,
                            optional: element.optional,
                            rest: element.rest,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(TypeExpr::Tuple {
                    elements: std::sync::Arc::from(elements),
                    readonly: *readonly,
                })
            }
            TypeExpr::Union(members) => Some(TypeExpr::Union(std::sync::Arc::from(
                members
                    .iter()
                    .map(|member| {
                        self.rewrite_fast_shallow_alias_body(
                            scope_canonical_id,
                            member,
                            active_aliases,
                        )
                    })
                    .collect::<Option<Vec<_>>>()?,
            ))),
            TypeExpr::Intersection(members) => Some(TypeExpr::Intersection(std::sync::Arc::from(
                members
                    .iter()
                    .map(|member| {
                        self.rewrite_fast_shallow_alias_body(
                            scope_canonical_id,
                            member,
                            active_aliases,
                        )
                    })
                    .collect::<Option<Vec<_>>>()?,
            ))),
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => Some(TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: std::sync::Arc::from(
                    expressions
                        .iter()
                        .map(|expression| {
                            self.rewrite_fast_shallow_alias_body(
                                scope_canonical_id,
                                expression,
                                active_aliases,
                            )
                        })
                        .collect::<Option<Vec<_>>>()?,
                ),
            }),
            TypeExpr::Function(function) => Some(TypeExpr::Function(std::sync::Arc::new(
                verter_type_expr::FunctionExpr {
                    parameters: function
                        .parameters
                        .iter()
                        .map(|parameter| {
                            Some(verter_type_expr::FunctionParam {
                                name: parameter.name.clone(),
                                ty: self.rewrite_fast_shallow_alias_body(
                                    scope_canonical_id,
                                    &parameter.ty,
                                    active_aliases,
                                )?,
                                optional: parameter.optional,
                                rest: parameter.rest,
                            })
                        })
                        .collect::<Option<Vec<_>>>()?,
                    return_type: match function.return_type.as_deref() {
                        Some(return_type) => {
                            Some(std::sync::Arc::new(self.rewrite_fast_shallow_alias_body(
                                scope_canonical_id,
                                return_type,
                                active_aliases,
                            )?))
                        }
                        None => None,
                    },
                    type_parameters: function
                        .type_parameters
                        .iter()
                        .map(|parameter| {
                            Some(verter_type_expr::TypeParam {
                                name: parameter.name.clone(),
                                constraint: match parameter.constraint.as_deref() {
                                    Some(constraint) => Some(std::sync::Arc::new(
                                        self.rewrite_fast_shallow_alias_body(
                                            scope_canonical_id,
                                            constraint,
                                            active_aliases,
                                        )?,
                                    )),
                                    None => None,
                                },
                                default: match parameter.default.as_deref() {
                                    Some(default) => Some(std::sync::Arc::new(
                                        self.rewrite_fast_shallow_alias_body(
                                            scope_canonical_id,
                                            default,
                                            active_aliases,
                                        )?,
                                    )),
                                    None => None,
                                },
                            })
                        })
                        .collect::<Option<Vec<_>>>()?,
                },
            ))),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if !type_arguments.is_empty() {
                    return None;
                }
                match self.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref()) {
                    BareRefOrigin::Imported | BareRefOrigin::Local => {
                        let root_identity =
                            self.root_identity_in_scope(scope_canonical_id, name.as_ref())?;
                        if is_package_canonical(self.ctx, &root_identity.canonical_id) {
                            return Some(expr.clone());
                        }
                        let active_key = format!(
                            "{}::{}",
                            root_identity.canonical_id, root_identity.symbol_name
                        );
                        if !active_aliases.insert(active_key.clone()) {
                            return None;
                        }
                        let rewritten = self
                            .prepared_type_decl(
                                &root_identity.canonical_id,
                                &root_identity.symbol_name,
                            )
                            .and_then(|prepared| {
                                prepared.type_parameters.is_empty().then_some(prepared)
                            })
                            .and_then(|prepared| {
                                self.rewrite_fast_shallow_alias_body(
                                    root_identity.canonical_id.as_str(),
                                    &prepared.body,
                                    active_aliases,
                                )
                            });
                        active_aliases.remove(&active_key);
                        rewritten
                    }
                    _ => None,
                }
            }
            TypeExpr::Object(object) => object
                .properties
                .is_empty()
                .then(|| TypeExpr::Object(object.clone())),
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. } => None,
        }
    }

    fn bare_ref_origin_in_scope(
        &mut self,
        scope_canonical_id: &str,
        name: &str,
    ) -> verter_semantic::analysis::type_solver::host::BareRefOrigin {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;
        let payload = self.scope_payload_for_scope(scope_canonical_id);
        if let Some(payload) = payload.as_deref() {
            if payload.import_bindings.contains_key(name) {
                return BareRefOrigin::Imported;
            }
            if payload.scope_type_bindings.contains_key(name)
                || payload.scope_type_names.contains(name)
                || payload.scope_value_names.contains(name)
            {
                return BareRefOrigin::Local;
            }
        }
        BareRefOrigin::Unknown
    }

    fn root_identity_in_scope(
        &mut self,
        scope_canonical_id: &str,
        name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity> {
        let payload = self.scope_payload_for_scope(scope_canonical_id);
        crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self.ctx,
            scope_canonical_id,
            payload.as_deref(),
            name,
        )
    }

    /// Walk an Object's properties/methods and resolve any
    /// `TypeExpr::Ref` leaves (inside property types, function return
    /// types, array elements, union/intersection arms) to their
    /// dispatch-projected surface (replacement for the
    /// retired `type_eval_build::deep_resolve_slot_function_refs`).
    ///
    /// Non-Object inputs are returned verbatim. Ref resolution routes
    /// through [`Self::project_expr_surface_expr`] so it uses the same
    /// dispatch memo (`SemanticGraphStore`) + `instantiate_active`
    /// guards as the rest of the component-meta pipeline,
    /// guaranteeing one cache entry per `(scope, expr)` regardless of
    /// entry point.
    pub fn deep_resolve_slot_function_refs(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> TypeExpr {
        use verter_type_expr::{ObjectMember, ObjectProperty};

        match expr {
            TypeExpr::Object(obj) => {
                let properties: Vec<ObjectMember> = obj
                    .properties
                    .iter()
                    .map(|member| match member {
                        ObjectMember::Property(p) => ObjectMember::Property(ObjectProperty {
                            name: p.name.clone(),
                            ty: self.deep_resolve_type_refs(scope_canonical_id, &p.ty),
                            optional: p.optional,
                            readonly: p.readonly,
                        }),
                        ObjectMember::Method(m) => {
                            ObjectMember::Method(verter_type_expr::MethodSignature {
                                name: m.name.clone(),
                                function: self
                                    .deep_resolve_fn_refs(scope_canonical_id, &m.function),
                                optional: m.optional,
                            })
                        }
                        other => other.clone(),
                    })
                    .collect();
                TypeExpr::Object(std::sync::Arc::new(verter_type_expr::ObjectExpr {
                    properties,
                }))
            }
            // Path C C11-residual-A: walk compound shapes so
            // `defineSlots<TabsSlots<T>>` patterns with
            // `{ leading?, content? } & DynamicSlots<...>` bodies still
            // resolve their explicit Object arm's `SlotProps<T>` members
            // into Function signatures for slot-binding extraction.
            TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(std::sync::Arc::new(
                self.deep_resolve_slot_function_refs(scope_canonical_id, inner),
            )),
            TypeExpr::Intersection(parts) => TypeExpr::Intersection(std::sync::Arc::from(
                parts
                    .iter()
                    .map(|p| self.deep_resolve_slot_function_refs(scope_canonical_id, p))
                    .collect::<Vec<_>>(),
            )),
            TypeExpr::Union(variants) => TypeExpr::Union(std::sync::Arc::from(
                variants
                    .iter()
                    .map(|v| self.deep_resolve_slot_function_refs(scope_canonical_id, v))
                    .collect::<Vec<_>>(),
            )),
            _ => expr.clone(),
        }
    }

    fn deep_resolve_type_refs(&mut self, scope_canonical_id: &str, expr: &TypeExpr) -> TypeExpr {
        match expr {
            TypeExpr::Ref { .. } => {
                // Block 6.i leak-close-2 — this callsite is on the
                // leak path; it is DELETED in leak-close-3 (Q7 Claude
                // architecture) together with the whole `deep_resolve_*`
                // chain. For this commit, pass the legacy Expanded
                // base + Expanded terminal under Published demand so
                // behaviour is unchanged while the helper signature
                // becomes mode-explicit.
                crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
                    self,
                    scope_canonical_id,
                    expr,
                    crate::semantic_query::ProjectionMode::Expanded,
                    crate::semantic_query::ProjectionMode::Expanded,
                    crate::semantic_query::ReductionDemand::Published,
                )
                .unwrap_or_else(|| expr.clone())
            }
            TypeExpr::Function(func) => TypeExpr::Function(std::sync::Arc::new(
                self.deep_resolve_fn_refs(scope_canonical_id, func),
            )),
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: std::sync::Arc::new(
                    self.deep_resolve_type_refs(scope_canonical_id, element),
                ),
                readonly: *readonly,
            },
            TypeExpr::Union(variants) => TypeExpr::Union(std::sync::Arc::from(
                variants
                    .iter()
                    .map(|v| self.deep_resolve_type_refs(scope_canonical_id, v))
                    .collect::<Vec<_>>(),
            )),
            TypeExpr::Intersection(parts) => TypeExpr::Intersection(std::sync::Arc::from(
                parts
                    .iter()
                    .map(|p| self.deep_resolve_type_refs(scope_canonical_id, p))
                    .collect::<Vec<_>>(),
            )),
            // Operator shells preserve their symbolic identity
            // through deep-resolve. Per the type-resolution
            // architecture rule "type navigation must stay narrower
            // than expansion": projecting an `IndexedAccess` /
            // `Conditional` / `Mapped` / `KeyOf` / `TypeOf` at a
            // slot-binding boundary materialises the helper away
            // (e.g. `Button['ui']` -> `Object<base, label>`) and
            // erases the source-text shape downstream consumers
            // re-resolve from. The graph-native synthesizer handles
            // these shapes via empty-path Shallow + the standard
            // Conditional / IndexedAccess / Mapped dispatch arms;
            // deep-resolve does not need to pre-materialise them.
            TypeExpr::Conditional { .. }
            | TypeExpr::IndexedAccess { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::KeyOf(_)
            | TypeExpr::TypeOf(_) => expr.clone(),
            _ => expr.clone(),
        }
    }

    fn deep_resolve_fn_refs(
        &mut self,
        scope_canonical_id: &str,
        func: &verter_type_expr::FunctionExpr,
    ) -> verter_type_expr::FunctionExpr {
        verter_type_expr::FunctionExpr {
            parameters: func
                .parameters
                .iter()
                .map(|p| verter_type_expr::FunctionParam {
                    name: p.name.clone(),
                    ty: self.deep_resolve_type_refs(scope_canonical_id, &p.ty),
                    optional: p.optional,
                    rest: p.rest,
                })
                .collect(),
            return_type: func
                .return_type
                .as_ref()
                .map(|rt| std::sync::Arc::new(self.deep_resolve_type_refs(scope_canonical_id, rt))),
            type_parameters: func.type_parameters.clone(),
        }
    }
}
