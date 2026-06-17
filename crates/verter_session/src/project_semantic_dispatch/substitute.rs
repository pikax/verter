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
        // Cross-variant name bridge — Infer-BINDER-only, in BOTH
        // directions. `parameter_node` may be a `TypeParam` (the
        // primary path) OR an `Infer` (the Conditional reducer's
        // infer-bind consumer that interns an Infer node and calls
        // substitute). Only an `Infer` binder activates name-based
        // matching: it rewrites same-name `Infer` references AND
        // same-name `TypeParam` references (true_branch references
        // may lower as TypeParam shells via the
        // unresolved-TypeParameter path). A `TypeParam` binder never
        // matches by name — its sole authority is the node-id branch
        // above, and in particular it must NOT rewrite a same-name
        // `Infer { name }` node: TS `infer X` DECLARES a fresh
        // conditional-scoped binder that shadows the outer parameter,
        // so a collection-driven (TypeParam-binder) substitution
        // leaves it intact.
        let (parameter_name, parameter_is_infer): (Option<Arc<str>>, bool) =
            match self.graph().node_data(parameter_node).as_deref() {
                Some(SemanticNodeData::TypeParam { display_name, .. }) => {
                    (Some(Arc::clone(display_name)), false)
                }
                Some(SemanticNodeData::Infer { name }) => (Some(Arc::clone(name)), true),
                _ => (None, false),
            };
        // Cross-variant TypeParam-by-name fallback: when the caller
        // passed an Infer parameter_node, true_branch references
        // may be TypeParam nodes with the matching display_name
        // (the unresolved-TypeParameter lowering path). Match them
        // by name. This branch fires only when parameter_node is
        // an Infer (not a TypeParam) — for TypeParam parameter_node
        // the node-id branch above is the sole authority.
        if parameter_is_infer {
            if let SemanticNodeData::TypeParam { display_name, .. } = data.as_ref() {
                if let Some(name) = parameter_name.as_ref() {
                    if display_name.as_ref() == name.as_ref() {
                        return (arg, true);
                    }
                }
            }
        }
        match data.as_ref() {
            // Same-name `Infer` occurrence: rewritten ONLY for an
            // `Infer` binder. Under a `TypeParam` binder the node is
            // a fresh conditional-scoped declaration shadowing the
            // parameter — never an occurrence of it.
            SemanticNodeData::Infer { name }
                if parameter_is_infer
                    && parameter_name
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
                // Normalize-on-intern (the variadic-spread rule): a
                // substituted `rest` element whose value settled to a
                // concrete tuple splices in place — `[...A, ...B]` with
                // `A = [1, 2]`, `B = [3, 4]` rebuilds as `[1, 2, 3, 4]` —
                // and a sole rest-of-array tuple collapses to the array.
                // Open / unresolved rest values keep their carrier
                // verbatim (no forced materialisation).
                match self.normalize_tuple_spread(&new_elements, *readonly) {
                    crate::project_semantic_dispatch::build::NormalizedTupleShape::Array(
                        array_node,
                    ) => (array_node, true),
                    crate::project_semantic_dispatch::build::NormalizedTupleShape::Tuple(
                        normalized,
                    ) => (
                        self.graph().intern_preserving_scope(
                            node,
                            SemanticNodeData::Tuple {
                                elements: Arc::from(normalized.into_boxed_slice()),
                                readonly: *readonly,
                            },
                        ),
                        true,
                    ),
                }
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
            SemanticNodeData::TypeOf(_) => {
                // The value-root + path are opaque to substitution (they are
                // not node ids). Structural child-integrity requires descending
                // into the instantiation `type_args` so a `T` inside
                // `typeof f<T>` is rewritten — STRUCTURAL recursion only, NOT
                // semantic instantiation application (that is a demand-time
                // carrier-resolution reduction). An empty / no-change arg list
                // returns unchanged and records the opaque counter, preserving
                // the dormant-state behaviour. Descent reads the args through
                // the shared `carrier_type_args` accessor; the rebuild
                // preserves the head fields via `map_carrier_type_args`.
                let type_args = data.carrier_type_args();
                let mut new_args = Vec::with_capacity(type_args.len());
                let mut any_changed = false;
                for arg_node in type_args.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*arg_node, parameter_node, arg);
                    any_changed |= c;
                    new_args.push(sub);
                }
                if !any_changed {
                    // "opaque TypeOf returns" counter.
                    if let Some(observer) = verter_audit::current_observer() {
                        observer.record_event(verter_audit::AuditEvent::SubstituteTypeOfOpaque);
                    }
                    return (node, false);
                }
                let rebuilt = data
                    .map_carrier_type_args(Arc::from(new_args.into_boxed_slice()))
                    .expect("TypeOf is a carrier");
                (self.graph().intern_preserving_scope(node, rebuilt), true)
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
            // Unresolved bare-name carrier `Foo<arg…>`: descend into the
            // structural `type_args` so a `T` inside an applied carrier is
            // rewritten when the binder fires — structural child-integrity,
            // NOT semantic instantiation application (a demand-time
            // carrier-resolution concern). `name` / `scope` are preserved
            // verbatim. An empty / no-change arg list returns unchanged.
            SemanticNodeData::BareRef(_) => {
                let type_args = data.carrier_type_args();
                let mut new_args = Vec::with_capacity(type_args.len());
                let mut any_changed = false;
                for arg_node in type_args.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*arg_node, parameter_node, arg);
                    any_changed |= c;
                    new_args.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                let rebuilt = data
                    .map_carrier_type_args(Arc::from(new_args.into_boxed_slice()))
                    .expect("BareRef is a carrier");
                (self.graph().intern_preserving_scope(node, rebuilt), true)
            }
            // Unresolved import-type carrier `import("m").Q<arg…>`: descend into
            // the structural `type_args` so a `T` inside an applied import-type
            // carrier is rewritten when the binder fires — same structural
            // child-integrity as the `BareRef` arm, NOT semantic instantiation
            // application or import resolution (a demand-time carrier-resolution
            // concern). `specifier` / `qualifier` / `typeof_query` are preserved
            // verbatim. An empty / no-change arg list returns unchanged.
            SemanticNodeData::ImportType(_) => {
                let type_args = data.carrier_type_args();
                let mut new_args = Vec::with_capacity(type_args.len());
                let mut any_changed = false;
                for arg_node in type_args.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*arg_node, parameter_node, arg);
                    any_changed |= c;
                    new_args.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                let rebuilt = data
                    .map_carrier_type_args(Arc::from(new_args.into_boxed_slice()))
                    .expect("ImportType is a carrier");
                (self.graph().intern_preserving_scope(node, rebuilt), true)
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
    /// `SemanticNodeId`s descends here too — including the three
    /// unresolved carriers (`BareRef` / `TypeOf` / `ImportType`), whose
    /// `type_args` slices ARE descended. As a read-only reachability SCAN
    /// it reaches them through the shared
    /// [`SemanticNodeData::carrier_type_args`] accessor in the catch-all
    /// arm (so a future carrier added to that exhaustive accessor is
    /// descended automatically, never silently dropped) — the mirror
    /// contract. Arms that return `(node, false)` without recursion (leaf
    /// TypeParam / Primitive / Literal / Opaque / DeclRef / Never /
    /// Unknown / Any / VueMacroElements) terminate here too — the accessor
    /// returns an empty slice for them. Cyclic graphs are guarded by a
    /// `visited` set.
    ///
    /// **`Mapped` shadowing** is honoured: when a nested mapped binder
    /// shadows the same `target` (`nested_mapper.parameter_node ==
    /// target`), the walker does NOT recurse into the shadowed
    /// `value_expr` / `name_remap` arms, matching substitute's
    /// `shadowed` short-circuit.
    ///
    /// **Cross-variant `Infer { name }` fallback** mirrors substitute's
    /// Infer-BINDER-only name bridge: when the `target` binder is itself
    /// an `Infer` node, a descendant `Infer { name }` (or `TypeParam`)
    /// whose name equals the target's is treated as a reference. Under a
    /// `TypeParam` target a same-name `Infer { name }` descendant is NOT
    /// a reference — TS `infer X` declares a fresh conditional-scoped
    /// binder that shadows the outer parameter, and substitute leaves it
    /// intact — so the walker reports `false` for it, keeping the two
    /// engines in agreement.
    pub(super) fn subtree_references_node(
        &self,
        root: SemanticNodeId,
        target: SemanticNodeId,
    ) -> bool {
        let (target_name, target_is_infer): (Option<Arc<str>>, bool) =
            match self.graph().node_data(target).as_deref() {
                Some(SemanticNodeData::TypeParam { display_name, .. }) => {
                    (Some(Arc::clone(display_name)), false)
                }
                Some(SemanticNodeData::Infer { name }) => (Some(Arc::clone(name)), true),
                _ => (None, false),
            };

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
                // whose `name` matches an Infer BINDER's name is a
                // structural reference, per
                // `substitute_with_change_tracking`'s `Infer` arm.
                // Under a `TypeParam` binder the same-name `Infer` is
                // a fresh conditional-scoped declaration shadowing the
                // parameter — substitute leaves it intact, so the
                // walker must not count it as a reference.
                SemanticNodeData::Infer { name } => {
                    if target_is_infer {
                        if let Some(t) = target_name.as_ref() {
                            if t.as_ref() == name.as_ref() {
                                return true;
                            }
                        }
                    }
                }
                // A `TypeParam` reference whose `display_name` matches
                // the binder's name is ALSO a structural reference when
                // the binder itself is `Infer` (substitute's
                // TypeParam-by-name cross-variant bridge). Under the
                // `build_mapped_type` hoist contract the binder is
                // always a `TypeParam`, so this branch is a no-op for
                // the hoist caller; including it keeps the walker
                // symmetric with substitute and robust against future
                // callers passing an `Infer` target.
                SemanticNodeData::TypeParam { display_name, .. } => {
                    if target_is_infer {
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
                SemanticNodeData::MergedDecl { contributors } => {
                    for contributor in contributors.iter() {
                        stack.push(*contributor);
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
                // Unresolved carriers (`BareRef` / `TypeOf` / `ImportType`)
                // descend into their structural `type_args` exactly as
                // substitute's carrier arms do — the mirror contract. As a
                // SCAN this routes through the shared `carrier_type_args`
                // accessor in the catch-all below rather than hand-binding
                // each carrier, so a future carrier added to the exhaustive
                // accessor is descended here automatically and can never be
                // silently dropped by the catch-all.
                //
                // The catch-all also covers substitute's `_ => (node, false)`
                // leaf variants (Primitive, Literal, Opaque, DeclRef, Never,
                // Unknown, Any, VueMacroElements): none have child
                // semantic-node references, and the accessor returns an empty
                // slice for them, so the walker pushes nothing.
                other => {
                    for arg in other.carrier_type_args() {
                        stack.push(*arg);
                    }
                }
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
