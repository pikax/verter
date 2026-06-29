//! Node-domain fallthrough value evaluation + spread / dynamic-root readers.
//!
//! The fallthrough resolver's root-consumption (spread keys), dynamic-root
//! (`is=`) and generic child-prop-override decisions read structural facts off a
//! value expression's PROJECTED NODE instead of a materialised `TypeExpr`. The
//! engine evaluates a value expression to a `SemanticNodeId` ONCE (override
//! forwarding + runtime-value env substitution + node Class-A projection); the
//! free-fn readers then walk `SemanticNodeData`. No semantic decision is taken
//! on a materialised `TypeExpr`.

use std::hash::{Hash, Hasher};

use rustc_hash::FxHashSet;
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
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{ProjectionMode, SemanticNodeData, SemanticNodeId};

/// Depth cap for the override structural content fingerprint. Bounds the
/// recursion over a value node's content (the interned arena is acyclic, but the
/// cap keeps an arbitrarily deep override value fingerprint terminating + cheap).
const OVERRIDE_FINGERPRINT_DEPTH: u32 = 64;

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

    /// Content-derived structural fingerprint over an override set's value
    /// NODES. Hashes each override's name plus a bounded recursive content hash
    /// of its value node's `SemanticNodeData` shape — NOT the raw arena
    /// `SemanticNodeId` ordinal — so the `override_fingerprint` cache-key
    /// dimension reuses a warm fallthrough surface only for equivalent overrides.
    pub(crate) fn fallthrough_override_fingerprint(
        &self,
        entries: &[FallthroughPropOverride],
    ) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        for entry in entries {
            entry.name.hash(&mut hasher);
            node_structural_content_hash(
                self.ctx,
                entry.node,
                OVERRIDE_FINGERPRINT_DEPTH,
                &mut hasher,
            );
        }
        hasher.finish()
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
    known_spread_keys_from_node_inner(ctx, node, &mut FxHashSet::default())
}

fn known_spread_keys_from_node_inner(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<KnownSpreadKeys> {
    if !active.insert(node) {
        return None;
    }
    let result = match node_data_for(ctx, node) {
        None => None,
        Some(data) => match data.as_ref() {
            // The `Alias` identity hop is the node equivalent of the
            // `TypeExpr::Parenthesized` wrap.
            SemanticNodeData::Alias(inner) => {
                known_spread_keys_from_node_inner(ctx, *inner, active)
            }
            SemanticNodeData::Object(surface) => Some(known_spread_keys_from_surface(surface)),
            SemanticNodeData::Intersection(arms) => {
                let mut result = KnownSpreadKeys {
                    exact: true,
                    ..KnownSpreadKeys::default()
                };
                let mut saw_any = false;
                for part in arms.iter() {
                    let Some(summary) = known_spread_keys_from_node_inner(ctx, *part, active)
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
                let Some(first_node) = iter.next() else {
                    active.remove(&node);
                    return None;
                };
                let Some(first) = known_spread_keys_from_node_inner(ctx, *first_node, active)
                else {
                    active.remove(&node);
                    return None;
                };
                let mut result = first.clone();
                let mut exact_same_keys = first.exact;
                let mut early_inexact = false;
                for branch in iter {
                    let Some(summary) = known_spread_keys_from_node_inner(ctx, *branch, active)
                    else {
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
            _ => None,
        },
    };
    active.remove(&node);
    result
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
    collect_dynamic_root_candidates_from_node_inner(ctx, node, imports, &mut FxHashSet::default())
}

fn collect_dynamic_root_candidates_from_node_inner(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    imports: &[AnalyzedImport],
    active: &mut FxHashSet<SemanticNodeId>,
) -> Vec<DynamicRootCandidate> {
    if !active.insert(node) {
        return Vec::new();
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
                    collect_dynamic_root_candidates_from_node_inner(ctx, *arm, imports, active)
                })
                .collect(),
            SemanticNodeData::Alias(inner) => {
                collect_dynamic_root_candidates_from_node_inner(ctx, *inner, imports, active)
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
    active.remove(&node);
    out
}

/// Bounded recursive CONTENT hash of a value node's `SemanticNodeData` shape.
///
/// Hashes the variant discriminant (so distinct kinds never collide) plus the
/// content-bearing fields, recursing into child node ids for structural shapes
/// and reading carrier HEADS for references — never the raw arena
/// `SemanticNodeId` ordinal (which is non-durable). At the depth cap a node
/// contributes its discriminant only, keeping the hash bounded + terminating.
fn node_structural_content_hash<H: Hasher>(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    depth: u32,
    hasher: &mut H,
) {
    let Some(data) = node_data_for(ctx, node) else {
        0u8.hash(hasher);
        return;
    };
    std::mem::discriminant(data.as_ref()).hash(hasher);
    if depth == 0 {
        return;
    }
    let next = depth - 1;
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => node_structural_content_hash(ctx, *inner, next, hasher),
        SemanticNodeData::Object(surface) => {
            (surface.members.len() as u64).hash(hasher);
            for member in surface.members.iter() {
                member.name.hash(hasher);
                member.optional.hash(hasher);
                member.readonly.hash(hasher);
                member.is_method.hash(hasher);
                node_structural_content_hash(ctx, member.value, next, hasher);
            }
            surface.has_index_signature.hash(hasher);
            (surface.index_signatures.len() as u64).hash(hasher);
            (surface.call_signatures.len() as u64).hash(hasher);
            (surface.construct_signatures.len() as u64).hash(hasher);
        }
        SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
            (arms.len() as u64).hash(hasher);
            for arm in arms.iter() {
                node_structural_content_hash(ctx, *arm, next, hasher);
            }
        }
        SemanticNodeData::Primitive(kind) => kind.hash(hasher),
        SemanticNodeData::Literal(value) => value.hash(hasher),
        SemanticNodeData::Array { element, readonly } => {
            readonly.hash(hasher);
            node_structural_content_hash(ctx, *element, next, hasher);
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            readonly.hash(hasher);
            (elements.len() as u64).hash(hasher);
            for element in elements.iter() {
                element.label.hash(hasher);
                element.optional.hash(hasher);
                element.rest.hash(hasher);
                node_structural_content_hash(ctx, element.value, next, hasher);
            }
        }
        SemanticNodeData::KeyOf { base } => node_structural_content_hash(ctx, *base, next, hasher),
        SemanticNodeData::IndexedAccess { object, .. } => {
            node_structural_content_hash(ctx, *object, next, hasher);
        }
        SemanticNodeData::TypeOf(_) => {
            if let Some((value_root, path)) = data.typeof_head() {
                value_root.name.hash(hasher);
                for segment in path.iter() {
                    segment.hash(hasher);
                }
            }
            for arg in data.carrier_type_args() {
                node_structural_content_hash(ctx, *arg, next, hasher);
            }
        }
        SemanticNodeData::BareRef(_) => {
            if let Some((name, _scope)) = data.bare_ref_head() {
                name.hash(hasher);
            }
            for arg in data.carrier_type_args() {
                node_structural_content_hash(ctx, *arg, next, hasher);
            }
        }
        // Remaining carriers / shells: the discriminant (hashed above)
        // distinguishes the kind; fold any carried type-args for extra
        // discrimination. Realistic fallthrough override values are literals /
        // objects / unions / `typeof` refs (covered above); the residual arms
        // keep the fingerprint stable + content-derived without an exhaustive
        // per-variant walk.
        _ => {
            for arg in data.carrier_type_args() {
                node_structural_content_hash(ctx, *arg, next, hasher);
            }
        }
    }
}
