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
        //
        // Evidence-blind-replay fence: the store-owned memo is
        // cross-request, so a walk whose canonical composite routing
        // deposited NON-TRIVIAL evidence (file self-roots, or an
        // incomplete comparison's warm suppression) must not be
        // replayed to a later request that would then skip the deposit
        // — a stale warm read served on an under-rooted entry. The
        // epoch advancing across the walk suppresses the publish; the
        // result itself still flows to the caller.
        let epoch_before = self.canonical_evidence_epoch.get();
        let result = self
            .substitute_with_change_tracking(node, parameter_node, arg)
            .0;
        if self.canonical_evidence_epoch.get() == epoch_before {
            self.graph()
                .substitute_memo_publish(node, parameter_node, arg, result);
        }
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
        // Same evidence-blind-replay fence as the top-level entry: a
        // sub-walk whose canonical routing deposited non-trivial
        // evidence is not replayable, and neither is any enclosing
        // sub-walk (ancestors observe the same epoch advance).
        let epoch_before = self.canonical_evidence_epoch.get();
        let (result, changed) =
            self.substitute_with_change_tracking_inner(node, parameter_node, arg);
        if self.canonical_evidence_epoch.get() == epoch_before {
            self.graph()
                .substitute_memo_publish(node, parameter_node, arg, result);
        }
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
        // Infer substitution is exact-binder-only. A legitimate reference is
        // an `InferRef` carrying the same opaque binder identity; a same-name
        // `TypeParam` is an unrelated declaration and is never rewritten.
        let parameter_infer_binder = match self.graph().node_data(parameter_node).as_deref() {
            Some(SemanticNodeData::Infer { binder, .. }) => Some(binder.clone()),
            _ => None,
        };
        let parameter_is_infer = parameter_infer_binder.is_some();
        match data.as_ref() {
            // Exact infer declaration/reference occurrence.
            SemanticNodeData::Infer { binder, .. } | SemanticNodeData::InferRef { binder, .. }
                if parameter_infer_binder == Some(binder.clone()) =>
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
            // Substitution is a composite CONSTRUCTION site (the ruling's
            // "substitution and post-substitution finalization" inclusion
            // arm): a CHANGED union routes through the canonical authority
            // — substituting `T := string` into `T | string` yields two
            // structurally equal arms the raw rebuild would keep (the
            // duplicate-constituent class), and `T := never` leaves a
            // `never` arm the lattice absorbs. The unchanged path still
            // short-circuits (no rebuild, no canonicalization). Union arm
            // order carries no overload precedence, so the commutative
            // route is unconditionally safe here.
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
                    self.intern_normalized_union_or_intersection(&new_members, true),
                    true,
                )
            }
            // A CHANGED intersection splits by CARRIER SEMANTICS, exactly
            // as the member-value merge does: a possibly-callable
            // contributor makes the intersection an overload-ordered
            // carrier (call resolution tries arms in declaration order),
            // so the substituted rebuild PRESERVES the authored order and
            // scope — the classification follows transparent carriers and
            // fails CLOSED on anything undecidable from the graph alone.
            // Otherwise (every substituted contributor provably
            // order-safe) the derived instantiation routes through the
            // canonical authority (structural dedup, `X & unknown = X`,
            // the proven-disjoint scalar collapse).
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
                let rebuilt = if new_members.iter().any(|member| {
                    crate::project_semantic_dispatch::walk::value_may_contribute_call_signatures(
                        self.graph(),
                        *member,
                    )
                }) {
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Intersection(
                            crate::semantic_query::composite::CompositeList::preserving_rebuild(
                                Arc::from(new_members.into_boxed_slice()),
                            ),
                        ),
                    )
                } else {
                    self.intern_normalized_union_or_intersection(&new_members, false)
                };
                (rebuilt, true)
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
                let mut new_members = Vec::with_capacity(surface.positive_members().len());
                for member in surface.positive_members().iter() {
                    let key = member.key.clone().map(
                        |computed| {
                            let (sub_key, changed) =
                                self.substitute_with_change_tracking(computed, parameter_node, arg);
                            any_changed |= changed;
                            sub_key
                        },
                        |identity| identity,
                    );
                    let (sub_value, c) =
                        self.substitute_with_change_tracking(member.value, parameter_node, arg);
                    any_changed |= c;
                    new_members.push(SurfaceMember {
                        key,
                        value: sub_value,
                        optional: member.optional,
                        readonly: member.readonly,
                        method_kind: member.method_kind,
                        has_implementation_body: member.has_implementation_body,
                        // Type-parameter substitution preserves the source
                        // member's declared accessibility and excess-property
                        // provenance (substitution changes only the value's
                        // type-param occurrences).
                        visibility: member.visibility,
                        excess_origin: member.excess_origin,
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
                let mut members = new_members.into_iter();
                let mut calls = new_call_signatures.into_iter();
                let mut constructs = new_construct_signatures.into_iter();
                let mut indexes = new_index_signatures.into_iter();
                let entries = surface
                    .entries
                    .iter()
                    .map(|entry| match entry {
                        crate::semantic_query::SurfaceEntry::Member(_) => {
                            crate::semantic_query::SurfaceEntry::Member(
                                members.next().expect("derived member index matches stream"),
                            )
                        }
                        crate::semantic_query::SurfaceEntry::CallSignature(_) => {
                            crate::semantic_query::SurfaceEntry::CallSignature(
                                calls.next().expect("derived call index matches stream"),
                            )
                        }
                        crate::semantic_query::SurfaceEntry::ConstructSignature(_) => {
                            crate::semantic_query::SurfaceEntry::ConstructSignature(
                                constructs
                                    .next()
                                    .expect("derived construct index matches stream"),
                            )
                        }
                        crate::semantic_query::SurfaceEntry::IndexSignature(_) => {
                            crate::semantic_query::SurfaceEntry::IndexSignature(
                                indexes.next().expect("derived index matches stream"),
                            )
                        }
                    })
                    .collect();
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Object(SurfaceView::from_entries(
                            entries,
                            new_keyspace,
                            surface.has_known_index_signature(),
                        )),
                    ),
                    true,
                )
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                let mut any_changed = false;
                let rebuilt = program.map_child_nodes(|child| {
                    let (substituted, changed) =
                        self.substitute_with_change_tracking(child, parameter_node, arg);
                    any_changed |= changed;
                    substituted
                });
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::ObjectSpreadProgram(rebuilt),
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
                // Exact node identity is the sole shadowing axis. A mapped
                // TypeParam that merely shares an infer display name is a
                // distinct declaration and cannot match an exact InferRef.
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
                // Capture-avoidance for an `Infer` binder — the same
                // shadow rule the Mapped arm applies, here on the
                // conditional-scope axis: a
                // nested conditional whose OWN `extends` pattern DECLARES
                // the same infer name RE-BINDS it — `Infer { name }` is
                // name-identity, so the inner declaration IS the binder
                // node. The re-binding scopes the extends pattern and the
                // TRUE branch; the check and the FALSE branch stay in the
                // OUTER binder's scope and still substitute. The
                // detection is DECLARATION-scoped
                // ([`Self::extends_pattern_declares_infer`]): a bare
                // REFERENCE to the outer binder (a same-name `TypeParam`
                // shell) does not shadow, and an `Infer` declared under a
                // deeper `Conditional`/`Mapped` scope inside `extends`
                // binds at THAT level, not here.
                let shadowed_by_inner_infer = parameter_is_infer
                    && self.extends_pattern_declares_infer(*extends, parameter_node);
                let (sub_check, cc) =
                    self.substitute_with_change_tracking(*check, parameter_node, arg);
                let (sub_extends, ec) = if shadowed_by_inner_infer {
                    (*extends, false)
                } else {
                    self.substitute_with_change_tracking(*extends, parameter_node, arg)
                };
                let (sub_true, tc) = if shadowed_by_inner_infer {
                    (*true_branch_ref, false)
                } else {
                    self.substitute_with_change_tracking(*true_branch_ref, parameter_node, arg)
                };
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
            SemanticNodeData::Signature {
                kind,
                params,
                return_type,
                type_parameters,
                occurrence,
                return_carrier,
                signature_span,
                return_type_span,
            } => {
                // Signature-local TypeParams need no spelling-based stop:
                // legitimate outer references carry an exact InferRef;
                // locally shadowed occurrences carry the local TypeParam.
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
                    // The parameter's own binder node embeds its declaration-
                    // local bounds: keep the binder verbatim when they are
                    // untouched; re-intern it with the substituted bounds
                    // (same decl identity + index + name) when they move, so
                    // `param` stays the decl's own `TypeParam` node.
                    let param = if new_constraint == tp.constraint && new_default == tp.default {
                        tp.param
                    } else {
                        match self.graph().node_data(tp.param).as_deref() {
                            Some(SemanticNodeData::TypeParam {
                                decl, param_index, ..
                            }) => self.graph().intern_preserving_scope(
                                tp.param,
                                SemanticNodeData::TypeParam {
                                    decl: decl.clone(),
                                    param_index: *param_index,
                                    constraint: new_constraint,
                                    default: new_default,
                                    display_name: Arc::clone(&tp.name),
                                },
                            ),
                            _ => tp.param,
                        }
                    };
                    new_type_parameters.push(TypeParamDecl {
                        name: Arc::clone(&tp.name),
                        param,
                        constraint: new_constraint,
                        default: new_default,
                        is_const: tp.is_const,
                    });
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Signature {
                            kind: *kind,
                            params: Arc::from(new_params.into_boxed_slice()),
                            return_type: sub_return,
                            type_parameters: Arc::from(new_type_parameters.into_boxed_slice()),
                            // Instantiation PRESERVES the occurrence — an
                            // instantiated candidate is the same occurrence.
                            occurrence: occurrence.clone(),
                            // A declared carrier retargets the substituted
                            // return node; a body-derived carrier is
                            // untouched.
                            return_carrier: match return_carrier {
                                crate::semantic_query::SignatureReturnCarrier::Declared(_) => {
                                    crate::semantic_query::SignatureReturnCarrier::Declared(
                                        sub_return,
                                    )
                                }
                                crate::semantic_query::SignatureReturnCarrier::Function(source) => {
                                    crate::semantic_query::SignatureReturnCarrier::Function(
                                        source.clone(),
                                    )
                                }
                            },
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
            // The sealed index-composed carrier substitutes its OWN
            // parameters and binder constraints/defaults; the occurrence
            // and the deferred return carrier are preserved.
            SemanticNodeData::DeferredCallable(callable) => {
                let parts = callable.parts(&crate::semantic_query::ResolveCallConsumer::witness());
                let mut any_changed = false;
                let mut new_params = Vec::with_capacity(parts.params.len());
                for param in parts.params.iter() {
                    let (sub_ty, changed) =
                        self.substitute_with_change_tracking(param.ty, parameter_node, arg);
                    any_changed |= changed;
                    new_params.push(FunctionParam {
                        name: param.name.clone(),
                        ty: sub_ty,
                        optional: param.optional,
                        rest: param.rest,
                        span: param.span,
                    });
                }
                let mut new_type_parameters = Vec::with_capacity(parts.type_parameters.len());
                for tp in parts.type_parameters.iter() {
                    let new_constraint = match tp.constraint {
                        Some(constraint) => {
                            let (sub, changed) = self.substitute_with_change_tracking(
                                constraint,
                                parameter_node,
                                arg,
                            );
                            any_changed |= changed;
                            Some(sub)
                        }
                        None => None,
                    };
                    let new_default = match tp.default {
                        Some(default) => {
                            let (sub, changed) =
                                self.substitute_with_change_tracking(default, parameter_node, arg);
                            any_changed |= changed;
                            Some(sub)
                        }
                        None => None,
                    };
                    new_type_parameters.push(TypeParamDecl {
                        name: Arc::clone(&tp.name),
                        param: tp.param,
                        constraint: new_constraint,
                        default: new_default,
                        is_const: tp.is_const,
                    });
                }
                if !any_changed {
                    return (node, false);
                }
                let rebuilt = SemanticNodeData::DeferredCallable(callable.with_substituted(
                    Arc::from(new_params.into_boxed_slice()),
                    Arc::from(new_type_parameters.into_boxed_slice()),
                ));
                (self.graph().intern_preserving_scope(node, rebuilt), true)
            }
            _ => (node, false),
        }
    }

    /// Declaration-scoped shadow predicate for the Conditional
    /// substitution arm: `true` iff `pattern` — a conditional's own
    /// `extends` pattern — DECLARES the infer binder `binder` at THIS
    /// pattern's level. Only a reachable `Infer` node with the binder's
    /// name counts (name-identity interning makes the inner declaration
    /// the binder node itself); explicitly NOT counted:
    ///
    /// - a bare REFERENCE to the name (a same-name `TypeParam` shell —
    ///   references never re-bind);
    /// - an `Infer` beneath a nested `Conditional` or `Mapped` inside the
    ///   pattern — TS scopes `infer` to the NEAREST enclosing conditional
    ///   (and a mapped type introduces its own binder scope), so such a
    ///   declaration binds at that inner level, never at this one.
    ///
    /// This is deliberately a separate predicate from
    /// [`Self::subtree_references_node`], whose unrestricted
    /// reference-reachability semantics its other callers depend on.
    pub(super) fn extends_pattern_declares_infer(
        &self,
        pattern: SemanticNodeId,
        binder: SemanticNodeId,
    ) -> bool {
        let Some(SemanticNodeData::Infer {
            binder: target_binder,
            ..
        }) = self.graph().node_data(binder).as_deref().cloned()
        else {
            return false;
        };
        let graph = self.graph();
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut stack: Vec<SemanticNodeId> = vec![pattern];
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            match data.as_ref() {
                SemanticNodeData::Infer { binder, .. } => {
                    if *binder == target_binder {
                        return true;
                    }
                }
                // A nested CONDITIONAL is the only infer-binding
                // boundary: an `infer` beneath it declares at THAT
                // conditional's level — do not descend. A `Mapped` is NOT
                // a boundary — an `infer` in its source / value /
                // `as`-remap declares for the ENCLOSING conditional (the
                // `conditional_binds_mapped_as_remap_infer_in_true_branch`
                // producer contract).
                SemanticNodeData::Conditional { .. } => {}
                SemanticNodeData::Mapped { source, mapper } => {
                    stack.push(*source);
                    stack.push(mapper.key_space);
                    stack.push(mapper.value_expr);
                    if let Some(remap) = mapper.name_remap {
                        stack.push(remap);
                    }
                }
                // A construction program is not an infer-binding boundary:
                // an `infer` inside a program effect declares for the
                // ENCLOSING conditional (same rule as an `infer` inside an
                // `Object` member value above). Mirrors the substitute
                // engine's `map_child_nodes` descent and `absorb`'s
                // `child_nodes` stack push.
                SemanticNodeData::ObjectSpreadProgram(program) => {
                    stack.extend(program.child_nodes());
                }
                SemanticNodeData::Alias(inner) => stack.push(*inner),
                SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                    for member in members.iter() {
                        stack.push(*member);
                    }
                }
                SemanticNodeData::Array { element, .. } => stack.push(*element),
                SemanticNodeData::Tuple { elements, .. } => {
                    for element in elements.iter() {
                        stack.push(element.value);
                    }
                }
                SemanticNodeData::Object(surface) => {
                    for member in surface.positive_members().iter() {
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
                SemanticNodeData::Signature {
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
                SemanticNodeData::InstantiationRef { args, .. } => {
                    for arg in args.iter() {
                        stack.push(*arg);
                    }
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    for expr in expressions.iter() {
                        stack.push(*expr);
                    }
                }
                SemanticNodeData::KeyOf { base } => stack.push(*base),
                SemanticNodeData::IndexedAccess { object, index } => {
                    stack.push(*object);
                    if let crate::semantic_query::IndexKey::Computed(idx_node) = index {
                        stack.push(*idx_node);
                    }
                }
                _ => {}
            }
        }
        false
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
    /// Unknown / Any) terminate here too — the accessor
    /// returns an empty slice for them. Cyclic graphs are guarded by a
    /// `visited` set.
    ///
    /// **`Mapped` shadowing** is honoured: when a nested mapped binder
    /// shadows the same `target` (`nested_mapper.parameter_node ==
    /// target`), the walker does NOT recurse into the shadowed
    /// `value_expr` / `name_remap` arms, matching substitute's
    /// `shadowed` short-circuit.
    ///
    /// Infer reachability mirrors substitution exactly: only `Infer` /
    /// `InferRef` nodes carrying the target declaration's opaque binder count.
    /// Display names never participate.
    pub(super) fn subtree_references_node(
        &self,
        root: SemanticNodeId,
        target: SemanticNodeId,
    ) -> bool {
        let target_infer_binder = match self.graph().node_data(target).as_deref() {
            Some(SemanticNodeData::Infer { binder, .. }) => Some(binder.clone()),
            _ => None,
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
                // Exact declaration/reference binder match.
                SemanticNodeData::Infer { binder, .. }
                | SemanticNodeData::InferRef { binder, .. } => {
                    if target_infer_binder == Some(binder.clone()) {
                        return true;
                    }
                }
                SemanticNodeData::TypeParam { .. } => {}
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
                    for member in surface.positive_members().iter() {
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
                    if let IndexKey::Computed(idx_node) = index {
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
                // Substitute descends program effects via `map_child_nodes`;
                // the scanner must descend the same children or it reports a
                // program value holding the binder as K-independent (the
                // `build_mapped_type` hoist) / binder-free (the
                // `record_target_shape` generic-key gate).
                SemanticNodeData::ObjectSpreadProgram(program) => {
                    stack.extend(program.child_nodes());
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    for contributor in contributors.iter() {
                        stack.push(*contributor);
                    }
                }
                SemanticNodeData::Signature {
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
                // Unknown, Any): none have child
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
            IndexKey::UniqueSymbol(identity) => (IndexKey::UniqueSymbol(identity.clone()), false),
            IndexKey::Computed(node) => {
                let (sub, changed) =
                    self.substitute_with_change_tracking(*node, parameter_node, arg);
                if !changed {
                    return (IndexKey::Computed(*node), false);
                }
                let normalised = self.normalized_index_key_node(sub);
                (normalised, true)
            }
        }
    }
}
