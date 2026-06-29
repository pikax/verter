//! Node-domain fallthrough value evaluation + spread / dynamic-root readers.
//!
//! The fallthrough resolver's root-consumption (spread keys), dynamic-root
//! (`is=`) and generic child-prop-override decisions read structural facts off a
//! value expression's PROJECTED NODE instead of a materialised `TypeExpr`. The
//! engine evaluates a value expression to a `SemanticNodeId` ONCE (override
//! forwarding + runtime-value env substitution + node Class-A projection); the
//! free-fn readers then walk `SemanticNodeData`. No semantic decision is taken
//! on a materialised `TypeExpr`.

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::template::TemplatePropUsage;
use verter_semantic::analysis::type_eval::EvalEnv;
use verter_semantic::analysis::types::AnalyzedImport;
use verter_type_expr::{LiteralValue, TypeExpr};

use super::ComponentMetaQueryEngine;
use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::resolver_core::fallthrough::{
    component_import_candidate_for_binding, intersect_known_spread_keys,
    normalize_public_spread_key, structural_substitute_typeof_refs, DynamicRootCandidate,
    FallthroughPropOverride, FallthroughPropOverrideSet, KnownSpreadKeys,
};
use crate::resolver_core::fallthrough_override_key::{
    FallthroughOverrideConditionalKey, FallthroughOverrideFunctionKey, FallthroughOverrideIdentity,
    FallthroughOverrideIndexKey, FallthroughOverrideIndexSigKey, FallthroughOverrideMappedKey,
    FallthroughOverrideMemberKey, FallthroughOverrideParamKey, FallthroughOverrideScopeKey,
    FallthroughOverrideSetKey, FallthroughOverrideSurfaceKey, FallthroughOverrideTupleElementKey,
    FallthroughOverrideTypeParamDeclKey, FallthroughOverrideValueKey, OpaqueErrorKey,
};
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{IndexKey, ProjectionMode, SemanticNodeData, SemanticNodeId};

/// Shared per-call prologue for the fallthrough node-DAG walkers
/// ([`known_spread_keys_from_node_inner`],
/// [`collect_dynamic_root_candidates_from_node_inner`], and the override-key
/// value projector). It is the SINGLE mechanism the three walkers reuse:
///
/// - **Persistent memo** (`memo`): each distinct `SemanticNodeId` is computed
///   ONCE per top-level call. A shared subtree reached through two sibling arms
///   of a content-interned diamond is therefore O(distinct nodes), not the
///   former O(2^depth) re-traversal.
/// - **Shared op-budget**: ONE unit of the EXISTING request projection budget
///   ([`crate::request_budget::RequestBudget`]) is charged per distinct node.
///   A trip halts the walk (no second budget engine); the partial result is
///   never warm-admitted (the fallthrough store gates admission on
///   `RequestBudget::is_exhausted`).
/// - **Cycle sentinel** (`active`): tracks the in-progress path so a (defensive)
///   re-entry halts instead of recursing — it is the cycle sentinel, NOT the
///   memo.
enum NodeWalkStep<T> {
    /// `node` was already computed — reuse this value.
    Cached(T),
    /// Cycle re-entry OR budget trip — return the walker's halt value.
    Halt,
    /// First visit, within budget, marked active — proceed to compute.
    Visit,
}

/// Run the shared walk prologue for `node`: memo probe, then per-distinct-node
/// budget charge, then cycle-sentinel insert. See [`NodeWalkStep`].
fn enter_node<T: Clone>(
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &FxHashMap<SemanticNodeId, T>,
) -> NodeWalkStep<T> {
    if let Some(cached) = memo.get(&node) {
        return NodeWalkStep::Cached(cached.clone());
    }
    if crate::request_context::current_request_budget()
        .is_some_and(|budget| budget.check_projection_op_count())
    {
        return NodeWalkStep::Halt;
    }
    if !active.insert(node) {
        return NodeWalkStep::Halt;
    }
    NodeWalkStep::Visit
}

/// Shared walk epilogue: pop the cycle sentinel and memoize `result` for `node`.
fn exit_node<T: Clone>(
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut FxHashMap<SemanticNodeId, T>,
    result: T,
) -> T {
    active.remove(&node);
    memo.insert(node, result.clone());
    result
}

impl ComponentMetaQueryEngine<'_> {
    /// Evaluate a fallthrough value expression to its resolved value NODE,
    /// entirely in node domain.
    ///
    /// Resolution order: (1) override forwarding — a bare single-segment
    /// `typeof <name>` whose name is a parent-propagated override resolves to
    /// the override value NODE directly (so a child re-forwarding a generic
    /// root prop reaches the propagated node, not a re-resolved declaration);
    /// (2) runtime-value env substitution — `structural_substitute_typeof_refs`
    /// folds imported value bindings, and a changed shape (a concrete value
    /// type) lowers to a node directly; (3) node Class-A projection of the
    /// lowered expression. `None` when the expression neither parses nor
    /// projects.
    pub(crate) fn evaluate_fallthrough_value_node(
        &mut self,
        scope_canonical_id: &str,
        expression: &str,
        env: Option<&EvalEnv>,
        overrides: Option<&FallthroughPropOverrideSet>,
    ) -> Option<SemanticNodeId> {
        let lowered =
            verter_semantic::analysis::type_eval_build::parse_value_expression_type(expression)?;

        // (1) Override forwarding: a bare single-segment `typeof <name>` whose
        // name is a propagated override resolves to the override value node.
        if let TypeExpr::TypeOf(value_ref) = &lowered {
            if value_ref.path.len() == 1 {
                if let Some(node) = overrides.and_then(|set| set.lookup(value_ref.path[0].as_str()))
                {
                    return Some(node);
                }
            }
        }

        let dispatch = ProjectSemanticDispatch::new(self.ctx);

        // (2) Runtime-value env substitution. A changed shape is already a
        // concrete value type; lower it to a node directly (Navigate).
        if let Some(env) = env {
            let substituted = structural_substitute_typeof_refs(&lowered, env);
            if substituted != lowered {
                return dispatch.lower_type_expr_in_scope_with_mode(
                    scope_canonical_id,
                    &substituted,
                    ProjectionMode::Navigate,
                );
            }
        }

        // (3) Node Class-A projection (registry route fast-path + terminal).
        let admitted = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
            self.ctx,
            Some(self),
            scope_canonical_id,
            &lowered,
        )?;
        Some(admitted.node())
    }

    /// Node-domain spread-key reader for a root-consumption spread value
    /// expression (`v-bind="expr"`). Returns `None` when the value node carries
    /// no statically-known key surface (the caller records an unknown spread).
    pub(crate) fn known_spread_keys_for_value_expression(
        &mut self,
        scope_canonical_id: &str,
        expression: &str,
        env: Option<&EvalEnv>,
        overrides: Option<&FallthroughPropOverrideSet>,
    ) -> Option<KnownSpreadKeys> {
        let node =
            self.evaluate_fallthrough_value_node(scope_canonical_id, expression, env, overrides)?;
        known_spread_keys_from_node(self.ctx, node)
    }

    /// Node-domain dynamic-root-candidate reader for an `is=` value expression.
    /// Returns the native-tag / component-import candidates discoverable from
    /// the value node.
    pub(crate) fn dynamic_root_candidates_for_value_expression(
        &mut self,
        scope_canonical_id: &str,
        expression: &str,
        env: Option<&EvalEnv>,
        overrides: Option<&FallthroughPropOverrideSet>,
        imports: &[AnalyzedImport],
    ) -> Vec<DynamicRootCandidate> {
        let Some(node) =
            self.evaluate_fallthrough_value_node(scope_canonical_id, expression, env, overrides)
        else {
            return Vec::new();
        };
        collect_dynamic_root_candidates_from_node(self.ctx, node, imports)
    }

    /// Build the override value NODE for one component-usage prop: an unbound
    /// prop lowers its literal string / `true`; a bound / shorthand prop
    /// evaluates its expression to a node (env + override aware), falling back
    /// to lowering the bare parse. `None` when the prop contributes no override.
    pub(crate) fn value_expression_override_node(
        &mut self,
        scope_canonical_id: &str,
        prop: &TemplatePropUsage,
        env: Option<&EvalEnv>,
        overrides: Option<&FallthroughPropOverrideSet>,
    ) -> Option<SemanticNodeId> {
        if prop.from_spread {
            return None;
        }

        if !prop.is_bound {
            let literal = match &prop.expression {
                Some(expression) => TypeExpr::string_literal(expression.clone()),
                None => TypeExpr::boolean_literal(true),
            };
            return self.lower_value_literal_node(scope_canonical_id, &literal);
        }

        if let Some(expression) = &prop.expression {
            if let Some(node) =
                self.evaluate_fallthrough_value_node(scope_canonical_id, expression, env, overrides)
            {
                return Some(node);
            }
            if let Some(parsed) =
                verter_semantic::analysis::type_eval_build::parse_value_expression_type(expression)
            {
                return self.lower_value_literal_node(scope_canonical_id, &parsed);
            }
        }

        if prop.is_shorthand {
            if let Some(node) =
                self.evaluate_fallthrough_value_node(scope_canonical_id, &prop.name, env, overrides)
            {
                return Some(node);
            }
            if let Some(parsed) =
                verter_semantic::analysis::type_eval_build::parse_value_expression_type(&prop.name)
            {
                return self.lower_value_literal_node(scope_canonical_id, &parsed);
            }
        }

        None
    }

    /// Lower a concrete value `TypeExpr` (a literal / bare parse) to a node at
    /// Navigate mode — the symbolic-input pipeline feed for the override carrier.
    fn lower_value_literal_node(
        &self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<SemanticNodeId> {
        ProjectSemanticDispatch::new(self.ctx).lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            expr,
            ProjectionMode::Navigate,
        )
    }

    /// Compute the EXACT, content-free override-set cache identity for
    /// `entries` (codex ruling C). Each override value NODE is projected to a
    /// full structural [`FallthroughOverrideValueKey`] — NOT a lossy digest —
    /// through the shared persistent-memo + op-budget node walker
    /// ([`project_override_value_key_inner`]), so two genuinely-different
    /// override sets never alias and a content edit reaching an override value
    /// keys distinctly.
    ///
    /// - Empty set → [`FallthroughOverrideIdentity::NoOverrides`].
    /// - Any override value that projects to an unrepresentable node
    ///   (`VueMacroElements`, missing node, a cycle anomaly, or an op-budget
    ///   trip) → [`FallthroughOverrideIdentity::Uncacheable`]: the request skips
    ///   override-bearing fallthrough cache admission + singleflight.
    /// - Otherwise → [`FallthroughOverrideIdentity::Exact`] with entries SORTED
    ///   by prop name and made UNIQUE by the runtime-effective (first-match)
    ///   winner, mirroring [`FallthroughPropOverrideSet::lookup`].
    pub(crate) fn fallthrough_override_identity(
        &self,
        entries: &[FallthroughPropOverride],
    ) -> FallthroughOverrideIdentity {
        if entries.is_empty() {
            return FallthroughOverrideIdentity::NoOverrides;
        }
        let dispatch = ProjectSemanticDispatch::new(self.ctx);
        let mut memo: FxHashMap<SemanticNodeId, Option<FallthroughOverrideValueKey>> =
            FxHashMap::default();
        let mut active: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut seen_names: FxHashSet<&str> = FxHashSet::default();
        let mut keyed: Vec<(std::sync::Arc<str>, FallthroughOverrideValueKey)> = Vec::new();
        for entry in entries {
            // First-match winner per prop name — mirrors
            // `FallthroughPropOverrideSet::lookup` (order-sensitive).
            if !seen_names.insert(entry.name.as_str()) {
                continue;
            }
            let Some(value_key) = project_override_value_key_inner(
                self.ctx,
                &dispatch,
                entry.node,
                &mut active,
                &mut memo,
            ) else {
                return FallthroughOverrideIdentity::Uncacheable;
            };
            keyed.push((std::sync::Arc::from(entry.name.as_str()), value_key));
        }
        // SORTED by (now-unique) prop name, so the same effective overrides in
        // any source order key identically.
        keyed.sort_by(|left, right| left.0.cmp(&right.0));
        FallthroughOverrideIdentity::Exact(std::sync::Arc::new(FallthroughOverrideSetKey {
            entries: keyed,
        }))
    }
}

/// Node-domain mirror of `known_spread_keys_from_type_expr`: walk a value
/// node's object / alias / intersection / union shape into the statically-known
/// attr + listener key sets. `None` for any node that exposes no static key
/// surface (the caller records an unknown spread).
pub(crate) fn known_spread_keys_from_node(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<KnownSpreadKeys> {
    known_spread_keys_from_node_inner(
        ctx,
        node,
        &mut FxHashSet::default(),
        &mut FxHashMap::default(),
    )
}

fn known_spread_keys_from_node_inner(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut FxHashMap<SemanticNodeId, Option<KnownSpreadKeys>>,
) -> Option<KnownSpreadKeys> {
    match enter_node(node, active, memo) {
        NodeWalkStep::Cached(cached) => return cached,
        NodeWalkStep::Halt => return None,
        NodeWalkStep::Visit => {}
    }
    let result = match node_data_for(ctx, node) {
        None => None,
        Some(data) => match data.as_ref() {
            // The `Alias` identity hop is the node equivalent of the
            // `TypeExpr::Parenthesized` wrap.
            SemanticNodeData::Alias(inner) => {
                known_spread_keys_from_node_inner(ctx, *inner, active, memo)
            }
            SemanticNodeData::Object(surface) => Some(known_spread_keys_from_surface(surface)),
            SemanticNodeData::Intersection(arms) => {
                let mut result = KnownSpreadKeys {
                    exact: true,
                    ..KnownSpreadKeys::default()
                };
                let mut saw_any = false;
                for part in arms.iter() {
                    let Some(summary) = known_spread_keys_from_node_inner(ctx, *part, active, memo)
                    else {
                        result.exact = false;
                        continue;
                    };
                    saw_any = true;
                    result.attrs.extend(summary.attrs);
                    result.listeners.extend(summary.listeners);
                    result.exact &= summary.exact;
                }
                saw_any.then_some(result)
            }
            SemanticNodeData::Union(arms) => {
                let mut iter = arms.iter();
                match iter.next() {
                    None => None,
                    Some(first_node) => {
                        match known_spread_keys_from_node_inner(ctx, *first_node, active, memo) {
                            None => None,
                            Some(first) => {
                                let mut result = first.clone();
                                let mut exact_same_keys = first.exact;
                                let mut early_inexact = false;
                                for branch in iter {
                                    let Some(summary) = known_spread_keys_from_node_inner(
                                        ctx, *branch, active, memo,
                                    ) else {
                                        result.exact = false;
                                        early_inexact = true;
                                        break;
                                    };
                                    exact_same_keys &= summary.exact
                                        && summary.attrs == result.attrs
                                        && summary.listeners == result.listeners;
                                    result = intersect_known_spread_keys(result, summary);
                                }
                                if !early_inexact {
                                    result.exact = exact_same_keys;
                                }
                                Some(result)
                            }
                        }
                    }
                }
            }
            _ => None,
        },
    };
    exit_node(node, active, memo, result)
}

/// Node-domain mirror of `known_spread_keys_from_object`: classify each surface
/// member name into the attr / listener sets (via the SHARED
/// [`normalize_public_spread_key`]) and mark the surface inexact when it carries
/// an index / call / construct signature (the node equivalent of the object
/// helper's signature-member arm).
fn known_spread_keys_from_surface(surface: &crate::semantic_query::SurfaceView) -> KnownSpreadKeys {
    let mut result = KnownSpreadKeys {
        exact: true,
        ..KnownSpreadKeys::default()
    };
    for member in surface.members.iter() {
        normalize_public_spread_key(
            member.name.as_ref(),
            &mut result.attrs,
            &mut result.listeners,
        );
    }
    if surface.has_index_signature
        || !surface.index_signatures.is_empty()
        || !surface.call_signatures.is_empty()
        || !surface.construct_signatures.is_empty()
    {
        result.exact = false;
    }
    result
}

/// Node-domain mirror of `collect_dynamic_root_candidates_from_type`: walk a
/// value node's literal-string / union / alias / `typeof` shape into the
/// native-tag + component-import dynamic-root candidates.
pub(crate) fn collect_dynamic_root_candidates_from_node(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    imports: &[AnalyzedImport],
) -> Vec<DynamicRootCandidate> {
    collect_dynamic_root_candidates_from_node_inner(
        ctx,
        node,
        imports,
        &mut FxHashSet::default(),
        &mut FxHashMap::default(),
    )
}

fn collect_dynamic_root_candidates_from_node_inner(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    imports: &[AnalyzedImport],
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut FxHashMap<SemanticNodeId, Vec<DynamicRootCandidate>>,
) -> Vec<DynamicRootCandidate> {
    match enter_node(node, active, memo) {
        NodeWalkStep::Cached(cached) => return cached,
        NodeWalkStep::Halt => return Vec::new(),
        NodeWalkStep::Visit => {}
    }
    let out = match node_data_for(ctx, node) {
        None => Vec::new(),
        Some(data) => match data.as_ref() {
            SemanticNodeData::Literal(LiteralValue::String(tag)) => {
                vec![DynamicRootCandidate::NativeTag { tag: tag.clone() }]
            }
            SemanticNodeData::Union(arms) => arms
                .iter()
                .flat_map(|arm| {
                    collect_dynamic_root_candidates_from_node_inner(
                        ctx, *arm, imports, active, memo,
                    )
                })
                .collect(),
            SemanticNodeData::Alias(inner) => {
                collect_dynamic_root_candidates_from_node_inner(ctx, *inner, imports, active, memo)
            }
            // A single-segment `typeof <name>` carrier maps to a component
            // import binding — the node equivalent of the `TypeOf(value_ref)`
            // arm (`value_ref.path.len() == 1`). The carrier head splits the
            // reference as `(value_root.name, path)`, so single-segment is an
            // empty trailing `path` with the head name as the binding name.
            SemanticNodeData::TypeOf(_) => match data.typeof_head() {
                Some((value_root, path)) if path.is_empty() => {
                    component_import_candidate_for_binding(imports, value_root.name.as_ref())
                        .into_iter()
                        .collect()
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        },
    };
    exit_node(node, active, memo, out)
}

/// The override-key value memo type — a per-call `SemanticNodeId` → projected
/// value key (or `None` = uncacheable) memo.
type OverrideKeyMemo = FxHashMap<SemanticNodeId, Option<FallthroughOverrideValueKey>>;

/// EXHAUSTIVE projection of a value node onto its content-free
/// [`FallthroughOverrideValueKey`]. `None` = UNCACHEABLE — the node is
/// unrepresentable for a durable cache key (`VueMacroElements`), is missing,
/// re-enters a cycle, or tripped the shared op-budget.
///
/// The walk reuses the shared persistent-memo + op-budget substrate
/// (`enter_node`/`exit_node`), so a shared override-value subtree is projected
/// once and an over-budget walk fails closed.
///
/// The inner `match` over [`SemanticNodeData`] has NO `_` wildcard: a new node
/// variant fails to compile here until it is classified, so a future field can
/// never be silently dropped from the override identity (the
/// `IndexedAccess.index` / signature / carrier-arg omission that caused the
/// cache-poison).
fn project_override_value_key_inner(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<FallthroughOverrideValueKey> {
    match enter_node(node, active, memo) {
        NodeWalkStep::Cached(cached) => return cached,
        NodeWalkStep::Halt => return None,
        NodeWalkStep::Visit => {}
    }
    let data = match node_data_for(ctx, node) {
        Some(data) => data,
        None => return exit_node(node, active, memo, None),
    };
    let result = match data.as_ref() {
        // Single-child recursive shells — direct self-calls (the
        // self-recursion the bounded-recursion guard allowlists).
        SemanticNodeData::Alias(inner) => {
            project_override_value_key_inner(ctx, dispatch, *inner, active, memo)
                .map(|child| FallthroughOverrideValueKey::Alias(Box::new(child)))
        }
        SemanticNodeData::KeyOf { base } => {
            project_override_value_key_inner(ctx, dispatch, *base, active, memo).map(|base| {
                FallthroughOverrideValueKey::KeyOf {
                    base: Box::new(base),
                }
            })
        }
        SemanticNodeData::Array { element, readonly } => {
            project_override_value_key_inner(ctx, dispatch, *element, active, memo).map(|element| {
                FallthroughOverrideValueKey::Array {
                    element: Box::new(element),
                    readonly: *readonly,
                }
            })
        }
        SemanticNodeData::ConstructorType { signature } => project_override_value_key_inner(
            ctx, dispatch, *signature, active, memo,
        )
        .map(|signature| FallthroughOverrideValueKey::ConstructorType {
            signature: Box::new(signature),
        }),
        // Multi-child structural shells.
        SemanticNodeData::Union(arms) => {
            project_override_children(ctx, dispatch, arms, active, memo)
                .map(FallthroughOverrideValueKey::Union)
        }
        SemanticNodeData::Intersection(arms) => {
            project_override_children(ctx, dispatch, arms, active, memo)
                .map(FallthroughOverrideValueKey::Intersection)
        }
        SemanticNodeData::MergedDecl { contributors } => {
            project_override_children(ctx, dispatch, contributors, active, memo)
                .map(|contributors| FallthroughOverrideValueKey::MergedDecl { contributors })
        }
        SemanticNodeData::Object(surface) => {
            project_override_surface(ctx, dispatch, surface, active, memo)
                .map(|surface| FallthroughOverrideValueKey::Object(Box::new(surface)))
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            project_override_tuple(ctx, dispatch, elements, active, memo).map(|elements| {
                FallthroughOverrideValueKey::Tuple {
                    elements,
                    readonly: *readonly,
                }
            })
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => {
            project_override_children(ctx, dispatch, expressions, active, memo).map(|expressions| {
                FallthroughOverrideValueKey::TemplateLiteral {
                    quasis: quasis.iter().cloned().collect(),
                    expressions,
                }
            })
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            project_override_indexed_access(ctx, dispatch, *object, index, active, memo)
        }
        SemanticNodeData::Mapped { source, mapper } => {
            project_override_mapped(ctx, dispatch, *source, mapper, active, memo)
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive,
        } => project_override_conditional(
            ctx,
            dispatch,
            *check,
            *extends,
            *true_branch_ref,
            *false_branch_ref,
            *distributive,
            active,
            memo,
        ),
        SemanticNodeData::Function {
            params,
            return_type,
            type_parameters,
            ..
        } => project_override_function(
            ctx,
            dispatch,
            params,
            *return_type,
            type_parameters,
            active,
            memo,
        ),
        SemanticNodeData::TypeParam {
            decl,
            param_index,
            constraint,
            default,
            ..
        } => {
            let constraint = match constraint {
                Some(node) => Some(Box::new(project_override_value_key_inner(
                    ctx, dispatch, *node, active, memo,
                )?)),
                None => None,
            };
            let default = match default {
                Some(node) => Some(Box::new(project_override_value_key_inner(
                    ctx, dispatch, *node, active, memo,
                )?)),
                None => None,
            };
            Some(FallthroughOverrideValueKey::TypeParam {
                decl: dispatch.type_slot_for(
                    std::sync::Arc::clone(&decl.canonical_id),
                    std::sync::Arc::clone(&decl.decl_name),
                ),
                param_index: *param_index,
                constraint,
                default,
            })
        }
        SemanticNodeData::DeclRef { identity } => Some(FallthroughOverrideValueKey::DeclRef {
            slot: dispatch.type_slot_for(
                std::sync::Arc::clone(&identity.canonical_id),
                std::sync::Arc::clone(&identity.decl_name),
            ),
        }),
        SemanticNodeData::InstantiationRef { base, args } => {
            project_override_children(ctx, dispatch, args, active, memo).map(|args| {
                FallthroughOverrideValueKey::InstantiationRef {
                    base: dispatch.type_slot_for(
                        std::sync::Arc::clone(&base.canonical_id),
                        std::sync::Arc::clone(&base.decl_name),
                    ),
                    args,
                }
            })
        }
        SemanticNodeData::TypeOf(_) => {
            project_override_typeof(ctx, dispatch, data.as_ref(), active, memo)
        }
        SemanticNodeData::BareRef(_) => {
            project_override_bare_ref(ctx, dispatch, data.as_ref(), active, memo)
        }
        SemanticNodeData::ImportType(_) => {
            project_override_import_type(ctx, dispatch, data.as_ref(), active, memo)
        }
        // Leaves — no node children.
        SemanticNodeData::Primitive(kind) => Some(FallthroughOverrideValueKey::Primitive(*kind)),
        SemanticNodeData::Literal(value) => {
            Some(FallthroughOverrideValueKey::Literal(value.clone()))
        }
        SemanticNodeData::Opaque(err) => Some(FallthroughOverrideValueKey::Opaque(
            OpaqueErrorKey::from_query_error(err),
        )),
        SemanticNodeData::Infer { name } => Some(FallthroughOverrideValueKey::Infer {
            name: std::sync::Arc::clone(name),
        }),
        SemanticNodeData::RawFallback { raw } => Some(FallthroughOverrideValueKey::RawFallback {
            raw: std::sync::Arc::clone(raw),
        }),
        SemanticNodeData::SyntheticBinding { id, .. } => {
            Some(FallthroughOverrideValueKey::SyntheticBinding { id: id.clone() })
        }
        // Unrepresentable as a durable, content-free key → UNCACHEABLE.
        SemanticNodeData::VueMacroElements(_) => None,
    };
    exit_node(node, active, memo, result)
}

/// Project a slice of child node ids, propagating uncacheable (`None`) if any
/// child is unrepresentable.
fn project_override_children(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    nodes: &[SemanticNodeId],
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<Vec<FallthroughOverrideValueKey>> {
    let mut out = Vec::with_capacity(nodes.len());
    for &child in nodes {
        out.push(project_override_value_key_inner(
            ctx, dispatch, child, active, memo,
        )?);
    }
    Some(out)
}

fn project_override_surface(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    surface: &crate::semantic_query::SurfaceView,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<FallthroughOverrideSurfaceKey> {
    let mut members = Vec::with_capacity(surface.members.len());
    for member in surface.members.iter() {
        members.push(FallthroughOverrideMemberKey {
            name: std::sync::Arc::clone(&member.name),
            value: project_override_value_key_inner(ctx, dispatch, member.value, active, memo)?,
            optional: member.optional,
            readonly: member.readonly,
            is_method: member.is_method,
            visibility: member.visibility,
            merge_role: member.merge_role,
        });
    }
    let mut index_signatures = Vec::with_capacity(surface.index_signatures.len());
    for sig in surface.index_signatures.iter() {
        index_signatures.push(FallthroughOverrideIndexSigKey {
            key_type: project_override_value_key_inner(ctx, dispatch, sig.key_type, active, memo)?,
            value_type: project_override_value_key_inner(
                ctx,
                dispatch,
                sig.value_type,
                active,
                memo,
            )?,
            readonly: sig.readonly,
        });
    }
    let call_signatures =
        project_override_children(ctx, dispatch, &surface.call_signatures, active, memo)?;
    let construct_signatures =
        project_override_children(ctx, dispatch, &surface.construct_signatures, active, memo)?;
    let keyspace = match surface.keyspace {
        Some(node) => Some(Box::new(project_override_value_key_inner(
            ctx, dispatch, node, active, memo,
        )?)),
        None => None,
    };
    Some(FallthroughOverrideSurfaceKey {
        members,
        index_signatures,
        call_signatures,
        construct_signatures,
        keyspace,
        has_index_signature: surface.has_index_signature,
    })
}

fn project_override_tuple(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    elements: &[crate::semantic_query::TupleElement],
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<Vec<FallthroughOverrideTupleElementKey>> {
    let mut out = Vec::with_capacity(elements.len());
    for element in elements {
        out.push(FallthroughOverrideTupleElementKey {
            label: element.label.clone(),
            value: project_override_value_key_inner(ctx, dispatch, element.value, active, memo)?,
            optional: element.optional,
            rest: element.rest,
        });
    }
    Some(out)
}

fn project_override_indexed_access(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    object: SemanticNodeId,
    index: &IndexKey,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<FallthroughOverrideValueKey> {
    let object = Box::new(project_override_value_key_inner(
        ctx, dispatch, object, active, memo,
    )?);
    let index = match index {
        IndexKey::String(name) => FallthroughOverrideIndexKey::String(std::sync::Arc::clone(name)),
        IndexKey::Number(value) => FallthroughOverrideIndexKey::Number(*value),
        IndexKey::TypeNode(node) => FallthroughOverrideIndexKey::TypeNode(Box::new(
            project_override_value_key_inner(ctx, dispatch, *node, active, memo)?,
        )),
    };
    Some(FallthroughOverrideValueKey::IndexedAccess { object, index })
}

fn project_override_mapped(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    source: SemanticNodeId,
    mapper: &crate::semantic_query::MapperKey,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<FallthroughOverrideValueKey> {
    let name_remap = match mapper.name_remap {
        Some(node) => Some(project_override_value_key_inner(
            ctx, dispatch, node, active, memo,
        )?),
        None => None,
    };
    Some(FallthroughOverrideValueKey::Mapped(Box::new(
        FallthroughOverrideMappedKey {
            source: project_override_value_key_inner(ctx, dispatch, source, active, memo)?,
            parameter_node: project_override_value_key_inner(
                ctx,
                dispatch,
                mapper.parameter_node,
                active,
                memo,
            )?,
            key_space: project_override_value_key_inner(
                ctx,
                dispatch,
                mapper.key_space,
                active,
                memo,
            )?,
            value_expr: project_override_value_key_inner(
                ctx,
                dispatch,
                mapper.value_expr,
                active,
                memo,
            )?,
            optionality: mapper.optionality,
            readonly: mapper.readonly,
            name_remap,
            kind: mapper.kind,
        },
    )))
}

#[allow(clippy::too_many_arguments)]
fn project_override_conditional(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    check: SemanticNodeId,
    extends: SemanticNodeId,
    true_branch: SemanticNodeId,
    false_branch: SemanticNodeId,
    distributive: bool,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<FallthroughOverrideValueKey> {
    Some(FallthroughOverrideValueKey::Conditional(Box::new(
        FallthroughOverrideConditionalKey {
            check: project_override_value_key_inner(ctx, dispatch, check, active, memo)?,
            extends: project_override_value_key_inner(ctx, dispatch, extends, active, memo)?,
            true_branch: project_override_value_key_inner(
                ctx,
                dispatch,
                true_branch,
                active,
                memo,
            )?,
            false_branch: project_override_value_key_inner(
                ctx,
                dispatch,
                false_branch,
                active,
                memo,
            )?,
            distributive,
        },
    )))
}

fn project_override_function(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    params: &[crate::semantic_query::FunctionParam],
    return_type: SemanticNodeId,
    type_parameters: &[crate::semantic_query::TypeParamDecl],
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<FallthroughOverrideValueKey> {
    let mut projected_params = Vec::with_capacity(params.len());
    for param in params {
        projected_params.push(FallthroughOverrideParamKey {
            name: param.name.clone(),
            ty: project_override_value_key_inner(ctx, dispatch, param.ty, active, memo)?,
            optional: param.optional,
            rest: param.rest,
        });
    }
    let mut projected_type_params = Vec::with_capacity(type_parameters.len());
    for tp in type_parameters {
        let constraint = match tp.constraint {
            Some(node) => Some(project_override_value_key_inner(
                ctx, dispatch, node, active, memo,
            )?),
            None => None,
        };
        let default = match tp.default {
            Some(node) => Some(project_override_value_key_inner(
                ctx, dispatch, node, active, memo,
            )?),
            None => None,
        };
        projected_type_params.push(FallthroughOverrideTypeParamDeclKey {
            name: std::sync::Arc::clone(&tp.name),
            constraint,
            default,
        });
    }
    Some(FallthroughOverrideValueKey::Function(Box::new(
        FallthroughOverrideFunctionKey {
            params: projected_params,
            return_type: project_override_value_key_inner(
                ctx,
                dispatch,
                return_type,
                active,
                memo,
            )?,
            type_parameters: projected_type_params,
        },
    )))
}

fn project_override_typeof(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    data: &SemanticNodeData,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<FallthroughOverrideValueKey> {
    let (value_root, path) = data.typeof_head()?;
    let type_args =
        project_override_children(ctx, dispatch, data.carrier_type_args(), active, memo)?;
    Some(FallthroughOverrideValueKey::TypeOf {
        value_root: dispatch.value_root_slot_for(value_root.clone()),
        path: path.iter().cloned().collect(),
        type_args,
    })
}

fn project_override_bare_ref(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    data: &SemanticNodeData,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<FallthroughOverrideValueKey> {
    let (name, scope) = data.bare_ref_head()?;
    let type_args =
        project_override_children(ctx, dispatch, data.carrier_type_args(), active, memo)?;
    Some(FallthroughOverrideValueKey::BareRef {
        name: std::sync::Arc::clone(name),
        scope: FallthroughOverrideScopeKey::from_node_scope(scope),
        type_args,
    })
}

fn project_override_import_type(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch,
    data: &SemanticNodeData,
    active: &mut FxHashSet<SemanticNodeId>,
    memo: &mut OverrideKeyMemo,
) -> Option<FallthroughOverrideValueKey> {
    let (specifier, qualifier, typeof_query) = data.import_type_head()?;
    let type_args =
        project_override_children(ctx, dispatch, data.carrier_type_args(), active, memo)?;
    Some(FallthroughOverrideValueKey::ImportType {
        specifier: std::sync::Arc::clone(specifier),
        qualifier: qualifier.iter().cloned().collect(),
        typeof_query,
        type_args,
    })
}

#[cfg(test)]
#[path = "fallthrough_value_eval_recursion_tests.rs"]
mod recursion_tests;
