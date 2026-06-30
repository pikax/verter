//! Shared node-domain callable / signature view.
//!
//! `CallableNodeView` and `SignatureNodeView` are lightweight borrowed value
//! types over the shared `ProjectSemanticDispatch` graph. They answer the
//! callable/signature questions the Vue and Svelte framework-surface
//! normalizers need (emits, slots, slot bindings, snippets, callback events)
//! ENTIRELY in the node domain: every method returns node-domain facts
//! (`SemanticNodeId`, `Arc<[FunctionParam]>`-derived data, `TypeInfoSurface`,
//! `Arc<str>` event names) and decides only on [`SemanticNodeData`].
//!
//! The view NEVER materializes a `TypeExpr` and NEVER decides on one: it does
//! not accept an `OutputProjector`, does not call
//! `materialize_output_type_expr` / `materialize_reduced_output_type_expr` /
//! `into_type_expr`, and holds no `TypeExpr`. Materialization stays at the
//! framework normalizers' existing terminal DTO output caps — decide in the
//! node domain, materialize once.
//!
//! The view COMPOSES [`realize_callable_member`] (the shared carrier-shell
//! normalizer) rather than duplicating its carrier walk, and OWNS the
//! Union/Intersection callable-arm recursion so the Vue and Svelte normalizers
//! never iterate `SemanticNodeData::Union` themselves.
#![allow(dead_code)] // consumed by the Vue/Svelte normalizers in §5a SP2/SP3; removed as each method is wired

use std::sync::Arc;

use verter_span::Span;

use crate::meta_resolve::dispatch_helpers::realize_callable_member;
use crate::meta_resolve::slot_binding_graph::slot_param_root_is_symbolic_only;
use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    LiteralValue, PathSegment, PrimitiveKind, ProjectionMode, ProjectionReductionContext,
    SemanticNodeData, SemanticNodeId,
};
use crate::typeinfo::surface::TypeInfoSurface;

/// Carrier / composite recursion fuse — mirrors [`realize_callable_member`]'s
/// own depth-32 bound. Real carrier nesting is shallow (Alias → InstantiationRef
/// → Conditional → Function is depth 4); the fuse fails loudly on pathological
/// graphs without consuming the test budget.
const CALLABLE_VIEW_DEPTH_FUSE: u32 = 32;

/// How to combine the RETURN types of a multi-arm slot callable — the
/// node-domain analogue of the `vue_exec::normalize::ArmCombine` enum. The
/// first params are ALWAYS intersected (a binding a template can rely on must
/// hold across every arm); only the return-type combiner differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArmCombineNode {
    Intersection,
    Union,
}

/// One positional parameter binding fact — the node-domain analogue of the
/// Svelte snippet normalizer's per-position binding. Carries the optional
/// source label (the parameter / tuple-element label, `None` when anonymous;
/// the consumer applies its own `arg{index}` fallback) and the binding type as
/// a `SemanticNodeId`. No `TypeExpr` anywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PositionalParamNode {
    pub(crate) label: Option<Arc<str>>,
    pub(crate) ty: SemanticNodeId,
}

/// Node-domain analogue of `slot_callable_param_and_return`'s return tuple: the
/// first-param binding node (when every arm supplies one), the combined return
/// node, and the return-type annotation span (only present for a single-arm
/// callable — a composed multi-arm callable has no single span). All facts are
/// node-domain; no `TypeExpr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlotCallableNodeParts {
    pub(crate) first_param: Option<SemanticNodeId>,
    pub(crate) return_type: Option<SemanticNodeId>,
    pub(crate) return_type_span: Option<Span>,
}

/// A lightweight borrowed view over a callable ROOT node in the shared graph.
pub(crate) struct CallableNodeView<'a, 'ctx> {
    dispatch: &'a ProjectSemanticDispatch<'ctx>,
    root: SemanticNodeId,
}

/// A lightweight borrowed view over a node guaranteed to be a
/// [`SemanticNodeData::Function`] (the realized signature of a callable).
pub(crate) struct SignatureNodeView<'a, 'ctx> {
    dispatch: &'a ProjectSemanticDispatch<'ctx>,
    /// A node guaranteed to intern as `SemanticNodeData::Function` by
    /// construction ([`CallableNodeView::signature`] only mints this view when
    /// the realized root is a `Function`).
    function: SemanticNodeId,
}

impl<'a, 'ctx> CallableNodeView<'a, 'ctx> {
    pub(crate) fn new(dispatch: &'a ProjectSemanticDispatch<'ctx>, root: SemanticNodeId) -> Self {
        Self { dispatch, root }
    }

    fn data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        node_data_for(self.dispatch.ctx, node)
    }

    fn intern(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.dispatch
            .ctx
            .project_type_store()
            .semantic_graph()
            .intern_node(data)
    }

    /// Normalize the root to its underlying callable node through the shared
    /// carrier-shell normalizer. Returns a `Function` node, or a
    /// `Union` / `Intersection` of realized `Function` arms, or `None` when the
    /// root does not realize to a callable.
    pub(crate) fn realized_callable_root(
        &self,
        context: ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        realize_callable_member(self.dispatch, self.root, context)
    }

    /// The single callable `Function` node the root denotes after stripping the
    /// nullish (`undefined` / `null`) arms an EXPLICIT nullish union/
    /// intersection VALUE carries — the node-domain replacement for the
    /// `TypeExpr` `callable_arm_from_raised`.
    ///
    /// It layers a stricter "exactly one callable after nullish stripping"
    /// classifier ON TOP of [`realize_callable_member`] (it does not subsume
    /// it): a non-callable non-nullish arm, or two distinct callable arms, both
    /// refuse (`None`).
    pub(crate) fn single_callable_arm(
        &self,
        context: ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        self.classify_single_callable(self.root, context, 0)
    }

    fn classify_single_callable(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        depth: u32,
    ) -> Option<SemanticNodeId> {
        if depth > CALLABLE_VIEW_DEPTH_FUSE {
            return None;
        }
        let data = self.data(node)?;
        match data.as_ref() {
            // A realized callable — return verbatim.
            SemanticNodeData::Function { .. } => Some(node),

            // The view OWNS the arm recursion. Skip nullish arms, compose
            // `realize_callable_member` on each non-nullish arm, then require
            // exactly one distinct callable function node.
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let mut callable: Option<SemanticNodeId> = None;
                for arm in arms.iter() {
                    // Nullish arms (`undefined` / `null`) of an explicit nullish
                    // composite are stripped — they are not the callable.
                    if self.node_is_nullish_primitive(*arm) {
                        continue;
                    }
                    // A non-nullish, non-callable arm means the root is not a
                    // pure callable — refuse.
                    let realized = realize_callable_member(self.dispatch, *arm, context)?;
                    let found = self.classify_single_callable(realized, context, depth + 1)?;
                    match callable {
                        // A second, distinct callable arm is ambiguous — refuse
                        // rather than pick one.
                        Some(existing) if existing != found => return None,
                        Some(_) => {}
                        None => callable = Some(found),
                    }
                }
                callable
            }

            // A carrier shell (Alias / DeclRef / InstantiationRef / Conditional
            // / DeclPlaceholder): normalize through the shared realizer, then
            // reclassify. A no-progress realize (or a non-callable shape)
            // refuses.
            _ => {
                let realized = realize_callable_member(self.dispatch, node, context)?;
                if realized == node {
                    None
                } else {
                    self.classify_single_callable(realized, context, depth + 1)
                }
            }
        }
    }

    fn node_is_nullish_primitive(&self, node: SemanticNodeId) -> bool {
        matches!(
            self.data(node).as_deref(),
            Some(SemanticNodeData::Primitive(
                PrimitiveKind::Null | PrimitiveKind::Undefined
            ))
        )
    }

    /// The realized callable as a [`SignatureNodeView`]. `None` when the root
    /// does not realize to a single `Function` node (e.g. a multi-arm
    /// composite, or a non-callable).
    pub(crate) fn signature(
        &self,
        context: ProjectionReductionContext,
    ) -> Option<SignatureNodeView<'a, 'ctx>> {
        let realized = realize_callable_member(self.dispatch, self.root, context)?;
        match self.data(realized).as_deref() {
            Some(SemanticNodeData::Function { .. }) => Some(SignatureNodeView {
                dispatch: self.dispatch,
                function: realized,
            }),
            _ => None,
        }
    }

    /// The Vue emit event name(s) the realized callable's FIRST parameter
    /// declares — its string-literal type, or each `Literal(String)` of a union
    /// first parameter, flattened recursively. `None` when the root is not a
    /// callable, has no first parameter, or the first parameter carries no
    /// string literal. Generalizes the node-domain `call_signature_event_names`
    /// / `collect_string_literal_names` precedent. Zero materialization.
    pub(crate) fn event_names(&self, context: ProjectionReductionContext) -> Option<Vec<Arc<str>>> {
        let callable = realize_callable_member(self.dispatch, self.root, context)?;
        let data = self.data(callable)?;
        let SemanticNodeData::Function { params, .. } = data.as_ref() else {
            return None;
        };
        let first = params.first()?;
        let first_ty = first.ty;
        drop(data);
        let mut names = Vec::new();
        self.collect_string_literal_names(first_ty, &mut names);
        if names.is_empty() {
            None
        } else {
            Some(names)
        }
    }

    fn collect_string_literal_names(&self, node: SemanticNodeId, out: &mut Vec<Arc<str>>) {
        let Some(data) = self.data(node) else {
            return;
        };
        match data.as_ref() {
            SemanticNodeData::Literal(LiteralValue::String(name)) => {
                out.push(Arc::from(name.as_str()));
            }
            SemanticNodeData::Union(members) => {
                let members = Arc::clone(members);
                drop(data);
                for member in members.iter() {
                    self.collect_string_literal_names(*member, out);
                }
            }
            _ => {}
        }
    }

    /// Project the realized callable's FIRST-param node to its one-level object
    /// surface in the NODE domain — reusing the shared symbolic-only gate and
    /// shallow-surface synthesiser the DTO slot-binding path uses. The first
    /// param is taken from the realized signature node directly; it is NOT
    /// re-materialized to a `TypeExpr` and re-navigated.
    ///
    /// `None` when the root is not a single callable, has no first parameter,
    /// or the first-param root is symbolic-only (an open Conditional / mapped /
    /// indexed / free `TypeParam`).
    pub(crate) fn first_param_object_surface(
        &self,
        ctx: &dyn ResolverContext,
        context: ProjectionReductionContext,
    ) -> Option<TypeInfoSurface> {
        let signature = self.signature(context)?;
        let first_param = signature.first_param()?;
        // Open-generic gate: a symbolic-only param root must NOT be materialised
        // into a committed object surface — the SAME gate
        // `navigate_param_to_object_surface` applies, keeping both binding paths
        // in agreement.
        if slot_param_root_is_symbolic_only(self.dispatch, first_param, 0) {
            return None;
        }
        ctx.host_for_fact_tracer_install()
            .project_shallow_surface_from_base(
                ctx,
                self.dispatch,
                first_param,
                Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
                ProjectionReductionContext::published(ProjectionMode::Shallow),
            )
    }

    /// All positional params of the realized callable — `this` skipped, a
    /// rest-tuple param expanded into one entry per tuple element. `None` when
    /// the root does not realize to a single `Function`.
    pub(crate) fn positional_params(
        &self,
        context: ProjectionReductionContext,
    ) -> Option<Vec<PositionalParamNode>> {
        Some(self.signature(context)?.positional_params_expanded())
    }

    /// The Vue multi-arm slot first-param + return facts. The root is realized
    /// to a callable (a `Function`, or a `Union` / `Intersection` of realized
    /// `Function` arms); the first-param binding exists ONLY when EVERY callable
    /// arm supplies a first param (else `first_param = None` — a no-param arm
    /// guarantees no binding); the return combines by `combine`.
    ///
    /// FAILS CLOSED (`None`) when the root does not realize to a callable, when
    /// any arm is non-callable after realization (which
    /// [`realize_callable_member`] already rejects for a composite), or when any
    /// required node data is missing.
    pub(crate) fn slot_param_and_return_by_arm(
        &self,
        combine: ArmCombineNode,
        context: ProjectionReductionContext,
    ) -> Option<SlotCallableNodeParts> {
        let realized = realize_callable_member(self.dispatch, self.root, context)?;
        let data = self.data(realized)?;
        match data.as_ref() {
            SemanticNodeData::Function {
                params,
                return_type,
                return_type_span,
                ..
            } => Some(SlotCallableNodeParts {
                first_param: params.first().map(|p| p.ty),
                return_type: Some(*return_type),
                return_type_span: *return_type_span,
            }),
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                self.combine_slot_arms(&arms, combine)
            }
            _ => None,
        }
    }

    fn combine_slot_arms(
        &self,
        arms: &[SemanticNodeId],
        combine: ArmCombineNode,
    ) -> Option<SlotCallableNodeParts> {
        let mut first_params: Vec<SemanticNodeId> = Vec::new();
        let mut returns: Vec<SemanticNodeId> = Vec::new();
        // A binding is guaranteed only when EVERY arm contributes a first param.
        let mut all_arms_have_first_param = true;
        for arm in arms {
            let arm_data = self.data(*arm)?;
            // FAIL CLOSED: a non-`Function` arm means the member is not purely
            // slot-callable.
            let SemanticNodeData::Function {
                params,
                return_type,
                ..
            } = arm_data.as_ref()
            else {
                return None;
            };
            match params.first() {
                Some(p) => first_params.push(p.ty),
                None => all_arms_have_first_param = false,
            }
            // Node-domain `Function` always carries a return-type node.
            returns.push(*return_type);
        }
        if first_params.is_empty() && returns.is_empty() {
            return None;
        }
        // First params: the INTERSECTION — but ONLY when every arm supplied one.
        // A no-param arm guarantees nothing, so the bindings are dropped.
        let first_param = if all_arms_have_first_param {
            match first_params.len() {
                0 => None,
                1 => Some(first_params[0]),
                _ => Some(self.intern(SemanticNodeData::Intersection(Arc::from(
                    first_params.into_boxed_slice(),
                )))),
            }
        } else {
            None
        };
        // Returns: combine per the caller's arm kind.
        let return_type = match returns.len() {
            0 => None,
            1 => Some(returns[0]),
            _ => {
                let boxed: Arc<[SemanticNodeId]> = Arc::from(returns.into_boxed_slice());
                Some(self.intern(match combine {
                    ArmCombineNode::Intersection => SemanticNodeData::Intersection(boxed),
                    ArmCombineNode::Union => SemanticNodeData::Union(boxed),
                }))
            }
        };
        Some(SlotCallableNodeParts {
            first_param,
            return_type,
            // A composed multi-arm callable has no single return-type span.
            return_type_span: None,
        })
    }
}

impl SignatureNodeView<'_, '_> {
    fn data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        node_data_for(self.dispatch.ctx, node)
    }

    /// The signature's first-parameter node, if any.
    pub(crate) fn first_param(&self) -> Option<SemanticNodeId> {
        let data = self.data(self.function)?;
        let SemanticNodeData::Function { params, .. } = data.as_ref() else {
            return None;
        };
        params.first().map(|p| p.ty)
    }

    /// The signature's return-type node. `function` is a `Function` by
    /// construction, so the fallback is unreachable.
    pub(crate) fn return_type(&self) -> SemanticNodeId {
        match self.data(self.function).as_deref() {
            Some(SemanticNodeData::Function { return_type, .. }) => *return_type,
            _ => self.function,
        }
    }

    /// The return-type annotation span, if the signature carries one.
    pub(crate) fn return_type_span(&self) -> Option<Span> {
        match self.data(self.function).as_deref() {
            Some(SemanticNodeData::Function {
                return_type_span, ..
            }) => *return_type_span,
            _ => None,
        }
    }

    /// All positional params — the leading `this` param skipped and a rest-tuple
    /// param expanded into one [`PositionalParamNode`] per tuple element. A rest
    /// param whose type is NOT a tuple (an open generic / `unknown[]`) carries
    /// no enumerable positional bindings.
    pub(crate) fn positional_params_expanded(&self) -> Vec<PositionalParamNode> {
        let Some(data) = self.data(self.function) else {
            return Vec::new();
        };
        let SemanticNodeData::Function { params, .. } = data.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for param in params.iter() {
            // Skip the `this` parameter (a snippet's vendored call signature is
            // `(this: void, ...args: Params)`); only the leading param can be
            // named `this` in valid TypeScript.
            if param.name.as_deref() == Some("this") {
                continue;
            }
            if param.rest {
                // A rest-tuple param spreads its tuple element-wise.
                if let Some(SemanticNodeData::Tuple { elements, .. }) =
                    self.data(param.ty).as_deref()
                {
                    for element in elements.iter() {
                        out.push(PositionalParamNode {
                            label: element.label.clone(),
                            ty: element.value,
                        });
                    }
                }
                continue;
            }
            out.push(PositionalParamNode {
                label: param.name.clone(),
                ty: param.ty,
            });
        }
        out
    }
}

#[cfg(test)]
#[path = "callable_view_tests.rs"]
mod tests;
