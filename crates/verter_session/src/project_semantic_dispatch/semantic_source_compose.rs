//! The fact-shell COMPOSITION half of the shared source-raising bridge
//! ([`super::semantic_source`]): the private `ProjectSemanticDispatch`
//! methods that compose closed / projected / synthesized FACT SHELLS into
//! carrier nodes — data assembly over already-defined closed facts, with
//! every interior body position lowered through the memoized locator query
//! and reference RESOLUTION still happening only at the consuming dispatch
//! demands. The fact-shell seam keeps composition focused here while the raise
//! ENTRIES, strict-raise sink, and locator absolutization stay in the parent module.

use std::sync::Arc;

use verter_type_expr::facts::{
    FactOrLocator, FunctionSignatureFact, IndexedAccessFact, LeafTypeFact, ObjectMemberFact,
    ObjectShapeFact, SynthesizedTypeFact, TuplePayloadFact,
};
use verter_type_expr::locators::{AuthoredBodyLocator, TypeBodySlot};

use super::semantic_source::{absolutize_locator, SourceRaiseContext, SourceRaiseOutcome};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    HotTypeRef, IndexKey, IndexSignature, NodeScopeId, QueryError, SemanticNodeData,
    SemanticNodeId, SurfaceEntry, SurfaceMember, SurfaceView, TupleElement,
};

impl ProjectSemanticDispatch<'_> {
    // ── fact-shell composition (private) ─────────────────────────────────

    /// Lower one `TypeBodySlot` (a decl-body sub-position) through the
    /// memoized locator query. Returns the typed
    /// [`SourceRaiseOutcome`] — a deref that fails with a non-absence
    /// disposition travels as a typed failure instead of vanishing.
    pub(in crate::project_semantic_dispatch) fn raise_body_slot(
        &self,
        slot: &TypeBodySlot,
        scope_canonical_id: &str,
    ) -> SourceRaiseOutcome {
        let locator = absolutize_locator(
            &AuthoredBodyLocator::DeclBody(slot.clone()),
            scope_canonical_id,
        );
        let owner = slot.anchor.owner;
        SourceRaiseOutcome::from_read(self.lower_locator(locator), |err| {
            self.intern_control_carrier(err, scope_canonical_id, owner)
        })
    }

    /// The file scope composed fact-shell nodes intern under: the raise
    /// scope's live file identity (mirroring the in-scope lowerer).
    pub(in crate::project_semantic_dispatch) fn raise_scope(
        &self,
        ctx: &SourceRaiseContext<'_>,
    ) -> NodeScopeId {
        let whole_hash = self
            .ctx
            .shallow_file_state(ctx.scope_canonical_id)
            .map(|state| state.whole_hash)
            .unwrap_or_default();
        NodeScopeId::File {
            canonical_id: Arc::from(ctx.scope_canonical_id),
            owner: ctx.scope_owner,
            whole_hash,
            local_scope: None,
        }
    }

    /// Intern the typed-miss carrier for an absent / unraisable interior
    /// position of a composed fact shell. The miss is a genuine typed
    /// non-result the demand side re-derives — never a fabricated body.
    fn miss_node(&self, scope: &NodeScopeId) -> SemanticNodeId {
        self.graph()
            .intern_node_with_scope(SemanticNodeData::Opaque(QueryError::Miss), scope.clone())
    }

    /// Raise one REQUIRED (schema-present) interior position of a composed
    /// fact shell. On a failed dereference the typed miss carrier is
    /// interned exactly as before AND — on the strict path — the failure is
    /// recorded at `step`'s nested position so the strict raise entry
    /// propagates it instead of letting the shell render it as `Unknown`.
    /// A SUCCESSFUL deref whose raised body still materializes an
    /// unknown-materializing failure records the conservative typed failure
    /// at the same position (strict path only). Genuinely ABSENT schema
    /// positions must NOT route through here — the schema `Option` is the
    /// absence proof that keeps their interned miss a valid typed
    /// `Unknown`.
    pub(in crate::project_semantic_dispatch) fn raise_required_interior(
        &self,
        ctx: &SourceRaiseContext<'_>,
        scope: &NodeScopeId,
        step: crate::meta_resolve::InteriorSourceStep,
        raise: impl FnOnce() -> Option<HotTypeRef>,
    ) -> SemanticNodeId {
        ctx.with_interior_step(step, || match raise() {
            Some(hot) => {
                ctx.check_raised_unknown_materializing(self, Some(&hot));
                hot.node()
            }
            None => {
                ctx.record_interior_failure();
                self.miss_node(scope)
            }
        })
    }

    /// Lower one fact-or-locator interior position. The POSITION step (a
    /// member, a tuple element, ...) is pushed by the caller; a failed
    /// dereference of the schema-present value records at that position on
    /// the strict path (every `FactOrLocator` arm carries a value), and a
    /// successful deref whose raised body materializes an
    /// unknown-materializing failure records the conservative typed
    /// failure at the same position.
    fn raise_fact_or_locator(
        &self,
        value: &FactOrLocator,
        ctx: &SourceRaiseContext<'_>,
        scope: &NodeScopeId,
    ) -> SemanticNodeId {
        let required = |raise: &dyn Fn() -> Option<HotTypeRef>| match raise() {
            Some(hot) => {
                ctx.check_raised_unknown_materializing(self, Some(&hot));
                hot.node()
            }
            None => {
                ctx.record_interior_failure();
                self.miss_node(scope)
            }
        };
        match value {
            FactOrLocator::Leaf(leaf) => required(&|| self.raise_leaf_fact(leaf, ctx)),
            // A closed union of leaves composes exactly as the top-level
            // `ClosedTypeFact::LeafUnion` source arm: each leaf lowers
            // through the shared in-scope lowerer and the ORDERED union node
            // is interned as data (a decided result — no re-resolution, no
            // normalization pass).
            FactOrLocator::LeafUnion(leaves) => {
                let members: Vec<SemanticNodeId> = leaves
                    .iter()
                    .enumerate()
                    .map(|(ordinal, leaf)| {
                        self.raise_required_interior(
                            ctx,
                            scope,
                            crate::meta_resolve::InteriorSourceStep::UnionArm {
                                ordinal: ordinal as u32,
                            },
                            || self.raise_leaf_fact(leaf, ctx),
                        )
                    })
                    .collect();
                self.graph().intern_node_with_scope(
                    SemanticNodeData::Union(Arc::from(members.into_boxed_slice())),
                    scope.clone(),
                )
            }
            FactOrLocator::Locator(slot) => required(&|| {
                self.raise_body_slot(slot, ctx.scope_canonical_id)
                    .at_optional_boundary()
            }),
            // The authored macro-payload escape (a synthesized component
            // default's `$props` / `$emit` / `$slots` / `$events` member
            // value): the payload lowers through the same single-engine
            // authored-locator routing as a `SemanticTypeSource::Authored`
            // position (hot-mirror producer for the type-argument position,
            // memoized locator deref otherwise).
            FactOrLocator::MacroPayload(payload) => required(&|| {
                let locator = absolutize_locator(
                    &AuthoredBodyLocator::MacroPayload(payload.clone()),
                    ctx.scope_canonical_id,
                );
                self.raise_authored_locator_to_hot(&locator, ctx.context)
                    .at_optional_boundary()
            }),
            // A fabricated depth-closed sub-object surface: named leaf
            // members compose directly (leaves lower through the shared
            // in-scope lowerer; there is no deeper structure by schema).
            FactOrLocator::LeafObject(members) => {
                let members: Vec<SurfaceMember> = members
                    .iter()
                    .map(|member| SurfaceMember {
                        key: crate::semantic_query::AuthoredPropertyKey::string(
                            member.name.as_str(),
                        ),
                        value: self.raise_required_interior(
                            ctx,
                            scope,
                            crate::meta_resolve::InteriorSourceStep::Member(
                                verter_type_expr::facts::FactAuthoredPropertyKey::string(
                                    member.name.as_str(),
                                ),
                            ),
                            || self.raise_leaf_fact(&member.ty, ctx),
                        ),
                        optional: member.optional,
                        readonly: false,
                        method_kind: None,
                        has_implementation_body: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        // Fact rehydration (declaration domain) is never a
                        // literal origin.
                        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                        spans: verter_type_expr::MemberSpans::default(),
                        declaration_origin: scope.canonical_file(),
                        declared_in_macro_type_arg:
                            crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                    })
                    .collect();
                self.graph().intern_node_with_scope(
                    SemanticNodeData::Object(SurfaceView::from_members(members, None)),
                    scope.clone(),
                )
            }
        }
    }

    /// Compose a synthesized shape ([`SynthesizedTypeFact`] =
    /// `ResolvedLocalShape`) into its carrier node.
    pub(in crate::project_semantic_dispatch) fn raise_synthesized_shape(
        &self,
        shape: &SynthesizedTypeFact,
        ctx: &SourceRaiseContext<'_>,
    ) -> HotTypeRef {
        let scope = self.raise_scope(ctx);
        match shape {
            verter_type_expr::facts::ResolvedLocalShape::Object(members) => {
                let members: Vec<SurfaceMember> = members
                    .iter()
                    .map(|member| SurfaceMember {
                        key: crate::semantic_query::AuthoredPropertyKey::string(
                            member.name.as_str(),
                        ),
                        value: ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::Member(
                                verter_type_expr::facts::FactAuthoredPropertyKey::string(
                                    member.name.as_str(),
                                ),
                            ),
                            || self.raise_fact_or_locator(&member.ty, ctx, &scope),
                        ),
                        optional: member.optional,
                        readonly: false,
                        method_kind: None,
                        has_implementation_body: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        // Fact rehydration (declaration domain) is never a
                        // literal origin.
                        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                        spans: verter_type_expr::MemberSpans::default(),
                        declaration_origin: scope.canonical_file(),
                        declared_in_macro_type_arg:
                            crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                    })
                    .collect();
                HotTypeRef::new(self.graph().intern_node_with_scope(
                    SemanticNodeData::Object(SurfaceView::from_members(members, None)),
                    scope,
                ))
            }
            verter_type_expr::facts::ResolvedLocalShape::Tuple(tuple) => {
                self.compose_tuple_fact_node(tuple, ctx)
            }
            verter_type_expr::facts::ResolvedLocalShape::IndexedAccess(access) => {
                self.compose_indexed_access_fact_node(access, ctx)
            }
            verter_type_expr::facts::ResolvedLocalShape::Leaf(leaf) => {
                self.raise_leaf_fact(leaf, ctx).unwrap_or_else(|| {
                    ctx.record_interior_failure();
                    HotTypeRef::new(self.miss_node(&scope))
                })
            }
            verter_type_expr::facts::ResolvedLocalShape::Ref(symbol) => {
                match self.raise_symbol_ref(symbol, ctx) {
                    Some(hot) => {
                        // A deref'd symbol body's interior failure records
                        // the conservative typed failure (strict path).
                        ctx.check_raised_unknown_materializing(self, Some(&hot));
                        hot
                    }
                    None => {
                        ctx.record_interior_failure();
                        HotTypeRef::new(self.miss_node(&scope))
                    }
                }
            }
        }
    }

    /// Compose a closed object-shape fact into an `Object` carrier node —
    /// member values lower through their body slots.
    pub(in crate::project_semantic_dispatch) fn compose_object_fact_node(
        &self,
        object: &ObjectShapeFact,
        ctx: &SourceRaiseContext<'_>,
    ) -> HotTypeRef {
        let scope = self.raise_scope(ctx);
        // A spread-bearing shape materializes through the shared spread
        // materializer's fold: direct runs compose as plain shapes, spread
        // operands raise through their body slots — never a silently
        // spread-less surface.
        if object
            .members
            .iter()
            .any(|m| matches!(m, ObjectMemberFact::Spread(_)))
        {
            return self.compose_spread_object_fact_node(object, ctx, &scope);
        }
        let mut entries = Vec::with_capacity(object.members.len());
        let mut call_ordinal = 0_u32;
        let mut construct_ordinal = 0_u32;
        let mut index_ordinal = 0_u32;
        for member in object.members.iter() {
            match member {
                // Unreachable by construction: the spread-bearing check above
                // routes the whole object through the spread fold before this
                // plain member loop runs.
                ObjectMemberFact::Spread(_) => {}
                ObjectMemberFact::Property(property) => {
                    let key = property.key.clone().map(
                        |slot| {
                            self.raise_body_slot(&slot, ctx.scope_canonical_id)
                                .at_optional_boundary()
                                .map(HotTypeRef::node)
                                .unwrap_or_else(|| self.miss_node(&scope))
                        },
                        |identity| identity,
                    );
                    entries.push(SurfaceEntry::Member(SurfaceMember {
                        key,
                        value: self.raise_required_interior(
                            ctx,
                            &scope,
                            crate::meta_resolve::InteriorSourceStep::Member(property.key.clone()),
                            || {
                                self.raise_body_slot(&property.ty, ctx.scope_canonical_id)
                                    .at_optional_boundary()
                            },
                        ),
                        optional: property.optional,
                        readonly: property.readonly,
                        method_kind: None,
                        has_implementation_body: false,
                        visibility: property.visibility,
                        // Fact rehydration (declaration domain) is never a
                        // literal origin.
                        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                        spans: verter_type_expr::MemberSpans::default(),
                        declaration_origin: scope.canonical_file(),
                        declared_in_macro_type_arg:
                            crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                    }));
                }
                ObjectMemberFact::Method(method) => {
                    let value = ctx.with_interior_step(
                        crate::meta_resolve::InteriorSourceStep::Member(method.key.clone()),
                        || self.compose_function_fact_node(&method.function, ctx, false),
                    );
                    entries.push(SurfaceEntry::Member(SurfaceMember {
                        key: method.key.clone().map(
                            |slot| {
                                self.raise_body_slot(&slot, ctx.scope_canonical_id)
                                    .at_optional_boundary()
                                    .map(HotTypeRef::node)
                                    .unwrap_or_else(|| self.miss_node(&scope))
                            },
                            |identity| identity,
                        ),
                        value: value.node(),
                        optional: method.optional,
                        readonly: false,
                        method_kind: Some(method.method_kind),
                        has_implementation_body: method.function.has_implementation_body,
                        visibility: method.visibility,
                        // Fact rehydration (declaration domain) is never a
                        // literal origin.
                        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                        spans: verter_type_expr::MemberSpans::default(),
                        declaration_origin: scope.canonical_file(),
                        declared_in_macro_type_arg:
                            crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                    }));
                }
                ObjectMemberFact::CallSignature(signature) => {
                    let ordinal = call_ordinal;
                    call_ordinal += 1;
                    entries.push(SurfaceEntry::CallSignature(
                        ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::CallSignature { ordinal },
                            || self.compose_function_fact_node(signature, ctx, false),
                        )
                        .node(),
                    ));
                }
                ObjectMemberFact::ConstructSignature(signature) => {
                    let ordinal = construct_ordinal;
                    construct_ordinal += 1;
                    entries.push(SurfaceEntry::ConstructSignature(
                        ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::ConstructSignature { ordinal },
                            || self.compose_function_fact_node(signature, ctx, false),
                        )
                        .node(),
                    ));
                }
                ObjectMemberFact::IndexSignature(signature) => {
                    let ordinal = index_ordinal;
                    index_ordinal += 1;
                    entries.push(SurfaceEntry::IndexSignature(IndexSignature {
                        key_type: ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::IndexSignatureKey { ordinal },
                            || self.raise_key_type_shape(&signature.key_type, ctx, &scope),
                        ),
                        value_type: self.raise_required_interior(
                            ctx,
                            &scope,
                            crate::meta_resolve::InteriorSourceStep::IndexSignatureValue {
                                ordinal,
                            },
                            || {
                                self.raise_body_slot(&signature.value_type, ctx.scope_canonical_id)
                                    .at_optional_boundary()
                            },
                        ),
                        readonly: signature.readonly,
                        spans: verter_type_expr::IndexSignatureSpans::default(),
                        declaration_origin: scope.canonical_file(),
                    }));
                }
            }
        }
        let has_index_signature = index_ordinal != 0;
        HotTypeRef::new(self.graph().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView::from_entries(
                entries,
                None,
                has_index_signature,
            )),
            scope,
        ))
    }

    /// Compose a spread-bearing fact into its canonical ordered program.
    fn compose_spread_object_fact_node(
        &self,
        object: &ObjectShapeFact,
        ctx: &SourceRaiseContext<'_>,
        scope: &NodeScopeId,
    ) -> HotTypeRef {
        let mut effects = Vec::new();
        let mut run: Vec<ObjectMemberFact> = Vec::new();
        let flush_run =
            |run: &mut Vec<ObjectMemberFact>,
             effects: &mut Vec<crate::semantic_query::ObjectConstructionEffect>| {
                if run.is_empty() {
                    return;
                }
                let run_fact = ObjectShapeFact {
                    members: Arc::from(std::mem::take(run).into_boxed_slice()),
                };
                let node = self.compose_object_fact_node(&run_fact, ctx).node();
                let Some(SemanticNodeData::Object(surface)) =
                    self.graph().node_data(node).as_deref().cloned()
                else {
                    return;
                };
                effects.extend(
                    super::object_spread_program_lowering::direct_effects_from_surface(&surface),
                );
            };
        for member in object.members.iter() {
            match member {
                ObjectMemberFact::Spread(spread) => {
                    flush_run(&mut run, &mut effects);
                    let operand = match self
                        .raise_body_slot(&spread.ty, ctx.scope_canonical_id)
                        .at_optional_boundary()
                    {
                        Some(hot) => hot.node(),
                        // An unresolvable operand fails the whole shape
                        // closed — never a silently spread-less surface.
                        None => return HotTypeRef::new(self.miss_node(scope)),
                    };
                    effects.push(crate::semantic_query::ObjectConstructionEffect::Spread(
                        operand,
                    ));
                }
                other => run.push(other.clone()),
            }
        }
        flush_run(&mut run, &mut effects);
        HotTypeRef::new(self.graph().intern_node_with_scope(
            SemanticNodeData::ObjectSpreadProgram(crate::semantic_query::ObjectSpreadProgram {
                effects: Arc::from(effects),
            }),
            scope.clone(),
        ))
    }

    /// Compose a projected whole-surface fact into an `Object` carrier node.
    pub(in crate::project_semantic_dispatch) fn compose_projected_surface_node(
        &self,
        surface: &verter_type_expr::facts::ProjectedSurfaceFact,
        ctx: &SourceRaiseContext<'_>,
    ) -> HotTypeRef {
        let scope = self.raise_scope(ctx);
        let members: Vec<SurfaceMember> = surface
            .members
            .iter()
            .map(|member| SurfaceMember {
                key: member.key.clone().map(
                    |slot| {
                        self.raise_body_slot(&slot, ctx.scope_canonical_id)
                            .at_optional_boundary()
                            .map(HotTypeRef::node)
                            .unwrap_or_else(|| self.miss_node(&scope))
                    },
                    |identity| identity,
                ),
                value: self.raise_required_interior(
                    ctx,
                    &scope,
                    crate::meta_resolve::InteriorSourceStep::Member(member.key.clone()),
                    || {
                        self.raise_body_slot(&member.ty, ctx.scope_canonical_id)
                            .at_optional_boundary()
                    },
                ),
                optional: member.optional,
                readonly: member.readonly,
                method_kind: member.method_kind,
                has_implementation_body: member.has_implementation_body,
                visibility: member.visibility,
                // Fact rehydration (declaration domain) is never a literal
                // origin.
                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                spans: verter_type_expr::MemberSpans::default(),
                declaration_origin: declaration_origin_file(&member.declaration_origin),
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            })
            .collect();
        let call_signatures: Vec<SemanticNodeId> = surface
            .call_signatures
            .iter()
            .enumerate()
            .map(|(ordinal, signature)| {
                ctx.with_interior_step(
                    crate::meta_resolve::InteriorSourceStep::CallSignature {
                        ordinal: ordinal as u32,
                    },
                    || self.compose_function_fact_node(signature, ctx, false),
                )
                .node()
            })
            .collect();
        let construct_signatures: Vec<SemanticNodeId> = surface
            .construct_signatures
            .iter()
            .enumerate()
            .map(|(ordinal, signature)| {
                ctx.with_interior_step(
                    crate::meta_resolve::InteriorSourceStep::ConstructSignature {
                        ordinal: ordinal as u32,
                    },
                    || self.compose_function_fact_node(signature, ctx, false),
                )
                .node()
            })
            .collect();
        let index_signatures: Vec<IndexSignature> = surface
            .index_signatures
            .iter()
            .enumerate()
            .map(|(ordinal, signature)| IndexSignature {
                key_type: ctx.with_interior_step(
                    crate::meta_resolve::InteriorSourceStep::IndexSignatureKey {
                        ordinal: ordinal as u32,
                    },
                    || self.raise_key_type_shape(&signature.key_type, ctx, &scope),
                ),
                value_type: self.raise_required_interior(
                    ctx,
                    &scope,
                    crate::meta_resolve::InteriorSourceStep::IndexSignatureValue {
                        ordinal: ordinal as u32,
                    },
                    || {
                        self.raise_body_slot(&signature.value_type, ctx.scope_canonical_id)
                            .at_optional_boundary()
                    },
                ),
                readonly: signature.readonly,
                spans: verter_type_expr::IndexSignatureSpans::default(),
                declaration_origin: declaration_origin_file(&signature.declaration_origin),
            })
            .collect();
        let has_index_signature = surface.has_index_signature || !index_signatures.is_empty();
        let entries = members
            .into_iter()
            .map(SurfaceEntry::Member)
            .chain(call_signatures.into_iter().map(SurfaceEntry::CallSignature))
            .chain(
                construct_signatures
                    .into_iter()
                    .map(SurfaceEntry::ConstructSignature),
            )
            .chain(
                index_signatures
                    .into_iter()
                    .map(SurfaceEntry::IndexSignature),
            )
            .collect();
        HotTypeRef::new(self.graph().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView::from_entries(
                entries,
                None,
                has_index_signature,
            )),
            scope,
        ))
    }

    /// Lower a declared index-signature key SHAPE. The position step is
    /// pushed by the caller; every arm is schema-present, so a failed lower
    /// records on the strict path (and a successful deref whose raised body
    /// materializes an unknown-materializing failure records the
    /// conservative typed failure).
    fn raise_key_type_shape(
        &self,
        key: &verter_type_expr::facts::KeyTypeShape,
        ctx: &SourceRaiseContext<'_>,
        scope: &NodeScopeId,
    ) -> SemanticNodeId {
        use verter_type_expr::facts::KeyTypeShape;
        let required = |raise: &dyn Fn() -> Option<HotTypeRef>| match raise() {
            Some(hot) => {
                ctx.check_raised_unknown_materializing(self, Some(&hot));
                hot.node()
            }
            None => {
                ctx.record_interior_failure();
                self.miss_node(scope)
            }
        };
        match key {
            KeyTypeShape::String => required(&|| {
                self.raise_leaf_fact(
                    &LeafTypeFact::Primitive(verter_type_expr::PrimitiveName::String),
                    ctx,
                )
            }),
            KeyTypeShape::Number => required(&|| {
                self.raise_leaf_fact(
                    &LeafTypeFact::Primitive(verter_type_expr::PrimitiveName::Number),
                    ctx,
                )
            }),
            KeyTypeShape::Symbol => required(&|| {
                self.raise_leaf_fact(
                    &LeafTypeFact::Primitive(verter_type_expr::PrimitiveName::Symbol),
                    ctx,
                )
            }),
            KeyTypeShape::Other(slot) => required(&|| {
                self.raise_body_slot(slot, ctx.scope_canonical_id)
                    .at_optional_boundary()
            }),
        }
    }

    /// Compose a function-signature fact into a `Signature` carrier node
    /// (`kind: Construct` for a construct signature). Parameter /
    /// return positions with authored slots lower through the memoized locator
    /// query; slot-less positions (unannotated / rest parameters, inferred
    /// returns) intern the typed miss the whole-signature demand re-derives.
    pub(in crate::project_semantic_dispatch) fn compose_function_fact_node(
        &self,
        signature: &FunctionSignatureFact,
        ctx: &SourceRaiseContext<'_>,
        construct: bool,
    ) -> HotTypeRef {
        let scope = self.raise_scope(ctx);
        // Schema split: a PRESENT slot whose raise fails records a strict
        // interior failure; an ABSENT slot (unannotated / rest parameter,
        // inferred return, no constraint/default) legitimately interns the
        // typed miss (or stays `None`) with NO failure — the two are
        // distinguished by the schema `Option`, never a heuristic.
        let params: Vec<crate::semantic_query::FunctionParam> = signature
            .parameters
            .iter()
            .enumerate()
            .map(|(ordinal, param)| crate::semantic_query::FunctionParam {
                name: param.name.as_deref().map(Arc::from),
                ty: match param.ty.as_ref() {
                    Some(slot) => self.raise_required_interior(
                        ctx,
                        &scope,
                        crate::meta_resolve::InteriorSourceStep::Parameter {
                            ordinal: ordinal as u32,
                        },
                        || {
                            self.raise_body_slot(slot, ctx.scope_canonical_id)
                                .at_optional_boundary()
                        },
                    ),
                    None => self.miss_node(&scope),
                },
                optional: param.optional,
                rest: param.rest,
                span: None,
            })
            .collect();
        let (return_type, return_carrier) = match &signature.return_source {
            verter_type_expr::facts::FunctionReturnSource::Declared(locator) => {
                let return_type = self.raise_required_interior(
                    ctx,
                    &scope,
                    crate::meta_resolve::InteriorSourceStep::ReturnType,
                    || {
                        self.raise_body_slot(locator.slot(), ctx.scope_canonical_id)
                            .at_optional_boundary()
                    },
                );
                (
                    return_type,
                    crate::semantic_query::SignatureReturnCarrier::Declared(return_type),
                )
            }
            // A body-derived return is demanded from the whole-function
            // producer through the sealed helper — NEVER the absent-slot
            // arm. A degraded evaluation marks the enclosing composition
            // partial / ReturnOnly. The carrier records the SAME served
            // position.
            verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
                let carrier = crate::semantic_query::SignatureReturnCarrier::Function(
                    verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
                );
                let return_type = match ctx.with_interior_step(
                    crate::meta_resolve::InteriorSourceStep::ReturnType,
                    || {
                        self.execute_function_return_source(
                            &verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
                            ctx.scope_canonical_id,
                        )
                    },
                ) {
                    super::flow_return::FunctionReturnNode::Flow(result) => result.return_type(),
                    super::flow_return::FunctionReturnNode::Declared(hot) => hot.node(),
                    super::flow_return::FunctionReturnNode::DeclaredMiss
                    | super::flow_return::FunctionReturnNode::NoValue(_) => {
                        ctx.record_interior_failure();
                        self.miss_node(&scope)
                    }
                    super::flow_return::FunctionReturnNode::Absent => self.miss_node(&scope),
                };
                (return_type, carrier)
            }
            verter_type_expr::facts::FunctionReturnSource::Absent => (
                self.miss_node(&scope),
                crate::semantic_query::SignatureReturnCarrier::Function(
                    verter_type_expr::facts::FunctionReturnSource::Absent,
                ),
            ),
        };
        let type_parameters: Vec<crate::semantic_query::TypeParamDecl> =
            signature
                .type_parameters
                .iter()
                .enumerate()
                .map(|(ordinal, param)| {
                    // A PRESENT constraint/default slot whose raise fails keeps
                    // the historical `None` node shape (no fabricated miss) —
                    // the strict path records the failure instead; a successful
                    // deref whose raised body materializes an
                    // unknown-materializing failure records the conservative
                    // typed failure.
                    let constraint = param.constraint.as_ref().and_then(|slot| {
                        ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::TypeParamConstraint {
                                ordinal: ordinal as u32,
                            },
                            || {
                                let raised = self
                                    .raise_body_slot(slot, ctx.scope_canonical_id)
                                    .at_optional_boundary();
                                match raised.as_ref() {
                                    Some(_) => ctx
                                        .check_raised_unknown_materializing(self, raised.as_ref()),
                                    None => ctx.record_interior_failure(),
                                }
                                raised.map(HotTypeRef::node)
                            },
                        )
                    });
                    let default = param.default.as_ref().and_then(|slot| {
                        ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::TypeParamDefault {
                                ordinal: ordinal as u32,
                            },
                            || {
                                let raised = self
                                    .raise_body_slot(slot, ctx.scope_canonical_id)
                                    .at_optional_boundary();
                                match raised.as_ref() {
                                    Some(_) => ctx
                                        .check_raised_unknown_materializing(self, raised.as_ref()),
                                    None => ctx.record_interior_failure(),
                                }
                                raised.map(HotTypeRef::node)
                            },
                        )
                    });
                    let display_name: Arc<str> = Arc::from(param.name.as_str());
                    let param_node = self.graph().intern_node_with_scope(
                        SemanticNodeData::TypeParam {
                            decl: crate::semantic_query::DeclIdentity::from_scope(
                                &scope,
                                Arc::clone(&display_name),
                            ),
                            // The shared signature-scoped binder convention
                            // (`BinderIdentityMode::Signature`): display-name-keyed
                            // at ordinal 0.
                            param_index: 0,
                            constraint,
                            default,
                            display_name: Arc::clone(&display_name),
                        },
                        scope.clone(),
                    );
                    crate::semantic_query::TypeParamDecl {
                        name: display_name,
                        param: param_node,
                        constraint,
                        default,
                        is_const: param.is_const,
                    }
                })
                .collect();
        let kind = if construct {
            crate::semantic_query::SignatureKind::Construct
        } else {
            crate::semantic_query::SignatureKind::Call
        };
        HotTypeRef::new(self.graph().intern_node_with_scope(
            SemanticNodeData::Signature {
                kind,
                params: Arc::from(params.into_boxed_slice()),
                return_type,
                type_parameters: Arc::from(type_parameters.into_boxed_slice()),
                // The fact-composition path does not carry occurrence-grade
                // provenance today; the overload-set producer treats an
                // occurrence-less candidate as an honest `Miss`.
                occurrence: None,
                return_carrier,
                signature_span: None,
                return_type_span: None,
            },
            scope,
        ))
    }

    /// Compose a tuple payload fact into a `Tuple` carrier node.
    pub(in crate::project_semantic_dispatch) fn compose_tuple_fact_node(
        &self,
        tuple: &TuplePayloadFact,
        ctx: &SourceRaiseContext<'_>,
    ) -> HotTypeRef {
        let scope = self.raise_scope(ctx);
        let elements: Vec<TupleElement> = tuple
            .elements
            .iter()
            .enumerate()
            .map(|(ordinal, element)| TupleElement {
                label: element.label.as_deref().map(Arc::from),
                value: ctx.with_interior_step(
                    crate::meta_resolve::InteriorSourceStep::TupleElement {
                        ordinal: ordinal as u32,
                    },
                    || self.raise_fact_or_locator(&element.ty, ctx, &scope),
                ),
                optional: element.optional,
                rest: element.rest,
            })
            .collect();
        HotTypeRef::new(self.graph().intern_node_with_scope(
            SemanticNodeData::Tuple {
                elements: Arc::from(elements.into_boxed_slice()),
                readonly: tuple.readonly,
            },
            scope,
        ))
    }

    /// Compose a path-precise indexed-access fact into a deferred
    /// `IndexedAccess` shell chain — the object lowers through its body slot;
    /// each index-path key stays a string key the consuming dispatch projects
    /// on demand.
    pub(in crate::project_semantic_dispatch) fn compose_indexed_access_fact_node(
        &self,
        access: &IndexedAccessFact,
        ctx: &SourceRaiseContext<'_>,
    ) -> HotTypeRef {
        let scope = self.raise_scope(ctx);
        let mut node = self.raise_required_interior(
            ctx,
            &scope,
            crate::meta_resolve::InteriorSourceStep::IndexedAccessObject,
            || {
                self.raise_body_slot(&access.object, ctx.scope_canonical_id)
                    .at_optional_boundary()
            },
        );
        for key in access.index_path.iter() {
            node = self.graph().intern_node_with_scope(
                SemanticNodeData::IndexedAccess {
                    object: node,
                    index: IndexKey::String(Arc::from(key.as_str())),
                },
                scope.clone(),
            );
        }
        HotTypeRef::new(node)
    }
}

/// Map a fact-side declaration origin onto the graph member's origin file.
fn declaration_origin_file(
    origin: &verter_type_expr::facts::DeclarationOrigin,
) -> Option<Arc<str>> {
    match origin {
        verter_type_expr::facts::DeclarationOrigin::Declared(canonical) => {
            Some(Arc::clone(canonical))
        }
        verter_type_expr::facts::DeclarationOrigin::Synthetic => None,
    }
}
