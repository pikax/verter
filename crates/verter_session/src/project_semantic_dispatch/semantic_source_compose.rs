//! The fact-shell COMPOSITION half of the shared source-raising bridge
//! ([`super::semantic_source`]): the private `ProjectSemanticDispatch`
//! methods that compose closed / projected / synthesized FACT SHELLS into
//! carrier nodes — data assembly over already-defined closed facts, with
//! every interior body position lowered through the memoized locator query
//! and reference RESOLUTION still happening only at the consuming dispatch
//! demands. Split from `semantic_source.rs` along the fact-shell seam for
//! the production file-size gate; the raise ENTRIES, the strict-raise sink,
//! and the locator absolutization stay in the parent-half module.

use std::sync::Arc;

use verter_type_expr::facts::{
    FactOrLocator, FunctionSignatureFact, IndexedAccessFact, LeafTypeFact, ObjectMemberFact,
    ObjectShapeFact, SynthesizedTypeFact, TuplePayloadFact,
};
use verter_type_expr::locators::{AuthoredBodyLocator, TypeBodySlot};

use super::semantic_source::{absolutize_locator, SourceRaiseContext};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    HotTypeRef, IndexKey, IndexSignature, NodeScopeId, QueryError, QueryResult, SemanticNodeData,
    SemanticNodeId, SurfaceMember, SurfaceView, TupleElement,
};

impl ProjectSemanticDispatch<'_> {
    // ── fact-shell composition (private) ─────────────────────────────────

    /// Lower one `TypeBodySlot` (a decl-body sub-position) through the
    /// memoized locator query.
    pub(in crate::project_semantic_dispatch) fn raise_body_slot(
        &self,
        slot: &TypeBodySlot,
        scope_canonical_id: &str,
    ) -> Option<HotTypeRef> {
        let locator = absolutize_locator(
            &AuthoredBodyLocator::DeclBody(slot.clone()),
            scope_canonical_id,
        );
        match self.lower_locator(locator) {
            QueryResult::Value(node) | QueryResult::Recursive(node) => Some(HotTypeRef::new(node)),
            QueryResult::Error(_) => None,
        }
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
            FactOrLocator::Locator(slot) => {
                required(&|| self.raise_body_slot(slot, ctx.scope_canonical_id))
            }
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
            }),
            // A fabricated depth-closed sub-object surface: named leaf
            // members compose directly (leaves lower through the shared
            // in-scope lowerer; there is no deeper structure by schema).
            FactOrLocator::LeafObject(members) => {
                let members: Vec<SurfaceMember> = members
                    .iter()
                    .map(|member| SurfaceMember {
                        name: Arc::from(member.name.as_str()),
                        value: self.raise_required_interior(
                            ctx,
                            scope,
                            crate::meta_resolve::InteriorSourceStep::Member(Arc::from(
                                member.name.as_str(),
                            )),
                            || self.raise_leaf_fact(&member.ty, ctx),
                        ),
                        optional: member.optional,
                        readonly: false,
                        is_method: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        spans: verter_type_expr::MemberSpans::default(),
                        declaration_origin: scope.canonical_file(),
                        declared_in_macro_type_arg:
                            crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                    })
                    .collect();
                self.graph().intern_node_with_scope(
                    SemanticNodeData::Object(SurfaceView {
                        members: Arc::from(members.into_boxed_slice()),
                        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        keyspace: None,
                        has_index_signature: false,
                    }),
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
                        name: Arc::from(member.name.as_str()),
                        value: ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::Member(Arc::from(
                                member.name.as_str(),
                            )),
                            || self.raise_fact_or_locator(&member.ty, ctx, &scope),
                        ),
                        optional: member.optional,
                        readonly: false,
                        is_method: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        spans: verter_type_expr::MemberSpans::default(),
                        declaration_origin: scope.canonical_file(),
                        declared_in_macro_type_arg:
                            crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                    })
                    .collect();
                HotTypeRef::new(self.graph().intern_node_with_scope(
                    SemanticNodeData::Object(SurfaceView {
                        members: Arc::from(members.into_boxed_slice()),
                        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        keyspace: None,
                        has_index_signature: false,
                    }),
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
                match self.raise_symbol_ref(symbol, ctx.scope_canonical_id) {
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
        let mut members: Vec<SurfaceMember> = Vec::new();
        let mut call_signatures: Vec<SemanticNodeId> = Vec::new();
        let mut construct_signatures: Vec<SemanticNodeId> = Vec::new();
        let mut index_signatures: Vec<IndexSignature> = Vec::new();
        for member in object.members.iter() {
            match member {
                ObjectMemberFact::Property(property) => members.push(SurfaceMember {
                    name: Arc::from(property.name.as_str()),
                    value: self.raise_required_interior(
                        ctx,
                        &scope,
                        crate::meta_resolve::InteriorSourceStep::Member(Arc::from(
                            property.name.as_str(),
                        )),
                        || self.raise_body_slot(&property.ty, ctx.scope_canonical_id),
                    ),
                    optional: property.optional,
                    readonly: property.readonly,
                    is_method: false,
                    visibility: property.visibility,
                    spans: verter_type_expr::MemberSpans::default(),
                    declaration_origin: scope.canonical_file(),
                    declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                    merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                }),
                ObjectMemberFact::Method(method) => {
                    let value = ctx.with_interior_step(
                        crate::meta_resolve::InteriorSourceStep::Member(Arc::from(
                            method.name.as_str(),
                        )),
                        || self.compose_function_fact_node(&method.function, ctx, false),
                    );
                    members.push(SurfaceMember {
                        name: Arc::from(method.name.as_str()),
                        value: value.node(),
                        optional: method.optional,
                        readonly: false,
                        is_method: true,
                        visibility: method.visibility,
                        spans: verter_type_expr::MemberSpans::default(),
                        declaration_origin: scope.canonical_file(),
                        declared_in_macro_type_arg:
                            crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                    });
                }
                ObjectMemberFact::CallSignature(signature) => {
                    let ordinal = call_signatures.len() as u32;
                    call_signatures.push(
                        ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::CallSignature { ordinal },
                            || self.compose_function_fact_node(signature, ctx, false),
                        )
                        .node(),
                    );
                }
                ObjectMemberFact::ConstructSignature(signature) => {
                    let ordinal = construct_signatures.len() as u32;
                    construct_signatures.push(
                        ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::ConstructSignature { ordinal },
                            || self.compose_function_fact_node(signature, ctx, false),
                        )
                        .node(),
                    );
                }
                ObjectMemberFact::IndexSignature(signature) => {
                    let ordinal = index_signatures.len() as u32;
                    index_signatures.push(IndexSignature {
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
                            || self.raise_body_slot(&signature.value_type, ctx.scope_canonical_id),
                        ),
                        readonly: signature.readonly,
                        spans: verter_type_expr::IndexSignatureSpans::default(),
                        declaration_origin: scope.canonical_file(),
                    });
                }
            }
        }
        let has_index_signature = !index_signatures.is_empty();
        HotTypeRef::new(self.graph().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::from(call_signatures.into_boxed_slice()),
                construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                keyspace: None,
                has_index_signature,
            }),
            scope,
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
                name: Arc::from(member.name.as_str()),
                value: self.raise_required_interior(
                    ctx,
                    &scope,
                    crate::meta_resolve::InteriorSourceStep::Member(Arc::from(
                        member.name.as_str(),
                    )),
                    || self.raise_body_slot(&member.ty, ctx.scope_canonical_id),
                ),
                optional: member.optional,
                readonly: member.readonly,
                is_method: member.is_method,
                visibility: member.visibility,
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
                    || self.raise_body_slot(&signature.value_type, ctx.scope_canonical_id),
                ),
                readonly: signature.readonly,
                spans: verter_type_expr::IndexSignatureSpans::default(),
                declaration_origin: declaration_origin_file(&signature.declaration_origin),
            })
            .collect();
        let has_index_signature = surface.has_index_signature || !index_signatures.is_empty();
        HotTypeRef::new(self.graph().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::from(call_signatures.into_boxed_slice()),
                construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                keyspace: None,
                has_index_signature,
            }),
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
            KeyTypeShape::Other(slot) => {
                required(&|| self.raise_body_slot(slot, ctx.scope_canonical_id))
            }
        }
    }

    /// Compose a function-signature fact into a `Function` carrier node
    /// (wrapped in `ConstructorType` for a construct signature). Parameter /
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
                        || self.raise_body_slot(slot, ctx.scope_canonical_id),
                    ),
                    None => self.miss_node(&scope),
                },
                optional: param.optional,
                rest: param.rest,
                span: None,
            })
            .collect();
        let return_type = match signature.return_ty.as_ref() {
            Some(slot) => self.raise_required_interior(
                ctx,
                &scope,
                crate::meta_resolve::InteriorSourceStep::ReturnType,
                || self.raise_body_slot(slot, ctx.scope_canonical_id),
            ),
            None => self.miss_node(&scope),
        };
        let type_parameters: Vec<crate::semantic_query::TypeParamDecl> =
            signature
                .type_parameters
                .iter()
                .enumerate()
                .map(|(ordinal, param)| crate::semantic_query::TypeParamDecl {
                    name: Arc::from(param.name.as_str()),
                    // A PRESENT constraint/default slot whose raise fails keeps
                    // the historical `None` node shape (no fabricated miss) —
                    // the strict path records the failure instead; a successful
                    // deref whose raised body materializes an
                    // unknown-materializing failure records the conservative
                    // typed failure.
                    constraint: param.constraint.as_ref().and_then(|slot| {
                        ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::TypeParamConstraint {
                                ordinal: ordinal as u32,
                            },
                            || {
                                let raised = self.raise_body_slot(slot, ctx.scope_canonical_id);
                                match raised.as_ref() {
                                    Some(_) => ctx
                                        .check_raised_unknown_materializing(self, raised.as_ref()),
                                    None => ctx.record_interior_failure(),
                                }
                                raised.map(HotTypeRef::node)
                            },
                        )
                    }),
                    default: param.default.as_ref().and_then(|slot| {
                        ctx.with_interior_step(
                            crate::meta_resolve::InteriorSourceStep::TypeParamDefault {
                                ordinal: ordinal as u32,
                            },
                            || {
                                let raised = self.raise_body_slot(slot, ctx.scope_canonical_id);
                                match raised.as_ref() {
                                    Some(_) => ctx
                                        .check_raised_unknown_materializing(self, raised.as_ref()),
                                    None => ctx.record_interior_failure(),
                                }
                                raised.map(HotTypeRef::node)
                            },
                        )
                    }),
                })
                .collect();
        let function = self.graph().intern_node_with_scope(
            SemanticNodeData::Function {
                params: Arc::from(params.into_boxed_slice()),
                return_type,
                type_parameters: Arc::from(type_parameters.into_boxed_slice()),
                signature_span: None,
                return_type_span: None,
            },
            scope.clone(),
        );
        if construct {
            HotTypeRef::new(self.graph().intern_node_with_scope(
                SemanticNodeData::ConstructorType {
                    signature: function,
                },
                scope,
            ))
        } else {
            HotTypeRef::new(function)
        }
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
            || self.raise_body_slot(&access.object, ctx.scope_canonical_id),
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
