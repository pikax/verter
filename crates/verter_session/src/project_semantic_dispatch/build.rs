//! Dispatch-layer builders (plan §3 Change Split). Every semantic query
//! variant that produces a new [`SemanticNodeId`] does so through one of
//! the `build_*` methods collected here. Kept on `ProjectSemanticDispatch`
//! via an `impl` block so the inner helpers share private accessors
//! (`graph`, `opaque`, `dep_signature_for`, etc.) without widening their
//! visibility beyond `pub(super)`.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_expr::{
    FunctionExpr, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
};
use verter_semantic::analysis::type_solver::host::{ResolvedRootIdentity, UtilitySource};
use verter_semantic::analysis::type_solver::PreparedTypeDecl;

use super::walk::PathWalker;
use super::{
    empty_signature, utility_param_names, DispatchHost, ProjectSemanticDispatch,
    SessionDispatchHost, ShallowRelation,
};
use crate::semantic_query::{
    BranchSelection, DeclIdentity, DepSignature, HostResolvedNamedTypeKey, IndexSignature,
    LiteralValue, NodeScopeId, OriginEdgeKind, OriginMeta, PathSegment, PrimitiveKind,
    ProjectionMode, QueryError, QueryResult, ResolveDeclKey, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey, SurfaceMember, SurfaceView, ValueRootKey,
};

// Phase 1B per-call counter (test-only). Incremented every time
// `find_longest_warm_prefix` returns `Some(_)` during a
// `ProjectSemanticDispatch::build_project_path` invocation. Used by
// `project_path_prefix_peek_short_circuits_sibling_walk` to discriminate
// pre-fix (counter never increments — peek helper not yet wired) vs
// post-fix (counter delta is exactly 1 across a sibling-prefix replay).
//
// Diagnostic-only — never read on the hot path. Tests using this
// counter MUST reset it before measuring (`with(|c| *c.borrow_mut() = 0)`)
// because the thread-local persists across tests in the same process.
#[cfg(test)]
thread_local! {
    pub(super) static PREFIX_PEEK_HITS: std::cell::RefCell<u32> = const { std::cell::RefCell::new(0) };
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Resolve a top-level declaration lookup via the host's shallow state.
    ///
    /// Path C C16: the retired `DeclAnchor` variant is no longer interned.
    /// Declaration identity is carried by the `Instantiate` key's
    /// `DeclIdentity` field directly. This builder validates that the
    /// name exists in the shallow state, records the file scope in the
    /// sidecar, and returns an `Opaque(Miss)` placeholder node — the
    /// actual identity is carried by the caller via the key.
    pub(super) fn build_resolve_decl(
        &self,
        key: &ResolveDeclKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let shallow = match self
            .host
            .shallow_file_state(key.scope.canonical_id.as_ref())
        {
            Some(state) => state,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()),
        };

        let has_type_symbol = shallow.symbol(key.name.as_ref()).is_some();
        let has_value_symbol = shallow.value_symbol(key.name.as_ref()).is_some();
        let has_export = shallow.exports.contains_key(key.name.as_ref());
        let has_import_local = shallow.import_targets.contains_key(key.name.as_ref());

        if !(has_type_symbol || has_value_symbol || has_export || has_import_local) {
            return (QueryResult::Error(QueryError::Miss), empty_signature());
        }

        // Record the declaration's origin scope in the sidecar (plan §7.10
        // + C1) so dispatch builders reached from this anchor can route
        // per-base-scope lookups through the scope's declaration-scope
        // payload.
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&key.scope.canonical_id),
            whole_hash: shallow.whole_hash,
            local_scope: key.scope.local_scope,
        };
        let signature = self.dep_signature_for(&key.scope.canonical_id, shallow.whole_hash);
        // C16: DeclAnchor retired. Return a DeclPlaceholder that carries
        // enough identity for callers to construct Instantiate keys.
        let node_id = self.graph().intern_node_with_scope(
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id: Arc::clone(&key.scope.canonical_id),
                name: Arc::clone(&key.name),
                whole_hash: shallow.whole_hash,
            }),
            scope,
        );
        (QueryResult::Value(node_id), signature)
    }

    /// `typeof`-rooted declaration lookup. Shape mirrors [`Self::build_resolve_decl`]
    /// but routes through the shallow value-symbol space so the result is
    /// keyed by the value binding's identity.
    pub(super) fn build_typeof(
        &self,
        value_root: &ValueRootKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let shallow = match self
            .host
            .shallow_file_state(value_root.scope.canonical_id.as_ref())
        {
            Some(state) => state,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()),
        };

        let has_value = shallow.value_symbol(value_root.name.as_ref()).is_some();
        let has_import_local = shallow
            .import_targets
            .contains_key(value_root.name.as_ref());
        let has_type_symbol = shallow.symbol(value_root.name.as_ref()).is_some();
        // Namespace-qualified root: `Ns.Member` where `Ns` is an import
        // alias (`import * as Ns from './m'`). The shallow state indexes
        // only the top-level alias; the dotted name itself never appears
        // as a literal symbol. Defer resolution to `resolve_bare_name_in_scope`,
        // which handles the namespace-member case via
        // `resolve_namespace_member_from_facts`.
        let has_namespace_prefix = value_root
            .name
            .split_once('.')
            .is_some_and(|(prefix, _)| shallow.import_targets.contains_key(prefix));

        if !(has_value || has_import_local || has_type_symbol || has_namespace_prefix) {
            return (QueryResult::Error(QueryError::Miss), empty_signature());
        }

        // Same scope-recording rule as `build_resolve_decl` — the value
        // binding's origin scope is the owning canonical so dispatch
        // builders downstream can reach the correct declaration file.
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&value_root.scope.canonical_id),
            whole_hash: shallow.whole_hash,
            local_scope: value_root.scope.local_scope,
        };
        let scope_payload = self
            .host
            .prepared_decl_bundle(value_root.scope.canonical_id.as_ref())
            .map(|bundle| {
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                )
            });
        let root_identity =
            match crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                self.host,
                value_root.scope.canonical_id.as_ref(),
                scope_payload.as_ref(),
                value_root.name.as_ref(),
            ) {
                Some(identity) => identity,
                None => return (QueryResult::Error(QueryError::Miss), empty_signature()),
            };
        let prepared = match self
            .host
            .prepared_value_decl(&root_identity.canonical_id, &root_identity.symbol_name)
            .or_else(|| {
                // Fallback to export-target walk when prepared cache misses
                // on the resolved root — matches the legacy
                // `SessionSolverHost::resolve_prepared_value_decl`.
                if root_identity.canonical_id.is_empty() {
                    return None;
                }
                let target = self.host.resolve_value_export_target(
                    &root_identity.canonical_id,
                    &root_identity.symbol_name,
                )?;
                if target.canonical_id == root_identity.canonical_id
                    && target.name == root_identity.symbol_name
                {
                    return None;
                }
                self.host
                    .prepared_value_decl(&target.canonical_id, &target.name)
            }) {
            Some(prepared) => prepared,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()),
        };
        let empty_env = FxHashMap::default();
        let mut substitutions = Vec::new();
        let node_id = if let Some(ty_ann) = prepared.type_annotation.as_ref() {
            self.shallow_lower_type_expr(
                ty_ann,
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &mut substitutions,
                crate::semantic_query::ProjectionMode::Expanded,
            )
        } else if let Some(shape) = prepared.object_shape.as_ref() {
            self.shallow_lower_type_expr(
                &TypeExpr::Object(Arc::new(shape.clone())),
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &mut substitutions,
                crate::semantic_query::ProjectionMode::Expanded,
            )
        } else if let Some(sig) = prepared.function_signature.as_ref() {
            let function_expr = FunctionExpr {
                parameters: sig.parameters.clone(),
                return_type: sig.return_type.clone().map(Arc::new),
                type_parameters: sig.type_parameters.clone(),
            };
            let object_expr = ObjectExpr {
                properties: vec![if prepared.kind
                    == verter_semantic::analysis::type_eval::ValueDeclKind::Class
                {
                    ObjectMember::ConstructSignature(function_expr)
                } else {
                    ObjectMember::CallSignature(function_expr)
                }],
            };
            self.shallow_lower_type_expr(
                &TypeExpr::Object(Arc::new(object_expr)),
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &mut substitutions,
                crate::semantic_query::ProjectionMode::Expanded,
            )
        } else if let Some(members) = prepared.enum_members.as_ref() {
            let object_expr = ObjectExpr {
                properties: members
                    .iter()
                    .map(|(name, ty)| {
                        ObjectMember::Property(ObjectProperty {
                            name: name.clone(),
                            ty: ty.clone(),
                            optional: false,
                            readonly: true,
                        })
                    })
                    .collect(),
            };
            self.shallow_lower_type_expr(
                &TypeExpr::Object(Arc::new(object_expr)),
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &mut substitutions,
                crate::semantic_query::ProjectionMode::Expanded,
            )
        } else {
            return (QueryResult::Error(QueryError::Miss), empty_signature());
        };
        let signature = self.dep_signature_for(&value_root.scope.canonical_id, shallow.whole_hash);
        (QueryResult::Value(node_id), signature)
    }

    /// Generic instantiation (plan §3 C1 + §2 lazy materialisation + §7.14).
    ///
    /// Path C C16: receives `DeclIdentity` directly from the
    /// `Instantiate` key instead of unwrapping a `DeclAnchor` node.
    /// Fetches the [`PreparedTypeDecl`] via [`DispatchHost`] and
    /// produces **one shell level** of the declaration's structural shape
    /// with `args` bound to the decl's type parameters.
    ///
    /// `body_mode` controls how the decl body and its argument
    /// expressions are lowered after substitution. Memo entries split
    /// per body_mode (see `family_and_slot` in
    /// [`semantic_query_memo`](crate::semantic_query_memo)) so a Navigate
    /// caller and an Expanded caller never collide on the same shell
    /// result. Member bodies are not recursively lowered — nested
    /// references emit `Opaque(Miss)` placeholders per the
    /// lazy-materialisation rule; deeper lowering is driven by
    /// `ProjectPath` sub-queries through the family memo.
    ///
    /// Origin edges emitted:
    /// - One [`OriginEdgeKind::Instantiate`] edge on the result, sourced
    ///   from `[base_placeholder, args...]`.
    /// - One [`OriginEdgeKind::SubstituteTypeParam`] edge per type-parameter
    ///   reference visited at the shell level, sourced from the bound arg.
    pub(super) fn build_instantiate(
        &self,
        identity: &DeclIdentity,
        args: &Arc<[SemanticNodeId]>,
        body_mode: crate::semantic_query::ProjectionMode,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let decl_canonical = &identity.canonical_id;
        let decl_name = &identity.decl_name;
        let decl_whole_hash = identity.whole_hash;

        // Intern a scope-carrying placeholder so DispatchHost methods
        // (utility_source, resolve_prepared_type_decl, etc.) can look
        // up the declaration scope via node_scope(base).
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(decl_canonical),
            whole_hash: decl_whole_hash,
            local_scope: None,
        };
        let base = self
            .graph()
            .intern_node_with_scope(SemanticNodeData::Opaque(QueryError::Miss), scope.clone());

        let adapter = SessionDispatchHost::new(self.host);

        // 2. Built-in utility dispatch (plan §3 C7 + §2 built-in utilities).
        // A utility name (Partial, Pick, ReturnType, etc.) that the user
        // has NOT shadowed routes through the utility-specific dispatch
        // path — producing the same shell structure + origin edges a
        // userland-equivalent alias would produce (userland-equivalence
        // rule). Shadowed names fall through to the ordinary
        // `resolve_prepared_type_decl` path.
        if matches!(
            adapter.utility_source(base, decl_name.as_ref()),
            UtilitySource::Builtin
        ) {
            return self.build_builtin_utility(base, decl_name.as_ref(), args);
        }

        // 3. Resolve prepared type decl via `DispatchHost` — the adapter
        // routes through the sidecar-recorded scope for `base`.
        let ri = ResolvedRootIdentity::new(decl_canonical.as_ref(), decl_name.as_ref());
        let prepared = match adapter.resolve_prepared_type_decl(base, &ri) {
            Some(p) => p,
            None => {
                return (
                    QueryResult::Value(self.opaque(QueryError::Miss)),
                    empty_signature(),
                )
            }
        };

        // 4. Bind type parameters to args (positional). When a
        // parameter has no explicit arg but carries a default
        // expression, lower the default in the decl's scope and bind
        // it — mirrors the solver's `resolve_type_parameters_in_body`
        // behaviour at solve.rs:2580 (plan §5.7 step 3).
        let scope_payload = self
            .host
            .prepared_decl_bundle(decl_canonical.as_ref())
            .map(|bundle| {
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                )
            });
        let mut env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        for (index, param) in prepared.type_parameters.iter().enumerate() {
            let arg_id = if let Some(explicit) = args.get(index).copied() {
                explicit
            } else if let Some(default) = param.default.as_deref() {
                self.shallow_lower_type_expr(
                    default,
                    &env,
                    &scope,
                    &prepared.name_resolution,
                    scope_payload.as_ref(),
                    &mut substitutions,
                    body_mode,
                )
            } else if body_mode == crate::semantic_query::ProjectionMode::Skeleton {
                // Plan §4.21 / R10-2 — Skeleton mode preserves open generics.
                // Bind unbound param to a TypeParam shell so body lowering
                // produces TypeParam graph nodes (instead of resolving
                // T-refs to Opaque(Miss)). The relation engine treats
                // TypeParam as deferred → Conditional branches stay live
                // → collect_ref_identities_node walks both → recursive refs
                // through nested mapped/template-literal/conditional
                // bodies become visible to the cycle BFS.
                let display_name: Arc<str> = Arc::from(param.name.as_str());
                let decl_identity = crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::clone(decl_canonical),
                    whole_hash: decl_whole_hash,
                    decl_name: Arc::clone(decl_name),
                };
                self.graph().intern_node_with_scope(
                    SemanticNodeData::TypeParam {
                        decl: decl_identity,
                        param_index: index as u16,
                        constraint: None,
                        default: None,
                        display_name,
                    },
                    scope.clone(),
                )
            } else {
                // Existing Navigate/Expanded behavior preserved: unbound
                // param means `Opaque(Miss)` propagates through the body.
                // Callers that genuinely need open-generic access must
                // explicitly request Skeleton mode.
                continue;
            };
            env.insert(param.name.clone(), arg_id);
        }

        // 5. Shallow-lower the body. Collects substitution facts for
        // origin-edge emission. `name_resolution` is the prepared
        // decl's map from bare names used inside its body to the
        // resolved declaration identities — the walker consults this
        // when it encounters `TypeExpr::Ref { name, args }` so member
        // bodies that reference other declarations produce proper
        // sub-Instantiate shells instead of opaque placeholders.
        //
        // Phase D recursive-ref guard: push `(decl_canonical, decl_name)`
        // onto the dispatcher's `instantiate_active` stack before body
        // lowering. A nested `TypeExpr::Ref` resolving back to the same
        // identity — e.g. `type TreeNode = { children: TreeNode[] }` —
        // sees the active entry in `shallow_lower_type_expr` and emits
        // `Opaque(RecursiveRef)` at the back-edge instead of recursing.
        // When the identity is already active (should never happen for
        // top-level `build_instantiate` calls, but safely handled),
        // short-circuit to `RecursiveRef` here too.
        let active_identity: super::InstantiateIdentity =
            (Arc::clone(decl_canonical), Arc::clone(decl_name));
        let pushed = self.push_instantiate_active(active_identity);
        if !pushed {
            return (
                QueryResult::Value(self.opaque(QueryError::RecursiveRef {
                    name: Arc::clone(decl_name),
                })),
                empty_signature(),
            );
        }
        let mut result = self.shallow_lower_type_expr(
            &prepared.body,
            &env,
            &scope,
            &prepared.name_resolution,
            scope_payload.as_ref(),
            &mut substitutions,
            body_mode,
        );
        result = self.backfill_member_index_surface(
            result,
            &prepared,
            &env,
            &scope,
            scope_payload.as_ref(),
            &mut substitutions,
        );
        self.pop_instantiate_active();

        // 6. Emit origin edges + build dep signature.
        self.graph().record_instantiate();
        let fence = self.dep_signature_for(decl_canonical, decl_whole_hash);

        // Instantiate edge: result <- [base, args...].
        let mut inst_sources: Vec<SemanticNodeId> = Vec::with_capacity(args.len() + 1);
        inst_sources.push(base);
        inst_sources.extend(args.iter().copied());
        self.graph().record_origin_edge(
            result,
            OriginEdgeKind::Instantiate,
            Arc::from(inst_sources.into_boxed_slice()),
            OriginMeta::None,
            Arc::clone(&fence),
        );

        // SubstituteTypeParam edges on the shell result: one per visited
        // substituted occurrence (plan §3 C1 — edges emitted at
        // substitution position; at shell level this aggregates on the
        // result node per plan §2 lazy block).
        for (param_name, arg_id) in substitutions {
            self.graph().record_origin_edge(
                result,
                OriginEdgeKind::SubstituteTypeParam,
                Arc::from(vec![arg_id].into_boxed_slice()),
                OriginMeta::SubstitutedParam(param_name),
                Arc::clone(&fence),
            );
        }

        (QueryResult::Value(result), fence)
    }

    pub(super) fn backfill_member_index_surface(
        &self,
        result: SemanticNodeId,
        prepared: &PreparedTypeDecl,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        scope_payload: Option<&crate::resolver_core::bare_name_resolve::DeclarationScopePayload>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
    ) -> SemanticNodeId {
        let Some(data) = self.graph().node_data(result) else {
            return result;
        };
        let SemanticNodeData::Object(surface) = data.as_ref() else {
            return result;
        };
        if prepared.member_index.is_empty() {
            return result;
        }

        let mut existing: FxHashSet<Arc<str>> = surface
            .members
            .iter()
            .map(|member| Arc::clone(&member.name))
            .collect();
        let mut added = prepared
            .member_index
            .iter()
            .filter(|(name, _)| !existing.contains(name.as_str()))
            .map(|(name, member)| {
                let value = self.shallow_lower_type_expr(
                    &member.ty,
                    env,
                    scope,
                    &prepared.name_resolution,
                    scope_payload,
                    substitutions,
                    crate::semantic_query::ProjectionMode::Expanded,
                );
                SurfaceMember {
                    name: Arc::from(name.as_str()),
                    value,
                    optional: member.optional,
                    readonly: member.readonly,
                    is_method: member.is_method,
                }
            })
            .collect::<Vec<_>>();
        if added.is_empty() {
            return result;
        }

        added.sort_unstable_by(|left, right| left.name.as_ref().cmp(right.name.as_ref()));
        let mut members = surface.members.iter().cloned().collect::<Vec<_>>();
        for member in &added {
            existing.insert(Arc::clone(&member.name));
        }
        members.extend(added);
        self.graph().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::clone(&surface.call_signatures),
                construct_signatures: Arc::clone(&surface.construct_signatures),
                index_signatures: Arc::clone(&surface.index_signatures),
                keyspace: surface.keyspace,
                has_index_signature: surface.has_index_signature,
            }),
            scope.clone(),
        )
    }

    /// Built-in utility dispatch (plan §3 C7 + §2 built-in utilities).
    ///
    /// Routes recognised utility names (`Partial`, `Required`, `Readonly`,
    /// `Record`, `NoInfer`, string intrinsics, etc.) through the same
    /// `SemanticQueryKey::{MappedType, ProjectMember, ProjectPath, Normalize}`
    /// dispatch as userland aliases. Userland equivalence rule: a userland
    /// alias `type MyPartial<T> = { [K in keyof T]?: T[K] }` and the
    /// built-in `Partial<T>` produce the same `SemanticNodeId` and the
    /// same origin-edge structure when they route through the same
    /// `MappedType` dispatch key.
    ///
    /// Utilities are classified into three groups by implementation shape:
    ///
    /// - **Mapper-based** (`Partial`, `Required`, `Readonly`, `Record`):
    ///   synthesise a `MapperKey` whose modifiers encode the utility
    ///   transformation and dispatch through `SemanticQueryKey::MappedType`.
    ///   The resulting node is shared with any userland mapped type that
    ///   happens to produce an equivalent `MapperKey` because the memo
    ///   dedups on the full key.
    /// - **Identity** (`NoInfer`): returns the first argument as an `Alias`
    ///   node, emitting `Instantiate` + `SubstituteTypeParam` +
    ///   `AliasResolve` edges.
    /// - **Opaque** (`Pick`, `Omit`, `Extract`, `Exclude`, `NonNullable`,
    ///   `ReturnType`, `Parameters`, `ConstructorParameters`,
    ///   `InstanceType`, `Awaited`, string intrinsics): return a shell
    ///   anchored to the utility + arg identity with `Instantiate` +
    ///   `SubstituteTypeParam` edges. The shell's body is lazy — callers
    ///   projecting into it follow the normal `ProjectPath` route which
    ///   terminates with `Miss` until a later track implements the full
    ///   shape. String intrinsics return the `String` primitive directly.
    ///
    /// Every utility path emits the `Instantiate` edge with sources
    /// `[base, args...]` and per-arg `SubstituteTypeParam` edges so the
    /// origin graph is walkable end-to-end.
    pub(super) fn build_builtin_utility(
        &self,
        base: SemanticNodeId,
        name: &str,
        args: &Arc<[SemanticNodeId]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        use crate::semantic_query::{MapperKey, OptionalityMod, ReadonlyMod};

        let graph = self.graph();
        let fence = self.project_generation_signature();
        self.graph().record_instantiate();

        // Look up the utility's real TS type-parameter names so
        // `SubstituteTypeParam` edges carry names identical to those
        // the userland-equivalent alias would emit. `Partial<T>` and
        // `type MyPartial<T> = ...` both produce
        // `SubstituteTypeParam("T", arg)` — a synthesised `"T0"`-style
        // name would break origin-walk equivalence.
        let param_names = utility_param_names(name);

        // Helper: emit the common `Instantiate` + per-arg
        // `SubstituteTypeParam` edges on a utility result node.
        let record_utility_edges = |result_id: SemanticNodeId| {
            let mut inst_sources: Vec<SemanticNodeId> = Vec::with_capacity(args.len() + 1);
            inst_sources.push(base);
            inst_sources.extend(args.iter().copied());
            graph.record_origin_edge(
                result_id,
                OriginEdgeKind::Instantiate,
                Arc::from(inst_sources.into_boxed_slice()),
                OriginMeta::None,
                Arc::clone(&fence),
            );
            for (idx, arg_id) in args.iter().enumerate() {
                // Use the utility's declared type-parameter name when
                // known; fall back to a positional label only for
                // unknown utilities (which return Opaque anyway).
                let param_name: Arc<str> = param_names
                    .get(idx)
                    .map(|n| Arc::<str>::from(*n))
                    .unwrap_or_else(|| Arc::<str>::from(format!("T{idx}")));
                graph.record_origin_edge(
                    result_id,
                    OriginEdgeKind::SubstituteTypeParam,
                    Arc::from(vec![*arg_id].into_boxed_slice()),
                    OriginMeta::SubstitutedParam(param_name),
                    Arc::clone(&fence),
                );
            }
        };

        // Mapper-based utilities route through `SemanticQueryKey::MappedType`.
        // The mapper's `value_expr` is a placeholder `T[K]` identity — the
        // real evaluator would substitute K across T's member types, but
        // the shell-level result (with per-member modifiers applied)
        // matches the userland mapped type's shell, which is what
        // equivalence tests assert.
        let mapper_for = |opt: OptionalityMod, ro: ReadonlyMod, source: SemanticNodeId| {
            let key_space = match self.execute(SemanticQueryKey::KeyOf { base: source }) {
                QueryResult::Value(id) => id,
                _ => self.opaque(QueryError::Miss),
            };
            // Value placeholder: the shell does not eagerly lower per-key
            // substitutions into the value expression. A caller that
            // projects `Partial<T>['x']` walks the ProjectPath into the
            // produced member (which has `value = Miss` under the lazy
            // rule) and the follow-up hop dispatches back through the
            // path walker.
            let value_expr = self.opaque(QueryError::Miss);
            // Path C C6a item 6: synthesise a TypeParam binder node
            // for the utility mapper's `K`. The synthetic binder
            // receives an ordinal from the per-dispatcher counter
            // and a `<utility-mapper>` decl_name sentinel so two
            // distinct utility invocations (Partial<X> and
            // Partial<Y>) get distinct binder identities.
            let parameter_node = self.graph().intern_node(SemanticNodeData::TypeParam {
                decl: crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::from("<utility>"),
                    whole_hash: crate::semantic_query::HashValue::default(),
                    decl_name: Arc::from("<utility-mapper>"),
                },
                param_index: self.next_mapped_binder_ordinal(),
                constraint: None,
                default: None,
                display_name: Arc::from("K"),
            });
            MapperKey {
                parameter_node,
                key_space,
                value_expr,
                optionality: opt,
                readonly: ro,
                name_remap: None,
                // Partial / Required / Readonly are the canonical
                // `{ [K in keyof T]: T[K] }` mappers — Path C C5
                // classifies them as `Identity` explicitly. The
                // placeholder `value_expr = Miss` is a shell marker
                // (the build path never reads it for Identity mappers;
                // it reads source member values directly), not a
                // runtime-discoverable `T[K]` shape.
                kind: crate::semantic_query::MapperKind::Identity,
            }
        };

        match name {
            // ---- Mapper-based utilities ----
            "Partial" if args.len() == 1 => {
                let source = args[0];
                let mapper = mapper_for(OptionalityMod::Add, ReadonlyMod::Keep, source);
                let result = match self.execute(SemanticQueryKey::MappedType { source, mapper }) {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "Required" if args.len() == 1 => {
                let source = args[0];
                let mapper = mapper_for(OptionalityMod::Remove, ReadonlyMod::Keep, source);
                let result = match self.execute(SemanticQueryKey::MappedType { source, mapper }) {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "Readonly" if args.len() == 1 => {
                let source = args[0];
                let mapper = mapper_for(OptionalityMod::Keep, ReadonlyMod::Add, source);
                let result = match self.execute(SemanticQueryKey::MappedType { source, mapper }) {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "Record" if args.len() == 2 => {
                // Record<K, V>: map K's key space to V.
                let key_arg = args[0];
                let value_arg = args[1];
                // Key space is K itself (usually a union of literals).
                // For equivalence with userland `{ [P in K]: V }`, both
                // paths set `key_space = K` and `value_expr = V`.
                // Path C C6a item 6: synthesise a TypeParam binder
                // node id for `P`.
                let parameter_node = self.graph().intern_node(SemanticNodeData::TypeParam {
                    decl: crate::semantic_query::DeclIdentity {
                        canonical_id: Arc::from("<utility>"),
                        whole_hash: crate::semantic_query::HashValue::default(),
                        decl_name: Arc::from("<utility-mapper>"),
                    },
                    param_index: self.next_mapped_binder_ordinal(),
                    constraint: None,
                    default: None,
                    display_name: Arc::from("P"),
                });
                let mapper = MapperKey {
                    parameter_node,
                    key_space: key_arg,
                    value_expr: value_arg,
                    optionality: OptionalityMod::Keep,
                    readonly: ReadonlyMod::Keep,
                    name_remap: None,
                    // `Record<K, V>` maps every key to the same `V`
                    // expression — a computed projection, not the
                    // identity `T[K]`. Path C C5 tags accordingly so
                    // `build_mapped_type` takes the substitute +
                    // evaluate path.
                    kind: crate::semantic_query::MapperKind::Computed,
                };
                // Source is K; `build_mapped_type` reads names from K's
                // keyspace branch when the source isn't an Object.
                let result = match self.execute(SemanticQueryKey::MappedType {
                    source: key_arg,
                    mapper,
                }) {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }

            // ---- Identity utility ----
            "NoInfer" if args.len() == 1 => {
                let source = args[0];
                let result = graph.intern_node(SemanticNodeData::Alias(source));
                graph.record_origin_edge(
                    result,
                    OriginEdgeKind::AliasResolve,
                    Arc::from(vec![source].into_boxed_slice()),
                    OriginMeta::None,
                    Arc::clone(&fence),
                );
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }

            // ---- String intrinsics ----
            // These always produce a string primitive. The actual
            // transformation (uppercase, lowercase, etc.) applies at the
            // literal-string level; with no literal-type support in the
            // semantic graph today the result is the `String` primitive.
            "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" if args.len() == 1 => {
                let result = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }

            // ---- Function-signature utilities ----
            // `ReturnType<F>` / `Parameters<F>` inspect call signatures;
            // `ConstructorParameters<C>` / `InstanceType<C>` inspect
            // construct signatures. Resolves when the argument is a
            // canonical `Function` node directly, or an `Object` surface
            // carrying exactly one call / construct signature and no
            // user-level members. Typical entry: `ReturnType<typeof fn>`
            // where `build_typeof` produced an Object with a single
            // lowered call signature, or `ReturnType<() => T>` where
            // lowering produced a `Function` node straight away.
            //
            // When the argument does not match either shape the branch
            // falls through to the opaque shell so downstream consumers
            // still see an `Instantiate` edge anchored to the utility
            // identity.
            "ReturnType" if args.len() == 1 => {
                if let Some(function_node) = self.resolve_call_signature_function(args[0]) {
                    if let Some(SemanticNodeData::Function { return_type, .. }) =
                        self.graph().node_data(function_node).as_deref()
                    {
                        let id = *return_type;
                        record_utility_edges(id);
                        return (QueryResult::Value(id), fence);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "Parameters" if args.len() == 1 => {
                if let Some(function_node) = self.resolve_call_signature_function(args[0]) {
                    if let Some(tuple_id) = self.intern_function_params_tuple(function_node) {
                        record_utility_edges(tuple_id);
                        return (QueryResult::Value(tuple_id), fence);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "ConstructorParameters" if args.len() == 1 => {
                if let Some(function_node) = self.resolve_construct_signature_function(args[0]) {
                    if let Some(tuple_id) = self.intern_function_params_tuple(function_node) {
                        record_utility_edges(tuple_id);
                        return (QueryResult::Value(tuple_id), fence);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "InstanceType" if args.len() == 1 => {
                if let Some(function_node) = self.resolve_construct_signature_function(args[0]) {
                    if let Some(SemanticNodeData::Function { return_type, .. }) =
                        self.graph().node_data(function_node).as_deref()
                    {
                        let id = *return_type;
                        record_utility_edges(id);
                        return (QueryResult::Value(id), fence);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }

            // ---- Object-filter utilities ----
            // `Pick<X, K>` produces an Object surface containing the
            // subset of `X`'s members whose names appear in `K`'s
            // enumerable key space. `Omit<X, K>` is the inverse —
            // members of `X` whose names are NOT in `K`. Both
            // implementations preserve the source's per-member
            // optional / readonly / is_method flags so downstream
            // path-walking lands on the same value SemanticNodeIds
            // a userland-equivalent definition would emit.
            //
            // When `K` cannot be enumerated (e.g. still a TypeParam
            // or deferred shell) OR `X` does not resolve to an
            // Object surface, the utility falls through to the
            // deferred shell so callers re-dispatch once the inputs
            // become enumerable.
            "Pick" if args.len() == 2 => {
                let source = args[0];
                let keys_arg = args[1];
                let pick_names = match self.key_names_from_keyspace_node(keys_arg) {
                    Some(names) => names,
                    None => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence);
                    }
                };
                let source_resolved = self.evaluate_deferred_semantic_node(source);
                let surface = match graph.node_data(source_resolved).as_deref() {
                    Some(SemanticNodeData::Object(view)) => view.clone(),
                    _ => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence);
                    }
                };
                let pick_set: FxHashSet<&str> = pick_names.iter().map(|s| s.as_ref()).collect();
                let picked: Vec<SurfaceMember> = surface
                    .members
                    .iter()
                    .filter(|m| pick_set.contains(m.name.as_ref()))
                    .cloned()
                    .collect();
                let result_surface = SurfaceView {
                    members: Arc::from(picked.into_boxed_slice()),
                    call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    construct_signatures: Arc::from(
                        Vec::<SemanticNodeId>::new().into_boxed_slice(),
                    ),
                    index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
                    keyspace: None,
                    has_index_signature: false,
                };
                let result = graph.intern_node(SemanticNodeData::Object(result_surface));
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "Omit" if args.len() == 2 => {
                let source = args[0];
                let keys_arg = args[1];
                let omit_names = match self.key_names_from_keyspace_node(keys_arg) {
                    Some(names) => names,
                    None => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence);
                    }
                };
                let source_resolved = self.evaluate_deferred_semantic_node(source);
                let surface = match graph.node_data(source_resolved).as_deref() {
                    Some(SemanticNodeData::Object(view)) => view.clone(),
                    _ => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence);
                    }
                };
                let omit_set: FxHashSet<&str> = omit_names.iter().map(|s| s.as_ref()).collect();
                let kept: Vec<SurfaceMember> = surface
                    .members
                    .iter()
                    .filter(|m| !omit_set.contains(m.name.as_ref()))
                    .cloned()
                    .collect();
                let result_surface = SurfaceView {
                    members: Arc::from(kept.into_boxed_slice()),
                    // Omit preserves source signatures (TS semantics):
                    // `Omit<T, K>` only filters property names, leaving
                    // call/construct/index signatures intact.
                    call_signatures: Arc::clone(&surface.call_signatures),
                    construct_signatures: Arc::clone(&surface.construct_signatures),
                    index_signatures: Arc::clone(&surface.index_signatures),
                    keyspace: surface.keyspace,
                    has_index_signature: surface.has_index_signature,
                };
                let result = graph.intern_node(SemanticNodeData::Object(result_surface));
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }

            // ---- Deferred utilities ----
            // Extract/Exclude/NonNullable require union-filter
            // semantics; Awaited requires recursive promise unwrapping.
            // Each emits an `Opaque(Miss)` shell anchored to the
            // instantiate identity so the origin walk remains coherent;
            // full implementation falls out of the path-precise
            // projection upgrades that land alongside the projection-
            // authority cutover (D3) and after.
            _ => {
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
        }
    }

    /// Resolve `node` to a `SemanticNodeData::Function` node via call
    /// signatures. Unwraps a canonical `Function` node directly, an
    /// `Object` surface with exactly one call signature and no user
    /// members / construct signatures, or an `Alias` chain (cycle
    /// guarded).
    ///
    /// Returns `None` when the shape is a plain object, primitive,
    /// opaque, or carries multiple overloads — callers fall through to
    /// the utility's `Opaque(Miss)` shell.
    fn resolve_call_signature_function(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        self.resolve_call_signature_function_inner(node, &mut visited)
    }

    fn resolve_call_signature_function_inner(
        &self,
        node: SemanticNodeId,
        visited: &mut FxHashSet<SemanticNodeId>,
    ) -> Option<SemanticNodeId> {
        if !visited.insert(node) {
            return None;
        }
        let data = self.graph().node_data(node)?;
        match &*data {
            SemanticNodeData::Function { .. } => Some(node),
            SemanticNodeData::Alias(target) => {
                self.resolve_call_signature_function_inner(*target, visited)
            }
            SemanticNodeData::Object(surface)
                if surface.members.is_empty()
                    && surface.construct_signatures.is_empty()
                    && surface.call_signatures.len() == 1 =>
            {
                self.resolve_call_signature_function_inner(surface.call_signatures[0], visited)
            }
            _ => None,
        }
    }

    /// Mirror of [`Self::resolve_call_signature_function`] for construct
    /// signatures (`ConstructorParameters`, `InstanceType`). Construct
    /// signatures are lowered as `Function` nodes too — distinguished
    /// only by the surface bucket they live in.
    fn resolve_construct_signature_function(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        self.resolve_construct_signature_function_inner(node, &mut visited)
    }

    fn resolve_construct_signature_function_inner(
        &self,
        node: SemanticNodeId,
        visited: &mut FxHashSet<SemanticNodeId>,
    ) -> Option<SemanticNodeId> {
        if !visited.insert(node) {
            return None;
        }
        let data = self.graph().node_data(node)?;
        match &*data {
            SemanticNodeData::Alias(target) => {
                self.resolve_construct_signature_function_inner(*target, visited)
            }
            SemanticNodeData::Object(surface)
                if surface.members.is_empty()
                    && surface.call_signatures.is_empty()
                    && surface.construct_signatures.len() == 1 =>
            {
                self.resolve_call_signature_function_inner(surface.construct_signatures[0], visited)
            }
            _ => None,
        }
    }

    /// Build a tuple node whose elements are the function's parameter
    /// types — the surface shape of `Parameters<F>` /
    /// `ConstructorParameters<F>`. Labels carry over from the parameter
    /// names (TS reflects them in hover); optional / rest flags track
    /// the original signature. `function_node` must be a
    /// `SemanticNodeData::Function`; returns `None` otherwise.
    fn intern_function_params_tuple(
        &self,
        function_node: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        use crate::semantic_query::TupleElement;

        let data = self.graph().node_data(function_node)?;
        let SemanticNodeData::Function { params, .. } = &*data else {
            return None;
        };
        let elements: Arc<[TupleElement]> = params
            .iter()
            .map(|param| TupleElement {
                label: param.name.as_ref().map(Arc::clone),
                value: param.ty,
                optional: param.optional,
                rest: param.rest,
            })
            .collect::<Vec<_>>()
            .into();
        Some(self.graph().intern_node(SemanticNodeData::Tuple {
            elements,
            readonly: false,
        }))
    }

    // `resolve_decl_anchor` retired in Path C C16. Declaration identity
    // is now carried directly by `SemanticQueryKey::Instantiate.base`
    // (`DeclIdentity`), so there is no arena node to unwrap.

    /// Path-precise projection (plan §3 C3). Walks each [`PathSegment`]
    /// from `base` via a fresh [`PathWalker`] that dispatches per-hop on
    /// every shell variant (`Object`, `Union`, `Intersection`,
    /// `Conditional`, `Alias`) and emits per-segment origin edges
    /// (`ProjectMember` / `ProjectIndex` / `AliasResolve` /
    /// `ConditionalSelect`). An empty path returns `base` directly —
    /// that is the canonical form of "expand the whole surface" (the
    /// retired `Expand` variant).
    ///
    /// Alias-cycle detection terminates with
    /// `Opaque(QueryError::AliasCycle)`; stack depth is additionally
    /// bounded by [`PathWalker::max_depth`]. Open conditionals
    /// distribute the remaining path into both branches via
    /// `SemanticQueryApi::execute` re-entry so each branch-projection
    /// is a separately memoised sub-query.
    ///
    /// Emits a whole-path `ProjectPath` edge on the result (when the
    /// result differs from the base) so consumers can recover the
    /// entry path without rebuilding it from per-segment edges.
    pub(super) fn build_project_path(
        &self,
        base: SemanticNodeId,
        path: &Arc<[PathSegment]>,
        mode: ProjectionMode,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let fence = self.project_generation_signature();
        self.graph().record_path_length(path.len() as u32);
        // Phase 1B: longest-prefix-first peek. Skip when path.len() < 2.
        // Codex-2 r3 fix: prefix entries are cached as Navigate regardless
        // of the caller's mode (path-precise rule — intermediate hops are
        // Navigate, terminal hop is the caller's mode).
        let (start_base, start_index) = if path.len() < 2 {
            (base, 0usize)
        } else {
            find_longest_warm_prefix(self.graph(), base, path).unwrap_or((base, 0))
        };
        let walker_path: Arc<[PathSegment]> = if start_index == 0 {
            Arc::clone(path)
        } else {
            Arc::from(path[start_index..].to_vec().into_boxed_slice())
        };
        let mut walker = PathWalker::new(self, mode, &fence);
        let result = walker.walk(start_base, walker_path.as_ref());
        // Emit a whole-path `ProjectPath` edge on the result so consumers
        // can recover the entry path without rebuilding it from per-hop
        // edges (plan §3 C3).
        if result != base {
            self.graph().record_origin_edge(
                result,
                OriginEdgeKind::ProjectPath,
                Arc::from(vec![base].into_boxed_slice()),
                OriginMeta::Path(Arc::clone(path)),
                Arc::clone(&fence),
            );
        }
        // Phase 1B2: backfill intermediate path prefixes so a sibling
        // dispatch sharing the same prefix can short-circuit through
        // `find_longest_warm_prefix`. Backfill always targets Navigate
        // (path-precise rule — intermediate hops are Navigate-mode
        // entries). The terminal full-path key keeps the caller's mode
        // and is published by `execute_cooperative`'s admission flow,
        // not by this helper.
        backfill_prefixes(
            self.graph(),
            start_base,
            &walker_path,
            &walker.intermediate_nodes,
            &fence,
        );
        (QueryResult::Value(result), fence)
    }

    /// `keyof` projection. For an `Object` surface, materializes a union of
    /// the member names as `Primitive(String)` anchors — this matches the
    /// TS semantics that `keyof T` yields a union of string literals.
    /// For non-objects, returns `Opaque(Miss)`.
    ///
    /// Emits one `ProjectMember` edge per keyspace literal back to the
    /// source object base, carrying the member name in
    /// `OriginMeta::MemberName` (plan §3 C5). The edge lets walkers
    /// reconstruct which source member each keyspace literal derives from.
    pub(super) fn build_key_of(
        &self,
        base: SemanticNodeId,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let data = self.graph().node_data(base);
        let fence = self.project_generation_signature();
        let node = match data.as_deref() {
            Some(SemanticNodeData::Object(surface)) => self.intern_keyspace_names(
                base,
                surface
                    .members
                    .iter()
                    .map(|member| Arc::clone(&member.name)),
                &fence,
            ),
            Some(SemanticNodeData::Intersection(_) | SemanticNodeData::Union(_)) => self
                .key_names_from_base_node(base)
                .map(|names| self.intern_keyspace_names(base, names, &fence))
                .unwrap_or_else(|| self.graph().intern_node(SemanticNodeData::KeyOf { base })),
            Some(
                SemanticNodeData::TypeParam { .. }
                | SemanticNodeData::IndexedAccess { .. }
                | SemanticNodeData::Mapped { .. }
                | SemanticNodeData::TypeOf { .. }
                | SemanticNodeData::Conditional { .. }
                | SemanticNodeData::Alias(_)
                | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. }),
            ) => self.graph().intern_node(SemanticNodeData::KeyOf { base }),
            _ => self.opaque(QueryError::Miss),
        };
        (QueryResult::Value(node), fence)
    }

    pub(super) fn intern_keyspace_names<I>(
        &self,
        base: SemanticNodeId,
        names: I,
        fence: &DepSignature,
    ) -> SemanticNodeId
    where
        I: IntoIterator<Item = Arc<str>>,
    {
        let mut seen = FxHashSet::default();
        let member_literals: Vec<(SemanticNodeId, Arc<str>)> = names
            .into_iter()
            .filter(|name| seen.insert(Arc::clone(name)))
            .map(|name| {
                let lit =
                    self.graph()
                        .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                            name.as_ref().to_string(),
                        )));
                (lit, name)
            })
            .collect();
        for (lit_id, name) in &member_literals {
            self.graph().record_origin_edge(
                *lit_id,
                OriginEdgeKind::ProjectMember,
                Arc::from(vec![base].into_boxed_slice()),
                OriginMeta::MemberName(Arc::clone(name)),
                Arc::clone(fence),
            );
        }
        let ids: Vec<SemanticNodeId> = member_literals.into_iter().map(|(id, _)| id).collect();
        if ids.is_empty() {
            self.graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
        } else if ids.len() == 1 {
            ids[0]
        } else {
            self.graph()
                .intern_node(SemanticNodeData::Union(Arc::from(ids.into_boxed_slice())))
        }
    }

    pub(super) fn uses_synthetic_mapped_key_names(&self, members: &[SurfaceMember]) -> bool {
        !members.is_empty()
            && members.iter().all(|member| {
                member.name.strip_prefix("key_").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
                })
            })
    }

    /// Mapped-type rewrite (plan §3 C6 + §2 lazy block).
    ///
    /// For a mapped type `{ [K in key_space]: value_expr }` with
    /// optional / readonly modifiers (stored on the `MapperKey` and
    /// participating in the cache key), apply lazily:
    ///
    /// 1. Resolve the key space through `SemanticQueryKey::KeyOf` (or
    ///    use the explicit `MapperKey::key_space` if a caller passes a
    ///    pre-computed key union).
    /// 2. For each discovered key, reserve a member slot in the result
    ///    shell. Member optionality / readonly derive from the mapper's
    ///    modifiers (`Add` → always on, `Remove` → always off, `Keep` →
    ///    inherit from the source if available, else default off).
    /// 3. Member values are lazy: C6 interns them as opaque placeholders
    ///    because the full `K → key` substitution over `value_expr`
    ///    requires solver-scale `TypeExpr` lowering that lands in C7's
    ///    userland-equivalence pass. Callers projecting into a produced
    ///    member follow the ProjectPath sub-query route into the
    ///    keyspace + value expression.
    /// 4. Emit `Normalize` edges from the mapped result to each
    ///    contributing key. Emit one `ProjectMember` edge per produced
    ///    member sourcing `[source, key]` with `OriginMeta::MemberName`
    ///    carrying the produced name (post-remap if `name_remap` is
    ///    set).
    ///
    /// The `mapper: MapperKey` participates in the `SemanticQueryKey`
    /// hash so different modifier / value-expression combinations
    /// intern distinct entries — enforced by
    /// `mapped_type_optionality_and_readonly_modifiers_in_cache_key`.
    ///
    /// Path C C5 retired the `mapper_value_is_identity_t_of_k`
    /// runtime helper. Every `MapperKey` now carries a stable
    /// [`MapperKind`](crate::semantic_query::MapperKind) tag
    /// classified at lowering time (see
    /// [`crate::semantic_query::MapperKind::classify_value_expr`]).
    /// `build_mapped_type` matches on `mapper.kind` directly.
    pub(super) fn build_mapped_type(
        &self,
        source: SemanticNodeId,
        mapper: &crate::semantic_query::MapperKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let graph = self.graph();
        let fence = self.project_generation_signature();

        // 1. Resolve the key space.
        //
        // TS semantics: `{ [K in keyof T]: V }` walks T's member names.
        // When `source` is an Object, its member names ARE the correct
        // keys — even if `mapper.key_space` was pre-computed as a union
        // of string-literal primitives (the current graph model has no
        // literal-type PrimitiveKind, so we recover names from the
        // source directly). If `source` is not an Object we can read
        // member names from, fall back to the keyspace shape — but
        // opaque keyspaces terminate the mapped dispatch cleanly.
        let source_members: Vec<SurfaceMember> = match graph.node_data(source).as_deref() {
            Some(SemanticNodeData::Object(view)) => view.members.to_vec(),
            _ => Vec::new(),
        };
        let key_names: Vec<Arc<str>> = if !source_members.is_empty() {
            if self.uses_synthetic_mapped_key_names(&source_members) {
                match self.key_names_from_keyspace_node(mapper.key_space) {
                    Some(names) => names,
                    None => source_members.iter().map(|m| Arc::clone(&m.name)).collect(),
                }
            } else {
                source_members.iter().map(|m| Arc::clone(&m.name)).collect()
            }
        } else if let Some(names) = self.key_names_from_keyspace_node(mapper.key_space) {
            names
        } else {
            // Plan §3 Change M: `KeyEnumeration::Unresolvable`. Neither the
            // source surface nor the key space enumerate to concrete names.
            // The canonical form is a deferred
            // `SemanticNodeData::Mapped { source, mapper }` shell — callers
            // can re-dispatch through `MappedType` once one of the inputs
            // becomes enumerable. One `Normalize` edge captures the
            // contribution set (`[source, key_space, value_expr]`); the
            // `mapper.name_remap` field is preserved verbatim via the
            // interned `mapper` key.
            //
            // This replaces the retired `Alias(KeyOf(source))` surrogate.
            // The surrogate reinterpreted the mapped result AS its keyspace
            // (a relation confusion), which no downstream consumer could
            // safely navigate. `SemanticNodeData::Mapped` is the
            // dispatch-native deferred form.
            let node = graph.intern_node(SemanticNodeData::Mapped {
                source,
                mapper: mapper.clone(),
            });
            graph.record_origin_edge(
                node,
                OriginEdgeKind::Normalize,
                Arc::from(vec![source, mapper.key_space, mapper.value_expr].into_boxed_slice()),
                OriginMeta::None,
                Arc::clone(&fence),
            );
            return (QueryResult::Value(node), fence);
        };

        // 2. Build member slots.
        //
        // For each key K, the mapped member's VALUE type comes from
        // the value expression. The fast path — reading
        // `source_member.value` directly from the enumerated source
        // object — is ONLY valid when `mapper.kind` is
        // `MapperKind::Identity` (the canonical
        // `{ [K in keyof T]: T[K] }` pattern behind `Partial<T>` /
        // `Required<T>` / `Readonly<T>`). For any other value shape
        // (e.g. `keyof T['variants'][K]`, `ExtendSlotWithPlan<TPlan, K>`,
        // `infer`-bearing conditional bodies), classification yields
        // `MapperKind::Computed` and the value goes through the
        // substitute-and-evaluate path: intern `Literal(name)`,
        // substitute the mapper parameter in `mapper.value_expr`,
        // then evaluate the substituted node. Evaluation yielding
        // `Opaque(_)` publishes the un-evaluated substituted node so
        // the value stays addressable by path re-dispatch.
        //
        // Path C C5 moved the classification to lowering time; the
        // pre-C5 runtime helper `mapper_value_is_identity_t_of_k`
        // is retired.
        let value_is_identity = matches!(mapper.kind, crate::semantic_query::MapperKind::Identity);
        let mut produced: Vec<SurfaceMember> = Vec::with_capacity(key_names.len());
        let mut project_member_edges: Vec<(SemanticNodeId, Arc<str>)> = Vec::new();
        for name in &key_names {
            let source_member = source_members.iter().find(|m| &m.name == name);
            let optional = match mapper.optionality {
                crate::semantic_query::OptionalityMod::Add => true,
                crate::semantic_query::OptionalityMod::Remove => false,
                crate::semantic_query::OptionalityMod::Keep => {
                    source_member.map(|m| m.optional).unwrap_or(false)
                }
            };
            let readonly = match mapper.readonly {
                crate::semantic_query::ReadonlyMod::Add => true,
                crate::semantic_query::ReadonlyMod::Remove => false,
                crate::semantic_query::ReadonlyMod::Keep => {
                    source_member.map(|m| m.readonly).unwrap_or(false)
                }
            };
            // Value selection per plan §3 Cluster B:
            //
            // - `source_members` matches this key AND `value_expr` IS
            //   structurally `T[K]` → use the member value directly
            //   (Partial/Required/Readonly-style mapped types).
            // - Otherwise (`value_expr` is not `T[K]`, or the source
            //   has no matching member) → substitute `name →
            //   Literal(name)` into `mapper.value_expr`, evaluate; if
            //   evaluation yields `Opaque(_)`, publish the
            //   un-evaluated substituted node (preserves re-dispatch
            //   once one of the inputs becomes enumerable).
            let value = if let (Some(source_member), true) = (source_member, value_is_identity) {
                source_member.value
            } else {
                let key_arg =
                    self.graph()
                        .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                            name.as_ref().to_string(),
                        )));
                let substituted = self.substitute_semantic_type_param(
                    mapper.value_expr,
                    mapper.parameter_node,
                    key_arg,
                );
                let evaluated = self.evaluate_deferred_semantic_node(substituted);
                if matches!(
                    self.graph().node_data(evaluated).as_deref(),
                    Some(SemanticNodeData::Opaque(_))
                ) {
                    substituted
                } else {
                    evaluated
                }
            };
            produced.push(SurfaceMember {
                name: Arc::clone(name),
                value,
                optional,
                readonly,
                is_method: false,
            });
            project_member_edges.push((value, Arc::clone(name)));
        }

        let view = SurfaceView {
            members: Arc::from(produced.into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: Some(mapper.key_space),
            has_index_signature: false,
        };
        let node = graph.intern_node(SemanticNodeData::Object(view));

        // 3. Emit origin edges.
        //    - Normalize: result ← [source, key_space, value_expr].
        //    - ProjectMember per produced member.
        graph.record_origin_edge(
            node,
            OriginEdgeKind::Normalize,
            Arc::from(vec![source, mapper.key_space, mapper.value_expr].into_boxed_slice()),
            OriginMeta::None,
            Arc::clone(&fence),
        );
        for (value_id, name) in project_member_edges {
            graph.record_origin_edge(
                value_id,
                OriginEdgeKind::ProjectMember,
                Arc::from(vec![source, mapper.key_space].into_boxed_slice()),
                OriginMeta::MemberName(name),
                Arc::clone(&fence),
            );
        }

        (QueryResult::Value(node), fence)
    }

    /// Conditional type (plan §3 C2 + §2 lazy block + §3 D-Cutover
    /// distributive-conditional authority).
    ///
    /// Evaluates `check extends extends ? true_branch : false_branch`
    /// using the shared relation engine and returns one of:
    ///
    /// - **Distributive union check** — when `distributive == true` AND
    ///   `check` resolves to a [`SemanticNodeData::Union`], the builder
    ///   distributes per-member by re-entering the dispatcher with
    ///   `SemanticQueryApi::execute(SemanticQueryKey::Conditional {
    ///   check: member, extends, true_branch, false_branch,
    ///   distributive: false })` for every member, then combines the
    ///   per-member results through
    ///   `SemanticQueryApi::execute(SemanticQueryKey::NormalizeUnion {
    ///   members: per_member_results })`. Termination is guaranteed by
    ///   the `distributive: false` flag on each sub-query (no re-
    ///   distribution), the family memo's per-member dedup, and the
    ///   dispatch layer's same-path recursion sentinel. Plan §3
    ///   D-Cutover: dispatch owns distributive distribution; the
    ///   solver's retired distributive loop is gone.
    /// - **Closed/decidable check** — one of the branch shell references
    ///   directly (no `Conditional` node interned). Emits a
    ///   [`OriginEdgeKind::ConditionalSelect`] edge with
    ///   [`BranchSelection::True`] or [`BranchSelection::False`]. The
    ///   unselected branch is NOT materialised beyond its shell
    ///   reference (it already has one via the key's
    ///   `true_branch` / `false_branch` fields).
    /// - **Open/undecidable check** — a
    ///   [`SemanticNodeData::Conditional`] shell with both branch
    ///   references intact. Emits
    ///   [`OriginEdgeKind::ConditionalSelect`] with
    ///   [`BranchSelection::Deferred`]. Neither branch is recursively
    ///   materialised; path projection into the result (C3) drives
    ///   per-subexpression lazy expansion.
    ///
    /// C2's relation evaluator handles the decidable shapes the shallow
    /// walker reaches directly: primitive identity, primitive-to-top/any,
    /// `never` bottom, exact node identity, and the obvious
    /// non-assignability cases. Object / union / intersection / generic
    /// relations stay deferred — the full solver routing lands via the
    /// `resolve_conditional` dispatch handoff in D-Cutover. Bare-infer
    /// bindings (`T extends infer X`) are handled by the shortcut below;
    /// nested-infer in complex patterns defers to the relation engine.
    pub(super) fn build_conditional(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        distributive: bool,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let graph = self.graph();
        let fence = self.project_generation_signature();

        // Plan §3 C2 + §3 D-Cutover: distributive distribution is the
        // dispatch layer's responsibility. When `distributive == true`
        // and `check` is a union, re-enter `execute` per-member with
        // `distributive: false`, then normalise the per-member results
        // through `NormalizeUnion`. Each sub-dispatch lands in a
        // different family entry (check differs per member) and the
        // `distributive: false` flag guarantees no re-distribution, so
        // the cooperative-wait mechanism terminates and the same-path
        // sentinel catches any accidental self-recursion.
        //
        // Robustness: if any sub-query returns `Recursive` or `Error`
        // (cycle or miss), fall through to the ordinary deferred-shell
        // path below so the caller sees a well-formed conditional node
        // rather than a partial distribution.
        if distributive {
            if let Some(members) = graph.node_data(check).and_then(|data| match &*data {
                SemanticNodeData::Union(members) => Some(Arc::clone(members)),
                _ => None,
            }) {
                let mut per_member: Vec<SemanticNodeId> = Vec::with_capacity(members.len());
                let mut distribution_ok = true;
                for &member in members.iter() {
                    match self.execute(SemanticQueryKey::Conditional {
                        check: member,
                        extends,
                        true_branch,
                        false_branch,
                        distributive: false,
                    }) {
                        QueryResult::Value(id) => per_member.push(id),
                        _ => {
                            distribution_ok = false;
                            break;
                        }
                    }
                }
                if distribution_ok {
                    if let QueryResult::Value(normalised) =
                        self.execute(SemanticQueryKey::NormalizeUnion {
                            members: Arc::from(per_member.into_boxed_slice()),
                        })
                    {
                        return (QueryResult::Value(normalised), fence);
                    }
                }
                // Fall through to the deferred-shell path below.
            }
        }

        // Plan §3 Cluster A: single-infer conditional `T extends infer
        // X ? X : ...`. When `extends` is a bare
        // [`SemanticNodeData::Infer`], bind the infer name to `check`
        // on the true branch and emit an `InferBind` origin edge
        // carrying the binding name. The true-branch substitution
        // picks the Infer up symmetrically with TypeParam via
        // `substitute_semantic_type_param`.
        //
        // Multi-infer (`T extends [infer A, infer B] ? ...`), nested-
        // infer, and template-literal-infer patterns stay deferred;
        // they require the full relation-engine bindings integration
        // still pending per TODO(plan §3 Change S) below.
        if let Some(SemanticNodeData::Infer { name }) = graph.node_data(extends).as_deref() {
            let infer_name: Arc<str> = Arc::clone(name);
            // Path C C6a item 9a: intern the Infer node, then
            // substitute by node id. substitute's Infer arm still
            // matches by display_name for now (plan §14.2 item 8
            // footnote — TypeScript `infer X` is a separate
            // name-slot mechanism, C11a re-evaluates).
            let infer_node = graph.intern_node(SemanticNodeData::Infer {
                name: Arc::clone(&infer_name),
            });
            let result = self.substitute_semantic_type_param(true_branch, infer_node, check);
            graph.record_origin_edge(
                result,
                OriginEdgeKind::InferBind,
                Arc::from(vec![check, extends].into_boxed_slice()),
                OriginMeta::SubstitutedParam(infer_name),
                Arc::clone(&fence),
            );
            graph.record_conditional_decided();
            graph.record_branch_selection_true();
            return (QueryResult::Value(result), fence);
        }

        // Path C C11a — nested-infer in Function types.
        // TypeScript patterns of the form
        // `T extends (x: infer U, y: infer V) => infer R ? true_branch : false_branch`
        // bind each `infer` binding to the corresponding position in
        // `check`'s signature, then substitute those bindings into the
        // true branch. Pre-C11a this pattern lowered to a deferred
        // Conditional shell because `relate_nodes`'s Function arm
        // short-circuits to `Unknown` in the presence of Infer
        // positions (the relation engine does not currently emit
        // infer bindings — see the TODO on the Assignable branch
        // below). Post-C11a the infer bindings are extracted directly
        // from the lowered shapes so the true-branch substitution can
        // surface the concrete result.
        //
        // `check` is first materialised via `evaluate_deferred_semantic_node`
        // so `PricingPlanSlots["badge"]` / mapped-type references resolve
        // to their underlying Function before position-wise binding.
        if let Some(SemanticNodeData::Function {
            params: extends_params,
            return_type: extends_return,
            ..
        }) = graph.node_data(extends).as_deref()
        {
            let extends_params = Arc::clone(extends_params);
            let extends_return = *extends_return;
            let has_infer_position = extends_params.iter().any(|p| {
                matches!(
                    graph.node_data(p.ty).as_deref(),
                    Some(SemanticNodeData::Infer { .. })
                )
            }) || matches!(
                graph.node_data(extends_return).as_deref(),
                Some(SemanticNodeData::Infer { .. })
            );
            if has_infer_position {
                let check_resolved = self.evaluate_deferred_semantic_node(check);
                if let Some(SemanticNodeData::Function {
                    params: check_params,
                    return_type: check_return,
                    ..
                }) = graph.node_data(check_resolved).as_deref()
                {
                    let check_params = Arc::clone(check_params);
                    let check_return = *check_return;
                    let mut result = true_branch;
                    let mut any_bound = false;
                    for (e_param, c_param) in extends_params.iter().zip(check_params.iter()) {
                        if let Some(SemanticNodeData::Infer { name }) =
                            graph.node_data(e_param.ty).as_deref()
                        {
                            let infer_name: Arc<str> = Arc::clone(name);
                            let infer_node = graph.intern_node(SemanticNodeData::Infer {
                                name: Arc::clone(&infer_name),
                            });
                            result =
                                self.substitute_semantic_type_param(result, infer_node, c_param.ty);
                            graph.record_origin_edge(
                                result,
                                OriginEdgeKind::InferBind,
                                Arc::from(vec![check, extends].into_boxed_slice()),
                                OriginMeta::SubstitutedParam(infer_name),
                                Arc::clone(&fence),
                            );
                            any_bound = true;
                        }
                    }
                    if let Some(SemanticNodeData::Infer { name }) =
                        graph.node_data(extends_return).as_deref()
                    {
                        let infer_name: Arc<str> = Arc::clone(name);
                        let infer_node = graph.intern_node(SemanticNodeData::Infer {
                            name: Arc::clone(&infer_name),
                        });
                        result =
                            self.substitute_semantic_type_param(result, infer_node, check_return);
                        graph.record_origin_edge(
                            result,
                            OriginEdgeKind::InferBind,
                            Arc::from(vec![check, extends].into_boxed_slice()),
                            OriginMeta::SubstitutedParam(infer_name),
                            Arc::clone(&fence),
                        );
                        any_bound = true;
                    }
                    if any_bound {
                        graph.record_conditional_decided();
                        graph.record_branch_selection_true();
                        return (QueryResult::Value(result), fence);
                    }
                }
            }
        }

        // Phase D §3 Change S: conditionals route through the shared
        // relation authority. The shallow path handles the trivial
        // primitive/identity cases inline for hot-path efficiency; any
        // non-trivial pair falls through to the full relation engine
        // for memoised assignability. `relate_nodes` internally guards
        // cyclic re-entry and caps structural descent at
        // `RELATION_MAX_DEPTH` as a stack-safety rail.
        let relation = self.shallow_relation_check(check, extends);
        let (result, branch, is_deferred) = match relation {
            ShallowRelation::Assignable => (true_branch, BranchSelection::True, false),
            ShallowRelation::NotAssignable => (false_branch, BranchSelection::False, false),
            ShallowRelation::Unknown => {
                // Full relation authority. `bindings` from a successful
                // Assignable result become infer bindings on the true
                // branch; `relate_nodes` memoises all three outcomes
                // with dep-signature fencing.
                match self.relate_nodes(check, extends).0 {
                    crate::semantic_query::RelationResult::Assignable { bindings: _ } => {
                        // TODO(plan §3 Change S): substitute infer
                        // bindings into `true_branch` via
                        // `substitute_semantic_type_param` and emit
                        // `InferBind` origin edges for non-empty
                        // bindings. Infer-bearing conditionals lower
                        // to the deferred shell today; see §6.3 test
                        // `relate_result_assignable_carries_infer_bindings_into_conditional`.
                        (true_branch, BranchSelection::True, false)
                    }
                    crate::semantic_query::RelationResult::NotAssignable => {
                        (false_branch, BranchSelection::False, false)
                    }
                    crate::semantic_query::RelationResult::Unknown => {
                        let node = graph.intern_node(SemanticNodeData::Conditional {
                            check,
                            extends,
                            true_branch_ref: true_branch,
                            false_branch_ref: false_branch,
                            distributive,
                        });
                        (node, BranchSelection::Deferred, true)
                    }
                }
            }
        };
        graph.record_origin_edge(
            result,
            OriginEdgeKind::ConditionalSelect,
            Arc::from(vec![check, extends].into_boxed_slice()),
            OriginMeta::Branch(branch),
            Arc::clone(&fence),
        );
        if is_deferred {
            graph.record_conditional_deferred();
        } else {
            graph.record_conditional_decided();
            match branch {
                BranchSelection::True => graph.record_branch_selection_true(),
                BranchSelection::False => graph.record_branch_selection_false(),
                BranchSelection::Deferred => {}
            }
        }
        (QueryResult::Value(result), fence)
    }

    /// Shallow hot-path relation check used by [`Self::build_conditional`].
    /// Decides the trivial primitive/identity/top/bottom cases inline
    /// without descending into the full relation engine. Non-trivial
    /// pairs return `Unknown`, in which case `build_conditional` falls
    /// through to [`Self::relate_nodes`] for the full structural
    /// decision.
    pub(super) fn shallow_relation_check(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> ShallowRelation {
        if source == target {
            return ShallowRelation::Assignable;
        }
        let graph = self.graph();
        let Some(source_data) = graph.node_data(source) else {
            return ShallowRelation::Unknown;
        };
        let Some(target_data) = graph.node_data(target) else {
            return ShallowRelation::Unknown;
        };
        match (&*source_data, &*target_data) {
            (SemanticNodeData::Primitive(PrimitiveKind::Never), _) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Any)) => ShallowRelation::Assignable,
            (SemanticNodeData::Primitive(PrimitiveKind::Any), _) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Never)) => {
                ShallowRelation::NotAssignable
            }
            (SemanticNodeData::Primitive(a), SemanticNodeData::Primitive(b)) => {
                if a == b {
                    ShallowRelation::Assignable
                } else {
                    ShallowRelation::NotAssignable
                }
            }
            _ => ShallowRelation::Unknown,
        }
    }

    /// Union normalization. Structurally sorts + dedups the supplied members
    /// and publishes the canonical union node. Singleton unions fold to
    /// their only member; empty unions fold to `Primitive(Never)`.
    ///
    /// Emits one `Normalize` origin edge from the result to each
    /// contributing source member (plan §3 C5). The edge lets walkers
    /// recover the pre-canonical input set even after dedup / sorting.
    /// Single-member / empty folds emit no edge — the result IS one of
    /// the inputs (or a fresh Never node) and there's no canonicalisation
    /// fact to record.
    pub(super) fn build_normalize_union(
        &self,
        members: &Arc<[SemanticNodeId]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.intern_normalized_union_or_intersection(members, /* is_union */ true);
        let fence = self.project_generation_signature();
        if members.len() > 1 {
            self.graph().record_origin_edge(
                node,
                OriginEdgeKind::Normalize,
                Arc::clone(members),
                OriginMeta::None,
                Arc::clone(&fence),
            );
        }
        (QueryResult::Value(node), fence)
    }

    /// Intersection normalization. Structurally sorts + dedups; singleton
    /// folds to the only member; empty folds to `Primitive(Never)`.
    ///
    /// Emits one `Normalize` origin edge from the result to each
    /// contributing source member (plan §3 C5).
    pub(super) fn build_normalize_intersection(
        &self,
        members: &Arc<[SemanticNodeId]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.intern_normalized_union_or_intersection(members, /* is_union */ false);
        let fence = self.project_generation_signature();
        if members.len() > 1 {
            self.graph().record_origin_edge(
                node,
                OriginEdgeKind::Normalize,
                Arc::clone(members),
                OriginMeta::None,
                Arc::clone(&fence),
            );
        }
        (QueryResult::Value(node), fence)
    }

    /// Vue macro resolution lookup.
    ///
    /// Hot-path reads go through
    /// [`SemanticGraphStore::get_resolved_named_type`](crate::semantic_query_memo::SemanticGraphStore::get_resolved_named_type)
    /// directly from the parser's
    /// [`NamedTypeCache`](verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache)
    /// adapter — the formal `execute` path stays available as an entry
    /// point for callers that want to check presence through the shared
    /// query API but must not be relied on in the refcount-only hot
    /// path. Writes enter from the adapter side via
    /// [`SemanticGraphStore::insert_resolved_named_type`](crate::semantic_query_memo::SemanticGraphStore::insert_resolved_named_type).
    ///
    /// Returns a warm node id when the identity map has an entry, or
    /// [`QueryError::Miss`] when the entry has not been written yet.
    /// Carries a dep-signature fragment capturing
    /// `(canonical_id, whole_hash, project_generation)` so
    /// [`HostFenceValidator`](crate::host_manage::HostFenceValidator)
    /// catches stale warm hits if any downstream layer memoizes this
    /// dispatch path.
    pub(super) fn build_resolved_named_type(
        &self,
        key: &HostResolvedNamedTypeKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let graph = self.graph();
        match graph.resolved_named_type_node_id(key) {
            Some(node_id) => (
                QueryResult::Value(node_id),
                self.dep_signature_for(&key.canonical_id, key.whole_hash),
            ),
            None => (QueryResult::Error(QueryError::Miss), empty_signature()),
        }
    }

    pub(super) fn intern_normalized_union_or_intersection(
        &self,
        members: &[SemanticNodeId],
        is_union: bool,
    ) -> SemanticNodeId {
        let mut sorted: Vec<SemanticNodeId> = members.to_vec();
        sorted.sort_by_key(|id| id.0);
        sorted.dedup();
        if sorted.is_empty() {
            return self
                .graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
        }
        if sorted.len() == 1 {
            return sorted[0];
        }
        let boxed: Arc<[SemanticNodeId]> = Arc::from(sorted.into_boxed_slice());
        if is_union {
            self.graph().intern_node(SemanticNodeData::Union(boxed))
        } else {
            self.graph()
                .intern_node(SemanticNodeData::Intersection(boxed))
        }
    }
}

/// Phase 1B path-prefix peek (plan §1.B). Walks `path` from longest to
/// shortest non-empty prefix, returning the warm `(base, path[..k],
/// Navigate)` entry's resolved node and `k` if any such prefix is
/// memoized. Returns `None` when no prefix is warm — caller falls back
/// to walking the full path from `base`.
///
/// **Codex-2 r3 fix.** The lookup forces `mode: Navigate` regardless of
/// the caller's mode because intermediate path hops MUST be cached as
/// Navigate per the path-precise rule (CLAUDE.md "Macro Type Traversal
/// Rule"). The terminal hop keeps the caller's mode and is published by
/// `execute_cooperative` directly; the prefix peek only inspects
/// intermediate hops.
///
/// Increments the test-only `PREFIX_PEEK_HITS` thread-local on success
/// so `project_path_prefix_peek_short_circuits_sibling_walk` can
/// discriminate pre-fix (no helper / always None) vs post-fix (peek hits
/// the warm prefix).
fn find_longest_warm_prefix(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    base: SemanticNodeId,
    path: &Arc<[PathSegment]>,
) -> Option<(SemanticNodeId, usize)> {
    for k in (1..path.len()).rev() {
        let prefix_path: Arc<[PathSegment]> = Arc::from(path[..k].to_vec().into_boxed_slice());
        let prefix_key = SemanticQueryKey::ProjectPath {
            base,
            path: prefix_path,
            mode: ProjectionMode::Navigate,
        };
        if let Some(hit) = graph.get(&prefix_key) {
            if let QueryResult::Value(prefix_node) = hit.value {
                #[cfg(test)]
                PREFIX_PEEK_HITS.with(|c| *c.borrow_mut() += 1);
                return Some((prefix_node, k));
            }
        }
    }
    None
}

/// Phase 1B2 backfill helper (plan §1.B). For each linear-member-step
/// intermediate captured by the [`PathWalker`] in `intermediates`,
/// publish the corresponding `(base, path[..i+1], Navigate)` key into
/// the warm map via the shared
/// [`SemanticGraphStore::publish_warm_if_absent`] helper (which
/// internally reuses the same warm-publish path that
/// `execute_cooperative` uses, gated by an "absent only" check).
///
/// Skips the last index — the full key is owned by
/// `execute_cooperative` (it carries the caller's mode, not Navigate;
/// the path-precise rule places terminal hops at the caller's mode).
///
/// Skips `None` entries (arm-splits at Union / Intersection /
/// open-Conditional positions); those positions have no single
/// canonical answer for `(base, path[..k], Navigate)`.
fn backfill_prefixes(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    base: SemanticNodeId,
    path: &Arc<[PathSegment]>,
    intermediates: &[Option<SemanticNodeId>],
    full_dep_signature: &DepSignature,
) {
    // Backfill is only meaningful for the contiguous LINEAR prefix of
    // the walk — the leading run of `Some(node)` entries before any
    // arm-split. Once the walker hits a Union / Intersection /
    // Conditional, subsequent intermediates may belong to per-arm
    // sub-walks (which the iterative worklist runs as their own
    // advance_step calls) and no longer line up with the trunk's
    // path index.
    //
    // Bound `i` so that:
    //   - `i < intermediates.len() - 1` — skip the last intermediate;
    //     the terminal full-path key is owned by `execute_cooperative`
    //     (it carries the caller's mode, not Navigate).
    //   - `i < path.len() - 1` — keep `path[..i + 1]` strictly shorter
    //     than the full path (sibling-sharable prefixes only) and avoid
    //     out-of-range slicing when arm-split sub-walks pushed extra
    //     entries past `path.len()`.
    //   - Break at the first `None` — after an arm-split the index no
    //     longer lines up with `path[..i + 1]` so subsequent entries
    //     are not canonical answers for that key.
    let max_i = intermediates.len().min(path.len()).saturating_sub(1);
    for i in 0..max_i {
        let Some(node) = intermediates[i] else { break };
        let prefix_path: Arc<[PathSegment]> = Arc::from(path[..i + 1].to_vec().into_boxed_slice());
        let prefix_key = SemanticQueryKey::ProjectPath {
            base,
            path: prefix_path,
            mode: ProjectionMode::Navigate,
        };
        graph.publish_warm_if_absent(prefix_key, node, Arc::clone(full_dep_signature));
    }
}
