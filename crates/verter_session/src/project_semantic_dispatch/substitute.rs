//! `substitute_semantic_type_param` — generic type-parameter
//! substitution into the semantic graph.
//!
//! The public substitute delegates to
//! `substitute_with_change_tracking`, which returns `(result,
//! changed)`. Each match arm short-circuits the rebuild when no
//! descendant produced a different `SemanticNodeId`, skipping
//! `intern_preserving_scope` and the per-arm `Vec<>` allocations.
//! The output is identical to the all-rebuild path (the existing
//! shard dedup collapses identical rebuilds back to the same id);
//! the change-tracking avoids the wasted recursive walk and
//! allocations on the hot substitute path.
//!
//! Both helpers operate on immutable `SemanticNodeData` and publish
//! new shell identity via [`SemanticGraphStore::intern_preserving_scope`]
//! so the rebuilt shell's scope is preserved from the origin shell.
//! The caller's completion fence observes the new dep-signature
//! through the shared memo once the substituted result enters a
//! build flow.
//!
//! **Binder identity contract.** Binder matching is done by
//! `SemanticNodeId` equality (the binder's interned `TypeParam`
//! node id) rather than by `display_name` string equality. This
//! makes substitution correct even when two binders in the same
//! file share a display name (`K`) but are otherwise distinct
//! identities — the substitute only touches the binder whose node
//! id matches the caller's `parameter_node` argument.

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::request_context::RecursiveSubstituteIdentity;
use crate::semantic_query::{
    FunctionParam, IndexKey, IndexSignature, MapperKey, QueryError, SemanticNodeData,
    SemanticNodeId, SurfaceMember, SurfaceView, TypeParamDecl,
};

impl<'a> ProjectSemanticDispatch<'a> {
    pub(super) fn substitute_semantic_type_param(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> SemanticNodeId {
        // Top-level substitute-call telemetry. Bumped at the
        // entry of `substitute_semantic_type_param` (the public
        // surface), so the snapshot reflects the number of distinct
        // top-level substitutions issued for this request — NOT the
        // recursive `substitute_with_change_tracking` walks. Paired
        // with `SubstituteMemoHit` (next line) to expose the
        // hit-rate of the hash-cons memo.
        if let Some(observer) = verter_audit::current_observer() {
            observer.record_event(verter_audit::AuditEvent::SubstituteTopLevelCall);
        }
        // Hash-cons memo. The store-owned `substitute_memo` collapses
        // identical `(value_expr, parameter_node, arg)` triples that
        // reach this entry-point from different call paths (per-K
        // materialiser loops on the SAME mapped-type instance reduced
        // from multiple components, repeated `Pick<T, K>` projections
        // across the corpus, etc.) to a single result. The store is
        // arena-scoped, and semantic-node ids inside one arena are
        // content-addressed integers — so a triple of ids is a
        // complete identity for the substitution result. Substitution
        // is a pure function of its three inputs, so the cache needs
        // no fact-signature validation.
        if let Some(cached) = self.graph().substitute_memo_get(node, parameter_node, arg) {
            if let Some(observer) = verter_audit::current_observer() {
                observer.record_event(verter_audit::AuditEvent::SubstituteMemoHit);
            }
            return cached;
        }
        // Change-tracking through recursion. The internal helper
        // returns (result, changed) so each branch can short-circuit
        // the rebuild path when no descendant produced a different
        // node id. The public signature is unchanged; callers see only
        // the result id.
        let result = self
            .substitute_with_change_tracking(node, parameter_node, arg)
            .0;
        self.graph()
            .substitute_memo_publish(node, parameter_node, arg, result);
        result
    }

    /// Change-tracking internal helper for
    /// [`Self::substitute_semantic_type_param`]. Returns
    /// `(result, changed)` where `changed` is `true` if any
    /// descendant produced a different `SemanticNodeId` from its
    /// input. When `changed` is `false`, every recursive arm
    /// returns `(node, false)` directly so the rebuild +
    /// `intern_preserving_scope` allocations are skipped entirely.
    ///
    /// Identity preservation: the existing
    /// `intern_preserving_scope` shard dedup also collapses
    /// rebuilt-but-identical structures back to the same
    /// `SemanticNodeId`, so the OUTPUT is identical between the
    /// change-tracking fast path and the unconditional rebuild
    /// path. The optimization removes the wasted recursive walk +
    /// `Vec<>` allocations on the hot substitute path.
    fn substitute_with_change_tracking(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> (SemanticNodeId, bool) {
        // Classification + recursive hash-cons memo probe.
        //
        // Key insight: the recursive helper BYPASSES the
        // top-level `substitute_memo` even though
        // `(node, parameter_node, arg)` is a complete identity for
        // the substitution result. Wiring the existing store-owned
        // memo at the recursive entry collapses repeated structural
        // sub-tree substitutions across one substitution walk and
        // across substitution walks within one workspace generation.
        //
        // Classification (unique vs repeated triples) fires on every
        // recursive entry — including the trivial-identity branch
        // (`node == parameter_node` ⇒ return `arg`) because that
        // branch is still a recursive arrival at the SAME logical
        // identity tuple and the per-request audit footprint must
        // report it. A `_repeated` bump on the identity branch is
        // structurally correct: visiting the binder twice in
        // `Foo<K, K>`-style fixtures IS the recursive memo's hit
        // case at the boundary.
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.classify_recursive_substitute(RecursiveSubstituteIdentity {
                node: node.0,
                parameter_node: parameter_node.0,
                arg: arg.0,
            });
        }
        // Trivial-identity short-circuit runs AFTER classification:
        // the per-request audit needs to see every entry, but the
        // identity branch returns `arg` without touching graph
        // state.
        if node == parameter_node {
            return (arg, true);
        }
        if let Some(cached) = self.graph().substitute_memo_get(node, parameter_node, arg) {
            if let Some(observer) = verter_audit::current_observer() {
                observer.record_event(verter_audit::AuditEvent::RecursiveSubstituteMemoHit);
            }
            // The memo stores the substitution RESULT. `changed` is
            // recovered cheaply from `result != node` — this is
            // safe because substitution is a pure
            // function of its three inputs and `intern_preserving_scope`
            // already dedups structurally-equivalent rebuilds back
            // to the same `SemanticNodeId`. A stored result that
            // equals `node` is exactly the "no descendant changed"
            // outcome the change-tracking short-circuit reports.
            let changed = cached != node;
            return (cached, changed);
        }
        let (result, changed) =
            self.substitute_with_change_tracking_inner(node, parameter_node, arg);
        self.graph()
            .substitute_memo_publish(node, parameter_node, arg, result);
        (result, changed)
    }

    /// Inner body of [`Self::substitute_with_change_tracking`].
    /// Split out so the public wrapper owns the recursive
    /// classification + hash-cons memo probe / publish, leaving
    /// the structural match arms purely descent-focused.
    fn substitute_with_change_tracking_inner(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> (SemanticNodeId, bool) {
        // Node-id equality match BEFORE destructuring. A string-
        // match arm
        // (`TypeParam { display_name, .. } if display_name == parameter`)
        // would substitute every binder sharing the parameter's
        // display_name; under the complete identity model, only the
        // binder whose `SemanticNodeId` matches gets substituted.
        if node == parameter_node {
            return (arg, true);
        }
        let Some(data) = self.graph().node_data(node) else {
            return (self.opaque(QueryError::Miss), true);
        };
        // Cross-variant name fallback. `parameter_node` may be a
        // `TypeParam` (the primary path) OR an `Infer` (the
        // build.rs:1432 Infer-arm cross-variant consumer that
        // interns an Infer node and calls substitute). Extract the
        // binder's "name" from either variant so the Infer arm
        // matches Infer references AND any TypeParam references in
        // `node` whose display_name matches the binder's name. The
        // latter handles cases where a TypeScript `infer X` is
        // referenced in the true_branch as a regular TypeParam
        // (lowered from a name-only reference), not as a literal
        // Infer node.
        //
        // Item 8's "leave Infer string-match alone" is
        // preserved; the addition is purely cross-variant
        // bridging — it does NOT fall back for ordinary TypeParam
        // substitutions (those go through the node-id branch above).
        // C11a re-evaluates whether nested-infer needs full
        // node-id matching.
        let parameter_name: Option<Arc<str>> =
            self.graph()
                .node_data(parameter_node)
                .and_then(|param_data| match param_data.as_ref() {
                    SemanticNodeData::TypeParam { display_name, .. } => {
                        Some(Arc::clone(display_name))
                    }
                    SemanticNodeData::Infer { name } => Some(Arc::clone(name)),
                    _ => None,
                });
        // Cross-variant TypeParam-by-name fallback: when the caller
        // passed an Infer parameter_node, true_branch references
        // may be TypeParam nodes with the matching display_name
        // (the unresolved-TypeParameter lowering path). Match them
        // by name. This branch fires only when parameter_node is
        // an Infer (not a TypeParam) — for TypeParam parameter_node
        // the node-id branch above is the sole authority.
        if let Some(SemanticNodeData::Infer { .. }) =
            self.graph().node_data(parameter_node).as_deref()
        {
            if let SemanticNodeData::TypeParam { display_name, .. } = data.as_ref() {
                if let Some(name) = parameter_name.as_ref() {
                    if display_name.as_ref() == name.as_ref() {
                        return (arg, true);
                    }
                }
            }
        }
        match data.as_ref() {
            SemanticNodeData::Infer { name }
                if parameter_name
                    .as_ref()
                    .map(|n| n.as_ref() == name.as_ref())
                    .unwrap_or(false) =>
            {
                (arg, true)
            }
            SemanticNodeData::Alias(target) => {
                let (sub, changed) =
                    self.substitute_with_change_tracking(*target, parameter_node, arg);
                if !changed {
                    return (node, false);
                }
                (
                    self.graph()
                        .intern_preserving_scope(node, SemanticNodeData::Alias(sub)),
                    true,
                )
            }
            SemanticNodeData::Union(members) => {
                let mut new_members = Vec::with_capacity(members.len());
                let mut any_changed = false;
                for member in members.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*member, parameter_node, arg);
                    any_changed |= c;
                    new_members.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Union(Arc::from(new_members.into_boxed_slice())),
                    ),
                    true,
                )
            }
            SemanticNodeData::Intersection(members) => {
                let mut new_members = Vec::with_capacity(members.len());
                let mut any_changed = false;
                for member in members.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*member, parameter_node, arg);
                    any_changed |= c;
                    new_members.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Intersection(Arc::from(new_members.into_boxed_slice())),
                    ),
                    true,
                )
            }
            SemanticNodeData::MergedDecl { contributors } => {
                // Substitute into each merged contributor, preserving the
                // distinct carrier so the peer-merge reducer still applies to
                // the instantiated declaration.
                let mut new_contributors = Vec::with_capacity(contributors.len());
                let mut any_changed = false;
                for contributor in contributors.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*contributor, parameter_node, arg);
                    any_changed |= c;
                    new_contributors.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::MergedDecl {
                            contributors: Arc::from(new_contributors.into_boxed_slice()),
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::Array { element, readonly } => {
                let (sub_element, changed) =
                    self.substitute_with_change_tracking(*element, parameter_node, arg);
                if !changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Array {
                            element: sub_element,
                            readonly: *readonly,
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::Tuple { elements, readonly } => {
                let mut new_elements = Vec::with_capacity(elements.len());
                let mut any_changed = false;
                for element in elements.iter() {
                    let (sub_value, c) =
                        self.substitute_with_change_tracking(element.value, parameter_node, arg);
                    any_changed |= c;
                    new_elements.push(crate::semantic_query::TupleElement {
                        label: element.label.clone(),
                        value: sub_value,
                        optional: element.optional,
                        rest: element.rest,
                    });
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Tuple {
                            elements: Arc::from(new_elements.into_boxed_slice()),
                            readonly: *readonly,
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::Object(surface) => {
                let mut any_changed = false;
                let mut new_members = Vec::with_capacity(surface.members.len());
                for member in surface.members.iter() {
                    let (sub_value, c) =
                        self.substitute_with_change_tracking(member.value, parameter_node, arg);
                    any_changed |= c;
                    new_members.push(SurfaceMember {
                        name: Arc::clone(&member.name),
                        value: sub_value,
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                        // Type-parameter substitution preserves the source
                        // member's declared accessibility (substitution changes
                        // only the value's type-param occurrences).
                        visibility: member.visibility,
                        // Type-parameter substitution preserves the
                        // source member's structural shape — only the
                        // value's type-param occurrences change.
                        // Preserve the upstream
                        // `declared_in_macro_type_arg` fact, merge role, the
                        // member's OXC declaration-site spans, and its
                        // declaration file (substitution does not move the
                        // member's declaration site).
                        spans: member.spans,
                        declaration_origin: member.declaration_origin.clone(),
                        declared_in_macro_type_arg: member.declared_in_macro_type_arg,
                        merge_role: member.merge_role,
                    });
                }
                let mut new_call_signatures = Vec::with_capacity(surface.call_signatures.len());
                for signature in surface.call_signatures.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*signature, parameter_node, arg);
                    any_changed |= c;
                    new_call_signatures.push(sub);
                }
                let mut new_construct_signatures =
                    Vec::with_capacity(surface.construct_signatures.len());
                for signature in surface.construct_signatures.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*signature, parameter_node, arg);
                    any_changed |= c;
                    new_construct_signatures.push(sub);
                }
                let mut new_index_signatures = Vec::with_capacity(surface.index_signatures.len());
                for signature in surface.index_signatures.iter() {
                    let (sub_key, ck) = self.substitute_with_change_tracking(
                        signature.key_type,
                        parameter_node,
                        arg,
                    );
                    let (sub_value, cv) = self.substitute_with_change_tracking(
                        signature.value_type,
                        parameter_node,
                        arg,
                    );
                    any_changed |= ck || cv;
                    new_index_signatures.push(IndexSignature {
                        key_type: sub_key,
                        value_type: sub_value,
                        readonly: signature.readonly,
                        // Preserve the index signature's OXC spans + declaration file.
                        spans: signature.spans,
                        declaration_origin: signature.declaration_origin.clone(),
                    });
                }
                let new_keyspace = match surface.keyspace {
                    Some(k) => {
                        let (sub, c) = self.substitute_with_change_tracking(k, parameter_node, arg);
                        any_changed |= c;
                        Some(sub)
                    }
                    None => None,
                };
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Object(SurfaceView {
                            members: Arc::from(new_members.into_boxed_slice()),
                            call_signatures: Arc::from(new_call_signatures.into_boxed_slice()),
                            construct_signatures: Arc::from(
                                new_construct_signatures.into_boxed_slice(),
                            ),
                            index_signatures: Arc::from(new_index_signatures.into_boxed_slice()),
                            keyspace: new_keyspace,
                            has_index_signature: surface.has_index_signature,
                        }),
                    ),
                    true,
                )
            }
            SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            } => {
                let mut new_expressions = Vec::with_capacity(expressions.len());
                let mut any_changed = false;
                for expr in expressions.iter() {
                    let (sub, c) = self.substitute_with_change_tracking(*expr, parameter_node, arg);
                    any_changed |= c;
                    new_expressions.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::TemplateLiteral {
                            quasis: Arc::clone(quasis),
                            expressions: Arc::from(new_expressions.into_boxed_slice()),
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::KeyOf { base } => {
                let (sub_base, changed) =
                    self.substitute_with_change_tracking(*base, parameter_node, arg);
                if !changed {
                    return (node, false);
                }
                (
                    self.graph()
                        .intern_preserving_scope(node, SemanticNodeData::KeyOf { base: sub_base }),
                    true,
                )
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                let (sub_object, oc) =
                    self.substitute_with_change_tracking(*object, parameter_node, arg);
                let (sub_index, ic) =
                    self.substitute_index_key_with_change_tracking(index, parameter_node, arg);
                if !(oc || ic) {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::IndexedAccess {
                            object: sub_object,
                            index: sub_index,
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::Mapped { source, mapper } => {
                // "mapped descents" counter.
                if let Some(observer) = verter_audit::current_observer() {
                    observer.record_event(verter_audit::AuditEvent::SubstituteMappedTypeDescend);
                }
                // Shadowing check by node-id equality. Both
                // `mapper.parameter` and `parameter` are
                // `SemanticNodeId`s, so the comparison is direct
                // and distinguishes a mapper that binds the same
                // node-id as the caller's `parameter_node` from one
                // that binds a structurally-equivalent but distinct
                // binder identity.
                let shadowed = mapper.parameter_node == parameter_node;
                let (sub_source, source_changed) =
                    self.substitute_with_change_tracking(*source, parameter_node, arg);
                let (sub_key_space, key_space_changed) =
                    self.substitute_with_change_tracking(mapper.key_space, parameter_node, arg);
                let (sub_value_expr, value_expr_changed) = if shadowed {
                    (mapper.value_expr, false)
                } else {
                    self.substitute_with_change_tracking(mapper.value_expr, parameter_node, arg)
                };
                let (sub_name_remap, name_remap_changed) = match mapper.name_remap {
                    Some(n) if !shadowed => {
                        let (sub, c) = self.substitute_with_change_tracking(n, parameter_node, arg);
                        (Some(sub), c)
                    }
                    other => (other, false),
                };
                let any_changed =
                    source_changed || key_space_changed || value_expr_changed || name_remap_changed;
                if !any_changed {
                    return (node, false);
                }
                // "Mapped rebuilt" counter.
                // Distinct from `SubstituteMappedTypeDescend` (every
                // visit) — fires only on the rebuild branch after at
                // least one descendant sub-tree changed.
                if let Some(observer) = verter_audit::current_observer() {
                    observer.record_event(verter_audit::AuditEvent::SubstituteMappedRebuild);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Mapped {
                            source: sub_source,
                            mapper: MapperKey {
                                parameter_node: mapper.parameter_node,
                                key_space: sub_key_space,
                                value_expr: sub_value_expr,
                                optionality: mapper.optionality,
                                readonly: mapper.readonly,
                                name_remap: sub_name_remap,
                                // Propagate the lowering-time mapper kind
                                // through substitution. Identity classification
                                // is preserved because substitution applies
                                // uniformly to both `source` and
                                // `value_expr.object` for non-shadowed
                                // substitutions, so a mapper that lowered as
                                // identity `T[K]` remains identity after the
                                // type-parameter rewrite.
                                kind: mapper.kind,
                            },
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::TypeOf { .. } => {
                // TypeOf carries an opaque value-root + path. Substitution
                // never descends into it, so the input node id is returned
                // unchanged with `changed = false` — the caller skips the
                // rebuild + re-intern entirely.
                //
                // "opaque TypeOf returns"
                // counter (brief site `substitute.rs:452`).
                if let Some(observer) = verter_audit::current_observer() {
                    observer.record_event(verter_audit::AuditEvent::SubstituteTypeOfOpaque);
                }
                (node, false)
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
            } => {
                // "conditional descents"
                // counter — every visit of the Conditional arm
                // descends into its four sub-trees, regardless of
                // whether the rebuild ultimately fires.
                if let Some(observer) = verter_audit::current_observer() {
                    observer.record_event(verter_audit::AuditEvent::SubstituteConditionalDescend);
                }
                let (sub_check, cc) =
                    self.substitute_with_change_tracking(*check, parameter_node, arg);
                let (sub_extends, ec) =
                    self.substitute_with_change_tracking(*extends, parameter_node, arg);
                let (sub_true, tc) =
                    self.substitute_with_change_tracking(*true_branch_ref, parameter_node, arg);
                let (sub_false, fc) =
                    self.substitute_with_change_tracking(*false_branch_ref, parameter_node, arg);
                if !(cc || ec || tc || fc) {
                    return (node, false);
                }
                // "Conditional rebuilt"
                // counter. Distinct from `SubstituteConditionalDescend`
                // (every visit) — fires only on the rebuild branch.
                if let Some(observer) = verter_audit::current_observer() {
                    observer.record_event(verter_audit::AuditEvent::SubstituteConditionalRebuild);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Conditional {
                            check: sub_check,
                            extends: sub_extends,
                            true_branch_ref: sub_true,
                            false_branch_ref: sub_false,
                            distributive: *distributive,
                        },
                    ),
                    true,
                )
            }
            // InstantiationRef is a lazy carrier of `Helper<arg1, arg2>`
            // where `base` is the declaration identity and `args` is the
            // call-site type-argument vector. Substitution must descend
            // into each arg so type-parameter references inside an
            // unrealised instantiation (e.g. `Helper<TPlan, K>` inside a
            // mapped-type binder loop) are rewritten when the outer
            // binder fires per-key realisation. `base`/`DeclIdentity`
            // carries no type-parameter references and is preserved
            // verbatim. Without this descent, an unrealised
            // `Instantiate { ExtendSlotWithPlan<TPlan, "badge"> }` body
            // would re-bind its inner `TKey ← K-typeparam` instead of
            // `TKey ← "badge"-literal`, and its Conditional payload
            // would never close.
            SemanticNodeData::InstantiationRef { base, args } => {
                let mut new_args = Vec::with_capacity(args.len());
                let mut any_changed = false;
                for arg_node in args.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*arg_node, parameter_node, arg);
                    any_changed |= c;
                    new_args.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::InstantiationRef {
                            base: base.clone(),
                            args: Arc::from(new_args.into_boxed_slice()),
                        },
                    ),
                    true,
                )
            }
            // Function arm. Substitution must descend into every
            // parameter type and the return type so `T` / `infer X`
            // references inside `(x: T, y: infer X) => R` are rewritten
            // when the outer binder fires. This is the primary
            // materialisation path for nested-infer in TS conditional
            // `extends` clauses.
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
                signature_span,
                return_type_span,
            } => {
                let mut any_changed = false;
                let mut new_params = Vec::with_capacity(params.len());
                for param in params.iter() {
                    let (sub_ty, c) =
                        self.substitute_with_change_tracking(param.ty, parameter_node, arg);
                    any_changed |= c;
                    new_params.push(FunctionParam {
                        name: param.name.clone(),
                        ty: sub_ty,
                        optional: param.optional,
                        rest: param.rest,
                        // Substitution preserves the parameter's OXC span.
                        span: param.span,
                    });
                }
                let (sub_return, return_changed) =
                    self.substitute_with_change_tracking(*return_type, parameter_node, arg);
                any_changed |= return_changed;
                let mut new_type_parameters = Vec::with_capacity(type_parameters.len());
                for tp in type_parameters.iter() {
                    let new_constraint = match tp.constraint {
                        Some(c) => {
                            let (sub, ch) =
                                self.substitute_with_change_tracking(c, parameter_node, arg);
                            any_changed |= ch;
                            Some(sub)
                        }
                        None => None,
                    };
                    let new_default = match tp.default {
                        Some(d) => {
                            let (sub, ch) =
                                self.substitute_with_change_tracking(d, parameter_node, arg);
                            any_changed |= ch;
                            Some(sub)
                        }
                        None => None,
                    };
                    new_type_parameters.push(TypeParamDecl {
                        name: Arc::clone(&tp.name),
                        constraint: new_constraint,
                        default: new_default,
                    });
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Function {
                            params: Arc::from(new_params.into_boxed_slice()),
                            return_type: sub_return,
                            type_parameters: Arc::from(new_type_parameters.into_boxed_slice()),
                            // Substitution preserves the signature's OXC spans.
                            signature_span: *signature_span,
                            return_type_span: *return_type_span,
                        },
                    ),
                    true,
                )
            }
            _ => (node, false),
        }
    }

    /// Read-only walker that returns `true` iff `target` is structurally
    /// reachable from `root` through the same recursion edges that
    /// [`Self::substitute_with_change_tracking`] descends into. Mirrors
    /// the substitute helper's structural recursion so the two stay in
    /// lock-step: a `false` return here means an identical
    /// `substitute_with_change_tracking(root, target, _)` would return
    /// `(root, false)` — i.e. no descendant references `target` so
    /// substitution is the identity on the entire subtree.
    ///
    /// Used by [`Self::build_mapped_type`] to hoist key-independent
    /// `value_expr` reduction out of the per-K materialisation loop:
    /// when the mapper's binder is not reachable inside `value_expr`,
    /// the per-K substituted carrier collapses to `value_expr` itself
    /// for every K, so the downstream evaluation is shared across the
    /// entire key space and the materialiser runs ONCE per mapped type
    /// rather than ONCE per enumerated key.
    ///
    /// **Recursion mirrors substitute exactly.** Every arm of
    /// `substitute_with_change_tracking` that descends into child
    /// `SemanticNodeId`s descends here too; arms that return
    /// `(node, false)` without recursion (TypeOf, leaf TypeParam /
    /// Primitive / Literal / Opaque / DeclRef / Never / Unknown / Any /
    /// VueMacroElements) terminate here too. Cyclic graphs are guarded
    /// by a `visited` set.
    ///
    /// **`Mapped` shadowing** is honoured: when a nested mapped binder
    /// shadows the same `target` (`nested_mapper.parameter_node ==
    /// target`), the walker does NOT recurse into the shadowed
    /// `value_expr` / `name_remap` arms, matching substitute's
    /// `shadowed` short-circuit.
    ///
    /// **Cross-variant `Infer { name }` fallback** mirrors substitute's
    /// name-based bridge at line 131 of `substitute_with_change_tracking`:
    /// when the descendant is an `Infer { name }` and `name` equals the
    /// `target` binder's display name (extracted from either `TypeParam`
    /// or `Infer`), substitute treats it as a reference. The walker
    /// returns `true` for that case so the hoist correctly declines on
    /// `infer`-bearing value expressions whose `infer`-name shadows the
    /// outer binder.
    pub(super) fn subtree_references_node(
        &self,
        root: SemanticNodeId,
        target: SemanticNodeId,
    ) -> bool {
        let target_name: Option<Arc<str>> =
            self.graph()
                .node_data(target)
                .and_then(|d| match d.as_ref() {
                    SemanticNodeData::TypeParam { display_name, .. } => {
                        Some(Arc::clone(display_name))
                    }
                    SemanticNodeData::Infer { name } => Some(Arc::clone(name)),
                    _ => None,
                });

        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut stack: Vec<SemanticNodeId> = Vec::new();
        stack.push(root);
        while let Some(node) = stack.pop() {
            if node == target {
                return true;
            }
            if !visited.insert(node) {
                continue;
            }
            let Some(data) = self.graph().node_data(node) else {
                continue;
            };
            match data.as_ref() {
                // Cross-variant name fallback: an `Infer { name }`
                // whose `name` matches the binder's display name is a
                // structural reference, per
                // `substitute_with_change_tracking`'s `Infer` arm.
                SemanticNodeData::Infer { name } => {
                    if let Some(t) = target_name.as_ref() {
                        if t.as_ref() == name.as_ref() {
                            return true;
                        }
                    }
                }
                // A `TypeParam` reference whose `display_name` matches
                // the binder's name is ALSO a structural reference when
                // the binder itself is `Infer` (substitute's lines 120–
                // 130 cross-variant bridge). Under the `build_mapped_type`
                // hoist contract the binder is always a `TypeParam`, so
                // this branch is a no-op for the hoist caller; including
                // it keeps the walker symmetric with substitute and
                // robust against future callers passing an `Infer` target.
                SemanticNodeData::TypeParam { display_name, .. } => {
                    if matches!(
                        self.graph().node_data(target).as_deref(),
                        Some(SemanticNodeData::Infer { .. })
                    ) {
                        if let Some(t) = target_name.as_ref() {
                            if t.as_ref() == display_name.as_ref() {
                                return true;
                            }
                        }
                    }
                }
                SemanticNodeData::Alias(t) => {
                    stack.push(*t);
                }
                SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                    for member in members.iter() {
                        stack.push(*member);
                    }
                }
                SemanticNodeData::Array { element, .. } => {
                    stack.push(*element);
                }
                SemanticNodeData::Tuple { elements, .. } => {
                    for element in elements.iter() {
                        stack.push(element.value);
                    }
                }
                SemanticNodeData::Object(surface) => {
                    for member in surface.members.iter() {
                        stack.push(member.value);
                    }
                    for signature in surface.call_signatures.iter() {
                        stack.push(*signature);
                    }
                    for signature in surface.construct_signatures.iter() {
                        stack.push(*signature);
                    }
                    for signature in surface.index_signatures.iter() {
                        stack.push(signature.key_type);
                        stack.push(signature.value_type);
                    }
                    if let Some(k) = surface.keyspace {
                        stack.push(k);
                    }
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    for expr in expressions.iter() {
                        stack.push(*expr);
                    }
                }
                SemanticNodeData::KeyOf { base } => {
                    stack.push(*base);
                }
                SemanticNodeData::IndexedAccess { object, index } => {
                    stack.push(*object);
                    if let IndexKey::TypeNode(idx_node) = index {
                        stack.push(*idx_node);
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    let shadowed = mapper.parameter_node == target;
                    stack.push(*source);
                    stack.push(mapper.key_space);
                    if !shadowed {
                        stack.push(mapper.value_expr);
                        if let Some(remap) = mapper.name_remap {
                            stack.push(remap);
                        }
                    }
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    ..
                } => {
                    stack.push(*check);
                    stack.push(*extends);
                    stack.push(*true_branch_ref);
                    stack.push(*false_branch_ref);
                }
                SemanticNodeData::InstantiationRef { args, .. } => {
                    for arg in args.iter() {
                        stack.push(*arg);
                    }
                }
                SemanticNodeData::Function {
                    params,
                    return_type,
                    type_parameters,
                    ..
                } => {
                    for param in params.iter() {
                        stack.push(param.ty);
                    }
                    stack.push(*return_type);
                    for tp in type_parameters.iter() {
                        if let Some(c) = tp.constraint {
                            stack.push(c);
                        }
                        if let Some(d) = tp.default {
                            stack.push(d);
                        }
                    }
                }
                // Substitute's catch-all `_ => (node, false)` covers
                // TypeOf, Primitive, Literal, Opaque, DeclRef, Never,
                // Unknown, Any, VueMacroElements. None of these have
                // child semantic-node references that substitute would
                // recurse into, so neither does the walker.
                _ => {}
            }
        }
        false
    }

    /// Change-tracking companion of `substitute_index_key`.
    /// Returns `(result, changed)` where `changed` is `true` iff
    /// the underlying typed-node substitution actually rewrote the
    /// key. The string / number key arms always return `false`
    /// because their content is invariant under type-parameter
    /// substitution.
    fn substitute_index_key_with_change_tracking(
        &self,
        index: &IndexKey,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> (IndexKey, bool) {
        match index {
            IndexKey::String(text) => (IndexKey::String(Arc::clone(text)), false),
            IndexKey::Number(number) => (IndexKey::Number(*number), false),
            IndexKey::TypeNode(node) => {
                let (sub, changed) =
                    self.substitute_with_change_tracking(*node, parameter_node, arg);
                if !changed {
                    return (IndexKey::TypeNode(*node), false);
                }
                let normalised = self.normalized_index_key_node(sub);
                (normalised, true)
            }
        }
    }
}
